//! Parsing of RCON answers shared by the GUI and the console tool: ban lists (`listid`,
//! `listip`), map lists (`maps *`), the stock maps of the common games and the map search.

#[derive(Clone, Debug)]
pub struct BanEntry {
    pub is_ip: bool,
    pub ident: String,
    pub duration: String,
}

/// `listid` / `listip` output: "1 STEAM_1:0:123 : permanent" / "2 1.2.3.4 : 30.000 min".
pub fn parse_ban_list(text: &str, is_ip: bool) -> Vec<BanEntry> {
    text.lines()
        .filter_map(|line| {
            let (left, duration) = line.trim().split_once(" : ")?;
            let (index, rest) = left.trim().split_once(char::is_whitespace)?;
            index.parse::<u32>().ok()?;
            // HLDS pads each octet of an IP to three characters ("193. 24.211.152"), so the
            // address may contain spaces: join everything between the index and " : ".
            let ident: String = rest.split_whitespace().collect();
            if ident.is_empty() {
                return None;
            }
            Some(BanEntry {
                is_ip,
                ident,
                duration: duration.trim().to_string(),
            })
        })
        .collect()
}

/// Maps that ship with the game. srcds' `maps *` skips maps packed in VPK files, so the
/// default maps would be missing from Change Level without this list.
pub fn stock_maps(app_id: u32, folder: &str) -> &'static [&'static str] {
    match (app_id, folder.to_ascii_lowercase().as_str()) {
        (240, _) => &[
            "cs_assault",
            "cs_compound",
            "cs_havana",
            "cs_italy",
            "cs_militia",
            "cs_office",
            "de_aztec",
            "de_cbble",
            "de_chateau",
            "de_dust",
            "de_dust2",
            "de_inferno",
            "de_nuke",
            "de_piranesi",
            "de_port",
            "de_prodigy",
            "de_tides",
            "de_train",
        ],
        (730, _) => &[
            "ar_baggage",
            "ar_pool_day",
            "ar_shoots",
            "cs_italy",
            "cs_office",
            "de_ancient",
            "de_anubis",
            "de_dust2",
            "de_inferno",
            "de_mirage",
            "de_nuke",
            "de_overpass",
            "de_train",
            "de_vertigo",
        ],
        (300, _) => &[
            "dod_anzio",
            "dod_argentan",
            "dod_avalanche",
            "dod_colmar",
            "dod_donner",
            "dod_flash",
            "dod_jagd",
            "dod_kalt",
            "dod_palermo",
        ],
        (320, _) => &[
            "dm_lockdown",
            "dm_overwatch",
            "dm_powerhouse",
            "dm_resistance",
            "dm_runoff",
            "dm_steamlab",
            "dm_underpass",
            "halls3",
        ],
        (440, _) => &[
            "cp_badlands",
            "cp_dustbowl",
            "cp_granary",
            "cp_gorge",
            "cp_gravelpit",
            "cp_process_final",
            "cp_well",
            "ctf_2fort",
            "ctf_doublecross",
            "ctf_turbine",
            "koth_harvest_final",
            "koth_viaduct",
            "pl_badwater",
            "pl_goldrush",
            "pl_upward",
            "plr_hightower",
        ],
        (_, "cstrike" | "czero") => &[
            "as_oilrig",
            "cs_747",
            "cs_assault",
            "cs_backalley",
            "cs_estate",
            "cs_havana",
            "cs_italy",
            "cs_militia",
            "cs_office",
            "cs_siege",
            "de_airstrip",
            "de_aztec",
            "de_cbble",
            "de_chateau",
            "de_dust",
            "de_dust2",
            "de_inferno",
            "de_nuke",
            "de_piranesi",
            "de_prodigy",
            "de_storm",
            "de_survivor",
            "de_torn",
            "de_train",
            "de_vertigo",
        ],
        _ => &[],
    }
}

/// `maps *` output: every token ending in ".bsp", without path and extension.
pub fn parse_map_list(text: &str) -> Vec<String> {
    let mut maps: Vec<String> = text
        .split_whitespace()
        .filter_map(|t| t.strip_suffix(".bsp"))
        .map(|t| t.rsplit(['/', '\\']).next().unwrap_or(t).to_string())
        .filter(|m| !m.is_empty() && !m.contains(['*', '?']))
        .collect();
    maps.sort_by_key(|m| m.to_lowercase());
    maps.dedup();
    maps
}

/// Map search of the Change Level tab, case insensitive. Plain text finds maps containing
/// it; with `*` (any number of characters) or `?` (exactly one) the whole name must match,
/// like a file pattern: `de_*`, `*_2`, `cs_?ffice`.
pub fn map_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    let name = name.to_lowercase();
    if !pattern.contains(['*', '?']) {
        return name.contains(&pattern);
    }
    let (pat, txt): (Vec<char>, Vec<char>) = (pattern.chars().collect(), name.chars().collect());
    let (mut pi, mut ti) = (0, 0);
    // Position after the last `*` and the text position it is matched up to.
    let mut star: Option<(usize, usize)> = None;
    while ti < txt.len() {
        match pat.get(pi) {
            Some('*') => {
                star = Some((pi + 1, ti));
                pi += 1;
            }
            Some(&c) if c == '?' || c == txt[ti] => {
                pi += 1;
                ti += 1;
            }
            _ => match star {
                // Let the last `*` take one more character and try again.
                Some((after, at)) => {
                    pi = after;
                    ti = at + 1;
                    star = Some((after, at + 1));
                }
                None => return false,
            },
        }
    }
    pat[pi..].iter().all(|&c| c == '*')
}
