//! Update check at startup (Settings → Startup → Check for updates on startup) and from
//! "? → Check for updates...": asks GitHub for the latest release and offers it when it is
//! newer than this program.
//!
//! The request goes through the system's `curl` (Windows 10 and later, macOS and Linux
//! distributions ship it), so openHLSW needs no TLS library of its own. Any failure (no
//! curl, offline, timeout, rate limit, unexpected answer) is silent at startup; the check
//! from the menu says that it did not work. GitHub's `releases/latest` never returns drafts or pre-releases; the answer is
//! checked for them anyway.

use std::process::Command;

use serde::Deserialize;

/// The program's own version (also shown in the title bar).
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// Whole request, connecting included.
const TIMEOUT_SECS: &str = "5";

/// `releases/latest` of the GitHub API for the repository at `homepage`
/// (`https://github.com/<owner>/<repo>`).
fn api_url(homepage: &str) -> Option<String> {
    let repo = homepage.strip_prefix("https://github.com/")?;
    Some(format!(
        "https://api.github.com/repos/{repo}/releases/latest"
    ))
}

/// The releases page opened by "Download latest release".
pub fn releases_page(homepage: &str) -> String {
    format!("{homepage}/releases/latest")
}

/// Version as (major, minor, patch). Accepts "v0.6.0" and "0.6.0"; pre-release ("-rc.1"),
/// build metadata and anything else is not a version this check offers.
pub fn parse_version(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim();
    let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
    let mut parts = s.split('.');
    let mut next = || {
        let p = parts.next()?;
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        p.parse().ok()
    };
    let v = (next()?, next()?, next()?);
    parts.next().is_none().then_some(v)
}

/// The version to offer ("0.6.0"), if the release `tag` is newer than `current` and is not
/// the version the user chose not to be reminded about (`skipped`, may be empty).
pub fn to_offer(tag: &str, current: &str, skipped: &str) -> Option<String> {
    let latest = parse_version(tag)?;
    if latest <= parse_version(current)? || parse_version(skipped) == Some(latest) {
        return None;
    }
    Some(format!("{}.{}.{}", latest.0, latest.1, latest.2))
}

/// What an update check shows.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// A newer release ("0.6.0").
    Offer(String),
    UpToDate,
    Failed,
}

/// What to show for the answer of a check (`latest`: the release tag, `None` when GitHub
/// gave none). The check at start (`manual` false) shows only a newer release that was not
/// skipped; the one from the menu always answers and offers a skipped release too.
pub fn outcome(
    latest: Option<&str>,
    current: &str,
    skipped: &str,
    manual: bool,
) -> Option<Outcome> {
    let result = match latest {
        Some(tag) if parse_version(tag).is_some() => {
            match to_offer(tag, current, if manual { "" } else { skipped }) {
                Some(version) => Outcome::Offer(version),
                None => Outcome::UpToDate,
            }
        }
        _ => Outcome::Failed,
    };
    (manual || matches!(result, Outcome::Offer(_))).then_some(result)
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

/// Tag of a published, final release from the API answer.
fn release_tag(json: &[u8]) -> Option<String> {
    let r: Release = serde_json::from_slice(json).ok()?;
    (!r.draft && !r.prerelease).then_some(r.tag_name)
}

/// Asks GitHub for the tag of the latest release (blocking, at most about 5 s).
pub fn latest_release(homepage: &str) -> Result<String, String> {
    let url = api_url(homepage).ok_or("no GitHub homepage")?;
    let mut cmd = Command::new("curl");
    cmd.args([
        "--silent",
        "--fail",
        "--location",
        "--proto",
        "=https",
        "--max-time",
        TIMEOUT_SECS,
        // The answer is a few KB; never read more than 1 MB.
        "--max-filesize",
        "1000000",
        "--header",
        "Accept: application/vnd.github+json",
        "--user-agent",
        concat!("openHLSW/", env!("CARGO_PKG_VERSION")),
        &url,
    ]);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("curl: {}", out.status));
    }
    release_tag(&out.stdout).ok_or_else(|| "unexpected answer".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("v0.5.3"), Some((0, 5, 3)));
        assert_eq!(parse_version("0.10.0"), Some((0, 10, 0)));
        assert_eq!(parse_version(" V1.2.3 "), Some((1, 2, 3)));
        for bad in [
            "",
            "v",
            "0.6",
            "0.6.0.1",
            "v0.6.0-rc.1",
            "0.6.0+b1",
            "0.6.x",
            "0..6",
        ] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert!(parse_version(CURRENT).is_some());
    }

    #[test]
    fn compares_versions() {
        assert_eq!(to_offer("v0.6.0", "0.5.3", ""), Some("0.6.0".into()));
        assert_eq!(to_offer("v0.5.10", "0.5.9", ""), Some("0.5.10".into()));
        assert_eq!(to_offer("v1.0.0", "0.99.99", ""), Some("1.0.0".into()));
        assert_eq!(to_offer("v0.5.3", "0.5.3", ""), None);
        assert_eq!(to_offer("v0.5.2", "0.5.3", ""), None);
        assert_eq!(to_offer("v0.7.0-beta.1", "0.5.3", ""), None);
        assert_eq!(to_offer("nightly", "0.5.3", ""), None);
    }

    #[test]
    fn skips_the_skipped_version_only() {
        assert_eq!(to_offer("v0.6.0", "0.5.3", "0.6.0"), None);
        assert_eq!(to_offer("v0.6.0", "0.5.3", "v0.6.0"), None);
        // A newer release than the skipped one is offered again.
        assert_eq!(to_offer("v0.6.1", "0.5.3", "0.6.0"), Some("0.6.1".into()));
        assert_eq!(to_offer("v0.6.0", "0.5.3", "garbage"), Some("0.6.0".into()));
    }

    #[test]
    fn startup_check_is_silent_unless_newer() {
        assert_eq!(
            outcome(Some("v0.6.0"), "0.5.3", "", false),
            Some(Outcome::Offer("0.6.0".into()))
        );
        assert_eq!(outcome(Some("v0.6.0"), "0.5.3", "0.6.0", false), None);
        assert_eq!(outcome(Some("v0.5.3"), "0.5.3", "", false), None);
        assert_eq!(outcome(Some("garbage"), "0.5.3", "", false), None);
        assert_eq!(outcome(None, "0.5.3", "", false), None);
    }

    #[test]
    fn manual_check_always_answers() {
        assert_eq!(
            outcome(Some("v0.6.0"), "0.5.3", "", true),
            Some(Outcome::Offer("0.6.0".into()))
        );
        // A skipped release is offered again when asked for.
        assert_eq!(
            outcome(Some("v0.6.0"), "0.5.3", "0.6.0", true),
            Some(Outcome::Offer("0.6.0".into()))
        );
        assert_eq!(
            outcome(Some("v0.5.3"), "0.5.3", "", true),
            Some(Outcome::UpToDate)
        );
        assert_eq!(
            outcome(Some("v0.5.2"), "0.5.3", "", true),
            Some(Outcome::UpToDate)
        );
        assert_eq!(
            outcome(Some("v0.7.0-rc.1"), "0.5.3", "", true),
            Some(Outcome::Failed)
        );
        assert_eq!(outcome(None, "0.5.3", "", true), Some(Outcome::Failed));
    }

    #[test]
    fn reads_the_api_answer() {
        let ok = br#"{"tag_name":"v0.6.0","draft":false,"prerelease":false,"name":"v0.6.0"}"#;
        assert_eq!(release_tag(ok), Some("v0.6.0".into()));
        assert_eq!(
            release_tag(br#"{"tag_name":"v0.6.0"}"#),
            Some("v0.6.0".into())
        );
        assert_eq!(
            release_tag(br#"{"tag_name":"v0.6.0","prerelease":true}"#),
            None
        );
        assert_eq!(release_tag(br#"{"tag_name":"v0.6.0","draft":true}"#), None);
        // Rate limit / error answers.
        assert_eq!(
            release_tag(br#"{"message":"API rate limit exceeded"}"#),
            None
        );
        assert_eq!(release_tag(b"<html>"), None);
        assert_eq!(release_tag(b""), None);
    }

    #[test]
    fn urls_from_homepage() {
        let home = "https://github.com/example/project";
        assert_eq!(
            api_url(home).as_deref(),
            Some("https://api.github.com/repos/example/project/releases/latest")
        );
        assert_eq!(releases_page(home), format!("{home}/releases/latest"));
        assert_eq!(api_url("https://example.org/x"), None);
    }
}
