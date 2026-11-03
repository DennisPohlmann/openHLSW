//! Background work: server queries and RCON sessions, each on its own thread. Results come
//! back to the UI thread as [`Event`]s.

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

use openhlsw::a2s::{self, Player, QueryMeta, Rule, ServerInfo};
use openhlsw::address::ServerAddress;
use openhlsw::gamespy2;
use openhlsw::query::{self, Protocol};
use openhlsw::rcon::{GoldSrcRcon, RconClient, RconError};

const QUERY_TIMEOUT: Duration = Duration::from_millis(1500);
/// The other protocols may try several query ports.
const OTHER_TIMEOUT: Duration = Duration::from_millis(2500);
const RCON_TIMEOUT: Duration = Duration::from_secs(4);

pub type Id = u64;

/// What a query found out. Players and rules only when they were asked for.
pub struct Snapshot {
    pub info: ServerInfo,
    pub players: Option<Vec<Player>>,
    pub rules: Option<Vec<Rule>>,
    pub ping_ms: u32,
    pub challenge: Option<[u8; 4]>,
    /// Protocol found by the automatic detection (servers added without a game).
    pub detected: Option<Protocol>,
}

pub enum Event {
    Query {
        id: Id,
        result: Result<Box<Snapshot>, String>,
    },
    /// Answer of an RCON command.
    Rcon {
        id: Id,
        tag: Tag,
        result: Result<String, String>,
    },
    RconState {
        id: Id,
        state: RconState,
    },
}

pub struct QueryJob {
    pub id: Id,
    pub address: ServerAddress,
    /// Protocol of the chosen game, or the one detected before (`None` = detect).
    pub protocol: Option<Protocol>,
    /// HLSW key of the game ("" = none), picks the usual query ports.
    pub game: &'static str,
    /// Also ask for players and rules.
    pub details: bool,
    pub challenge: Option<[u8; 4]>,
}

pub fn spawn_query(tx: &Sender<Event>, job: QueryJob) {
    let tx = tx.clone();
    thread::spawn(move || {
        let id = job.id;
        let result = run_query(job).map(Box::new);
        let _ = tx.send(Event::Query { id, result });
    });
}

fn ms(meta: &QueryMeta) -> u32 {
    meta.rtt.as_millis().min(u128::from(u32::MAX)) as u32
}

fn run_query(job: QueryJob) -> Result<Snapshot, String> {
    match job.protocol {
        Some(Protocol::A2s) => valve(&job),
        Some(Protocol::GameSpy2) => gamespy(&job),
        Some(p) => other(&job, p),
        None => {
            // Same order as the GUI: Valve A2S, GameSpy 2, GameSpy 1 (GameSpy 2 first when
            // the address has its own query port).
            let order = if job.address.query_port.is_some() {
                [Protocol::GameSpy2, Protocol::A2s, Protocol::GameSpy1]
            } else {
                [Protocol::A2s, Protocol::GameSpy2, Protocol::GameSpy1]
            };
            let mut first_error = None;
            for p in order {
                let r = match p {
                    Protocol::A2s => valve(&job),
                    Protocol::GameSpy2 => gamespy(&job),
                    _ => other(&job, p),
                };
                match r {
                    Ok(mut s) => {
                        s.detected = Some(p);
                        return Ok(s);
                    }
                    Err(e) => {
                        first_error.get_or_insert(e);
                    }
                }
            }
            Err(first_error.unwrap_or_else(|| "Timeout".into()))
        }
    }
}

fn valve(job: &QueryJob) -> Result<Snapshot, String> {
    let addr = job
        .address
        .query_address()
        .map_err(|e| format!("DNS: {e}"))?;
    let (info, meta) =
        a2s::query_info_with(addr, QUERY_TIMEOUT, job.challenge).map_err(|e| e.to_string())?;
    let mut challenge = meta.challenge.or(job.challenge);
    let (mut players, mut rules) = (None, None);
    if job.details {
        if let Ok((p, m)) = a2s::query_players_with(addr, QUERY_TIMEOUT, challenge) {
            challenge = m.challenge.or(challenge);
            players = Some(p);
        }
        if let Ok((r, m)) = a2s::query_rules_with(addr, QUERY_TIMEOUT, challenge) {
            challenge = m.challenge.or(challenge);
            rules = Some(r);
        }
    }
    Ok(Snapshot {
        info,
        players,
        rules,
        ping_ms: ms(&meta),
        challenge,
        detected: None,
    })
}

fn gamespy(job: &QueryJob) -> Result<Snapshot, String> {
    let addr = job
        .address
        .query_address()
        .map_err(|e| format!("DNS: {e}"))?;
    let (info, meta) = gamespy2::query_info(addr, QUERY_TIMEOUT).map_err(|e| e.to_string())?;
    let (mut players, mut rules) = (None, None);
    if job.details {
        players = gamespy2::query_players(addr, QUERY_TIMEOUT)
            .ok()
            .map(|(p, _)| p);
        rules = gamespy2::query_rules(addr, QUERY_TIMEOUT)
            .ok()
            .map(|(r, _)| r);
    }
    Ok(Snapshot {
        info,
        players,
        rules,
        ping_ms: ms(&meta),
        challenge: None,
        detected: None,
    })
}

/// Quake, GameSpy 1, Unreal 2, TeamSpeak …: one query answers everything.
fn other(job: &QueryJob, protocol: Protocol) -> Result<Snapshot, String> {
    let target = query::Target {
        host: job.address.host.clone(),
        port: job.address.port,
        query_port: job.address.query_port,
        game: job.game.to_string(),
    };
    let (status, meta) =
        query::query(protocol, &target, OTHER_TIMEOUT).map_err(|e| e.to_string())?;
    Ok(Snapshot {
        info: status.info,
        players: Some(status.players),
        rules: Some(status.rules),
        ping_ms: ms(&meta),
        challenge: None,
        detected: None,
    })
}

// --------------------------------------------------------------------------
// RCON
// --------------------------------------------------------------------------

/// Which part of the UI a command's answer belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tag {
    Console,
    /// Answer not shown (say, writeid …).
    Silent,
    BanIds,
    BanIps,
    Maps,
}

pub enum Request {
    /// Log in (again) and report the result.
    Test,
    Disconnect,
    Exec(Tag, String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum RconState {
    Unknown,
    Connected,
    BadPassword,
    Failed(String),
    Disconnected,
}

enum Conn {
    Source(RconClient),
    GoldSrc(GoldSrcRcon),
}

impl Conn {
    fn exec(&mut self, command: &str) -> Result<String, RconError> {
        match self {
            Conn::Source(c) => c.exec(command),
            Conn::GoldSrc(c) => c.exec(command),
        }
    }
}

/// Starts an RCON session for one server: requests sent to the returned channel run one
/// after another; the connection is opened on the first command and again after it broke.
/// A wrong password ends the session (servers ban addresses after a few failed logins), so
/// it is never tried twice. The thread also ends when the sender is dropped.
pub fn spawn_rcon(
    tx: &Sender<Event>,
    id: Id,
    address: ServerAddress,
    password: String,
    goldsrc: bool,
) -> Sender<Request> {
    let (req_tx, req_rx): (Sender<Request>, Receiver<Request>) = mpsc::channel();
    let tx = tx.clone();
    thread::spawn(move || {
        let mut conn: Option<Conn> = None;
        let state = |s: RconState| {
            let _ = tx.send(Event::RconState { id, state: s });
        };
        let output = |tag: Tag, result: Result<String, String>| {
            let _ = tx.send(Event::Rcon { id, tag, result });
        };
        let connect = || -> Result<Conn, RconError> {
            let addr = address.resolve().map_err(RconError::Io)?;
            if goldsrc {
                GoldSrcRcon::connect(addr, &password, RCON_TIMEOUT).map(Conn::GoldSrc)
            } else {
                RconClient::connect(addr, &password, RCON_TIMEOUT).map(Conn::Source)
            }
        };
        while let Ok(request) = req_rx.recv() {
            let (tag, command) = match request {
                Request::Disconnect => {
                    conn = None;
                    state(RconState::Disconnected);
                    continue;
                }
                Request::Test => {
                    conn = None;
                    (None, String::new())
                }
                Request::Exec(tag, command) => (Some(tag), command),
            };
            if conn.is_none() {
                match connect() {
                    Ok(c) => {
                        conn = Some(c);
                        state(RconState::Connected);
                    }
                    Err(RconError::BadPassword) => {
                        state(RconState::BadPassword);
                        break;
                    }
                    Err(e) => {
                        state(RconState::Failed(e.to_string()));
                        if let Some(tag) = tag {
                            output(tag, Err(format!("RCON: {e}")));
                        }
                        continue;
                    }
                }
            }
            let (Some(tag), Some(c)) = (tag, conn.as_mut()) else {
                continue;
            };
            match c.exec(&command) {
                Ok(out) => output(tag, Ok(out)),
                Err(RconError::BadPassword) => {
                    state(RconState::BadPassword);
                    break;
                }
                Err(e) => {
                    output(tag, Err(format!("RCON: {e}")));
                    // Reconnect on the next command (also after timeouts, the stream may
                    // be out of step).
                    if e.is_connection_lost() || matches!(e, RconError::Timeout) {
                        conn = None;
                        state(RconState::Failed(e.to_string()));
                    }
                }
            }
        }
    });
    req_tx
}
