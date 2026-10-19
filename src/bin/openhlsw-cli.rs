//! Small command line helper around the openHLSW protocol core.
//!
//!   openhlsw-cli <host:port>                       query info, players and rules
//!   openhlsw-cli <host:port:queryport>             same for a GameSpy 2 server
//!   openhlsw-cli <host:port> <rcon-password> <cmd> run an RCON command

use std::process::ExitCode;
use std::time::Duration;

use openhlsw::a2s;
use openhlsw::address::ServerAddress;
use openhlsw::rcon::{GoldSrcRcon, RconClient};

const TIMEOUT: Duration = Duration::from_secs(3);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(address) = args.first().and_then(|a| ServerAddress::parse(a)) else {
        eprintln!("usage: openhlsw-cli <host:port> [rcon-password command...]");
        return ExitCode::from(2);
    };
    let gamespy = address.query_port.is_some();
    let addr = match address.query_address() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("cannot resolve {address}: {e}");
            return ExitCode::FAILURE;
        }
    };

    if args.len() >= 3 {
        let command = args[2..].join(" ");
        // GoldSrc (Half-Life 1) servers use UDP RCON.
        let goldsrc = a2s::query_info(addr, TIMEOUT).is_ok_and(|(i, _)| i.goldsrc);
        let result = if goldsrc {
            GoldSrcRcon::connect(addr, &args[1], TIMEOUT).and_then(|mut c| c.exec(&command))
        } else {
            RconClient::connect(addr, &args[1], TIMEOUT).and_then(|mut c| c.exec(&command))
        };
        return match result {
            Ok(out) => {
                print!("{out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("rcon: {e}");
                ExitCode::FAILURE
            }
        };
    }

    let info = if gamespy {
        openhlsw::gamespy2::query_info(addr, TIMEOUT).map(|(i, m)| (i, m.rtt))
    } else {
        a2s::query_info(addr, TIMEOUT)
    };
    match info {
        Ok((info, ping)) => {
            println!("{} ({}) ping {} ms", info.name, address, ping.as_millis());
            println!(
                "  game    {} [{}], appid {}{}",
                info.game,
                info.folder,
                info.app_id,
                if info.goldsrc { " (GoldSrc)" } else { "" }
            );
            println!("  map     {}", info.map);
            println!(
                "  players {}/{} ({} bots)",
                info.players, info.max_players, info.bots
            );
            println!(
                "  version {} {} {} vac={} pw={}",
                info.version,
                info.os_name(),
                info.server_type_name(),
                info.vac,
                info.passworded
            );
            if let Some(k) = &info.keywords {
                println!("  tags    {k}");
            }
        }
        Err(e) => {
            eprintln!("info: {e}");
            return ExitCode::FAILURE;
        }
    }
    let players = if gamespy {
        openhlsw::gamespy2::query_players(addr, TIMEOUT).map(|(p, _)| p)
    } else {
        a2s::query_players(addr, TIMEOUT)
    };
    match players {
        Ok(players) => {
            println!("players ({}):", players.len());
            for p in players {
                println!("  {:<32} {:>5} {:>8.0}s", p.name, p.score, p.duration);
            }
        }
        Err(e) => eprintln!("players: {e}"),
    }
    let rules = if gamespy {
        openhlsw::gamespy2::query_rules(addr, TIMEOUT).map(|(r, _)| r)
    } else {
        a2s::query_rules(addr, TIMEOUT)
    };
    match rules {
        Ok(rules) => {
            println!("rules ({}):", rules.len());
            for r in rules.iter().take(15) {
                println!("  {} = {}", r.key, r.value);
            }
            if rules.len() > 15 {
                println!("  ...");
            }
        }
        Err(e) => eprintln!("rules: {e}"),
    }
    ExitCode::SUCCESS
}
