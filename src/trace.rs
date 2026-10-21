//! HLSW style Trace Route window.
//!
//! * **Windows**: like HLSW, every hop is probed with an ICMP echo to the target whose TTL
//!   ends at that hop (`IcmpSendEcho` from iphlpapi, no admin rights needed). All TTLs are
//!   probed in parallel, so the route shows up within about a second, and routers that
//!   ignore pings to their own address still answer with "TTL expired".
//! * **Linux** (and IPv6): the output of `traceroute` / `tracepath` is shown line by line
//!   while it runs, then every hop is pinged with the system `ping`.
//!
//! Each hop is probed once a second. Hops whose address changes are marked as
//! "Different Route".

use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::net::{IpAddr, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use eframe::egui;

/// Samples kept per hop for the graph.
pub const HISTORY: usize = 120;
const PROBE_EVERY: Duration = Duration::from_secs(1);
const RETRACE_EVERY: Duration = Duration::from_secs(60);
const MAX_HOPS: usize = 30;

pub enum TraceEvent {
    /// Address of hop `hop` (0-based); `None` = no answer ("* * *").
    Hop {
        hop: usize,
        ip: Option<IpAddr>,
    },
    /// The target was reached at hop `hops - 1`: the route has this many hops.
    Length(usize),
    Ping {
        hop: usize,
        ms: Option<u32>,
    },
    Rdns {
        ip: IpAddr,
        name: String,
    },
    Error(String),
}

#[derive(Default)]
pub struct Hop {
    pub ip: Option<IpAddr>,
    pub rdns: String,
    pub pings: VecDeque<Option<u32>>,
    /// The hop answered from a different address than before.
    pub changed: bool,
}

impl Hop {
    pub fn last(&self) -> Option<Option<u32>> {
        self.pings.back().copied()
    }

    pub fn avg(&self) -> Option<f32> {
        let (sum, n) = self
            .pings
            .iter()
            .flatten()
            .fold((0u32, 0u32), |(s, n), &p| (s + p, n + 1));
        (n > 0).then(|| sum as f32 / n as f32)
    }
}

/// State of the Trace Route window.
pub struct TraceRoute {
    pub target: String,
    pub hops: Vec<Hop>,
    pub status: String,
    /// Number of hops once the target answered; later events beyond it are dropped.
    length: Option<usize>,
    rx: Option<Receiver<TraceEvent>>,
    stop: Arc<AtomicBool>,
}

impl TraceRoute {
    pub fn new(target: String) -> Self {
        Self {
            target,
            hops: Vec::new(),
            status: String::new(),
            length: None,
            rx: None,
            stop: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn running(&self) -> bool {
        !self.stop.load(Ordering::Relaxed)
    }

    pub fn start(&mut self, ctx: &egui::Context) {
        self.stop_trace();
        let host = self.target.trim().to_string();
        if host.is_empty() {
            return;
        }
        self.hops.clear();
        self.length = None;
        self.status = tr!("Tracing route to {host}…", host = host);
        let (tx, rx) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        self.rx = Some(rx);
        self.stop = stop.clone();
        let ctx = ctx.clone();
        thread::spawn(move || {
            let n = Notify { tx, ctx, stop };
            if let Err(e) = run(&host, &n) {
                n.send(TraceEvent::Error(e));
            }
        });
    }

    pub fn stop_trace(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.rx = None;
    }

    /// Applies the events of the worker threads.
    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(event) = rx.try_recv() {
            match event {
                TraceEvent::Hop { hop, ip } => {
                    if self.length.is_some_and(|l| hop >= l) {
                        continue;
                    }
                    if self.hops.len() <= hop {
                        self.hops.resize_with(hop + 1, Hop::default);
                    }
                    let h = &mut self.hops[hop];
                    if ip.is_some() && h.ip != ip {
                        h.changed = h.ip.is_some();
                        h.ip = ip;
                        h.rdns.clear();
                    }
                }
                TraceEvent::Length(n) => {
                    let n = n.max(1);
                    self.length = Some(n);
                    self.hops.truncate(n);
                    self.status = format!("{} hops", self.hops.len());
                }
                TraceEvent::Ping { hop, ms } => {
                    if let Some(h) = self.hops.get_mut(hop) {
                        h.pings.push_back(ms);
                        while h.pings.len() > HISTORY {
                            h.pings.pop_front();
                        }
                    }
                }
                TraceEvent::Rdns { ip, name } => {
                    for h in self.hops.iter_mut().filter(|h| h.ip == Some(ip)) {
                        h.rdns = name.clone();
                    }
                }
                TraceEvent::Error(e) => {
                    self.status = e;
                    self.stop.store(true, Ordering::Relaxed);
                }
            }
        }
    }
}

impl Drop for TraceRoute {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Channel to the window plus the stop flag; every event wakes the UI.
#[derive(Clone)]
struct Notify {
    tx: Sender<TraceEvent>,
    ctx: egui::Context,
    stop: Arc<AtomicBool>,
}

impl Notify {
    fn send(&self, e: TraceEvent) -> bool {
        let ok = self.tx.send(e).is_ok();
        self.ctx.request_repaint();
        ok
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    /// Reverse DNS of `ip` in the background.
    fn rdns(&self, ip: IpAddr) {
        let n = self.clone();
        thread::spawn(move || {
            if let Ok(name) = dns_lookup::lookup_addr(&ip)
                && name != ip.to_string()
            {
                n.send(TraceEvent::Rdns { ip, name });
            }
        });
    }
}

fn run(host: &str, n: &Notify) -> Result<(), String> {
    let target = (host, 0)
        .to_socket_addrs()
        .map_err(|e| tr!("Cannot resolve {host}: {e}", host = host, e = e))?
        .map(|a| a.ip())
        .find(IpAddr::is_ipv4)
        .ok_or_else(|| tr!("Cannot resolve {host}", host = host))?;
    #[cfg(windows)]
    if let IpAddr::V4(v4) = target
        && icmp::available()
    {
        return run_icmp(v4, n);
    }
    run_commands(host, target, n)
}

/// Windows: TTL limited ICMP probes to the target, all hops in parallel.
#[cfg(windows)]
fn run_icmp(target: std::net::Ipv4Addr, n: &Notify) -> Result<(), String> {
    let length = Arc::new(std::sync::atomic::AtomicUsize::new(MAX_HOPS));
    let mut handles = Vec::new();
    for hop in 0..MAX_HOPS {
        let (n, length) = (n.clone(), length.clone());
        handles.push(thread::spawn(move || {
            let mut known: Option<IpAddr> = None;
            while !n.stopped() {
                let started = Instant::now();
                if hop >= length.load(Ordering::Relaxed) {
                    return;
                }
                let reply = icmp::probe(target, hop as u8 + 1, 1000);
                if hop >= length.load(Ordering::Relaxed) {
                    return; // the target answered at an earlier hop
                }
                match reply {
                    Some(r) => {
                        if r.reached {
                            // Shorten the route to this hop (keep the smallest one).
                            length.fetch_min(hop + 1, Ordering::Relaxed);
                            n.send(TraceEvent::Length(length.load(Ordering::Relaxed)));
                        }
                        let ip = IpAddr::V4(r.from);
                        if known != Some(ip) {
                            known = Some(ip);
                            n.send(TraceEvent::Hop { hop, ip: Some(ip) });
                            n.rdns(ip);
                        }
                        n.send(TraceEvent::Ping {
                            hop,
                            ms: Some(r.ms),
                        });
                    }
                    None => {
                        if known.is_none() {
                            n.send(TraceEvent::Hop { hop, ip: None });
                        }
                        n.send(TraceEvent::Ping { hop, ms: None });
                    }
                }
                thread::sleep(PROBE_EVERY.saturating_sub(started.elapsed()));
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

/// Linux / fallback: stream `traceroute` (`tracert` on Windows) and ping every hop.
fn run_commands(host: &str, target: IpAddr, n: &Notify) -> Result<(), String> {
    let route: Arc<Mutex<Vec<Option<IpAddr>>>> = Arc::default();
    let mut pingers = 0;
    let mut last_trace: Option<Instant> = None;
    while !n.stopped() {
        if last_trace.is_none_or(|t| t.elapsed() >= RETRACE_EVERY) {
            last_trace = Some(Instant::now());
            let mut found = 0;
            stream_trace(host, n, |hop, ip| {
                found = found.max(hop + 1);
                {
                    let mut r = route.lock().unwrap();
                    if r.len() <= hop {
                        r.resize(hop + 1, None);
                    }
                    if ip.is_some() {
                        r[hop] = ip;
                    }
                }
                n.send(TraceEvent::Hop { hop, ip });
                if let Some(ip) = ip {
                    n.rdns(ip);
                }
                // Start pinging the hop right away.
                while pingers <= hop {
                    spawn_pinger(pingers, route.clone(), n.clone());
                    pingers += 1;
                }
            })?;
            if found == 0 {
                return Err(tr!("Trace route found no hops").into());
            }
            let len = route
                .lock()
                .unwrap()
                .iter()
                .position(|ip| *ip == Some(target));
            n.send(TraceEvent::Length(len.map_or(found, |i| i + 1)));
        }
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}

fn spawn_pinger(hop: usize, route: Arc<Mutex<Vec<Option<IpAddr>>>>, n: Notify) {
    thread::spawn(move || {
        while !n.stopped() {
            let started = Instant::now();
            let ip = route.lock().unwrap().get(hop).copied().flatten();
            if let Some(ip) = ip {
                let ms = ping(ip);
                if n.stopped() || !n.send(TraceEvent::Ping { hop, ms }) {
                    return;
                }
            }
            thread::sleep(PROBE_EVERY.saturating_sub(started.elapsed()));
        }
    });
}

fn hidden(mut cmd: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.stdin(Stdio::null()).stderr(Stdio::null());
    cmd
}

/// Runs tracert / traceroute / tracepath and reports every hop as soon as its line arrives.
fn stream_trace(
    host: &str,
    n: &Notify,
    mut on_hop: impl FnMut(usize, Option<IpAddr>),
) -> Result<(), String> {
    #[cfg(windows)]
    let candidates = {
        let mut c = Command::new("tracert");
        c.args(["-d", "-w", "1000", "-h", &MAX_HOPS.to_string(), host]);
        vec![c]
    };
    #[cfg(not(windows))]
    let candidates = {
        let mut a = Command::new("traceroute");
        a.args([
            "-n",
            "-q",
            "1",
            "-w",
            "1",
            "-m",
            &MAX_HOPS.to_string(),
            host,
        ]);
        let mut b = Command::new("tracepath");
        b.args(["-n", "-m", &MAX_HOPS.to_string(), host]);
        vec![a, b]
    };
    for cmd in candidates {
        let Ok(mut child) = hidden(cmd).stdout(Stdio::piped()).spawn() else {
            continue;
        };
        if let Some(out) = child.stdout.take() {
            for line in BufReader::new(out).split(b'\n').map_while(Result::ok) {
                if n.stopped() {
                    let _ = child.kill();
                    break;
                }
                let line = openhlsw::a2s::decode_text(&line);
                if let Some((hop, ip)) = parse_trace_line(&line) {
                    on_hop(hop, ip);
                }
            }
        }
        let _ = child.wait();
        return Ok(());
    }
    Err(if cfg!(windows) {
        tr!("tracert not found").into()
    } else {
        tr!("No trace route program found (install traceroute or tracepath)").into()
    })
}

/// One hop line ("  3    12 ms ... 1.2.3.4", " 3:  1.2.3.4  0.5ms", " 3  * * *") →
/// (0-based hop, address). Language independent: only numbers and addresses are used.
pub fn parse_trace_line(line: &str) -> Option<(usize, Option<IpAddr>)> {
    let trimmed = line.trim_start();
    let num: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
    let n = num.parse::<usize>().ok()?;
    // tracepath prints "1?:", tracert/traceroute a space after the number.
    let rest = trimmed[num.len()..].trim_start_matches(['?', ':']);
    if !rest.starts_with(char::is_whitespace) || n == 0 || n > MAX_HOPS {
        return None;
    }
    let ip = rest
        .split(|c: char| c.is_whitespace() || "[]()".contains(c))
        .find_map(|t| t.parse::<IpAddr>().ok());
    Some((n - 1, ip))
}

/// All hops of a complete trace output (used by the tests).
#[cfg(test)]
fn parse_trace(text: &str) -> Vec<Option<IpAddr>> {
    let mut hops: Vec<Option<IpAddr>> = Vec::new();
    for (hop, ip) in text.lines().filter_map(parse_trace_line) {
        if hops.len() <= hop {
            hops.resize(hop + 1, None);
        }
        if hops[hop].is_none() {
            hops[hop] = ip;
        }
    }
    hops
}

/// One ping with the system `ping`; `None` on timeout.
fn ping(ip: IpAddr) -> Option<u32> {
    let mut cmd = Command::new("ping");
    #[cfg(windows)]
    cmd.args(["-n", "1", "-w", "1000", &ip.to_string()]);
    // macOS' ping takes the wait time in milliseconds, Linux' in seconds.
    #[cfg(target_os = "macos")]
    cmd.args(["-n", "-c", "1", "-W", "1000", &ip.to_string()]);
    #[cfg(not(any(windows, target_os = "macos")))]
    cmd.args(["-n", "-c", "1", "-W", "1", &ip.to_string()]);
    let out = hidden(cmd).output().ok()?;
    parse_ping_ms(&String::from_utf8_lossy(&out.stdout))
}

/// `IcmpSendEcho` (iphlpapi.dll): ICMP echo with a TTL, without admin rights.
#[cfg(windows)]
mod icmp {
    use std::ffi::c_void;
    use std::net::Ipv4Addr;
    use std::time::Instant;

    const IP_SUCCESS: u32 = 0;
    const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;
    const IP_TTL_EXPIRED_REASSEM: u32 = 11014;

    #[repr(C)]
    #[allow(dead_code)]
    struct IpOptionInformation {
        ttl: u8,
        tos: u8,
        flags: u8,
        options_size: u8,
        options_data: *mut u8,
    }

    #[repr(C)]
    #[allow(dead_code)]
    struct IcmpEchoReply {
        address: u32,
        status: u32,
        round_trip_time: u32,
        data_size: u16,
        reserved: u16,
        data: *mut c_void,
        options: IpOptionInformation,
    }

    #[link(name = "iphlpapi")]
    unsafe extern "system" {
        fn IcmpCreateFile() -> isize;
        fn IcmpCloseHandle(handle: isize) -> i32;
        fn IcmpSendEcho(
            handle: isize,
            destination: u32,
            request_data: *const c_void,
            request_size: u16,
            request_options: *const IpOptionInformation,
            reply_buffer: *mut c_void,
            reply_size: u32,
            timeout: u32,
        ) -> u32;
    }

    pub struct Reply {
        pub from: Ipv4Addr,
        pub ms: u32,
        /// The target itself answered (echo reply), not a router on the way.
        pub reached: bool,
    }

    pub fn available() -> bool {
        // SAFETY: plain Win32 calls; the handle is closed right away.
        unsafe {
            let h = IcmpCreateFile();
            if h == -1 || h == 0 {
                return false;
            }
            IcmpCloseHandle(h);
            true
        }
    }

    /// One echo request to `target` that expires after `ttl` hops.
    pub fn probe(target: Ipv4Addr, ttl: u8, timeout_ms: u32) -> Option<Reply> {
        let payload = [0x61u8; 32];
        // Room for the reply, its data, an ICMP error and the IP header (u64 for alignment).
        let mut buffer = [0u64; 64];
        let options = IpOptionInformation {
            ttl,
            tos: 0,
            flags: 0,
            options_size: 0,
            options_data: std::ptr::null_mut(),
        };
        // SAFETY: all pointers point to live, correctly sized buffers for the call.
        unsafe {
            let h = IcmpCreateFile();
            if h == -1 || h == 0 {
                return None;
            }
            let started = Instant::now();
            IcmpSendEcho(
                h,
                u32::from_ne_bytes(target.octets()),
                payload.as_ptr().cast(),
                payload.len() as u16,
                &options,
                buffer.as_mut_ptr().cast(),
                std::mem::size_of_val(&buffer) as u32,
                timeout_ms,
            );
            let elapsed = started.elapsed().as_millis() as u32;
            IcmpCloseHandle(h);
            let reply = &*(buffer.as_ptr() as *const IcmpEchoReply);
            let ok = matches!(
                reply.status,
                IP_SUCCESS | IP_TTL_EXPIRED_TRANSIT | IP_TTL_EXPIRED_REASSEM
            );
            if !ok || reply.address == 0 {
                return None;
            }
            Some(Reply {
                from: Ipv4Addr::from(reply.address.to_ne_bytes()),
                ms: if reply.round_trip_time > 0 {
                    reply.round_trip_time
                } else {
                    elapsed
                },
                reached: reply.status == IP_SUCCESS,
            })
        }
    }
}

/// "time=12ms", "Zeit<1ms", "time=10.4 ms" → milliseconds (rounded).
pub fn parse_ping_ms(text: &str) -> Option<u32> {
    let b = text.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        if c != b'=' && c != b'<' {
            continue;
        }
        let num: String = text[i + 1..]
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
            .collect();
        if num.is_empty() {
            continue;
        }
        let after = text[i + 1 + num.len()..].trim_start();
        if after.starts_with("ms") {
            let v: f32 = num.replace(',', ".").parse().ok()?;
            // "<1ms" means below one millisecond.
            return Some(if c == b'<' { 0 } else { v.round() as u32 });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_trace_output() {
        let tracert = "\nRoutenverfolgung zu 203.0.113.174 über maximal 30 Hops\n\n  1    <1 ms    <1 ms    <1 ms  192.168.178.1\n  2    43 ms     8 ms     8 ms  198.51.100.3\n  3     *        *        *     Zeitüberschreitung der Anforderung.\n  4    45 ms    45 ms    45 ms  203.0.113.174\n\nAblaufverfolgung beendet.\n";
        let h = parse_trace(tracert);
        assert_eq!(h.len(), 4);
        assert_eq!(h[0], Some("192.168.178.1".parse().unwrap()));
        assert_eq!(h[2], None);
        let traceroute = "traceroute to 1.1.1.1 (1.1.1.1), 30 hops max\n 1  10.0.0.1  0.412 ms\n 2  *\n 3  1.1.1.1  9.1 ms\n";
        let h = parse_trace(traceroute);
        assert_eq!(
            h,
            vec![
                Some("10.0.0.1".parse().unwrap()),
                None,
                Some("1.1.1.1".parse().unwrap())
            ]
        );
        let tracepath =
            " 1?: [LOCALHOST]  pmtu 1500\n 1:  10.0.0.1  0.5ms\n 2:  1.1.1.1  9.0ms reached\n";
        let h = parse_trace(tracepath);
        assert_eq!(h[0], Some("10.0.0.1".parse().unwrap()));
        assert_eq!(h.len(), 2);
    }

    #[test]
    fn parses_ping_times() {
        assert_eq!(
            parse_ping_ms("Antwort von 1.2.3.4: Bytes=32 Zeit=12ms TTL=57"),
            Some(12)
        );
        assert_eq!(
            parse_ping_ms("Reply from 1.2.3.4: bytes=32 time<1ms TTL=64"),
            Some(0)
        );
        assert_eq!(
            parse_ping_ms("64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=10.4 ms"),
            Some(10)
        );
        assert_eq!(parse_ping_ms("Request timed out."), None);
    }
}
