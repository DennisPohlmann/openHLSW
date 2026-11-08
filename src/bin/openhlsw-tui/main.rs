//! openhlsw-tui: openHLSW in the terminal, laid out like the GUI and used like htop /
//! Midnight Commander. Same server list as the GUI (same config file), with details,
//! players, rules and the tabs Rcon Config, Console, Chat, Ban List and Change Level.
//!
//!   openhlsw-tui [-d SECONDS] [-c CONFIG] [ADDRESS...]

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use openhlsw::address::ServerAddress;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind,
};
use ratatui::crossterm::execute;

mod app;
// Which firewall may drop the server log (Get Log); the GUI's check.
#[path = "../../firewall.rs"]
mod firewall;
// The game list of the GUI (which protocol a chosen game speaks); plain data.
#[path = "../../games.rs"]
#[allow(dead_code)]
mod games;
mod net;
mod store;
mod ui;

const USAGE: &str = "\
openhlsw-tui: openHLSW in the terminal, laid out like the GUI (F1 shows the keys)

usage: openhlsw-tui [options] [host:port ...]

  -d, --delay SECONDS   refresh every SECONDS (default: the GUI's refresh interval,
                        1 s unless changed there; at least 1)
  -c, --config FILE     server list from FILE instead of openHLSW's config.json
  -h, --help            this text
  -V, --version         version

Without options the servers of the openHLSW GUI are shown (same config file).
Addresses given here are shown too, but not saved.";

struct Args {
    /// `None`: the refresh interval of the GUI.
    delay: Option<Duration>,
    config: Option<PathBuf>,
    servers: Vec<ServerAddress>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        delay: None,
        config: store::default_path(),
        servers: Vec::new(),
    };
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "-V" | "--version" => {
                println!("openhlsw-tui {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-d" | "--delay" => {
                let v = it.next().ok_or("-d needs a number of seconds")?;
                let secs: f64 = v.parse().map_err(|_| format!("not a number: {v}"))?;
                args.delay = Some(delay(secs).ok_or(format!("not a number: {v}"))?);
            }
            "-c" | "--config" => {
                args.config = Some(it.next().ok_or("-c needs a file")?.into());
            }
            _ if a.starts_with('-') => return Err(format!("unknown option {a}")),
            _ => args
                .servers
                .push(ServerAddress::parse(&a).ok_or(format!("not a server address: {a}"))?),
        }
    }
    Ok(args)
}

/// The refresh interval: 1 s … 1 h.
fn delay(secs: f64) -> Option<Duration> {
    secs.is_finite()
        .then(|| Duration::from_secs_f64(secs.clamp(1.0, 3600.0)))
}

/// Copies `text` to the terminal's clipboard (OSC 52; most terminals, also over SSH).
fn osc52(text: &str) {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in text.as_bytes().chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(T[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "\x1b]52;c;{out}\x07");
    let _ = stdout.flush();
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("openhlsw-tui: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let store = store::Store::load(args.config);
    // Same interval as the GUI (Settings → Misc), so both refresh alike.
    let delay = args
        .delay
        .or_else(|| store.setting_f64("refresh_interval").and_then(delay))
        .unwrap_or(Duration::from_secs(1));
    let mut app = app::App::new(store, args.servers, delay);

    // ratatui::init also restores the terminal when the program panics.
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = run(&mut terminal, &mut app);
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    app.save();
    // Removes the port forwarding in the router (Get Log), as the GUI does on exit.
    if let Some(mapper) = &mut app.log.mapper {
        mapper.shutdown(Duration::from_secs(3));
    }
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("openhlsw-tui: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut app::App) -> std::io::Result<()> {
    while !app.quit {
        app.tick(false);
        terminal.draw(|f| ui::draw(f, app))?;
        if let Some(text) = app.clipboard.take() {
            osc52(&text);
        }
        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                // Windows also reports key releases.
                Event::Key(k) if k.kind != KeyEventKind::Release => app.key(k),
                Event::Mouse(m) => app.mouse(m),
                _ => {}
            }
        }
    }
    Ok(())
}
