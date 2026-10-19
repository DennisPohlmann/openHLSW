//! Steam ID conversion: the IDs servers report in `status` (`STEAM_0:0:11101` on
//! Half-Life 1 and older Source games, `[U:1:22202]` on newer Source games, sometimes the
//! 64-bit ID) into all common forms.

/// Offset between a 64-bit Steam ID of an individual account and its account number.
const STEAM64_BASE: u64 = 76_561_197_960_265_728;

/// A Steam user account (individual, public universe).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteamId {
    /// Account number (the `N` in `[U:1:N]`).
    pub account: u32,
    /// Universe digit of the `STEAM_X:...` form as the server reported it (0 on HL1 and
    /// old Source, 1 on some newer Source games); 0 if the ID came in another form.
    pub universe: u8,
}

impl SteamId {
    /// Parses `STEAM_X:Y:Z`, `[U:1:N]` / `U:1:N` or a 64-bit ID. Returns `None` for bots,
    /// LAN and pending players (`BOT`, `STEAM_ID_LAN`, `VALVE_ID_LAN`, `STEAM_ID_PENDING`, …).
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        if let Some(rest) = t
            .strip_prefix("STEAM_")
            .or_else(|| t.strip_prefix("steam_"))
        {
            let mut parts = rest.split(':');
            let universe: u8 = parts.next()?.parse().ok()?;
            let y: u32 = parts.next()?.parse().ok()?;
            let z: u32 = parts.next()?.parse().ok()?;
            if parts.next().is_some() || y > 1 {
                return None;
            }
            let account = z.checked_mul(2)?.checked_add(y)?;
            return Some(Self { account, universe });
        }
        let inner = t.trim_start_matches('[').trim_end_matches(']');
        if let Some(rest) = inner.strip_prefix("U:") {
            let (_, n) = rest.split_once(':')?;
            let account: u32 = n.parse().ok()?;
            return Some(Self {
                account,
                universe: 0,
            });
        }
        if t.len() == 17 && t.bytes().all(|b| b.is_ascii_digit()) {
            let id: u64 = t.parse().ok()?;
            let account = u32::try_from(id.checked_sub(STEAM64_BASE)?).ok()?;
            return Some(Self {
                account,
                universe: 0,
            });
        }
        None
    }

    /// `STEAM_0:0:11101`
    pub fn steam2(&self) -> String {
        format!(
            "STEAM_{}:{}:{}",
            self.universe,
            self.account % 2,
            self.account / 2
        )
    }

    /// `[U:1:22202]`
    pub fn steam3(&self) -> String {
        format!("[U:1:{}]", self.account)
    }

    /// `76561197960287930`
    pub fn steam64(&self) -> u64 {
        STEAM64_BASE + u64::from(self.account)
    }

    /// `https://steamcommunity.com/profiles/76561197960287930`
    pub fn profile_url(&self) -> String {
        format!("https://steamcommunity.com/profiles/{}", self.steam64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_all_forms() {
        for input in ["STEAM_0:0:11101", "[U:1:22202]", "76561197960287930"] {
            let id = SteamId::parse(input).unwrap();
            assert_eq!(id.steam2(), "STEAM_0:0:11101", "{input}");
            assert_eq!(id.steam3(), "[U:1:22202]");
            assert_eq!(id.steam64(), 76_561_197_960_287_930);
            assert_eq!(
                id.profile_url(),
                "https://steamcommunity.com/profiles/76561197960287930"
            );
        }
        assert_eq!(
            SteamId::parse("STEAM_1:0:1234").unwrap().steam2(),
            "STEAM_1:0:1234"
        );
        assert_eq!(SteamId::parse("U:1:42").unwrap().account, 42);
    }

    #[test]
    fn rejects_bots_and_lan() {
        for input in [
            "BOT",
            "STEAM_ID_LAN",
            "VALVE_ID_LAN",
            "STEAM_ID_PENDING",
            "",
            "4",
        ] {
            assert_eq!(SteamId::parse(input), None, "{input}");
        }
    }
}
