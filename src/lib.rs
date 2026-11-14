//! openHLSW protocol core: server addresses, Valve A2S, GameSpy 2 and the other query
//! protocols of HLSW's game list (`query`), RCON, the HTTP log receiver for
//! Counter-Strike 2 (`httplog`), port forwarding in the router (`upnp`), the MCP server
//! for AI clients (`mcp`).
//! Everything here is blocking std networking with timeouts; the GUI runs it on
//! worker threads.

pub mod a2s;
pub mod address;
pub mod gamespy2;
pub mod history;
pub mod httplog;
pub mod mcp;
pub mod query;
pub mod rcon;
pub mod rcon_text;
pub mod serverlog;
pub mod stats;
pub mod steamid;
pub mod upnp;

/// Text from a server, safe for the terminal: control characters (escape sequences!)
/// become spaces, tabs four spaces.
pub fn clean_terminal_text(text: &str) -> String {
    text.replace('\t', "    ")
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// Writes `data` to `path` readable only by the owner on Unix (mode 0600, also when the
/// file already exists): for files that hold RCON passwords.
pub fn write_private(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path)?;
    // `mode` only applies to a new file; an existing one keeps its permissions.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    std::io::Write::write_all(&mut file, data)
}
