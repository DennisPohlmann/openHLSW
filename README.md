# openHLSW

A free remake of **HLSW** – the game server browser and RCON admin tool – for Windows,
Linux, macOS and FreeBSD, written in Rust. One native binary, no runtime needed.

![openHLSW](docs/screenshot.png)

![openHLSW Black](docs/screenshot-dark.png)

Supported servers: **Source Dedicated Server (SRCDS)** – CS:S, CS:GO, CS2, TF2, HL2:DM,
GMod, L4D and other Source games – and **Half-Life 1 / GoldSrc (HLDS)**: CS 1.6,
Condition Zero, DoD, TFC, NS, TS and other HL1 mods (queries, UDP RCON, server log) – and
**GameSpy 2 / 3** servers such as **Battlefield 2** (incl. Special Forces and other mods),
Battlefield 2142, ArmA, Crysis and **Deer Hunter 2005** (queries only).

See [CHANGELOG.md](CHANGELOG.md) for what changed between versions.

## Download

Ready-made packages are on the [releases page](https://github.com/DennisPohlmann/openHLSW/releases/latest):

| System | Architectures | Packages |
|---|---|---|
| Windows 10 / 11 | x64, x86, ARM64 | `.msi` (setup wizard), `-portable.zip` |
| Linux | x86_64, aarch64 | `.deb`, `.rpm`, `.flatpak`, `.tar.gz` |
| Linux | riscv64 | `.deb`, `.rpm`, `.tar.gz` |
| macOS 11+ | universal (Apple silicon + Intel) | `.dmg`, `.zip` |
| FreeBSD | x86_64 | `.tar.gz` |

- **Windows**: the `.msi` lets you choose the installation folder and adds a Start menu
  entry; updates install over the previous version and keep your settings. The portable
  `.zip` runs from any folder.
- **Linux**: the `.deb` / `.rpm` install a desktop entry and icon. The binaries are built
  with an old glibc and run on older distributions too. Flatpak:
  `flatpak install --user openhlsw-…flatpak`.
- **macOS**: drag openHLSW to Applications. The app is only ad-hoc signed: on the first
  start, right-click → Open (or `xattr -dr com.apple.quarantine /Applications/openHLSW.app`).
- **FreeBSD**: unpack the `.tar.gz` and start `openhlsw` (X11 or Wayland).

**Languages**: English, German, French, Spanish (Argentina), Portuguese (Portugal), Dutch,
Danish, Swedish, Norwegian, Finnish, Polish, Czech, Slovak, Hungarian, Turkish, Russian and
Chinese (Simplified and Traditional) – Settings → Misc → Language, by default the system
language.

## Features

Layout and menus like HLSW 1.4, skin White (default) or Black.

- **Server lists**: several lists (Server list → New/Open/Delete/Import/Export/Properties,
  Multi Server Insert, Change ID to reorder); add servers as `IP:port`, host name or `steam://connect/…`;
  GameSpy 2 servers as `IP:gameport:queryport` (e.g. `203.0.113.232:23459:34567`);
  Import Steam Favorites and Search LAN in the Server list menu.
  The game list next to "IP/Port" works like HLSW's: **Auto detect** (Source, Half-Life 1
  and CS2 are recognised by themselves), then the **not detectable games** (Armed Assault,
  Crysis, GRAW 2, Sniper Elite, …) and **all games** of HLSW with their icons. Games whose
  query protocol openHLSW does not speak yet are greyed out. For GameSpy 2 games with a
  known query port (e.g. ArmA: 2303) `IP:port` is enough.
  Columns: flag + #, Addr, Name (with game icon), Game, Map, Players, Ping (current + average),
  Version, Location (provider), AddOn, Anti Cheat Tool, Ranking, Limits, Comment.
  AddOns are detected from the rules (Source: MM:S, SMod, …; Half-Life 1: AMXX, ATAC, PTB,
  MM, …), Anti Cheat Tool shows VAC plus HLGuard / sXe / SMAC.
  Yellow star = RCON password stored. A server that has not answered for 10 s is offline:
  the whole line turns red and moves to the end of the list (whatever column is sorted);
  its last known name, map and infos are kept (also after a restart). When it answers
  again, it moves back up into the sorted online servers.
  Arrow up / down select the previous / next server.
- **Column width**: double-click a column header or the column separator to fit the column
  to its longest value – of all rows, also those scrolled out of view (server list, players,
  rules). A single click on a header sorts.
  Right-click a header of the server or players list to show/hide columns or to reset the
  column widths.
- **Refresh** every **1 s** by default (can be lowered to 0.4 s, see below).
- **Right-click menus** like HLSW: servers (Connect, Copy, Delete, Move/Copy, Reset Average
  Ping, Properties, Trace Route, Rcon Status Command), players (Copy Name/Steam ID/Addr/Whole
  Table, Steam ID / ID3 / ID64 / profile URL, Open Steam Profile, Global ID Database, Rcon
  Status Command, Kick, Kick & Connect, Ban ID, Ban ID Kick, Ban IP, Ban IP Kick with a
  duration), rules (Copy Key/Value/Both/All Rules/All Keys, Change Value…).
- **Copy / paste / cut** in every text field (Ctrl+C/V/X/A and right-click menu).
- **Marking rows** in the players and rules lists like in Windows lists: click, Ctrl+click,
  Shift+click, Ctrl+A, arrow keys (with Shift); Ctrl+C copies the marked rows. The
  right-click menu acts on all marked rows (copy, kick, ban, open Steam profiles, …).
- **Server info** (bottom left) with an HLSW style ping graph that grows with the panel,
  drawn in 3D on a fixed scale (100 ms in the middle, 200 ms at the top): every bar runs from
  green at the foot through yellow to red, timeouts are full red bars; scale, average, peak
  values, the current ping and the value under the mouse are shown without overlapping:
  address,
  location + provider, game, players, map, limits, friendly fire, server type, version,
  add-ons, anti-cheat, password, tags, Steam ID, SourceTV, last seen, last connected, comment.
- **Players** (right) with filter: country flag, #, name, frags, time and – via RCON
  `status` – ping, loss, Steam ID, IP; for Battlefield 2 score, deaths, skill, ping, team
  and bot straight from the server query.
- **Rules / cvars** (bottom right) with filter.
- **Every RCON command** a button sends (ban list, maps, kick, say, Get Log, …) is shown in
  the console with the server's answer. For games without RCON (GameSpy) the Rcon section
  is shown greyed out.
- **Rcon Config**: password per server, Test, status, Hide/Save, Receive Server Log
  (Get Log Internet/LAN, Retrieve log on map change), **Auto Update Status**
  (periodic `status` + automatic `logaddress_add`).
- **Console** like HLSW: selectable text (mouse, Ctrl+A, Ctrl+C, right-click), time stamps,
  coloured commands/answers, switches
  C (translate log), * (hide password), F (hide answers), D (date), E (empty lines),
  A (hide automatic commands), / (find, Ctrl+Shift+F / F3), S (auto scroll), X (clear);
  history, Tab completion / Complete.
- **Chat** (from the server log; **Admin Notification**: messages starting with `@` play a
  sound and show up in the status bar), **Ban List** (`listid`/`listip`, ban, unban, filter,
  Write, Reload, to / from the Global ID Database),
  **Change Level** (`maps *` + `listmaps` + the game's stock maps, Update, Change Level,
  Kick all on map change).
- **Trace Route** window like HLSW: per hop Ping, Ø Ping, Diff, IP, Reverse DNS, Provider and
  a ping graph, updated every second; changed routes are marked. Windows probes every hop
  with TTL limited ICMP echoes (Windows ICMP API, like HLSW); Linux uses `traceroute` /
  `tracepath` + `ping`. No admin rights are needed.
- **Filter** (toolbar check box, "Edit filter..."): hide servers by timeout, ping, password,
  game, map, country, AddOns, players, slots, free slots and time / frag / round limits.
- **Find** server name (Ctrl+F), player or rule in all servers of the list, Find Next (F3).
- **Connect** like HLSW: player name (View → Player Name, `+name`), start parameters per game
  (Settings → My Games, right-click presets like `-novid`, `+rcon_password <RCONPASSWORD>`)
  and per server (Properties → Additional parameters), asks for a missing password, waits for
  a free slot on full servers and connects automatically ("Connecting..." window).
- **Global Player ID Database** (File menu, Ctrl+W): Steam IDs with names, type (Normal,
  Friend, Lamer, Blue, Brown – colours the player name), comment and banned flag; names are
  added automatically from `status`; import / export `banned.cfg`; columns "Global ID
  Database Names / Comment" in the players list. Stored in `iddb.json` next to config.json.
- **Menus**: File (Global Player ID Database Editor, Connect Log, AppData folder, Advanced,
  Minimize), Server list, Server (Connect, Copy, Delete, Move/Copy to server list, Find,
  Properties, Trace Route), Settings (Sort Server List),
  View (Rcon Section, Server List, Rules, Password, Filter, Comment, Toolbar, Status Bar,
  Ping Graph, Skin). Keyboard shortcuts like HLSW (F3, F5, F6, Ctrl+G/D/F/P/T/I/O/R/L/U/M/W).
- **Settings**: My Games (all HLSW games with icons, own game executable and start
  parameters), Startup, Connecting (incl. Quit openHLSW), Rcon, Connections, Plugins (empty),
  Sounds, Misc (laid out like HLSW: server list, **language**, skin, then **font** – every font
  installed on the computer or the system font, Arial by default (the bundled Liberation Sans
  where Arial is missing, Wine's Tahoma likewise) – and **text size** 12–16 px; font sizes are
  snapped to whole screen pixels so text stays sharp).
- **File → Advanced**: clear the location / provider cache, reset all ping graphs.
- **Unicode**: names in every script (fallback fonts for symbols, CJK, Cyrillic …).
- Servers: all Source games, **Counter-Strike 2** and all **Half-Life 1 games** (GoldSrc,
  with their own icons, e.g. CS 1.6 and Condition Zero), plus GameSpy 2 games
  (Armed Assault, ARMA 2, Battlefield 2 / 2142, Crysis, F.E.A.R., Deer Hunter 2005, …).
- **Not included** on purpose: HLSW login, buddy list and HLSW's web / global server lists
  (those services are gone), Metamod plugin management.

### Fast refresh and the srcds limit

srcds answers only `sv_max_queries_sec` queries per second per IP (default 3, averaged over
30 s). openHLSW respects that: every server has a budget of 2.5 queries/s
(Settings → Misc → Query limit), so the refresh interval cannot go below 1 / query limit
(= 0.4 s). The default is 1 s. A lower limit the server announces in its rules (`sv_max_queries_sec`, HLDS
`max_queries_sec`) is respected as well. The A2S challenge is reused (saves one packet
per query) and rules are only fetched every 10 s. The selected server has priority.
A server counts as offline after 10 seconds without an answer.

**Why timeouts happen:** a timeout (red bar in the ping graph) means the server did not
answer a query and its retry. Usually the server or a firewall / DDoS protection in front of
it drops queries because too many arrive from your IP – for example when several servers on
the same host are refreshed quickly, since some protections count all ports of a host
together. After a timeout openHLSW halves the query rate for that server (down to a quarter)
and slowly raises it again. If timeouts persist, raise the refresh interval or lower the
query limit in Settings → Misc, or raise `sv_max_queries_sec` on your servers.

### GoldSrc (Half-Life 1)

HL1 servers are detected from their answer (old `m` info format or a Half-Life 1 AppID such
as 10 = CS 1.6, 80 = Condition Zero). For them openHLSW uses the GoldSrc split packet format,
UDP RCON (`challenge rcon` + `rcon <challenge> "<password>" <command>`), `kick #<userid>`
and `logaddress_add <ip> <port>`. If the server info is not known yet and the TCP RCON port
is closed, RCON falls back to UDP automatically.

### GameSpy 2 / 3 (Battlefield 2, Deer Hunter 2005 …)

GameSpy servers answer queries on a separate port, so they are entered as
`IP:gameport:queryport` (for games with a known query port, e.g. ArmA, `IP:port` is enough
when the game is chosen in the IP/Port list). openHLSW finds out by itself whether a server
speaks GameSpy 2 or the GameSpy 3 form (answers split over several packets, optionally with
a challenge, e.g. Battlefield 2 / 2142) and remembers it. It queries server info, players
and rules from the query port; Battlefield 2 mods (Special Forces = `xpack`, Project
Reality, …) are shown like in HLSW ("BF2 - Special Forces Expansion (gpm_coop)") with their
own icon, plus the operating system (`bf2_os`), PunkBuster and VOIP.
RCON is not available for them. Connect starts the game set in Settings → My Games;
Battlefield 2 / 2142 join the server directly (`+modPath mods/<mod> +joinServer <ip> +port
<port>`), other games are joined from their own server browser.

### Location

Country, city and provider of server and player IPs come from **ip-api.com** (free, no key)
and are cached in `geo_cache.json` next to config.json. Local addresses are not looked up.
Can be turned off in Settings → Misc.

### Receiving the server log (chat)

"Get Log (LAN)" / "Get Log (Internet)" sends `logaddress_add <your IP>:<port>`
(HLDS: `logaddress_add <IP> <port>`) and `log on` via RCON. The UDP port (default 27500,
Settings → Connections) must be reachable – for Internet that means forwarded in your router.
If the public IP field is empty, it is looked up via api.ipify.org.

## Building

Requirement: Rust (stable, ≥ 1.95; update with `rustup update`), see <https://rustup.rs>.

```sh
cargo build --release
```

The programs end up in `target/release/`:

| File | Purpose |
|---|---|
| `openhlsw` / `openhlsw.exe` | the GUI |
| `openhlsw-cli` / `openhlsw-cli.exe` | command line helper for testing |

**Linux** needs the usual OpenGL, X11 and Wayland packages to build and run,
e.g. on Debian/Ubuntu: `sudo apt install libxkbcommon-dev libgl1-mesa-dev libwayland-dev`.

**Windows**: `cargo build --release` in a normal command prompt is enough
(MSVC toolchain from rustup). The release binary does not open a console window.

**macOS** and **FreeBSD**: `cargo build --release` (FreeBSD: Rust from rustup or
`pkg install rust`, plus `pkg install pkgconf libX11 libxkbcommon`).

### Packages (GitHub Actions)

`.github/workflows/build.yml` builds and packages openHLSW when a version tag is pushed:

| System | Architectures | Packages |
|---|---|---|
| Linux | x86_64, aarch64 | `.tar.gz`, `.deb`, `.rpm`, `.flatpak` |
| Linux | riscv64 | `.tar.gz`, `.deb`, `.rpm` |
| Windows | x64, x86, arm64 | `.msi`, `-portable.zip` |
| macOS | universal (Apple silicon + Intel) | `.dmg`, `.zip` |
| FreeBSD | x86_64 | `.tar.gz` |

It only runs for version tags: `git tag v0.1.0 && git push --tags` builds everything and
creates a GitHub release with all packages; the release text is that version's section of
`CHANGELOG.md`. Linux builds use [cross](https://github.com/cross-rs/cross) (old glibc,
so the binaries run on older distributions too), `.deb` / `.rpm` are made with `cargo-deb` /
`cargo-generate-rpm` (settings in `Cargo.toml`), the `.msi` with WiX 5
(`packaging/windows/openhlsw.wxs`) as a setup wizard: license, choice of the installation
folder, progress and a finish page with "Launch openHLSW". The Linux packages install a desktop entry and icon
(`packaging/linux/`); the Flatpak (`packaging/linux/flatpak/`, freedesktop runtime 24.08)
packs the same binaries – install it with `flatpak install --user openhlsw-…flatpak`.
FreeBSD is built natively in a FreeBSD VM. The macOS binaries are universal (`lipo`). The macOS app bundle (`packaging/macos/Info.plist`) is only ad-hoc
signed, not notarized: on the first start, right-click the app → Open (or run
`xattr -dr com.apple.quarantine /Applications/openHLSW.app`).

## Command line

```sh
openhlsw-cli 1.2.3.4:27015                    # query info, players, rules
openhlsw-cli 1.2.3.4:27015 myRconPw status    # run an RCON command (Source or GoldSrc)
openhlsw-cli 1.2.3.4:23459:34567              # query a GameSpy 2 server
```

## Testing without a real server

`tools/fake_srcds.py` simulates an SRCDS: A2S with challenge and split packets, plus RCON
with multi-packet answers. With `--goldsrc` it behaves like HLDS (Condition Zero, old info
format, UDP RCON, GoldSrc split packets); `--gamespy2 <queryport>` also answers GameSpy 2
queries like a Deer Hunter 2005 server; `--latency <ms>` simulates ping.

```sh
python3 tools/fake_srcds.py --port 27015 --password secret
python3 tools/fake_srcds.py --port 27020 --password secret --goldsrc --latency 60
cargo run   # then add 127.0.0.1:27015, RCON password "secret"
```

Unit tests: `cargo test`.

## Settings

The server list, comments, all settings and (only if "Save" is ticked) RCON passwords are
stored in

- Windows: `%APPDATA%\openHLSW\config.json`
- Linux: `~/.config/openHLSW/config.json`

**Note:** saved RCON passwords are stored as plain text in this file.
eframe stores window and panel sizes separately (`app.ron`, Windows: `%APPDATA%\openHLSW\data\`).
Saved server logs are in `logs\<server>\` next to config.json.

## Layout of the code

```
src/
  lib.rs, address.rs   address parsing (host:port, [IPv6]:port, steam://connect/)
  a2s.rs               A2S_INFO / A2S_PLAYER / A2S_RULES (challenge, Source + GoldSrc split packets)
  gamespy2.rs          GameSpy 2 / 3 queries (Battlefield 2, Deer Hunter 2005 …)
  rcon.rs              RCON clients: Source (TCP, multi-packet answers) and GoldSrc (UDP)
  worker.rs            background threads: queries + one RCON thread per server, log listener
  trace.rs             Trace Route window (tracert/traceroute + ping per hop)
  stats.rs             traffic counters (status bar)
  geo.rs               server / player location (ip-api.com, cache)
  filter.rs            server list filter
  iddb.rs              Global Player ID Database (iddb.json, banned.cfg)
  native_window.rs     dialogs as real windows of the system (native title bar, like HLSW)
  games.rs             HLSW's game list (IP/Port box): names, icons, protocols, query ports
  discover.rs          Steam favorites (serverbrowser_hist.vdf) and LAN search
  flags.rs             country flags (assets/flags.png)
  game_icons.rs        game icons (assets/games/*.ico, atlas built by build.rs)
  toolbar_icons.rs     toolbar icons (assets/toolbar/*.png, drawn from assets/icon-sources/toolbar/*.svg)
  config.rs            JSON settings
  settings.rs          settings + settings dialog
  theme.rs             skins (White/Black), text size, fonts (system / Arial / Tahoma / Liberation Sans / Inconsolata)
  i18n.rs              user interface languages (tr! / t(), assets/lang/*.json)
  app.rs               GUI (egui/eframe) in the HLSW layout
  main.rs              program start
  bin/openhlsw-cli.rs  CLI
build.rs               icon atlas, window icon, .exe icon (Windows)
assets/                flags, game icons, program icon (openhlsw.ico + SVG sources)
tools/fake_srcds.py    test server
```

### Translations

The user interface is available in English, German, French, Spanish (Argentina),
Portuguese (Portugal), Dutch, Danish, Swedish, Norwegian, Finnish, Polish, Czech, Slovak,
Hungarian, Turkish, Russian and Chinese (Simplified and Traditional), chosen under
Settings → Misc → Language ("Same as the system" by default).

The English text in the code is the key: `tr!("Refresh")`, or with values
`tr!("Added {count} servers", count = n)`. Each language is a JSON file in `assets/lang/`
that maps the English text to the translation (`{"Refresh": "Aktualisieren", …}`);
missing entries fall back to English. Keep the `{placeholders}` and the `\t` shortcuts
(`"Refresh\tF5"`) in the translation – `cargo test` checks the placeholders.

Protocols: [Server queries](https://developer.valvesoftware.com/wiki/Server_queries),
[Source RCON Protocol](https://developer.valvesoftware.com/wiki/Source_RCON_Protocol).

## Not in yet (ideas for later)

- More query protocols for the greyed-out games of the list (Quake 3 / Call of Duty,
  GameSpy 1, Unreal, Doom 3, All Seeing Eye, Battlefield: Bad Company 2 / BF3, TeamSpeak …)
- System tray, View Manager
- Change Settings Password, Select All (multi selection)

## License

openHLSW is free software: you can redistribute it and/or modify it under the terms of the
[GNU General Public License v3.0 or later](LICENSE). Copyright (C) 2026 the openHLSW contributors.

The game icons in `assets/games/` are **not** covered by this license: they show logos and
artwork of the respective games, all rights belong to their owners, see
[`assets/games/NOTICE.txt`](assets/games/NOTICE.txt). The country flags are public domain
(`assets/FLAGS-LICENSE.txt`). The bundled fonts (Inconsolata and Liberation Sans under the
SIL Open Font License 1.1, Wine's Tahoma under the LGPL-2.1-or-later) are listed in
`assets/fonts/NOTICE.txt`.
openHLSW is not affiliated with the original HLSW or with any
game publisher. The license and these notices are also shown in the program under
**? → License**.
