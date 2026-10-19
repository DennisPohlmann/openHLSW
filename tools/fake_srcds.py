#!/usr/bin/env python3
"""Minimal fake SRCDS for testing openHLSW without a real game server.

Answers A2S_INFO / A2S_PLAYER / A2S_RULES on UDP (with challenges and split
packets, like current srcds builds) and speaks Source RCON on TCP.

    python3 tools/fake_srcds.py --port 27015 --password secret
"""
import argparse
import random
import socket
import struct
import threading
import time

CHALLENGE = struct.pack("<i", random.randint(1, 2**31 - 1))
START = time.time()


def cstr(s):
    return s.encode() + b"\0"


class Server:
    def __init__(self, args):
        self.args = args
        self.map = args.map
        self.players = [
            ("Aha*GunPower*", 12), ("ZaZa", 7), ("LEOXI", 3), ("IMM", 0),
            ("BEEFCAKE", 21), ("Prillan", 5), ("Мясорубка", 9),
            ("▄︻デ══━一", 2), ("<ĦЗλÐ§ħØŦ>", 1), ("★彡 Ｓａｍ 彡★", 4), ("김철수", 6), ("☠ ☢ ☣ ♛", 3),
        ][: args.players]
        self.joined = {name: START - random.randint(60, 4 * 3600) for name, _ in self.players}
        self.rules = [
            ("metamod_version", "1.12.0-dev+1225V"),
            ("sourcemod_version", "1.13.0.7427"),
            ("mp_timelimit", "20"),
            ("mp_friendlyfire", "0"),
            ("mp_maxrounds", "0"),
            ("sv_gravity", "800"),
            ("hlxce_version", "1.7.0"),
        ] + [(f"fake_cvar_{i:03d}", str(random.randint(0, 999))) for i in range(180)]
        self.ban_ids = {"STEAM_1:0:11111": "permanent", "STEAM_1:1:22222": "30.000 min"}
        self.ban_ips = {"10.11.12.13": "permanent"}
        self.maps = ["de_dust2", "de_inferno", "de_nuke", "cs_office", "cs_italy", "de_train", "aim_deagle_forever"]
        self.log_targets = set()
        self.query_times = {}
        self.stats = {"queries": 0, "dropped": 0}
        self.log_enabled = False
        self.log_sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)

    # ---------------------------------------------------------------- A2S ---
    def info(self):
        a = self.args
        if a.goldsrc:
            # obsolete GoldSrc 'm' answer (Condition Zero)
            p = b"m" + cstr(f"{a.host}:{a.port}") + cstr(a.name) + cstr(self.map) + cstr("czero")
            p += cstr("Condition Zero") + bytes([len(self.players), 32, 48, ord("D"), ord("L"),
                                                1 if a.sv_password else 0, 0, 1, 0])
            return p
        p = b"I" + bytes([17]) + cstr(a.name) + cstr(self.map) + cstr("cstrike")
        p += cstr("Counter-Strike: Source") + struct.pack("<H", 240)
        p += bytes([len(self.players), 32, 0, ord("d"), ord("l"), 1 if a.sv_password else 0, 1])
        p += cstr("6630498")
        p += bytes([0x80 | 0x10 | 0x20 | 0x01])
        p += struct.pack("<H", a.port) + struct.pack("<Q", 90000000000000001)
        p += cstr("alltalk,increased_maxplayers,startmoney")
        p += struct.pack("<Q", 240)
        return p

    def player_list(self):
        p = b"D" + bytes([len(self.players)])
        for i, (name, score) in enumerate(self.players):
            p += bytes([i]) + cstr(name) + struct.pack("<if", score, time.time() - self.joined[name])
        return p

    def rule_list(self):
        p = b"E" + struct.pack("<H", len(self.rules))
        for k, v in self.rules:
            p += cstr(k) + cstr(v)
        return p

    def rate_limited(self, ip):
        """Like srcds: drop queries above sv_max_queries_sec, averaged over sv_max_queries_window."""
        now = time.time()
        q = self.query_times.setdefault(ip, [])
        q.append(now)
        while q and q[0] < now - self.args.max_queries_window:
            q.pop(0)
        self.stats["queries"] += 1
        if len(q) > self.args.max_queries_sec * self.args.max_queries_window:
            self.stats["dropped"] += 1
            return True
        return False

    def report(self):
        while True:
            time.sleep(10)
            print(f"[{self.args.port}] last 10 s: {self.stats['queries'] / 10:.2f} queries/s, "
                  f"{self.stats['dropped']} dropped", flush=True)
            self.stats = {"queries": 0, "dropped": 0}

    def udp_loop(self):
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind((self.args.host, self.args.port))
        while True:
            data, addr = sock.recvfrom(4096)
            if data[:4] != b"\xff\xff\xff\xff" or len(data) < 5:
                continue
            if self.args.max_queries_sec > 0 and self.rate_limited(addr[0]):
                continue
            kind = data[4:5]
            if self.args.goldsrc and data[4:].startswith(b"challenge rcon"):
                sock.sendto(b"\xff\xff\xff\xffchallenge rcon 1234567890\n", addr)
                continue
            if self.args.goldsrc and data[4:].startswith(b"rcon "):
                self.goldsrc_rcon(sock, addr, data[9:].decode(errors="replace"))
                continue
            if kind == b"T":
                if data[25:29] != CHALLENGE:
                    self.send(sock, addr, b"A" + CHALLENGE)
                else:
                    self.send(sock, addr, self.info())
            elif kind in (b"U", b"V"):
                if data[5:9] != CHALLENGE:
                    self.send(sock, addr, b"A" + CHALLENGE)
                else:
                    self.send(sock, addr, self.player_list() if kind == b"U" else self.rule_list())

    def gamespy2_loop(self):
        """GameSpy 2 query port (Deer Hunter 2005 style)."""
        sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        sock.bind((self.args.host, self.args.gamespy2))
        while True:
            data, addr = sock.recvfrom(4096)
            if data[:3] != b"\xfe\xfd\x00" or len(data) < 10:
                continue
            if self.args.latency:
                time.sleep(random.uniform(0.3, 1.0) * self.args.latency / 1000)
            out = b"\x00" + data[3:7]
            if data[7] == 0xFF:
                info = [("hostname", self.args.name), ("gamename", "dh2005"), ("gamever", "1.2"),
                        ("mapname", "Whitetail Valley"), ("gametype", "Hunt"), ("numplayers", str(len(self.players))),
                        ("maxplayers", "16"), ("password", "0"), ("hostport", str(self.args.port))]
                out += b"".join(cstr(k) + cstr(v) for k, v in info) + b"\0"
            if data[8] == 0xFF:
                out += b"\x00" + bytes([len(self.players)]) + cstr("player_") + cstr("score_") + cstr("ping_") + b"\0"
                for name, score in self.players:
                    out += cstr(name) + cstr(str(score)) + cstr(str(random.randint(20, 90)))
            sock.sendto(out, addr)

    def goldsrc_rcon(self, sock, addr, text):
        # rcon <challenge> "<password>" <command>
        try:
            challenge, rest = text.split(" ", 1)
            _, password, command = rest.split('"', 2)
        except ValueError:
            return
        if challenge.strip() != "1234567890":
            out = "Bad challenge.\n"
        elif password != self.args.password:
            out = "Bad rcon_password.\n"
        else:
            out = self.command(command.strip())
        if not out:
            return  # HLDS sends nothing for commands without output
        raw = out.encode()
        for i in range(0, len(raw), 1200):  # several 'l' packets for long output
            sock.sendto(b"\xff\xff\xff\xffl" + raw[i:i + 1200] + b"\0", addr)

    def send(self, sock, addr, payload):
        if self.args.latency:
            time.sleep(random.uniform(0.3, 1.0) * self.args.latency / 1000)
        data = b"\xff\xff\xff\xff" + payload
        if len(data) <= 1248:
            sock.sendto(data, addr)
            return
        chunks = [data[i:i + 1248] for i in range(0, len(data), 1248)]
        pid = random.randint(1, 2**30)
        order = list(range(len(chunks)))
        random.shuffle(order)  # arrive out of order on purpose
        for n in order:
            if self.args.goldsrc:
                hdr = struct.pack("<iiB", -2, pid, n << 4 | len(chunks))
            else:
                hdr = struct.pack("<iiBBH", -2, pid, len(chunks), n, 1248)
            sock.sendto(hdr + chunks[n], addr)

    # --------------------------------------------------------------- RCON ---
    def rcon_loop(self):
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind((self.args.host, self.args.port))
        srv.listen(8)
        while True:
            conn, _ = srv.accept()
            threading.Thread(target=self.rcon_client, args=(conn,), daemon=True).start()

    def rcon_client(self, conn):
        def recv_exact(n):
            buf = b""
            while len(buf) < n:
                chunk = conn.recv(n - len(buf))
                if not chunk:
                    raise ConnectionError
                buf += chunk
            return buf

        def send(pid, ptype, body=b""):
            payload = struct.pack("<ii", pid, ptype) + body + b"\0\0"
            conn.sendall(struct.pack("<i", len(payload)) + payload)

        authed = False
        try:
            while True:
                size = struct.unpack("<i", recv_exact(4))[0]
                data = recv_exact(size)
                pid, ptype = struct.unpack("<ii", data[:8])
                body = data[8:].rstrip(b"\0").decode(errors="replace")
                if ptype == 3:
                    send(pid, 0)
                    if body == self.args.password:
                        authed = True
                        send(pid, 2)
                    else:
                        send(-1, 2)
                elif not authed:
                    return
                elif ptype == 2:
                    out = self.command(body).encode()
                    for i in range(0, max(len(out), 1), 4000):
                        send(pid, 0, out[i:i + 4000])
                elif ptype == 0:
                    send(pid, 0)  # mirror the empty packet ...
                    send(pid, 0, b"\x00\x01\x00\x00")  # ... plus srcds' odd trailer
        except (ConnectionError, OSError):
            pass
        finally:
            conn.close()

    def command(self, cmd):
        parts = cmd.split()
        if not parts:
            return ""
        name, rest = parts[0].lower(), parts[1:]
        if name == "status":
            lines = [
                f"hostname: {self.args.name}",
                "version : 6630498/24 6630498 secure",
                f"udp/ip  : {self.args.host}:{self.args.port}  (public ip: 127.0.0.1)",
                f"map     : {self.map} at: 0 x, 0 y, 0 z",
                f"players : {len(self.players)} humans, 0 bots (32 max)",
                "",
                "# userid name                uniqueid            connected ping loss state  adr",
            ]
            if self.args.goldsrc:
                lines[-1] = "#      name userid uniqueid frag time ping loss adr"
                for i, (n, score) in enumerate(self.players):
                    lines.append(f'#{i + 1:2d} "{n}" {i + 2} STEAM_0:1:{1000 + i} {score} 01:{i:02d}:11 {30 + i} 0 94.31.108.{10 + i}:27005')
                return "\n".join(lines) + f"\n{len(self.players)} users\n"
            for i, (n, _) in enumerate(self.players):
                lines.append(f'#  {i + 2:3d} "{n}" STEAM_0:1:{1000 + i} 01:{i:02d}:11 {30 + i} 0 active 94.31.108.{10 + i}:27005')
            return "\n".join(lines) + "\n"
        if name == "cvarlist":
            return "".join(f"fake_cvar_{i:05d} : {i} : , \"sv\" : description number {i}\n" for i in range(1500))
        if name == "echo":
            return " ".join(rest) + "\n"
        if name == "changelevel" and rest:
            self.map = rest[0]
            return ""
        if name == "say":
            self.log(f'"Console<0><Console><Console>" say "{" ".join(rest)}"')
            return f"Console: {' '.join(rest)}\n"
        if name == "listmaps":
            return "Map Cycle:\n" + "".join(f"{m}\n" for m in self.maps[:4]) + "\nMap list:\n" + "".join(f"{m}\n" for m in self.maps)
        if name == "kick" and rest:
            return f"Kicked {rest[0]}\n"
        if name == "kickid" and rest:
            return f"Kicked by Console : {' '.join(rest[1:])}\n"
        if name == "maps":
            return "Dir:  maps/*.bsp\n" + "".join(f"PENDING:   (fs) {m}.bsp\n" for m in self.maps)
        if name == "listid":
            lines = [f"ID filter list: {len(self.ban_ids)} entries"]
            lines += [f"{i + 1} {k} : {v}" for i, (k, v) in enumerate(self.ban_ids.items())]
            return "\n".join(lines) + "\n"
        if name == "listip":
            lines = [f"IP filter list: {len(self.ban_ips)} entries"]
            lines += [f"{i + 1} {k} : {v}" for i, (k, v) in enumerate(self.ban_ips.items())]
            return "\n".join(lines) + "\n"
        if name in ("banid", "addip") and len(rest) >= 2:
            target = self.ban_ids if name == "banid" else self.ban_ips
            target[rest[1]] = "permanent" if rest[0] == "0" else f"{float(rest[0]):.3f} min"
            return ""
        if name in ("removeid", "removeip") and rest:
            (self.ban_ids if name == "removeid" else self.ban_ips).pop(rest[0], None)
            return ""
        if name in ("writeid", "writeip"):
            return ""
        if name == "logaddress_add" and rest:
            host, port = (rest[0], rest[1]) if len(rest) >= 2 else rest[0].rsplit(":", 1)
            self.log_targets.add((host, int(port)))
            return f"logaddress_add:  {rest[0]}\n"
        if name == "log":
            self.log_enabled = not rest or rest[0] == "on"
            return "Server logging enabled.\n" if self.log_enabled else "Server logging disabled.\n"
        return f'Unknown command "{name}"\n'

    def log(self, text):
        if not self.log_enabled:
            return
        stamp = time.strftime("%m/%d/%Y - %H:%M:%S")
        pkt = (b"\xff\xff\xff\xfflog L " if self.args.goldsrc else b"\xff\xff\xff\xffRL ") + f"{stamp}: {text}\n".encode() + b"\0"
        for target in list(self.log_targets):
            try:
                self.log_sock.sendto(pkt, target)
            except OSError:
                pass

    def chatter(self):
        lines = ["gg", "rush b", "nice shot", "who is coming?", "lol", "noob", "brb"]
        teams = ["CT", "TERRORIST"]
        while True:
            time.sleep(random.uniform(2, 5))
            name, _ = random.choice(self.players)
            i = [n for n, _ in self.players].index(name)
            team = teams[i % 2]
            kind = random.choice(["say", "say", "say_team"])
            self.log(f'"{name}<{i + 2}><STEAM_1:0:{1000 + i}><{team}>" {kind} "{random.choice(lines)}"')
            # some fights for the player statistics
            j = random.randrange(len(self.players))
            if j != i:
                victim = self.players[j][0]
                vteam = teams[j % 2]
                a = f'"{name}<{i + 2}><STEAM_1:0:{1000 + i}><{team}>"'
                v = f'"{victim}<{j + 2}><STEAM_1:0:{1000 + j}><{vteam}>"'
                self.log(f'{a} [1 2 3] attacked {v} [4 5 6] with "ak47" (damage "{random.randint(10, 60)}") '
                         f'(damage_armor "{random.randint(0, 9)}") (health "{random.randint(1, 99)}") (armor "{random.randint(0, 100)}") (hitgroup "chest")')
                if random.random() < 0.5:
                    self.log(f'{a} [1 2 3] killed {v} [4 5 6] with "ak47"')

    def run(self):
        threading.Thread(target=self.udp_loop, daemon=True).start()
        if self.args.gamespy2:
            threading.Thread(target=self.gamespy2_loop, daemon=True).start()
        if not self.args.goldsrc:
            threading.Thread(target=self.rcon_loop, daemon=True).start()
        threading.Thread(target=self.chatter, daemon=True).start()
        threading.Thread(target=self.report, daemon=True).start()
        print(f"fake srcds '{self.args.name}' on {self.args.host}:{self.args.port} (rcon password '{self.args.password}')")
        while True:
            time.sleep(3600)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=27015)
    ap.add_argument("--name", default="openHLSW Test Server | Fake SRCDS")
    ap.add_argument("--map", default="de_dust2")
    ap.add_argument("--password", default="secret")
    ap.add_argument("--players", type=int, default=7)
    ap.add_argument("--sv-password", action="store_true")
    ap.add_argument("--latency", type=float, default=0, help="simulated ping in ms (randomised 30-100 %%)")
    ap.add_argument("--gamespy2", type=int, default=0, metavar="QUERYPORT", help="also answer GameSpy 2 queries on this UDP port")
    ap.add_argument("--goldsrc", action="store_true", help="behave like HLDS (Condition Zero): 'm' info, UDP rcon")
    ap.add_argument("--max-queries-sec", type=float, default=3.0, help="like sv_max_queries_sec (0 = off)")
    ap.add_argument("--max-queries-window", type=float, default=30.0, help="like sv_max_queries_window")
    Server(ap.parse_args()).run()
