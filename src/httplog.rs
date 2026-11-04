//! Receiver for server logs sent over HTTP (Counter-Strike 2, `logaddress_add_http`).
//!
//! CS2 no longer sends its log as UDP packets: it POSTs batches of lines
//! (`MM/DD/YYYY - HH:MM:SS.mmm - text`, one per line) to an URL. This is a small,
//! strictly bounded HTTP/1.1 server for exactly that: `POST <prefix><server id>` with a
//! body of at most [`MAX_BODY`] bytes, keep-alive, no other methods or paths.
//!
//! The URL carries a random secret ([`new_token`]) and the id of the server that was
//! asked for its log, so a request is only taken from a server we gave the URL to.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

/// Largest request body taken (CS2 sends a few KB per request).
pub const MAX_BODY: usize = 1 << 20;
/// Largest request line or header line.
const MAX_LINE: usize = 8 * 1024;
/// Most header lines per request.
const MAX_HEADERS: usize = 64;
/// Connections handled at the same time; more are closed right away.
const MAX_CONNECTIONS: usize = 32;
/// Most requests on one keep-alive connection before it is closed.
const MAX_REQUESTS: usize = 10_000;
/// A connection that sends nothing for this long is closed.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// A random secret for the log URL (32 hex digits). From the OS-seeded keys of std's
/// `RandomState`, so it cannot be guessed from outside.
pub fn new_token() -> String {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let part = |salt: u64| {
        let mut h = RandomState::new().build_hasher();
        h.write_u64(salt);
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        );
        h.write_u32(std::process::id());
        h.finish()
    };
    format!("{:016x}{:016x}", part(1), part(2))
}

/// The path a server is told to send its log to: `/log/<token>/<id>`.
pub fn log_path(token: &str, id: u64) -> String {
    format!("/log/{token}/{id}")
}

/// Runs the HTTP log server on `listener` (on its own threads) and calls `on_line` with
/// the server id from the URL, the sender's address and every log line.
pub fn serve<F>(listener: TcpListener, token: String, on_line: F) -> std::io::Result<()>
where
    F: Fn(u64, SocketAddr, String) + Send + Sync + 'static,
{
    let on_line = Arc::new(on_line);
    let token = Arc::new(token);
    let active = Arc::new(AtomicUsize::new(0));
    thread::Builder::new()
        .name("http-log-listener".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    // e.g. too many open files: do not spin.
                    thread::sleep(Duration::from_millis(100));
                    continue;
                };
                if active.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
                    active.fetch_sub(1, Ordering::SeqCst);
                    continue; // dropped = closed
                }
                let (on_line, token, done) = (on_line.clone(), token.clone(), active.clone());
                let spawned =
                    thread::Builder::new()
                        .name("http-log-conn".into())
                        .spawn(move || {
                            let _ = handle_connection(stream, &token, &*on_line);
                            done.fetch_sub(1, Ordering::SeqCst);
                        });
                if spawned.is_err() {
                    active.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })?;
    Ok(())
}

fn handle_connection(
    stream: TcpStream,
    token: &str,
    on_line: &dyn Fn(u64, SocketAddr, String),
) -> std::io::Result<()> {
    let peer = stream.peer_addr()?;
    stream.set_read_timeout(Some(IDLE_TIMEOUT))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    serve_requests(&mut reader, &mut writer, token, &mut |id, line| {
        crate::stats::add_received(line.len() + 1);
        on_line(id, peer, line);
    })
}

/// Answers the requests on one connection until it is closed, an error occurs or a
/// request is refused. Generic over the stream for the tests.
pub fn serve_requests<R: BufRead, W: Write>(
    reader: &mut R,
    writer: &mut W,
    token: &str,
    on_line: &mut dyn FnMut(u64, String),
) -> std::io::Result<()> {
    for _ in 0..MAX_REQUESTS {
        let request = match read_request(reader, token) {
            Ok(Some(request)) => request,
            Ok(None) => return Ok(()), // closed between requests
            Err(status) => {
                respond(writer, status, true)?;
                return Ok(());
            }
        };
        for line in log_lines(&request.body) {
            on_line(request.server, line);
        }
        respond(writer, 200, request.close)?;
        if request.close {
            return Ok(());
        }
    }
    Ok(())
}

struct Request {
    server: u64,
    body: Vec<u8>,
    close: bool,
}

/// Reads one request. `Ok(None)`: the connection was closed before a request started;
/// `Err(status)`: refused with that HTTP status (the connection is closed then).
fn read_request<R: BufRead>(reader: &mut R, token: &str) -> Result<Option<Request>, u16> {
    let Some(request_line) = read_line(reader)? else {
        return Ok(None);
    };
    let mut parts = request_line.split(' ');
    let (method, target, version) = (parts.next(), parts.next(), parts.next());
    let (Some(method), Some(target), Some(version)) = (method, target, version) else {
        return Err(400);
    };
    if parts.next().is_some() || !version.starts_with("HTTP/1.") {
        return Err(400);
    }
    let mut length: Option<usize> = None;
    let mut chunked = false;
    let mut close = version == "HTTP/1.0";
    for _ in 0..=MAX_HEADERS {
        let line = read_line(reader)?.ok_or(400u16)?;
        if line.is_empty() {
            let server = parse_target(target, token).ok_or(404u16)?;
            if method != "POST" {
                return Err(405);
            }
            let body = if chunked {
                read_chunked(reader)?
            } else {
                let length = length.ok_or(411u16)?;
                if length > MAX_BODY {
                    return Err(413);
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).map_err(|_| 400u16)?;
                body
            };
            return Ok(Some(Request {
                server,
                body,
                close,
            }));
        }
        let (name, value) = line.split_once(':').ok_or(400u16)?;
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => {
                let n: usize = value.parse().map_err(|_| 400u16)?;
                if length.is_some_and(|old| old != n) {
                    return Err(400);
                }
                length = Some(n);
            }
            "transfer-encoding" => {
                if !value.eq_ignore_ascii_case("chunked") {
                    return Err(501);
                }
                chunked = true;
            }
            "connection" => {
                if value.eq_ignore_ascii_case("close") {
                    close = true;
                } else if value.eq_ignore_ascii_case("keep-alive") {
                    close = false;
                }
            }
            _ => {}
        }
    }
    Err(431)
}

/// `/log/<token>/<id>` (a query string is ignored) → the server id.
fn parse_target(target: &str, token: &str) -> Option<u64> {
    let path = target.split('?').next()?;
    let rest = path.strip_prefix("/log/")?;
    let (got, id) = rest.split_once('/')?;
    // Compare without stopping at the first difference.
    let same = got.len() == token.len()
        && got
            .bytes()
            .zip(token.bytes())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0;
    if !same || id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    id.parse().ok()
}

/// One line ending in LF (CR LF), without it. `Ok(None)`: end of stream before any byte.
fn read_line<R: BufRead>(reader: &mut R) -> Result<Option<String>, u16> {
    let mut buf = Vec::new();
    let n = reader
        .by_ref()
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf)
        .map_err(|_| 400u16)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.last() != Some(&b'\n') {
        // Too long, or the stream ended in the middle of a line.
        return Err(if buf.len() > MAX_LINE { 431 } else { 400 });
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    String::from_utf8(buf).map(Some).map_err(|_| 400)
}

/// A `Transfer-Encoding: chunked` body, at most [`MAX_BODY`] bytes in all.
fn read_chunked<R: BufRead>(reader: &mut R) -> Result<Vec<u8>, u16> {
    let mut body = Vec::new();
    loop {
        let line = read_line(reader)?.ok_or(400u16)?;
        let size = line.split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| 400u16)?;
        if size == 0 {
            // Trailers until the empty line.
            for _ in 0..=MAX_HEADERS {
                if read_line(reader)?.ok_or(400u16)?.is_empty() {
                    return Ok(body);
                }
            }
            return Err(431);
        }
        if size > MAX_BODY - body.len() {
            return Err(413);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..]).map_err(|_| 400u16)?;
        if !read_line(reader)?.ok_or(400u16)?.is_empty() {
            return Err(400);
        }
    }
}

fn respond<W: Write>(writer: &mut W, status: u16, close: bool) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        _ => "Not Implemented",
    };
    let connection = if close { "close" } else { "keep-alive" };
    write!(
        writer,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: {connection}\r\n\r\n"
    )?;
    writer.flush()
}

/// The log lines of a request body, in the form the UDP log has them
/// (`MM/DD/YYYY - HH:MM:SS: text`, see [`normalize_line`]).
pub fn log_lines(body: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(body)
        .lines()
        .map(|line| normalize_line(line.trim_end_matches(['\r', '\0'])))
        .filter(|line| !line.is_empty())
        .collect()
}

/// CS2's `10/18/2026 - 19:22:33.123 - text` → `10/18/2026 - 19:22:33: text` (the
/// milliseconds are dropped); other lines only lose a leading `L `.
pub fn normalize_line(line: &str) -> String {
    let line = line.strip_prefix("L ").unwrap_or(line);
    let stamp = line.get(..21);
    let is_stamp = stamp.is_some_and(|s| {
        s.bytes().enumerate().all(|(i, b)| match i {
            2 | 5 => b == b'/',
            10 | 12 => b == b' ',
            11 => b == b'-',
            15 | 18 => b == b':',
            _ => b.is_ascii_digit(),
        })
    });
    match (
        is_stamp,
        line.get(21..22),
        line.get(22..25),
        line.get(25..28),
    ) {
        (true, Some("."), Some(ms), Some(" - ")) if ms.bytes().all(|b| b.is_ascii_digit()) => {
            format!("{}: {}", &line[..21], &line[28..])
        }
        _ => line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "0123456789abcdef0123456789abcdef";

    fn run(input: &[u8]) -> (String, Vec<(u64, String)>) {
        let mut reader = BufReader::new(input);
        let mut out = Vec::new();
        let mut lines = Vec::new();
        serve_requests(&mut reader, &mut out, TOKEN, &mut |id, line| {
            lines.push((id, line))
        })
        .unwrap();
        (String::from_utf8(out).unwrap(), lines)
    }

    fn post(path: &str, body: &str) -> String {
        format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
    }

    #[test]
    fn normalizes_cs2_lines() {
        assert_eq!(
            normalize_line("11/04/2026 - 05:49:02.123 - \"Ann<2><[U:1:1]><CT>\" say \"hi\""),
            "11/04/2026 - 05:49:02: \"Ann<2><[U:1:1]><CT>\" say \"hi\""
        );
        assert_eq!(
            normalize_line("L 10/18/2026 - 19:22:33: World triggered \"Round_Start\""),
            "10/18/2026 - 19:22:33: World triggered \"Round_Start\""
        );
        assert_eq!(normalize_line("short"), "short");
        assert_eq!(
            normalize_line("ää/04/2026 - 05:49:02.123 - x"),
            "ää/04/2026 - 05:49:02.123 - x"
        );
    }

    #[test]
    fn takes_log_posts_on_one_connection() {
        let path = log_path(TOKEN, 7);
        let body = "11/04/2026 - 05:49:02.123 - one\n11/04/2026 - 05:49:02.456 - two\n";
        let input = format!(
            "{}{}",
            post(&path, body),
            post(&path, "11/04/2026 - 05:49:03.000 - three\n")
        );
        let (out, lines) = run(input.as_bytes());
        assert_eq!(out.matches("HTTP/1.1 200 OK").count(), 2);
        assert_eq!(
            lines,
            vec![
                (7, "11/04/2026 - 05:49:02: one".to_string()),
                (7, "11/04/2026 - 05:49:02: two".to_string()),
                (7, "11/04/2026 - 05:49:03: three".to_string()),
            ]
        );
    }

    #[test]
    fn takes_chunked_bodies() {
        let input = format!(
            "POST {} HTTP/1.1\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n",
            log_path(TOKEN, 3)
        );
        let (out, lines) = run(input.as_bytes());
        assert!(out.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(lines, vec![(3, "hello world".to_string())]);
    }

    #[test]
    fn refuses_wrong_requests() {
        let wrong_token = post("/log/0123456789abcdef0123456789abcdee/7", "x\n");
        let cases = [
            (wrong_token.as_str(), "404"),
            (
                "GET /log/0123456789abcdef0123456789abcdef/7 HTTP/1.1\r\n\r\n",
                "405",
            ),
            (
                "POST /log/0123456789abcdef0123456789abcdef/x HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
                "404",
            ),
            (
                "POST /log/0123456789abcdef0123456789abcdef/7 HTTP/1.1\r\n\r\n",
                "411",
            ),
            (
                "POST /log/0123456789abcdef0123456789abcdef/7 HTTP/1.1\r\nContent-Length: 99999999999\r\n\r\n",
                "413",
            ),
            (
                "POST /log/0123456789abcdef0123456789abcdef/7 HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nfffffff\r\n",
                "413",
            ),
            (
                "POST /log/0123456789abcdef0123456789abcdef/7 HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n",
                "501",
            ),
            ("garbage\r\n\r\n", "400"),
            (
                "POST /log/0123456789abcdef0123456789abcdef/7 HTTP/1.1\r\nContent-Length: 10\r\n\r\nshort",
                "400",
            ),
        ];
        for (input, status) in cases {
            let (out, lines) = run(input.as_bytes());
            assert!(
                out.starts_with(&format!("HTTP/1.1 {status} ")),
                "{input:?} → {out:?}"
            );
            assert!(lines.is_empty(), "{input:?}");
        }
        let long = format!("POST /{} HTTP/1.1\r\n\r\n", "a".repeat(MAX_LINE + 10));
        assert!(run(long.as_bytes()).0.starts_with("HTTP/1.1 431 "));
        let many = format!(
            "POST / HTTP/1.1\r\n{}\r\n",
            "X: y\r\n".repeat(MAX_HEADERS + 2)
        );
        assert!(run(many.as_bytes()).0.starts_with("HTTP/1.1 431 "));
        // Closed right away: no answer at all.
        assert_eq!(run(b"").0, "");
    }

    #[test]
    fn tokens_differ() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }

    #[test]
    fn serves_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        serve(listener, TOKEN.into(), move |id, from, line| {
            let _ = tx.send((id, from.ip(), line));
        })
        .unwrap();
        let mut stream = TcpStream::connect(addr).unwrap();
        stream
            .write_all(post(&log_path(TOKEN, 9), "11/04/2026 - 05:49:02.123 - hi\n").as_bytes())
            .unwrap();
        let mut answer = [0u8; 15];
        stream.read_exact(&mut answer).unwrap();
        assert_eq!(&answer, b"HTTP/1.1 200 OK");
        let got = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(got, (9, addr.ip(), "11/04/2026 - 05:49:02: hi".to_string()));
    }
}
