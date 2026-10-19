//! User interface translations.
//!
//! The English text in the source code is the key: `t("Refresh")` returns the text in the
//! selected language, or the English text if there is no translation. Messages with values
//! use [`tr!`](crate::tr) with named placeholders:
//! `tr!("Added {address}", address = addr)`.
//!
//! The translations live in `assets/lang/<code>.json` (`{"English": "Translation", …}`) and
//! are compiled into the program.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// Languages of the user interface (the same set as HLSW's language files).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum Language {
    /// Follow the operating system's language (English if it is not available).
    #[default]
    System,
    English,
    German,
    French,
    SpanishArgentina,
    PortuguesePortugal,
    Dutch,
    Danish,
    Swedish,
    Norwegian,
    Finnish,
    Polish,
    Czech,
    Slovak,
    Hungarian,
    Turkish,
    Russian,
    ChineseSimplified,
    ChineseTraditional,
}

impl Language {
    pub const ALL: [Language; 19] = [
        Language::System,
        Language::English,
        Language::German,
        Language::French,
        Language::SpanishArgentina,
        Language::PortuguesePortugal,
        Language::Dutch,
        Language::Danish,
        Language::Swedish,
        Language::Norwegian,
        Language::Finnish,
        Language::Polish,
        Language::Czech,
        Language::Slovak,
        Language::Hungarian,
        Language::Turkish,
        Language::Russian,
        Language::ChineseSimplified,
        Language::ChineseTraditional,
    ];

    /// Name of the language in that language (shown in the selector).
    pub fn native_name(self) -> &'static str {
        match self {
            Language::System => "System",
            Language::English => "English (United States)",
            Language::German => "Deutsch (Deutschland)",
            Language::French => "Français (France)",
            Language::SpanishArgentina => "Español (Argentina)",
            Language::PortuguesePortugal => "Português (Portugal)",
            Language::Dutch => "Nederlands (Nederland)",
            Language::Danish => "Dansk",
            Language::Swedish => "Svenska",
            Language::Norwegian => "Norsk",
            Language::Finnish => "Suomi",
            Language::Polish => "Polski",
            Language::Czech => "Čeština",
            Language::Slovak => "Slovenčina",
            Language::Hungarian => "Magyar",
            Language::Turkish => "Türkçe",
            Language::Russian => "Русский",
            Language::ChineseSimplified => "简体中文",
            Language::ChineseTraditional => "繁體中文 (台灣)",
        }
    }

    /// The language actually used: `System` resolved to the operating system's language.
    pub fn resolve(self) -> Language {
        if self != Language::System {
            return self;
        }
        sys_locale::get_locale()
            .and_then(|l| Language::from_locale(&l))
            .unwrap_or(Language::English)
    }

    /// Maps a BCP 47 / POSIX locale (`de-DE`, `pt_BR.UTF-8`, `zh-Hant-TW`) to a language.
    pub fn from_locale(locale: &str) -> Option<Language> {
        let l = locale.to_ascii_lowercase().replace('_', "-");
        let lang = l.split(['-', '.', '@']).next().unwrap_or_default();
        Some(match lang {
            "en" => Language::English,
            "de" => Language::German,
            "fr" => Language::French,
            "es" => Language::SpanishArgentina,
            "pt" => Language::PortuguesePortugal,
            "nl" => Language::Dutch,
            "da" => Language::Danish,
            "sv" => Language::Swedish,
            "no" | "nb" | "nn" => Language::Norwegian,
            "fi" => Language::Finnish,
            "pl" => Language::Polish,
            "cs" => Language::Czech,
            "sk" => Language::Slovak,
            "hu" => Language::Hungarian,
            "tr" => Language::Turkish,
            "ru" => Language::Russian,
            "zh" => {
                if l.contains("hant") || l.contains("-tw") || l.contains("-hk") || l.contains("-mo")
                {
                    Language::ChineseTraditional
                } else {
                    Language::ChineseSimplified
                }
            }
            _ => return None,
        })
    }

    /// Embedded translation file (None for English).
    fn catalog(self) -> Option<&'static str> {
        Some(match self {
            Language::System | Language::English => return None,
            Language::German => include_str!("../assets/lang/de.json"),
            Language::French => include_str!("../assets/lang/fr.json"),
            Language::SpanishArgentina => include_str!("../assets/lang/es-AR.json"),
            Language::PortuguesePortugal => include_str!("../assets/lang/pt-PT.json"),
            Language::Dutch => include_str!("../assets/lang/nl.json"),
            Language::Danish => include_str!("../assets/lang/da.json"),
            Language::Swedish => include_str!("../assets/lang/sv.json"),
            Language::Norwegian => include_str!("../assets/lang/nb.json"),
            Language::Finnish => include_str!("../assets/lang/fi.json"),
            Language::Polish => include_str!("../assets/lang/pl.json"),
            Language::Czech => include_str!("../assets/lang/cs.json"),
            Language::Slovak => include_str!("../assets/lang/sk.json"),
            Language::Hungarian => include_str!("../assets/lang/hu.json"),
            Language::Turkish => include_str!("../assets/lang/tr.json"),
            Language::Russian => include_str!("../assets/lang/ru.json"),
            Language::ChineseSimplified => include_str!("../assets/lang/zh-CN.json"),
            Language::ChineseTraditional => include_str!("../assets/lang/zh-TW.json"),
        })
    }
}

type Catalog = HashMap<&'static str, &'static str>;

/// Translations of the current language (None = English). Catalogs are parsed once and
/// kept for the lifetime of the program.
static CURRENT: RwLock<Option<&'static Catalog>> = RwLock::new(None);
static LOADED: RwLock<Vec<(Language, &'static Catalog)>> = RwLock::new(Vec::new());

fn load(lang: Language) -> Option<&'static Catalog> {
    let json = lang.catalog()?;
    if let Some((_, c)) = LOADED.read().ok()?.iter().find(|(l, _)| *l == lang) {
        return Some(c);
    }
    let parsed: HashMap<String, String> = serde_json::from_str(json).unwrap_or_default();
    let catalog: Catalog = parsed
        .into_iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (&*k.leak(), &*v.leak()))
        .collect();
    let catalog: &'static Catalog = Box::leak(Box::new(catalog));
    if let Ok(mut loaded) = LOADED.write() {
        loaded.push((lang, catalog));
    }
    Some(catalog)
}

/// Switches the user interface language (`System` follows the operating system).
pub fn set_language(lang: Language) {
    let catalog = load(lang.resolve());
    if let Ok(mut current) = CURRENT.write() {
        *current = catalog;
    }
}

/// The text in the current language (the English text if there is no translation).
pub fn t(english: &str) -> &str {
    let Ok(current) = CURRENT.read() else {
        return english;
    };
    match *current {
        Some(catalog) => catalog.get(english).copied().unwrap_or(english),
        None => english,
    }
}

/// Fills `{name}` placeholders of a (translated) template.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        if let Some(stripped) = after.strip_prefix('{') {
            out.push('{');
            rest = stripped;
            continue;
        }
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, value)) => out.push_str(&value.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(&rest.replace("}}", "}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_placeholders() {
        assert_eq!(
            fill("Added {a} of {b}", &[("a", &3), ("b", &"x")]),
            "Added 3 of x"
        );
        assert_eq!(fill("{missing} {{x}}", &[]), "{missing} {x}");
    }

    #[test]
    fn locale_mapping() {
        assert_eq!(Language::from_locale("de-DE"), Some(Language::German));
        assert_eq!(
            Language::from_locale("zh-Hant-TW"),
            Some(Language::ChineseTraditional)
        );
        assert_eq!(
            Language::from_locale("zh_CN.UTF-8"),
            Some(Language::ChineseSimplified)
        );
        assert_eq!(Language::from_locale("nb-NO"), Some(Language::Norwegian));
        assert_eq!(Language::from_locale("ja-JP"), None);
    }

    /// Every catalog is valid JSON, and every placeholder of a translation also exists in
    /// its English text (so no value goes missing in a translated message).
    #[test]
    fn catalogs_are_consistent() {
        for lang in Language::ALL {
            let Some(json) = lang.catalog() else { continue };
            let map: HashMap<String, String> =
                serde_json::from_str(json).unwrap_or_else(|e| panic!("{lang:?}: {e}"));
            for (k, v) in &map {
                if v.is_empty() {
                    continue;
                }
                let names = |s: &str| {
                    let mut n: Vec<String> = s
                        .split('{')
                        .skip(1)
                        .filter_map(|p| p.split_once('}').map(|(a, _)| a.to_string()))
                        .collect();
                    n.sort();
                    n
                };
                assert_eq!(names(k), names(v), "{lang:?}: placeholders differ in {k:?}");
            }
        }
    }
}
