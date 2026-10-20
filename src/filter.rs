//! Server list filter (HLSW "Filter" / "Advanced Filter"): hides servers by state, ping,
//! password, game, map, country, players and limits.

use serde::{Deserialize, Serialize};

/// Password condition of the filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PasswordFilter {
    #[default]
    Any,
    /// Only servers without a join password.
    Without,
    /// Only password protected servers.
    With,
}

/// A min / max range; 0 means "no limit" on that side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Range {
    pub min: u32,
    pub max: u32,
}

impl Range {
    fn contains(&self, value: u32) -> bool {
        (self.min == 0 || value >= self.min) && (self.max == 0 || value <= self.max)
    }

    pub fn is_set(&self) -> bool {
        self.min != 0 || self.max != 0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ServerFilter {
    /// Filter switched on (toolbar check box).
    pub enabled: bool,
    /// Hide servers that do not answer. Off by default: offline servers stay in the list
    /// (red, at the end). Stored under a new key, so the old default "on" is not taken over.
    #[serde(rename = "hide_offline")]
    pub hide_timeout: bool,
    /// Hide servers with a higher ping (0 = off).
    pub max_ping: u32,
    pub password: PasswordFilter,
    /// Game name, folder or short name contains one of these (comma separated).
    pub game: String,
    /// Map name contains one of these (comma separated).
    pub map: String,
    /// Two-letter country codes (comma separated), e.g. "DE, AT, CH".
    pub country: String,
    /// AddOn name contains one of these (comma separated), e.g. "AMXX, SourceMod".
    pub addons: String,
    /// Human players.
    pub players: Range,
    /// Maximum players (slots).
    pub slots: Range,
    /// Free slots.
    pub free_slots: Range,
    /// mp_timelimit in minutes.
    pub time_limit: Range,
    /// mp_fraglimit.
    pub frag_limit: Range,
    /// mp_maxrounds.
    pub round_limit: Range,
    /// mp_winlimit (team score).
    pub team_score: Range,
}

impl Default for ServerFilter {
    fn default() -> Self {
        Self {
            enabled: false,
            hide_timeout: false,
            max_ping: 0,
            password: PasswordFilter::Any,
            game: String::new(),
            map: String::new(),
            country: String::new(),
            addons: String::new(),
            players: Range::default(),
            slots: Range::default(),
            free_slots: Range::default(),
            time_limit: Range::default(),
            frag_limit: Range::default(),
            round_limit: Range::default(),
            team_score: Range::default(),
        }
    }
}

/// What the filter looks at, taken from a server of the list.
pub struct FilterInput<'a> {
    /// No answer (timeout / offline).
    pub offline: bool,
    /// Info received at least once.
    pub has_info: bool,
    pub ping: Option<u32>,
    pub passworded: bool,
    /// Game texts: full name, folder, short name ("CS:S").
    pub game: &'a [&'a str],
    pub map: &'a str,
    pub country: &'a str,
    pub addons: &'a str,
    pub humans: u32,
    pub max_players: u32,
    /// Server rule lookup (mp_timelimit …).
    pub rule: &'a dyn Fn(&str) -> Option<u32>,
}

fn words(list: &str) -> impl Iterator<Item = String> + '_ {
    list.split([',', ';'])
        .map(|w| w.trim().to_lowercase())
        .filter(|w| !w.is_empty())
}

fn any_contains(list: &str, texts: &[&str]) -> bool {
    let mut words = words(list).peekable();
    if words.peek().is_none() {
        return true;
    }
    words.any(|w| texts.iter().any(|t| t.to_lowercase().contains(&w)))
}

impl ServerFilter {
    /// Whether the server is shown.
    pub fn matches(&self, s: &FilterInput<'_>) -> bool {
        if !self.enabled {
            return true;
        }
        if self.hide_timeout && s.offline {
            return false;
        }
        // Servers without any info yet are only judged by state; the rest needs data.
        if !s.has_info {
            return !self.needs_info();
        }
        if self.max_ping > 0 && s.ping.is_some_and(|p| p > self.max_ping) {
            return false;
        }
        match self.password {
            PasswordFilter::Without if s.passworded => return false,
            PasswordFilter::With if !s.passworded => return false,
            _ => {}
        }
        if !any_contains(&self.game, s.game)
            || !any_contains(&self.map, &[s.map])
            || !any_contains(&self.addons, &[s.addons])
        {
            return false;
        }
        let mut countries = words(&self.country).peekable();
        if countries.peek().is_some() && !countries.any(|c| c.eq_ignore_ascii_case(s.country)) {
            return false;
        }
        if !self.players.contains(s.humans)
            || !self.slots.contains(s.max_players)
            || !self
                .free_slots
                .contains(s.max_players.saturating_sub(s.humans))
        {
            return false;
        }
        for (range, key) in [
            (&self.time_limit, "mp_timelimit"),
            (&self.frag_limit, "mp_fraglimit"),
            (&self.round_limit, "mp_maxrounds"),
            (&self.team_score, "mp_winlimit"),
        ] {
            if range.is_set() && !range.contains((s.rule)(key).unwrap_or(0)) {
                return false;
            }
        }
        true
    }

    fn needs_info(&self) -> bool {
        self.max_ping > 0
            || self.password != PasswordFilter::Any
            || !self.game.trim().is_empty()
            || !self.map.trim().is_empty()
            || !self.country.trim().is_empty()
            || !self.addons.trim().is_empty()
            || [
                self.players,
                self.slots,
                self.free_slots,
                self.time_limit,
                self.frag_limit,
                self.round_limit,
                self.team_score,
            ]
            .iter()
            .any(Range::is_set)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(rule: &'a dyn Fn(&str) -> Option<u32>) -> FilterInput<'a> {
        FilterInput {
            offline: false,
            has_info: true,
            ping: Some(40),
            passworded: false,
            game: &["Counter-Strike: Source", "cstrike", "CS:S"],
            map: "de_dust2",
            country: "DE",
            addons: "MM:S, SourceMod",
            humans: 10,
            max_players: 16,
            rule,
        }
    }

    #[test]
    fn filters() {
        let rule = |k: &str| (k == "mp_timelimit").then_some(30);
        let s = input(&rule);
        let mut f = ServerFilter {
            enabled: true,
            ..Default::default()
        };
        assert!(f.matches(&s));
        f.max_ping = 30;
        assert!(!f.matches(&s));
        f.max_ping = 0;
        f.game = "cs2, css".into();
        assert!(!f.matches(&s));
        f.game = "cs:s".into();
        assert!(f.matches(&s));
        f.country = "at, de".into();
        assert!(f.matches(&s));
        f.free_slots = Range { min: 7, max: 0 };
        assert!(!f.matches(&s));
        f.free_slots = Range::default();
        f.time_limit = Range { min: 0, max: 20 };
        assert!(!f.matches(&s));
        f.time_limit = Range::default();
        f.addons = "sourcemod".into();
        assert!(f.matches(&s));
        f.password = PasswordFilter::With;
        assert!(!f.matches(&s));
        f.enabled = false;
        assert!(f.matches(&s));
    }
}
