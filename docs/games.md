# Adding game servers: ports, RCON and server log

openHLSW asks a server for its name, map, players and rules with a **query**. Most games
answer the query on the **game port** (the port players connect to). Some games answer on a
separate **query port**, and a few only over TCP or HTTP. This page says, for every game in
the game list, which ports openHLSW uses and what the server must allow.

## 1. Add a server

Type the address in the field at the top and press Enter (or **Add**):

| You type | Meaning |
|---|---|
| `203.0.113.10:27015` | IP and game port |
| `play.example.org:27015` | host name and game port |
| `203.0.113.10` | only the host: the game's usual port is used (only with a game chosen in the list) |
| `203.0.113.10:16567:29900` | IP, game port **and query port** (games with a separate query port) |
| `steam://connect/203.0.113.10:27015` | Steam connect link |

The game dropdown next to the field:

- **Auto detect** (default): openHLSW tries Valve A2S, GameSpy 2 and GameSpy 1 by itself.
  Source, Half-Life 1 and Counter-Strike 2 servers are always found this way.
- **Not detectable games**: these cannot be recognised from the address alone. Choose the
  game *before* you press Add; openHLSW then fills in the usual game port and query port.
- **All games**: choosing a game also fills in its usual ports, even when it could be
  detected.

When you choose a game and leave out the port, its usual port is used. When the game has
its own query port and you do not give one, openHLSW adds it (fixed port such as 29900 for
Battlefield 2, or game port + offset such as + 19238 for ARK). You can always override it
with `ip:gameport:queryport`.

## 2. Ports and the firewall of the game server

openHLSW only **sends** queries; it needs nothing opened on your side for the server list.
The **game server** must let the query in:

- **UDP** for most protocols (column *Transport* below).
- **TCP** for Frostbite (Battlefield Bad Company 2 / 3 / 4 / Hardline), TeamSpeak 2/3,
  Minecraft Java, Open Tibia, StarMade, Shockvoice and the HTTP ones (Terraria/TShock, Eco,
  FiveM/RedM player list).
- A game with a separate query port needs **that** port open, not only the game port.

Example **Battlefield 2**: players connect on UDP 16567, openHLSW asks on UDP 29900
(GameSpy 3). Both have to be reachable. Add it as `ip:16567:29900`, or choose
*Battlefield 2* in the list and type `ip:16567`. openHLSW has no RCON for Battlefield 2.

Many hosts limit how often one address may ask; openHLSW slows down by itself when a
server answers with ICMP "port unreachable" (README, *Good to know*, *Query limit*).

## 3. RCON (remote console)

RCON in openHLSW is **Valve RCON only**:

- **Source / Counter-Strike 2**: Source RCON over **TCP** on the **game port**.
- **Half-Life 1 (GoldSrc)**: RCON over **UDP** on the game port.
- Servers added with a separate query port (`ip:gameport:queryport`) and all non-Valve
  protocols have **no RCON** in openHLSW.

On the game server set an RCON password, e.g. in `server.cfg`:

```
rcon_password "choose-a-long-password"
```

For Source servers the TCP game port must be open as well as the UDP one.
openHLSW only logs in when you click **Test** (or press Enter) in *Rcon Config*, because
servers ban addresses after a few failed logins.

Other games that answer Valve A2S on the game port ("maybe" in the table) are treated
like Source servers: openHLSW tries Source RCON on the game port. That works only if the
game's own RCON listens there and speaks Source RCON; check the game's server
documentation.

## 4. Server log (Console and Chat tabs)

Only for servers with RCON. **Get Log (LAN / Internet)** sends `logaddress_add <your IP> <port>`
and `log on` over RCON; the server then sends its log to openHLSW:

- **Half-Life 1 and Source**: over **UDP** to openHLSW's log port.
- **Counter-Strike 2**: only over **HTTP**; openHLSW sends
  `logaddress_add_http "http://<your IP>:<port>/log/<secret>/<n>"` and receives on the same
  port number over **TCP**.

On **your** side (where openHLSW runs) the log port must be reachable:

- Set a fixed port: *Settings → Connections → Fixed UDP port* (e.g. 27500).
- Allow it in the local firewall, e.g. `sudo ufw allow 27500/udp` (CS2: also `27500/tcp`).
- Over the Internet: openHLSW forwards the port with UPnP if the router allows it;
  otherwise forward UDP (CS2: UDP and TCP) by hand. Behind CGNAT / DS-Lite no forwarding
  helps.
- If no log line arrives within 15 s, openHLSW says what is missing.

## 5. Notes per protocol

| Protocol | Games (examples) | What the server needs |
|---|---|---|
| Valve A2S | Counter-Strike, TF2, Rust, ARK, Squad … | UDP query on the game port, or game port + offset (ARK, Squad, Arma 3 …: see table) |
| GameSpy 2/3 | Battlefield 2 / 2142 / Vietnam, ArmA, Crysis, F.E.A.R., SWAT 4 … | separate UDP query port; enter it if the game has no usual one |
| GameSpy 1 | Unreal Tournament, Battlefield 1942, Halo, Vietcong … | UDP; openHLSW tries game port + 1, + 10 and the game port (BF1942: 23000) |
| Unreal 2 | UT2003/2004, Killing Floor, Red Orchestra … | UDP game port + 1 (GameSpy 1 on + 10 as fallback) |
| Quake 1/2/3, Doom 3, MoHAA | Call of Duty 1–5, Quake, ET, Urban Terror, Doom 3, Medal of Honor … | UDP on the game port (MoHAA: also GameSpy 1 on + 97) |
| All-Seeing Eye | Far Cry, Multi Theft Auto | UDP game port + 123 |
| Frostbite | Battlefield Bad Company 2, 3, 4, Hardline | **TCP** admin port: BC2 48888, BF3 / BF4 / Hardline game port + 22000 or 47200; the query needs no login |
| TeamSpeak 2 / 3 | TeamSpeak | **TCP** query port 51234 (TS2) / 10011 (TS3, ServerQuery); the address holds the voice port |
| Mumble | Mumble | UDP ping on the voice port; user list only with the channel viewer on TCP 27800 |
| Minecraft Java | Minecraft | **TCP** server list ping on the game port (25565) |
| Minecraft Bedrock | Minecraft: Bedrock Edition | UDP RakNet ping on 19132 |
| CitizenFX | FiveM, RedM | UDP and **TCP** on 30120 (`/players.json`) |
| HTTP | Terraria (TShock), Eco, Stationeers | TShock: its REST API (`/v2/server/status`) on game port + 101 or 7878; Eco: web server on game port + 1; Stationeers: the server must be in the game's public server list |
| others | SA-MP, Teeworlds, OpenTTD, CS2D, Tribes 2, Savage, Ventrilo, Mafia mods, Tibia, StarMade | see table; Mafia mods: UDP game port + 1; Tibia / StarMade: TCP |
| Shockvoice | Shockvoice | no query: openHLSW only checks that the TCP port answers |

## 6. All games

*Query port tried*: what openHLSW asks when the address has no query port, in this order.
*Auto detect* "no": choose the game in the list before adding.
*RCON*: "yes" = Valve game, RCON on the game port; "maybe" = see *RCON* above;
"no (own query port)" = added as `ip:gameport:queryport`, no RCON in openHLSW.

<!-- games:begin (tools/gen-games-doc.py) -->
| Game | Query | Game port (default) | Query port tried | Transport | Auto detect | RCON in openHLSW |
|---|---|---|---|---|---|---|
| 7 Days to Die | Valve A2S | 26900 | game port | UDP | yes | maybe, see RCON |
| Alien vs. Predator 2 | GameSpy 1 | (enter it) | game port, then game port + 1, + 10, game port | UDP | yes | no |
| America's Army 3 | Valve A2S | 8777 | game port + 18243 | UDP | no, choose it in the list | no (own query port) |
| America's Army: Operations | GameSpy 2/3 | (enter it) | **enter it**: `ip:gameport:queryport` | UDP | yes | no |
| America's Army: Proving Grounds | Valve A2S | 8777 | game port + 18243 | UDP | no, choose it in the list | no (own query port) |
| ARK: Survival Evolved | Valve A2S | 7777 | game port + 19238 | UDP | no, choose it in the list | no (own query port) |
| ARMA 2 | GameSpy 2/3 | (enter it) | 2303 | UDP | yes | no |
| ARMA 2: Operation Arrowhead | GameSpy 2/3 | (enter it) | 2303 | UDP | yes | no |
| Arma 3 | Valve A2S | 2302 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Armed Assault | GameSpy 2/3 | (enter it) | 2303 | UDP | no, choose it in the list | no |
| Atlas | Valve A2S | 5761 | game port + 51800 | UDP | no, choose it in the list | no (own query port) |
| Avorion | Valve A2S | 27000 | game port | UDP | yes | maybe, see RCON |
| Barotrauma | Valve A2S | 27015 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Battalion 1944 | Valve A2S | 7777 | game port + 3 | UDP | no, choose it in the list | no (own query port) |
| Battlefield 1942 | GameSpy 1 | 14567 | 23000, then game port + 1, + 10, game port | UDP | yes | no |
| Battlefield 2 | GameSpy 2/3 | 16567 | 29900 | UDP | yes | no |
| Battlefield 2142 | GameSpy 2/3 | 17567 | 29900 | UDP | yes | no |
| Battlefield 3 | Frostbite admin port | 25200 | 48888, game port + 22000, 47200, game port | TCP | no, choose it in the list | no |
| Battlefield 4 | Frostbite admin port | 25200 | 48888, game port + 22000, 47200, game port | TCP | no, choose it in the list | no |
| Battlefield Hardline | Frostbite admin port | 25200 | 48888, game port + 22000, 47200, game port | TCP | no, choose it in the list | no |
| Battlefield Vietnam | GameSpy 2/3 | (enter it) | 22000 | UDP | yes | no |
| Battlefield: Bad Company 2 | Frostbite admin port | 19567 | 48888, game port + 22000, 47200, game port | TCP | no, choose it in the list | no |
| Black Mesa | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Brink | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| C&C Renegade | GameSpy 1 | (enter it) | 25300, then game port + 1, + 10, game port | UDP | yes | no |
| Call of Duty | Quake 3 | 28960 | game port | UDP | yes | no |
| Call of Duty 2 | Quake 3 | 28960 | game port | UDP | yes | no |
| Call of Duty 4 | Quake 3 | 28960 | game port | UDP | yes | no |
| Call of Duty: Modern Warfare 2 | Quake 3 | 28960 | game port | UDP | no, choose it in the list | no |
| Call of Duty: Modern Warfare 3 | Valve A2S | 27015 | game port + 2 | UDP | no, choose it in the list | no (own query port) |
| Call of Duty: United Offensive | Quake 3 | 28960 | game port | UDP | yes | no |
| Call of Duty: World at War | Quake 3 | 28960 | game port | UDP | yes | no |
| Challenge Quake 3 | Quake 3 | (enter it) | game port | UDP | yes | no |
| Citadel: Forged with Fire | Valve A2S | 7777 | game port | UDP | yes | maybe, see RCON |
| Conan Exiles | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Contagion | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Counter-Strike 1.5 | Valve A2S | 27015 | game port | UDP | yes | yes |
| Counter-Strike 1.6 | Valve A2S | 27015 | game port | UDP | yes | yes |
| Counter-Strike 2 | Valve A2S | 27015 | game port | UDP | yes | yes |
| Counter-Strike 2D | CS2D | 36963 | game port | UDP | no, choose it in the list | no |
| Counter-Strike: Condition Zero | Valve A2S | 27015 | game port | UDP | yes | yes |
| Counter-Strike: Global Offensive | Valve A2S | 27015 | game port | UDP | yes | yes |
| Counter-Strike: Source | Valve A2S | 27015 | game port | UDP | yes | yes |
| Crysis | GameSpy 2/3 | (enter it) | **enter it**: `ip:gameport:queryport` | UDP | no, choose it in the list | no |
| Crysis 2 | GameSpy 2/3 | 64087 | **enter it**: `ip:gameport:queryport` | UDP | yes | no |
| Crysis Wars | GameSpy 2/3 | (enter it) | **enter it**: `ip:gameport:queryport` | UDP | no, choose it in the list | no |
| Dark and Light | Valve A2S | 7777 | game port + 19238 | UDP | no, choose it in the list | no (own query port) |
| Dark Messiah of Might and Magic | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Day of Defeat | Valve A2S | 27015 | game port | UDP | yes | yes |
| Day of Defeat: Source | Valve A2S | 27015 | game port | UDP | yes | yes |
| Days of War | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| DayZ | Valve A2S | 2303 | game port | UDP | yes | maybe, see RCON |
| DayZ Mod | Valve A2S | 2302 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Devastation | Unreal 2 (fallback GameSpy 1) | (enter it) | game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port | UDP | yes | no |
| Doom 3 | Doom 3 | 27666 | game port | UDP | yes | no |
| Eco | Eco web server | 3000 | game port + 1, game port | TCP (HTTP) | no, choose it in the list | no |
| Elite Force | Quake 3 | (enter it) | game port | UDP | yes | no |
| Elite Force II | Quake 3 | (enter it) | game port | UDP | yes | no |
| Empyrion - Galactic Survival | Valve A2S | 30000 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Enemy Territory: Quake Wars | Doom 3 | 27733 | game port, 27733 | UDP | yes | no |
| F.E.A.R. | GameSpy 2/3 | (enter it) | 27888 | UDP | yes | no |
| F.E.A.R. Perseus Mandate | GameSpy 2/3 | (enter it) | 27890 | UDP | yes | no |
| Far Cry | All-Seeing Eye | 49001 | game port + 123, game port | UDP | yes | no |
| Fistful of Frags | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| FiveM (GTA V) | CitizenFX | 30120 | game port (UDP getinfo, TCP /players.json) | UDP + TCP (HTTP) | no, choose it in the list | no |
| Fortress Forever | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Frontlines-Fuel of War | Frontlines (A2S variant) | 5476 | game port + 2, game port | UDP | yes | no |
| Garry's Mod | Valve A2S | 27015 | game port | UDP | yes | yes |
| Ghost Recon Advanced Warfighter 2 | GameSpy 2/3 | (enter it) | **enter it**: `ip:gameport:queryport` | UDP | no, choose it in the list | no |
| GRAV Online | Valve A2S | 7777 | game port | UDP | yes | maybe, see RCON |
| Half-Life | Valve A2S | 27015 | game port | UDP | yes | yes |
| Half-Life (WON) | Valve A2S | 27015 | game port | UDP | yes | yes |
| Half-Life 2 | Valve A2S | 27015 | game port | UDP | yes | yes |
| Half-Life 2: Deathmatch | Valve A2S | 27015 | game port | UDP | yes | yes |
| HALO | GameSpy 1 | 2302 | game port + 1, + 10, game port | UDP | yes | no |
| Hell Let Loose | Valve A2S | 27015 | game port + 15 | UDP | no, choose it in the list | no (own query port) |
| Hidden & Dangerous 2 | GameSpy 2/3 | 11001 | game port + 3 | UDP | yes | no |
| Hurtworld | Valve A2S | 12871 | game port | UDP | yes | maybe, see RCON |
| Insurgency | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Insurgency: Sandstorm | Valve A2S | 27102 | game port + 29 | UDP | no, choose it in the list | no (own query port) |
| ioQuake3 | Quake 3 | 27960 | game port | UDP | yes | no |
| Jedi Knight 2 | Quake 3 | (enter it) | game port | UDP | yes | no |
| Jedi Knight 3 | Quake 3 | (enter it) | game port | UDP | yes | no |
| Just Cause 2: Multiplayer | GameSpy 2/3 | 7777 | **enter it**: `ip:gameport:queryport` | UDP | yes | no |
| Just Cause 3 | Valve A2S | 4200 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Killing Floor | Unreal 2 (fallback GameSpy 1) | 7707 | game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port | UDP | yes | no |
| Killing Floor 2 | Valve A2S | 7777 | game port + 19238 | UDP | no, choose it in the list | no (own query port) |
| Kingpin: Life of Crime | Quake 1/2 | 31510 | game port | UDP | no, choose it in the list | no |
| Land Of The Dead: Road to Fiddler's Green | Unreal 2 (fallback GameSpy 1) | (enter it) | game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port | UDP | yes | no |
| Left 4 Dead | Valve A2S | 27015 | game port | UDP | yes | yes |
| Left 4 Dead 2 | Valve A2S | 27015 | game port | UDP | yes | yes |
| Life is Feudal | Valve A2S | 28000 | game port + 2 | UDP | no, choose it in the list | no (own query port) |
| Lost Heaven Multiplayer (Mafia) | LHMP | 27015 | game port + 1, game port | UDP | no, choose it in the list | no |
| Mafia 2 Multiplayer | M2MP | 27015 | game port + 1, game port | UDP | no, choose it in the list | no |
| Medal of Honor Airborne | Medal of Honor | (enter it) | game port, game port + 97 (GameSpy 1) | UDP | yes | no |
| Medal of Honor Allied Assault | Medal of Honor | 12203 | game port, game port + 97 (GameSpy 1) | UDP | yes | no |
| Medal of Honor Pacific Assault | Medal of Honor | (enter it) | game port, game port + 97 (GameSpy 1) | UDP | yes | no |
| Medal of Honor: Allied Assault Breakthrough | Medal of Honor | (enter it) | game port, game port + 97 (GameSpy 1) | UDP | yes | no |
| Medal of Honor: Allied Assault Spearhead | Medal of Honor | (enter it) | game port, game port + 97 (GameSpy 1) | UDP | yes | no |
| Minecraft | Server list ping | 25565 | game port | TCP | no, choose it in the list | no |
| Minecraft: Bedrock Edition | RakNet ping | 19132 | game port | UDP | no, choose it in the list | no |
| Miscreated | Valve A2S | 64090 | game port + 2 | UDP | no, choose it in the list | no (own query port) |
| Modiverse | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| MORDHAU | Valve A2S | 7777 | game port + 19238 | UDP | no, choose it in the list | no (own query port) |
| Multi Theft Auto | All-Seeing Eye | 22003 | game port + 123, game port | UDP | yes | no |
| Multi Theft Auto: San Andreas | All-Seeing Eye | 22003 | game port + 123, game port | UDP | yes | no |
| Mumble | Mumble ping | 64738 | voice port; user list from the channel viewer on TCP 27800, if the server has one | UDP (+ TCP 27800) | no, choose it in the list | no |
| Natural Selection 2 | Valve A2S | 27015 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Neverwinter Nights | GameSpy 2/3 | (enter it) | 5121 | UDP | yes | no |
| Nexuiz | Quake 3 | (enter it) | game port | UDP | yes | no |
| No More Room in Hell | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Open Fortress | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| OpenTTD | OpenTTD | 3979 | game port | UDP | no, choose it in the list | no |
| Operation Flashpoint | GameSpy 1 | (enter it) | game port + 1, + 10, game port | UDP | yes | no |
| Operation Flashpoint Resistance | GameSpy 1 | (enter it) | game port + 1, + 10, game port | UDP | yes | no |
| Painkiller | GameSpy 2/3 | (enter it) | 3455 | UDP | yes | no |
| Pariah | Unreal 2 (fallback GameSpy 1) | (enter it) | game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port | UDP | yes | no |
| PixARK | Valve A2S | 27015 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Post Scriptum | Valve A2S | 10027 | game port + 10 | UDP | no, choose it in the list | no (own query port) |
| Postal 2 | GameSpy 1 | (enter it) | game port + 1, + 10, game port | UDP | yes | no |
| Prey | Doom 3 | 27719 | game port | UDP | yes | no |
| Project IGI2: Covert Strike | GameSpy 1 | (enter it) | game port + 1, + 10, game port | UDP | yes | no |
| Project Reality: BF2 | GameSpy 2/3 | 16567 | 29900 | UDP | yes | no |
| Project Zomboid | Valve A2S | 16261 | game port | UDP | yes | maybe, see RCON |
| Q3 Compatible | Quake 3 | (enter it) | game port | UDP | yes | no |
| Quake 1 | Quake 1/2 | 26000 | game port | UDP | yes | no |
| Quake 2 | Quake 1/2 | 27910 | game port | UDP | yes | no |
| Quake 3 Arena | Quake 3 | 27960 | game port | UDP | yes | no |
| Quake 4 | Doom 3 | 28004 | game port | UDP | yes | no |
| Quake Live | Valve A2S | 27960 | game port | UDP | yes | maybe, see RCON |
| Red Orchestra 2: Heroes of Stalingrad | Valve A2S | 27015 | game port + 19238 | UDP | no, choose it in the list | no (own query port) |
| Red Orchestra: Ostfront 41-45 | Unreal 2 (fallback GameSpy 1) | (enter it) | game port + 10, then game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port | UDP | yes | no |
| RedM (Red Dead Redemption 2) | CitizenFX | 30120 | game port (UDP getinfo, TCP /players.json) | UDP + TCP (HTTP) | no, choose it in the list | no |
| Return to Castle Wolfenstein | Quake 3 | (enter it) | game port | UDP | yes | no |
| rFactor | Valve A2S | 34297 | game port | UDP | yes | maybe, see RCON |
| rFactor 2 | Valve A2S | 64297 | game port + 2 | UDP | no, choose it in the list | no (own query port) |
| Rising Storm 2: Vietnam | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Rune | GameSpy 1 | (enter it) | game port + 1, + 10, game port | UDP | yes | no |
| Rust | Valve A2S | 28015 | game port | UDP | yes | maybe, see RCON |
| S.T.A.L.K.E.R. - Shadow of Chernobyl | GameSpy 2/3 | (enter it) | 5447 | UDP | yes | no |
| San Andreas Multiplayer (SA-MP) | SA-MP / open.mp | 7777 | game port | UDP | no, choose it in the list | no |
| Savage: The Battle For Newerth | Savage | 11235 | game port | UDP | yes | no |
| Serious Sam: The First Encounter | GameSpy 1 | (enter it) | game port + 1, + 10, game port | UDP | yes | no |
| Serious Sam: The Second Encounter | GameSpy 1 | (enter it) | game port + 1, + 10, game port | UDP | yes | no |
| Shockvoice (v0.9.5+) | reachability only | 5150 | game port (TCP connect only) | TCP | yes | no |
| Sniper Elite PC | GameSpy 2/3 | (enter it) | 6500 | UDP | no, choose it in the list | no |
| Soldat | All-Seeing Eye | 23073 | game port + 123, game port | UDP | no, choose it in the list | no |
| Soldier of Fortune 2 | Quake 3 | (enter it) | game port | UDP | yes | no |
| Space Engineers | Valve A2S | 27016 | game port | UDP | yes | maybe, see RCON |
| Squad | Valve A2S | 7787 | game port + 19378 | UDP | no, choose it in the list | no (own query port) |
| Star Wars: Battlefront | GameSpy 2/3 | (enter it) | 3658 | UDP | yes | no |
| StarMade | StarMade | 4242 | game port | TCP | no, choose it in the list | no |
| Stationeers | Stationeers public server list | 27500 | none: the server is looked up in the game's public server list | TCP (HTTP) | no, choose it in the list | no |
| Stormworks | Valve A2S | 25564 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Sven Co-op | Valve A2S | 27015 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| SWAT 4 | GameSpy 2/3 | 10480 | 10481 | UDP | yes | no |
| SWAT 4: The Stetchkov Syndicate | GameSpy 2/3 | 10480 | 10481 | UDP | yes | no |
| Team Fortress 2 | Valve A2S | 27015 | game port | UDP | yes | yes |
| TeamSpeak 2 | TeamSpeak 2 query | 8767 | 51234 | TCP | no, choose it in the list | no |
| TeamSpeak 3 | TeamSpeak 3 ServerQuery | 9987 | 10011 | TCP | no, choose it in the list | no |
| Teeworlds | Teeworlds / DDNet | 8303 | game port | UDP | no, choose it in the list | no |
| Terraria (TShock) | TShock REST API | 7777 | game port + 101, 7878, game port | TCP (HTTP) | no, choose it in the list | no |
| The Forest | Valve A2S | 27015 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| The Ship | Valve A2S | 27015 | game port | UDP | yes | maybe, see RCON |
| Tibia (Open Tibia) | Open Tibia | 7171 | game port | TCP | no, choose it in the list | no |
| Tremulous | Quake 3 | (enter it) | game port | UDP | yes | no |
| Tribes 2 | Tribes 2 | 28000 | game port | UDP | yes | no |
| Tribes Vengeance | GameSpy 2/3 | (enter it) | 7778 | UDP | yes | no |
| Unreal Tournament | GameSpy 1 | 7777 | game port + 1, + 10, game port | UDP | yes | no |
| Unreal Tournament 2003 | Unreal 2 (fallback GameSpy 1) | 7777 | game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port | UDP | yes | no |
| Unreal Tournament 2004 | Unreal 2 (fallback GameSpy 1) | 7777 | game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port | UDP | yes | no |
| Unreal Tournament 3 | GameSpy 2/3 | 7777 | 7778 | UDP | yes | no |
| Unturned | Valve A2S | 27015 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Urban Terror | Quake 3 | 27960 | game port | UDP | yes | no |
| V Rising | Valve A2S | 9876 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Valheim | Valve A2S | 2456 | game port + 1 | UDP | no, choose it in the list | no (own query port) |
| Ventrilo (v2.3.0+) | Ventrilo status | 3784 | game port | UDP | yes | no |
| Vietcong | GameSpy 1 | (enter it) | game port + 10000, then game port + 1, + 10, game port | UDP | no, choose it in the list | no |
| Warsow | Quake 3 | 44400 | game port | UDP | yes | no |
| Wolfenstein - Enemy Territory | Quake 3 | 27960 | game port | UDP | yes | no |
| Wolfenstein 2009 | Doom 3 | (enter it) | game port | UDP | yes | no |
| Wurm Unlimited | Valve A2S | 3724 | game port | UDP | yes | maybe, see RCON |
<!-- games:end -->

