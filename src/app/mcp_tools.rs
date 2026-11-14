//! The tools of the MCP server (Settings → MCP): the server list as openHLSW shows it,
//! and, if allowed, RCON commands through the server's own RCON connection.

use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use serde_json::{Value, json};

use openhlsw::a2s::{Player, Rule};
use openhlsw::address::ServerAddress;
use openhlsw::query::{self, Protocol};

use super::{OpenHlswApp, QueryState, Server, lock_app};
use crate::worker::RconRequest;

/// How long an RCON command may take before the MCP client gets an error.
const RCON_REPLY_TIMEOUT: Duration = Duration::from_secs(20);
/// Timeout of the player and rules query of get_server.
const QUERY_TIMEOUT: Duration = Duration::from_millis(2500);

pub(super) struct Tools {
    pub(super) app: Weak<Mutex<OpenHlswApp>>,
}

/// Starts the MCP server with the app's settings.
pub(super) fn start(app: &OpenHlswApp) -> Result<openhlsw::mcp::McpServer, String> {
    let tools = Arc::new(Tools {
        app: app.this.clone(),
    });
    openhlsw::mcp::start(app.settings.mcp_port, app.settings.mcp_token.clone(), tools).map_err(
        |e| {
            tr!(
                "MCP server: port {port}: {e}",
                port = app.settings.mcp_port,
                e = e
            )
        },
    )
}

fn server_arg() -> Value {
    json!({
        "type": "string",
        "description": "Server id from list_servers, or its address as listed (host:port)"
    })
}

impl openhlsw::mcp::Backend for Tools {
    fn tools(&self) -> Vec<Value> {
        let mut tools = vec![
            json!({
                "name": "list_servers",
                "description": "Lists the game servers of the current openHLSW server list with name, map, game, players and ping.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "get_server",
                "description": "Details of one server: all server info, the player list and the rules (cvars) as last queried.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "server": server_arg() },
                    "required": ["server"]
                }
            }),
        ];
        let allow_rcon = self
            .app
            .upgrade()
            .is_some_and(|app| lock_app(&app).settings.mcp_allow_rcon);
        if allow_rcon {
            tools.push(json!({
                "name": "rcon_command",
                "description": "Runs an RCON command on a server whose RCON password is set in openHLSW and returns the answer. Commands can change the server, kick or ban players or shut it down.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "server": server_arg(),
                        "command": { "type": "string", "description": "Console command, e.g. status" }
                    },
                    "required": ["server", "command"]
                }
            }));
        }
        tools
    }

    fn call(&self, name: &str, args: &Value) -> Result<String, String> {
        let app = self
            .app
            .upgrade()
            .ok_or_else(|| "openHLSW is closing".to_string())?;
        let reply = {
            let mut app = lock_app(&app);
            let wanted = args
                .get("server")
                .and_then(Value::as_str)
                .unwrap_or_default();
            match name {
                "list_servers" => {
                    let list: Vec<Value> = app.servers.iter().map(summary).collect();
                    return Ok(pretty(&json!({ "list": app.list_name, "servers": list })));
                }
                "get_server" => {
                    let s = find(&app, wanted)?;
                    let (address, protocol) = (s.address.clone(), s.protocol());
                    let game = s.game.map_or("", |g| g.key);
                    let mut v = details(s);
                    drop(app);
                    // The app asks only the selected server for players and rules: ask now.
                    match fetch(&address, protocol, game) {
                        Ok((players, rules)) => {
                            v["players_list"] = players_json(&players);
                            v["rules"] = rules_json(&rules);
                        }
                        Err(e) => v["live_query_error"] = json!(e),
                    }
                    return Ok(pretty(&v));
                }
                "rcon_command" if app.settings.mcp_allow_rcon => {
                    let command = args
                        .get("command")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if command.is_empty() || command.contains(['\n', '\r', '\0']) {
                        return Err("command must be one non-empty line".into());
                    }
                    let id = find(&app, wanted)?.id;
                    let s = app.server_mut(id).ok_or("server not found")?;
                    if !s.supports_rcon() {
                        return Err("this server has no RCON".into());
                    }
                    if s.rcon_login.is_empty() {
                        return Err("no RCON password is set for this server in openHLSW (Rcon Config: enter it and click Test)".into());
                    }
                    s.configure_rcon();
                    s.append_console(tr!("*** Command from an MCP client:"));
                    let (tx, rx) = std::sync::mpsc::channel();
                    s.rcon.send(RconRequest::ExecReply {
                        command: command.to_string(),
                        reply: tx,
                    });
                    rx
                }
                _ => return Err(format!("unknown tool {name}")),
            }
        }; // the app is unlocked while the command runs (its answer arrives as an event)
        match reply.recv_timeout(RCON_REPLY_TIMEOUT) {
            // Lines stay, other control characters (escape sequences) go.
            Ok(Ok(answer)) => Ok(answer
                .chars()
                .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
                .collect()),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("no answer from the server".into()),
        }
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

fn find<'a>(app: &'a OpenHlswApp, wanted: &str) -> Result<&'a Server, String> {
    let wanted = wanted.trim();
    app.servers
        .iter()
        .find(|s| s.id.to_string() == wanted || s.address.to_string().eq_ignore_ascii_case(wanted))
        .ok_or_else(|| format!("no server {wanted:?} in the server list (see list_servers)"))
}

fn state(s: &Server) -> Value {
    match &s.state {
        QueryState::Unknown => json!("unknown"),
        QueryState::Online => json!("online"),
        QueryState::Offline(reason) => json!(format!("offline: {reason}")),
    }
}

fn summary(s: &Server) -> Value {
    let info = s.info.as_ref();
    json!({
        "id": s.id.to_string(),
        "address": s.address.to_string(),
        "state": state(s),
        "name": info.map(|i| i.name.as_str()),
        "map": info.map(|i| i.map.as_str()),
        "game": s.game_short(),
        "players": info.map(|i| i.players),
        "max_players": info.map(|i| i.max_players),
        "bots": info.map(|i| i.bots),
        "ping_ms": s.ping,
        "passworded": info.map(|i| i.passworded),
        "comment": s.comment,
    })
}

fn details(s: &Server) -> Value {
    let mut v = summary(s);
    let info = s.info.as_ref();
    let extra = json!({
        "game_description": info.map(|i| i.game.as_str()),
        "folder": info.map(|i| i.folder.as_str()),
        "app_id": info.map(|i| i.real_app_id()),
        "version": info.map(|i| i.version.as_str()),
        "os": info.map(|i| i.os_name()),
        "server_type": info.map(|i| i.server_type_name()),
        "vac": info.map(|i| i.vac),
        "keywords": info.and_then(|i| i.keywords.as_deref()),
        "average_ping_ms": s.avg_ping(),
        "rcon_available": s.supports_rcon() && !s.rcon_login.is_empty(),
        "players_list": players_json(&s.players),
        "rules": rules_json(&s.rules),
    });
    if let (Some(v), Value::Object(extra)) = (v.as_object_mut(), extra) {
        v.extend(extra);
    }
    v
}

/// Players and rules of any server, like the worker asks the selected one.
fn fetch(
    address: &ServerAddress,
    protocol: Protocol,
    game: &str,
) -> Result<(Vec<Player>, Vec<Rule>), String> {
    let e = |e: openhlsw::a2s::QueryError| e.to_string();
    match protocol {
        Protocol::A2s => {
            let a = address.query_address().map_err(|e| format!("DNS: {e}"))?;
            let players = openhlsw::a2s::query_players(a, QUERY_TIMEOUT).map_err(e)?;
            // Many servers do not answer rules (CS:GO, CS2): players alone are fine.
            let rules = openhlsw::a2s::query_rules(a, QUERY_TIMEOUT).unwrap_or_default();
            Ok((players, rules))
        }
        Protocol::GameSpy2 => {
            let a = address.query_address().map_err(|e| format!("DNS: {e}"))?;
            let (players, _) = openhlsw::gamespy2::query_players(a, QUERY_TIMEOUT).map_err(e)?;
            let rules = openhlsw::gamespy2::query_rules(a, QUERY_TIMEOUT)
                .map(|(r, _)| r)
                .unwrap_or_default();
            Ok((players, rules))
        }
        _ => {
            let target = query::Target {
                host: address.host.clone(),
                port: address.port,
                query_port: address.query_port,
                game: game.to_string(),
            };
            query::query(protocol, &target, QUERY_TIMEOUT)
                .map(|(status, _)| (status.players, status.rules))
                .map_err(|e| e.to_string())
        }
    }
}

fn players_json(players: &[Player]) -> Value {
    players
        .iter()
        .map(|p| {
            json!({
                "name": p.name,
                "score": p.score,
                "seconds_connected": p.duration.round() as i64,
                "deaths": p.deaths,
                "ping": p.ping,
                "team": p.team,
                "bot": p.bot,
            })
        })
        .collect()
}

fn rules_json(rules: &[Rule]) -> Value {
    let map: serde_json::Map<String, Value> = rules
        .iter()
        .map(|r| (r.key.clone(), json!(r.value)))
        .collect();
    Value::Object(map)
}
