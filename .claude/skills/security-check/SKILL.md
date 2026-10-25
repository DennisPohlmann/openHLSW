---
name: security-check
description: Security checklist for every change in openHLSW (Rust/egui desktop game server browser with RCON, UDP query parsers, log listener, ip-api.com lookups). Use before finishing, committing or pushing any code, config or packaging change.
---

# Security check for a change (openHLSW)

Scope: the current diff (`git diff` plus staged and untracked files). Walk
the points below, only as deep as the diff reaches, and report the result.

## Steps

1. Read the diff: `git status --short`, `git diff HEAD`.
2. List new or changed entry points for outside data: query protocols
   (`src/a2s.rs`, `src/gamespy2.rs`, `src/query/`), RCON (`src/rcon.rs`),
   the log listener and other code in `src/worker.rs`, HTTP lookups
   (`src/geo.rs`, `src/discover.rs`), the settings file (`src/config.rs`)
   and the command line (`src/bin/openhlsw-cli.rs`).
3. For each point below that the diff touches, check it in the code.
4. Run `cargo clippy --all-targets -- -D warnings` and `cargo test`.
5. Quick secret grep on the diff:
   `git diff HEAD | grep -nEi '(password|secret|token|apikey|api_key|BEGIN [A-Z ]*PRIVATE KEY)\s*[:=]'`.
6. Fix what you find, then report: touched points, findings with
   file:line, what you fixed, what is left.

## The checklist, adapted to openHLSW

1. **Secrets / keys:** no real RCON password, server address or API key in
   code, tests, docs or examples; examples use `RCON_PASSWORD` and
   documentation addresses (`203.0.113.x`, `2001:db8::`).
2. **Least privilege:** the app needs no admin/root rights; installers and
   the Flatpak ask for no more permissions than needed.
3. **Malformed packets:** every reply parser handles short, truncated,
   oversized and garbage packets without panic (`unwrap`, indexing,
   slicing, integer overflow, `as` casts on lengths).
4. **Git secrets:** see step 5; `*.ron`, `config.json`, `/target`, `/dist`,
   `/build` stay out of git.
5. **RCON:** the password is sent only on Test / Enter or a command, never
   while typing; no automatic retry after "Bad password" (servers ban
   addresses); challenge and request ids are checked.
6. **Spoofed replies:** replies are matched to the server that was asked
   (source address and port, challenge); data from one server never shows
   up for another.
7. **Rate limits:** refresh intervals, multi-server refresh, master/LAN
   discovery and ip-api.com lookups stay bounded (ip-api.com allows ~45
   requests per minute; results are cached, local addresses never looked up).
8. **Storage:** settings only in the documented config directory; no file
   paths built from network data; export/import dialogs only write where
   the user chose.
9. **Input validation:** addresses and ports typed by the user or read from
   files and server lists are parsed strictly (port range, IPv6 brackets,
   `IP:gameport:queryport`); RCON commands are not built from server data.
10. **Listening sockets:** the log listener binds only when the user asks
    for the log ("Get Log"), on the configured port, and ignores datagrams
    from servers it did not ask; no other listening socket is added
    silently.
11. **Command injection:** no shell or external process with data from the
    network or settings (Trace Route, opening URLs, "Connect" with game
    launch arguments): arguments passed as a list, URLs checked for scheme.
12. **Sensitive logs:** no RCON passwords, full RCON traffic with
    passwords or personal data in stdout, logs or panic messages.
13. **Field tampering:** a settings file edited by hand or from an older
    version is loaded with defaults for bad values, never a panic.
14. **Resource limits:** buffers, player/rule lists, multi-packet
    reassembly and server lists have upper
    bounds.
15. **UI thread:** network I/O runs in worker threads with timeouts; a
    server that never answers cannot freeze the UI.
16. **Trim data:** only data the user sees is stored; player names from
    servers are shown as text, never interpreted (links, markup).
17. **HTTPS / privacy:** outside requests (ip-api.com, server lists) are
    documented in the README and can be switched off where the README says
    so; nothing else phones home.
18. **Dependencies:** a change in `Cargo.toml` / `Cargo.lock` →
    `cargo audit` (if installed) and say what changed; `vendor/` changes
    keep the `openHLSW` markers and `vendor/README.md` in step.
19. **`unsafe`:** new `unsafe` needs a reason in a comment and a careful
    second read; prefer safe code.
20. **Attack your own change:** try one hostile input on the changed code
    (a truncated or huge reply via `tools/fake_srcds.py`, a forged source
    address, `../` in a path, a very long name, a negative or huge number)
    and check or test it.
