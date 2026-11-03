//! RCON console command history as kept across restarts (GUI and console tool share it).

/// Most console commands kept across restarts.
pub const CONSOLE_HISTORY_MAX: usize = 200;

/// Whether a console command may carry a secret (password, login, token) and so must never
/// be written to disk.
pub fn is_secret_command(command: &str) -> bool {
    let lower = command.to_lowercase();
    let mut words = lower.split_whitespace();
    let first = words.next().unwrap_or("");
    matches!(
        first,
        "rcon_password" | "sv_password" | "password" | "login" | "sv_setsteamaccount"
    ) || (first == "rcon" && words.next().is_some())
        || ["password", "passwd", "token"]
            .iter()
            .any(|w| lower.contains(w))
}

/// The console history as stored in the config: no empty lines, no commands that may carry
/// a secret, no consecutive duplicates, at most [`CONSOLE_HISTORY_MAX`] entries (the newest).
pub fn history_to_save(history: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for command in history.iter().map(|c| c.trim()) {
        if command.is_empty() || is_secret_command(command) {
            continue;
        }
        if kept.last().map(String::as_str) != Some(command) {
            kept.push(command.to_string());
        }
    }
    let excess = kept.len().saturating_sub(CONSOLE_HISTORY_MAX);
    kept.drain(..excess);
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn secret_commands_are_not_saved() {
        for c in [
            "rcon_password PLACEHOLDER",
            "SV_PASSWORD PLACEHOLDER",
            "password x",
            "login USER PLACEHOLDER",
            "rcon pw status",
            "sv_setsteamaccount PLACEHOLDER",
            "tv_password x",
            "admin_passwd y",
            "set_token z",
        ] {
            assert!(is_secret_command(c), "{c}");
        }
        for c in [
            "status",
            "changelevel de_dust2",
            "rcon",
            "users",
            "sv_cheats 0",
        ] {
            assert!(!is_secret_command(c), "{c}");
        }
    }

    #[test]
    fn history_is_filtered_and_capped() {
        let history = strings(&[
            "status",
            "  ",
            "status",
            "rcon_password x",
            "status",
            "users",
        ]);
        assert_eq!(history_to_save(&history), strings(&["status", "users"]));

        let long: Vec<String> = (0..CONSOLE_HISTORY_MAX + 50)
            .map(|i| format!("say {i}"))
            .collect();
        let saved = history_to_save(&long);
        assert_eq!(saved.len(), CONSOLE_HISTORY_MAX);
        assert_eq!(saved.first().unwrap(), "say 50");
        assert_eq!(
            saved.last().unwrap(),
            &format!("say {}", CONSOLE_HISTORY_MAX + 49)
        );
    }
}
