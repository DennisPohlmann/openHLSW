---
name: release-security-gate
description: Full security pass over openHLSW before release-relevant commits are pushed to main (semantic-release then bumps, tags and releases). Use whenever a release is being prepared; ends with a go/no-go report and never tags or pushes on its own.
---

# Release security gate (openHLSW)

Run all steps over the whole repository, not only the last diff. Report a
table with the 20 points (ok / finding / n.a.) and a go/no-go.

Releases are made by semantic-release in CI: every push to `main` that
contains a `feat`, `fix` or breaking-change commit (Conventional Commits)
sets the version in `Cargo.toml` / `Cargo.lock`, writes `CHANGELOG.md`, tags
`vX.Y.Z`, publishes the GitHub release and adds all packages to it. So this
gate runs **before pushing release-relevant commits to `main`**, without
asking first. Never tag by hand, never edit the version fields or
`CHANGELOG.md` by hand, and do not push to `main` yourself: report the gate
result and the version semantic-release will make (`feat` → minor, `fix` →
patch, `!`/`BREAKING CHANGE` → major), then ask Dennis for the go.

## Steps

1. **Clean tree and version:** `git status` is clean; the version in
   `Cargo.toml`, the `openhlsw` entry in `Cargo.lock` and the top
   `CHANGELOG.md` entry agree with the latest tag
   (`git describe --tags --abbrev=0`).
2. **Build:** `cargo clippy --all-targets -- -D warnings`, `cargo test`,
   `cargo build --release`.
3. **Secrets in the whole history:** prefer
   `gitleaks detect --source . --log-opts="--all"` (or
   `trufflehog git file://. --only-verified`). Without those tools:
   `git log -p --all | grep -nEi '(password|secret|token|BEGIN [A-Z ]*PRIVATE KEY)\s*[:=]' | grep -v RCON_PASSWORD`.
   No `config.json`, `*.ron` or other local settings with RCON passwords
   are tracked (`git ls-files | grep -Ei 'config\.json|\.ron$'`).
4. **Dependencies:** `cargo audit` (if installed, else say so) and
   `cargo update --dry-run`; report each advisory with crate, severity and
   fixed version.
5. **Network inventory:** list every place that opens a socket or makes a
   request (`grep -rnE 'UdpSocket|TcpStream|TcpListener|http' src`):
   game queries, RCON, log listener, ip-api.com lookup, Steam / master
   server lists, Trace Route. Confirm each one is started by the user or a
   documented setting and that the README "Good to know" section is true.
6. **Parsers of untrusted data:** every query protocol in `src/a2s.rs`,
   `src/gamespy2.rs`, `src/query/` and the log listener reads packets an
   attacker can send. Check bounds on lengths and indexes, no `unwrap()` /
   slicing that can panic on short or malformed packets, limits on
   multi-packet reassembly.
7. **Attack the build:** run `tools/fake_srcds.py` (or a hostile variant)
   and feed truncated, oversized and garbage replies to `openhlsw-cli`;
   send unexpected UDP datagrams to the log port. No panic, no hang, no
   unbounded memory.
8. **`unsafe` code:** list every `unsafe` block (`grep -rn unsafe src`) and
   confirm each is still needed and sound; the same for the changes in
   `vendor/` (marked `openHLSW`).
9. **Packaging:** `.deb` / `.rpm` assets and modes in `Cargo.toml`, the WiX
   installer (`packaging/windows/`), macOS `Info.plist` and the Flatpak
   manifest (`packaging/linux/flatpak/`, its permissions as small as
   possible) install only what they should; the macOS app is ad-hoc signed.
10. **The 20 points:** go through the `security-check` skill's list for the
    whole code base, with extra attention on changes since the last tag
    (`git log --oneline <last-tag>..HEAD`).
11. **Security and privacy audit:** run the `security-privacy-audit`
    skill over the whole repository, all branches and tags since the last
    release, the release commit messages and the `CHANGELOG.md` entry that
    semantic-release will write. Any `BLOCKED` item is a no-go.
12. **Report:** go/no-go, the `security-privacy-audit` summary, findings
    with file:line and fix, the version semantic-release will make, then
    ask Dennis for the go to push to `main`.
