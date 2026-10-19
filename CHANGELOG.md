# Changelog

All notable changes to openHLSW are listed here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the
project uses [Semantic Versioning](https://semver.org/).

## [v0.1.2] - 2026-10-19

### Added
- Players (Half-Life 1 and Source): right-click → Copy now offers **Steam ID**
  (`STEAM_X:Y:Z`), **Steam ID3** (`[U:1:N]`), **Steam ID64** and the **Steam Profile URL**,
  converted from whatever form the server reports in `status`; bots and LAN players have no
  Steam account, so those entries are greyed out.
- Players: right-click → **Open Steam Profile** opens the player's Steam community page in
  the default browser.
- Windows: the `.msi` is now a real setup wizard – welcome, license, **choice of the
  installation folder** (updates offer the previous folder), progress and a **finish page**
  with "Launch openHLSW" instead of closing silently.

### Changed
- Tahoma, the font of the original HLSW, on every system: Windows uses its own, Linux and
  macOS use Microsoft's Tahoma if installed and otherwise the bundled Tahoma clone of the
  Wine project (LGPL-2.1-or-later, shown under ? → License).
- Cleaner layout: text in input fields is vertically centred, list rows and headers are a
  little taller, more room above the menu bar and at the window edges.
- GitHub release notes contain only the changes of the released version instead of the
  whole changelog.

### Fixed
- Sharper, evenly spaced text: fonts use light hinting (horizontal strokes and baseline on
  whole pixels, letter widths as designed) instead of full hinting, which squeezed the
  letters, left uneven gaps ("Rem ove") and made the text look blurry.

## [v0.1.1] - 2026-10-19

### Fixed
- Windows: the taskbar and Alt+Tab showed a blank page instead of the openHLSW icon. The
  icon embedded in the .exe is now set on the window and its window class (also for the
  Trace Route window).

## [v0.1.0] - 2026-10-18

First release: a native HLSW 1.4 remake for Windows, Linux and macOS (Apple silicon),
written in Rust
(egui/eframe). One binary, no runtime.

### Look and feel
- Layout like HLSW 1.4: toolbar and server list on top, server info (bottom left), players
  (right), rules (bottom right), Rcon section with tabs at the bottom, status bar with
  online count and send/receive traffic.
- Skins White (default) and Black; starts maximized; title "openHLSW - v<version>" (e.g. "openHLSW - v0.1.0");
  program and window icon (orange crosshair with a server).
- Text size (Settings → Misc): Normal, Large, Extra large.
- Crisp text: font hinting with grid fitting, no sub-pixel glyph blur; Tahoma with fallback
  fonts, so player and server names in every script (symbols, CJK, Cyrillic, emoji …)
  display correctly (UTF-8, with Latin-1 fallback for old servers).
- Country flags and game icons from embedded image atlases that may have any resolution
  (RGB or RGBA flags, icons in any .ico size); game icons are stored at 32 px and cropped to
  their visible part, so they stay sharp at larger text sizes.
- Copy / paste / cut in every text field (Ctrl+C/V/X/A and a right-click menu with Cut,
  Copy, Paste, Select All).
- Every pane has a small margin, panel sizes are remembered.

### Menus and shortcuts
- File: Connect Log, open the AppData folder, Minimize (also "no traffic"), Exit.
- Server list: New, Open, Delete, Properties (rename), Import, Export, Multi Server Insert.
- Server: Connect, Copy, Delete, Move/Copy to server list, Find, Edit comment, Change ID,
  Reset Average Ping, Properties, Trace Route, Rcon Status Command.
- Settings: Settings dialog, Sort Server List (IP/Port, Game + Name, Game + IP/Port, …).
- View: Rcon Section, Server List, Rules, Password, Filter, Comment, Toolbar, Status Bar,
  Ping Graph, Skin.
- Shortcuts like HLSW: F5 (refresh all), F6 (refresh selected), Ctrl+G (connect),
  Ctrl+D / Del (delete), Ctrl+F (find), Ctrl+P (properties), Ctrl+T (trace route),
  Ctrl+I (multi insert), Ctrl+O (list properties), Ctrl+R / Ctrl+L / Ctrl+U (show Rcon
  section / server list / rules), Ctrl+M (minimize), + / - (change ID).

### Supported servers
- **Source** (SRCDS: CS:S, CS:GO, CS2, TF2, HL2:DM, DoD:S, GMod, L4D, L4D2, Insurgency,
  Synergy, ZPS, …) via Valve A2S (INFO, PLAYER, RULES) with challenge reuse and split
  packets; Source TCP RCON with multi-packet answers (also CS2).
- **Half-Life 1 / GoldSrc** (CS 1.6, Condition Zero, DoD, TFC, NS, TS, …): old `m` info
  format, GoldSrc split packets, UDP RCON (`challenge rcon`), `kick #userid`,
  `logaddress_add <ip> <port>`, GoldSrc `status`; "HL - …" game names and HL1 icons. RCON
  falls back to UDP automatically when the TCP RCON port is closed.
- **GameSpy 2** (e.g. Deer Hunter 2005): entered as `ip:gameport:queryport`; server info,
  players and rules (no RCON); Connect starts the game set in My Games.
- Game icons for all HLSW games (including Counter-Strike 2 and Deer Hunter 2005) and an own
  icon for GameSpy 2 servers (globe with a "2").

### Refresh and server state
- Auto refresh every **1 s** by default (Settings → Misc or the toolbar; minimum 0.4 s),
  F5 / F6 for manual refresh.
- Per-server query budget (2.5 queries/s, Settings → Misc) that respects srcds'
  `sv_max_queries_sec`; the interval cannot go below 1 / query limit; a lower limit the
  server announces in its rules (`sv_max_queries_sec` / HLDS `max_queries_sec` and window)
  is respected too; the A2S challenge is reused; the server info (ping) always comes first,
  rules are refreshed every 10 s, players of the selected server after that; the selected
  server has priority; after a timeout that server's query rate is halved and
  recovers slowly.
- A server that has not answered for **10 seconds** (at start or when it goes down) is
  offline: the whole line turns red and a notification sound plays.
- The last known name, map, game, players and other infos of every server are saved, so
  offline servers still show which server they were after a restart.

### Server list
- Several server lists; add servers in the "IP/Port" box as `IP:port`, host name, `[IPv6]:port`,
  `steam://connect/…` or `ip:port:queryport`, or many at once with Multi Server Insert;
  import / export lists as JSON.
- Game box next to "IP/Port" (Half-Life 2, Counter-Strike 2, Half-Life, GameSpy 2, with
  icons): type for new servers; shows the type of the selected server.
- Columns: country flag + # (yellow star = RCON password stored), Addr, Name with game icon,
  Game, Map, Players (humans, bots, max), Ping (current + average), Version, Location
  (provider), AddOn, Anti Cheat Tool, Ranking, Limits, Comment (editable by double-click).
- Server and player locations (country flag, city, provider) from ip-api.com, cached in
  `geo_cache.json`; local addresses show "LAN"; can be turned off.
- AddOn detection from the server rules: Source (MM:S, SMod, Stripper, Mani, ES) and
  Half-Life 1 (AMXX, AMX, ATAC, PTB, Admin Mod, dproto, MM – e.g.
  "AMXX v1.8.2, ATAC v3.0.1, PTB v1.8b3, MM v1.21.1-am"); Anti Cheat Tool shows VAC plus
  HLGuard, sXe Injected and SMAC; Ranking shows HLstatsX:CE; Limits shows time / round /
  win / frag limits.
- Search field (name, map, address); keep the list order while the mouse is over it
  (option).
- Click a header to sort; double-click a header or a column separator to fit the column to
  the longest value of **all** rows (also those scrolled out of view).
- Right-click a header: show/hide columns and **Reset column width** (server and players
  lists); hidden columns and widths are remembered.
- Right-click a server like HLSW: Connect, Copy (address, name, both, steam:// link),
  Delete, Move/Copy to server list, **Change ID** (Increase +, Decrease -, Set…, Set all
  server IDs to current sequence), Reset Average Ping, Properties (address, comment, RCON
  password), Trace Route, Rcon Status Command.
- Connect: starts the game from My Games (`+connect`, `+password`) or `steam://connect`,
  with the join password from the toolbar; logged in the Connect Log; optional actions on
  connect (minimize, minimize without traffic, start other applications); double-click to
  connect (option).

### Server details
- Server info: title, ping graph, address, ping (current and average), location, provider,
  game (folder, AppID), players, map (and next map), limits, friendly fire, server type and
  OS, version, add-ons, anti-cheat, password, tags, Steam ID, SourceTV, last seen, last
  connected, comment.
- HLSW style **ping graph** that grows with the panel, drawn in 3D: sunken frame, glossy
  hatched gray background, one shaded bar per query with a shadow – green for normal,
  orange for raised, red for high ping – and timeouts as full-height red bars; about two
  minutes of history. Numbers in the graph: scale lines, the average (dashed line), labelled
  peaks, the current ping and the value under the mouse. The scale follows the typical ping
  (90th percentile), so a single extreme ping or timeout does not squash the other bars.
- **Players** with filter (name, Steam ID, IP) and all columns of HLSW's players list
  (right-click the header): Country, #, Name, Score, Frags, Time; from RCON `status`: ID,
  Global ID (Steam ID), Ping, Loss, Rate, Slot, Addr, Bot; from the address: Country Name,
  Provider; from the server log: Kills, Deaths, Team, Statistics (Frags, Death,
  Efficiency), Teamkills, Suicides, Damage (HP, Armor) and Health (HP, Armor) – counted
  since the map started. Columns without data source (Skill, Global ID Database, Honor,
  QPort, Model, Extra) are listed greyed out like in HLSW. Right-click a player: Copy (Name, Steam ID, Addr,
  Whole Table), Rcon Status Command, Kick, Kick & Connect, Ban ID, Ban ID Kick, Ban IP,
  Ban IP Kick (permanent, 5 min, 30 min, 1 h, 1 day, 1 week).
- **Rules / cvars** with filter. Right-click: Copy (Key, Value, Both, All Rules, All Keys),
  Change Value… (sets the cvar via RCON).

### RCON
- **Rcon Config**: password per server (Hide, Save), Test, Disconnect, status, Properties;
  Receive Server Log (Get Log Internet / LAN via `logaddress_add` + `log on`, retrieve the
  log again on map change; public IP looked up automatically); **Auto Update Status**
  (periodic `status` for player details and automatic log forwarding).
- **Console** like HLSW: one selectable text area (scroll, mouse selection, Ctrl+A,
  Ctrl+C, right-click), time stamps, coloured commands and answers, switches C (translate
  log), * (hide RCON password), F (hide answers), D (date), E (empty lines), A (hide
  automatic commands), S (auto scroll), X (clear); command history (Up/Down and drop-down),
  Tab completion / Complete; cache size setting; logs can be saved to disk.
- **Chat** from the server log (team chat marked), say from openHLSW.
- **Ban List** (`listid` / `listip`): ban by Steam ID or IP with duration, unban.
- **Change Level**: map list from `maps *`, `listmaps` and the game's stock maps (maps inside
  VPK files are not reported by `maps *`), Update, Change Level, kick all players on map
  change (option).

### Window and layout
- Window size, position and the maximized state are remembered (the first start is
  maximized); the sizes of all panes (server list, info, players, rules, Rcon section) and the
  column widths of all lists are kept as well. While the window is minimized nothing is laid
  out, so minimizing no longer shrinks the panes.

### License
- openHLSW is licensed under the GNU General Public License v3.0 or later (`LICENSE`).
- The game icons are not covered by it and stay the property of their owners
  (`assets/games/NOTICE.txt`); the flags are public domain.
- **? → License** shows the license, the game icon notice and the third-party notes; the
  About window shows copyright and license. All packages include `LICENSE` and the game icon
  notice.

### Tools
- **Trace Route window** like HLSW: per hop Ping, Ø Ping, Diff, IP, Reverse DNS, Provider
  (flag, LAN) and a ping graph, updated every second; results appear immediately; route
  changes are marked black ("Different Route"), timeouts red; Start/Stop for any address.
  Windows probes every hop with TTL limited ICMP echoes (Windows ICMP API, no admin rights);
  Linux uses `traceroute` / `tracepath` + `ping`.
- **Settings** like HLSW: My Games (all HLSW games with icons and own executables), Startup
  (selected server at start), Connecting, Rcon (cache size, save logs, auto complete, map
  change options), Connections (log port, public IP), Plugins, Sounds (per event, can be
  disabled), Misc (auto refresh, interval, query limit, list order, locations, skin, text
  size, connect on double-click).
- Settings, server lists, comments and (only when "Save" is ticked) RCON passwords are
  stored in `%APPDATA%\openHLSW\config.json` (Linux: `~/.config/openHLSW/config.json`).
- `openhlsw-cli`: query Source, GoldSrc and GameSpy 2 servers and run RCON commands from
  the command line.
- GitHub Actions build (`.github/workflows/build.yml`),
  run for every `v*` tag: Linux `.tar.gz`, `.deb`, `.rpm` and Windows `.zip`, `.msi` for
  x86, x86_64 and arm64, macOS `.dmg` and `.tar.gz` for Apple silicon (app bundle with icon,
  ad-hoc signed); a GitHub release with all packages; Linux desktop entry and icon.
- macOS: system fonts (Tahoma, Arial Unicode, PingFang, …) and Trace Route with the
  system `traceroute` / `ping`.
- `tools/fake_srcds.py` test server: Source, `--goldsrc` (HLDS), `--gamespy2 <queryport>`,
  `--latency <ms>`, rate limiting, RCON, log and chat simulation.
