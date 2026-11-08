#!/usr/bin/env python3
"""Rewrites the game table in docs/games.md from src/games.rs (run from the repo root:
python3 tools/gen-games-doc.py). The query ports follow src/query/mod.rs (candidates)."""
import re,sys
s=open('src/games.rs',encoding='utf-8').read()
rows=[]
for b in re.findall(r'Game \{(.*?)\n    \}',s,re.S):
    d=dict(re.findall(r'(\w+): ([^,\n]+),',b))
    if 'key' not in d or d['key'].startswith('&'): continue
    rows.append(dict(key=d['key'].strip('"'),name=d['name'].strip('"'),proto=d['protocol'].split('::')[1],
      nd=d['not_detectable']=='true',q=int(d['default_query_port']),diff=int(d['port_diff']),gp=int(d['default_port'])))
def off(n): return f"game port + {n}"
# protocol -> (display, transport, query rule when game has none, what the server needs)
P={
 'A2s':("Valve A2S","UDP",None),
 'GameSpy2':("GameSpy 2/3","UDP",None),
 'GameSpy1':("GameSpy 1","UDP","game port + 1, + 10, game port"),
 'Unreal2':("Unreal 2 (fallback GameSpy 1)","UDP","game port + 1 (Unreal 2), + 10 / + 1 (GameSpy 1), game port"),
 'Quake3':("Quake 3","UDP","game port"),
 'MoHAA':("Medal of Honor","UDP","game port, game port + 97 (GameSpy 1)"),
 'Quake':("Quake 1/2","UDP","game port"),
 'Doom3':("Doom 3","UDP","game port"),
 'Ase':("All-Seeing Eye","UDP","game port + 123, game port"),
 'Frostbite':("Frostbite admin port","TCP","48888, game port + 22000, 47200, game port"),
 'TeamSpeak2':("TeamSpeak 2 query","TCP","51234"),
 'TeamSpeak3':("TeamSpeak 3 ServerQuery","TCP","10011"),
 'Ventrilo':("Ventrilo status","UDP","game port"),
 'Tribes2':("Tribes 2","UDP","game port"),
 'Savage':("Savage","UDP","game port"),
 'Ffow':("Frontlines (A2S variant)","UDP","game port + 2, game port"),
 'Shockvoice':("reachability only","TCP","game port (TCP connect only)"),
 'Samp':("SA-MP / open.mp","UDP","game port"),
 'Teeworlds':("Teeworlds / DDNet","UDP","game port"),
 'Mumble':("Mumble ping","UDP (+ TCP 27800)","voice port; user list from the channel viewer on TCP 27800, if the server has one"),
 'OpenTtd':("OpenTTD","UDP","game port"),
 'Raknet':("RakNet ping","UDP","game port"),
 'Minecraft':("Server list ping","TCP","game port"),
 'Cfx':("CitizenFX","UDP + TCP (HTTP)","game port (UDP getinfo, TCP /players.json)"),
 'Cs2d':("CS2D","UDP","game port"),
 'Tibia':("Open Tibia","TCP","game port"),
 'StarMade':("StarMade","TCP","game port"),
 'Lhmp':("LHMP","UDP","game port + 1, game port"),
 'M2mp':("M2MP","UDP","game port + 1, game port"),
 'TShock':("TShock REST API","TCP (HTTP)","game port + 101, 7878, game port"),
 'Eco':("Eco web server","TCP (HTTP)","game port + 1, game port"),
 'Stationeers':("Stationeers public server list","TCP (HTTP)","none: the server is looked up in the game's public server list"),
}
SPECIAL={'BF1942':"23000",'CCR':"25300",'_Custom:Vietcong':"game port + 10000",'ROO':"game port + 10",'RO':"game port + 10",'AvP2':"game port"}
def qrule(g):
    pr=g['proto']
    if pr in('A2s','GameSpy2'):
        if g['q']: return str(g['q'])
        if g['diff']: return off(g['diff'])
        return "game port" if pr=='A2s' else "**enter it**: `ip:gameport:queryport`"
    r=P[pr][2]
    if g['key'] in SPECIAL: r=SPECIAL[g['key']]+", then "+r
    if g['key']=='ETQW': r+=", 27733"
    return r
VALVE={'CS15','CS16','CS2','CSCZ','CSGO','CSS','DoD','DoDS','GMod','HL','HLWON','HL2','HL2DM','L4D','L4D2','TF2'}
def rcon(g):
    if g['proto']!='A2s': return "no"
    if g['q'] or g['diff']: return "no (own query port)"
    if g['key'] in VALVE: return "yes"
    return "maybe, see RCON"
out=[]
out.append("| Game | Query | Game port (default) | Query port tried | Transport | Auto detect | RCON in openHLSW |")
out.append("|---|---|---|---|---|---|---|")
for g in rows:
    gp=str(g['gp']) if g['gp'] else ("27015" if g['proto']=='A2s' else "(enter it)")
    out.append(f"| {g['name']} | {P[g['proto']][0]} | {gp} | {qrule(g)} | {P[g['proto']][1]} | {'no, choose it in the list' if g['nd'] else 'yes'} | {rcon(g)} |")
BEGIN,END="<!-- games:begin (tools/gen-games-doc.py) -->","<!-- games:end -->"
doc=open('docs/games.md',encoding='utf-8').read()
a,b=doc.index(BEGIN)+len(BEGIN),doc.index(END)
doc=doc[:a]+"\n"+"\n".join(out)+"\n"+doc[b:]
open('docs/games.md','w',encoding='utf-8').write(doc)
print(f"{len(rows)} games",file=sys.stderr)
