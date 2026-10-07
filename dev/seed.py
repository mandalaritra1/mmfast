#!/usr/bin/env python3
"""Seed the local Mattermost test server (http://localhost:8065).

Creates an admin and a few users, turns on the server settings the client
needs, and fills a team with channels, threads, images, attachments, a
channel header, reactions, pins, DMs and one long channel for scrolling.

Credentials are generated once and kept in dev/seed-credentials.json
(git-ignored). Re-running reuses them and skips what already exists, except
posts, which are only seeded into channels that are still empty.

Standard library only.
"""

from __future__ import annotations

import json
import os
import random
import secrets
import struct
import sys
import time
import urllib.error
import urllib.request
import uuid
import zlib
from pathlib import Path

SERVER = os.environ.get("MM_SERVER", "http://localhost:8065")
API = SERVER + "/api/v4"
HERE = Path(__file__).resolve().parent
CREDS = HERE / "seed-credentials.json"

USERS = [
    # username, first, last, system admin
    ("aritra", "Aritra", "Mandal", True),
    ("alice", "Alice", "Nguyen", False),
    ("bob", "Bob", "Schmidt", False),
    ("carol", "Carol", "Rossi", False),
    ("dave", "Dave", "Okafor", False),
]

DAY = 86_400_000


# ---------------------------------------------------------------- http


class Api:
    def __init__(self, token: str | None = None):
        self.token = token

    def request(self, method, path, body=None, raw=None, content_type=None, ok404=False):
        url = path if path.startswith("http") else API + path
        data = None
        headers = {"X-Requested-With": "XMLHttpRequest"}
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        if raw is not None:
            data = raw
            headers["Content-Type"] = content_type
        elif body is not None:
            data = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        req = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=60) as resp:
                payload = resp.read()
                token = resp.headers.get("Token")
                out = json.loads(payload) if payload else {}
                if token and isinstance(out, dict):
                    out["__token"] = token
                return out
        except urllib.error.HTTPError as err:
            if ok404 and err.code == 404:
                return None
            detail = err.read().decode(errors="replace")[:400]
            raise RuntimeError(f"{method} {path} -> {err.code}: {detail}") from None

    def get(self, path, **kw):
        return self.request("GET", path, **kw)

    def post(self, path, body=None, **kw):
        return self.request("POST", path, body, **kw)

    def put(self, path, body=None, **kw):
        return self.request("PUT", path, body, **kw)

    def upload(self, channel_id, filename, data, mime):
        boundary = "----seed" + secrets.token_hex(8)
        parts = []
        parts.append(
            f'--{boundary}\r\nContent-Disposition: form-data; name="channel_id"\r\n\r\n{channel_id}\r\n'.encode()
        )
        parts.append(
            (
                f"--{boundary}\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{filename}\"\r\n"
                f"Content-Type: {mime}\r\n\r\n"
            ).encode()
            + data
            + b"\r\n"
        )
        parts.append(f"--{boundary}--\r\n".encode())
        out = self.request(
            "POST", "/files", raw=b"".join(parts), content_type=f"multipart/form-data; boundary={boundary}"
        )
        return out["file_infos"][0]["id"]


# ---------------------------------------------------------------- media


def png(width, height, pixel):
    """Encode an RGB PNG from pixel(x, y) -> (r, g, b)."""
    rows = bytearray()
    for y in range(height):
        rows.append(0)
        for x in range(width):
            rows.extend(pixel(x, y))

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(bytes(rows), 6)) + chunk(b"IEND", b"")


def histogram_png(seed, width=900, height=560):
    """A fake mass histogram: white background, axes, filled bars and a peak."""
    rng = random.Random(seed)
    peak = rng.uniform(0.35, 0.65)
    bins = 60
    heights = []
    for i in range(bins):
        t = i / bins
        bg = 0.55 * (1 - t) ** 2
        sig = 0.45 * pow(2.718, -((t - peak) ** 2) / 0.002)
        heights.append(min(0.95, bg + sig + rng.uniform(0, 0.04)))
    left, bottom, top = 70, height - 50, 30
    plot_w = width - left - 30
    plot_h = bottom - top
    fill = [(66, 133, 244), (219, 68, 55), (15, 157, 88), (244, 160, 0)][seed % 4]

    def pixel(x, y):
        if (x == left and top <= y <= bottom) or (y == bottom and left <= x <= left + plot_w):
            return (20, 20, 20)
        if left < x < left + plot_w and top <= y < bottom:
            b = int((x - left) / plot_w * bins)
            h = heights[min(b, bins - 1)]
            if y >= bottom - h * plot_h:
                edge = (x - left) % max(1, plot_w // bins) == 0
                return (40, 40, 40) if edge else fill
            if (y - top) % 60 == 0:
                return (230, 230, 230)
        return (255, 255, 255)

    return png(width, height, pixel)


def photo_png(seed, width=640, height=420):
    """A smooth gradient 'photo' with a sun, different per seed."""
    rng = random.Random(seed)
    a = [rng.randint(20, 120) for _ in range(3)]
    b = [rng.randint(140, 255) for _ in range(3)]
    sx, sy, sr = rng.randint(100, width - 100), rng.randint(60, 180), rng.randint(30, 60)

    def pixel(x, y):
        if (x - sx) ** 2 + (y - sy) ** 2 < sr * sr:
            return (255, 236, 150)
        t = y / height
        if y > height * 0.7 + 20 * (x % 97) / 97:
            return (int(a[0] * 0.6), int(a[1] * 0.8), int(a[2] * 0.5))
        return tuple(int(b[i] * (1 - t) + a[i] * t) for i in range(3))

    return png(width, height, pixel)


def pdf_bytes(title, lines):
    text = "BT /F1 18 Tf 72 740 Td (" + title + ") Tj ET\n"
    y = 710
    for line in lines:
        text += f"BT /F1 11 Tf 72 {y} Td ({line}) Tj ET\n"
        y -= 16
    objs = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>",
        f"<< /Length {len(text)} >>\nstream\n{text}endstream",
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>",
    ]
    out = b"%PDF-1.4\n"
    offsets = []
    for i, obj in enumerate(objs, 1):
        offsets.append(len(out))
        out += f"{i} 0 obj\n{obj}\nendobj\n".encode()
    xref = len(out)
    out += f"xref\n0 {len(objs) + 1}\n0000000000 65535 f \n".encode()
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode()
    out += f"trailer\n<< /Size {len(objs) + 1} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return out


# ---------------------------------------------------------------- content

FIREHOSE_LINES = [
    "Anyone else seeing the condor queue stuck at LPC?",
    "Rerunning the skim with the new JECs now.",
    "The cutflow for 2018 looks off by about 3% compared to last week.",
    "Pushed a fix for the b-tag SF lookup, please pull.",
    "Coffee? :coffee:",
    "Meeting moved to 15:00 CET.",
    "`xrdcp` keeps timing out from eos, switching to the fnal redirector.",
    "The **2D Alphabet** fit converged on the third try.",
    "Can someone review https://github.com/example/analysis/pull/42 ?",
    "Pulls look fine, the largest is 1.3 sigma on the JMS nuisance.",
    "I'll be out tomorrow, ping me on email if something breaks.",
    "Merged. Thanks everyone :tada:",
    "Data/MC agreement in the control region is within 5% after the reweighting.",
    "Who owns the ParticleNet tagger SF file?",
    "Rebinning to 50 GeV in mtt makes the signal peak much clearer.",
]


def main():
    creds = json.loads(CREDS.read_text()) if CREDS.exists() else {"server": SERVER, "users": {}}
    anon = Api()
    anon.get("/system/ping")

    # Users: the first one created on a fresh server becomes system admin and
    # creates the rest (open sign-up is off after the first account).
    creator = anon
    if "aritra" in creds["users"]:
        try:
            out = anon.post("/users/login", {"login_id": "aritra", "password": creds["users"]["aritra"]["password"]})
            creator = Api(out["__token"])
        except RuntimeError:
            pass
    for username, first, last, _ in USERS:
        entry = creds["users"].setdefault(username, {"password": "Test-" + secrets.token_urlsafe(12)})
        entry["email"] = f"{username}@example.test"
        CREDS.write_text(json.dumps(creds, indent=2))
        try:
            creator.post(
                "/users",
                {"username": username, "email": entry["email"], "password": entry["password"],
                 "first_name": first, "last_name": last},
            )
            print(f"created user {username}")
        except RuntimeError as err:
            if "exists" not in str(err) and "taken" not in str(err):
                raise
        if creator is anon:
            out = anon.post("/users/login", {"login_id": username, "password": entry["password"]})
            creator = Api(out["__token"])
    CREDS.write_text(json.dumps(creds, indent=2))
    os.chmod(CREDS, 0o600)

    def login(username):
        out = anon.post("/users/login", {"login_id": username, "password": creds["users"][username]["password"]})
        return Api(out["__token"]), out

    admin, me = login("aritra")
    ids = {}
    for username, *_ in USERS:
        ids[username] = admin.get(f"/users/username/{username}")["id"]

    # Server settings the client relies on.
    admin.put(
        "/config/patch",
        {
            "ServiceSettings": {
                "EnableUserAccessTokens": True,
                "EnableLatex": True,
                "EnableInlineLatex": True,
                "CollapsedThreads": "default_on",
                "EnableLinkPreviews": False,
                "EnablePostUsernameOverride": False,
                "EnableCustomEmoji": True,
            },
            "RateLimitSettings": {"Enable": False},
            "FileSettings": {"EnableFileAttachments": True, "MaxFileSize": 104857600},
            "TeamSettings": {"EnableOpenServer": False},
        },
    )
    print("config patched")

    # Promote everyone while seeding so backdated create_at is honoured.
    for username, _, _, is_admin in USERS:
        if not is_admin:
            admin.put(f"/users/{ids[username]}/roles", {"roles": "system_user system_admin"})

    clients = {"aritra": admin}
    for username, *_ in USERS[1:]:
        clients[username] = login(username)[0]

    # Team.
    team = admin.get("/teams/name/cms-physics", ok404=True)
    if team is None:
        team = admin.post("/teams", {"name": "cms-physics", "display_name": "CMS Physics", "type": "O"})
        print("created team")
    for username, *_ in USERS:
        try:
            admin.post(f"/teams/{team['id']}/members", {"team_id": team["id"], "user_id": ids[username]})
        except RuntimeError:
            pass
    team2 = admin.get("/teams/name/lpc-computing", ok404=True)
    if team2 is None:
        team2 = admin.post("/teams", {"name": "lpc-computing", "display_name": "LPC Computing", "type": "O"})
    for username in ("aritra", "bob"):
        try:
            admin.post(f"/teams/{team2['id']}/members", {"team_id": team2["id"], "user_id": ids[username]})
        except RuntimeError:
            pass

    def channel(team_id, name, display, kind="O", header="", purpose="", members=None):
        ch = admin.get(f"/teams/{team_id}/channels/name/{name}", ok404=True)
        if ch is None:
            ch = admin.post(
                "/channels",
                {"team_id": team_id, "name": name, "display_name": display, "type": kind,
                 "header": header, "purpose": purpose},
            )
            print(f"created channel {name}")
        for username in members or [u for u, *_ in USERS]:
            try:
                admin.post(f"/channels/{ch['id']}/members", {"user_id": ids[username]})
            except RuntimeError:
                pass
        return ch

    town = admin.get(f"/teams/{team['id']}/channels/name/town-square")
    admin.put(f"/channels/{town['id']}/patch",
              {"header": "Welcome to **CMS Physics**. Group meeting Thursdays 16:00 CET, [agenda](https://indico.cern.ch/)",
               "purpose": "Team-wide announcements and chatter"})
    ttbar = channel(
        team["id"], "ttbar-analysis", "ttbar-analysis",
        header="All-hadronic $t\\bar{t}$ resonance search :rocket: | [AN draft](https://cms.cern.ch/) | "
               "Skims: `/store/user/ttbarhad/` | Contact: @aritra",
        purpose="Day-to-day work on the all-hadronic ttbar resonance search",
    )
    zjet = channel(team["id"], "zjet-substructure", "Z+jet substructure",
                   header="Jet mass and $\\rho$ unfolding in Z+jet events", purpose="Unfolding, systematics, plots")
    conveners = channel(team["id"], "conveners", "Conveners", kind="P",
                        header="Private: convener planning", members=["aritra", "alice", "carol"])
    firehose = channel(team["id"], "firehose", "firehose", header="Long channel for scroll testing",
                       purpose="Hundreds of messages")
    random_ch = channel(team["id"], "off-topic", "Off-Topic")
    lpc = channel(team2["id"], "condor-help", "condor-help", header="HTCondor at the LPC: `condor_q -better-analyze`",
                  members=["aritra", "bob"])

    def empty(ch):
        return not admin.get(f"/channels/{ch['id']}/posts?per_page=5")["order"] or all(
            p["type"] for p in admin.get(f"/channels/{ch['id']}/posts?per_page=50")["posts"].values()
        )

    now = int(time.time() * 1000)

    def post(user, ch, message, at, root="", files=None, props=None):
        body = {"channel_id": ch["id"], "message": message, "create_at": at, "root_id": root}
        if files:
            body["file_ids"] = files
        if props:
            body["props"] = props
        return clients[user].post("/posts", body)

    def react(user, p, emoji):
        clients[user].post("/reactions", {"user_id": ids[user], "post_id": p["id"], "emoji_name": emoji})

    # ttbar-analysis: threads, images, attachments, markdown, latex, edits, pins.
    if empty(ttbar):
        t0 = now - 6 * DAY
        p1 = post("alice", ttbar, "Morning! New skims for 2018 are on EOS. Cutflow below.", t0)
        f1 = clients["alice"].upload(ttbar["id"], "mtt_2018_signal_region.png", histogram_png(1), "image/png")
        p2 = post("alice", ttbar, "Here is $m_{t\\bar{t}}$ in the signal region:", t0 + 60_000, files=[f1])
        react("aritra", p2, "+1")
        react("bob", p2, "+1")
        react("carol", p2, "eyes")
        r = p2["id"]
        post("aritra", ttbar, "Nice. Is that with the new top-tagger working point?", t0 + 120_000, root=r)
        post("alice", ttbar, "Yes, ParticleNet at 0.5% mistag.", t0 + 180_000, root=r)
        post("bob", ttbar, "The bump near 0.5 of the range looks real-ish, what's the binning?", t0 + 240_000, root=r)
        post("alice", ttbar, "50 GeV bins, I'll post the fit later.", t0 + 300_000, root=r)
        post("aritra", ttbar,
             "For reference the transfer function we use:\n\n$$R_{P/F}(m_{t\\bar t}, m_t) = \\frac{N_{pass}}{N_{fail}}$$\n\n"
             "fit with a 2D polynomial of order (1,1).", t0 + 360_000, root=r)

        t1 = now - 5 * DAY
        post("carol", ttbar, "Did anyone check the JES uncertainties after the update?", t1)
        code = ("Here's how I'm running it:\n\n```python\nfrom coffea import processor\n\nrunner = processor.Runner(\n"
                "    executor=processor.FuturesExecutor(workers=8),\n    schema=NanoAODSchema,\n)\nout = runner(fileset, "
                "treename=\"Events\", processor_instance=TTbarResProcessor())\n```")
        post("dave", ttbar, code, t1 + 90_000)
        table = ("Yields so far:\n\n| Region | Data | MC | Ratio |\n|---|---:|---:|---:|\n| pass, cen | 1204 | 1188.3 | 1.01 |\n"
                 "| pass, fwd | 873 | 901.2 | 0.97 |\n| fail, cen | 45210 | 44980.1 | 1.01 |\n| fail, fwd | 31877 | 32003.5 | 1.00 |")
        p_table = post("aritra", ttbar, table, t1 + 200_000)
        admin.post(f"/posts/{p_table['id']}/pin")
        f_pdf = clients["bob"].upload(
            ttbar["id"], "AN-2026-042_draft_v3.pdf",
            pdf_bytes("AN-2026-042 draft v3", ["Search for heavy resonances decaying to ttbar", "All-hadronic channel",
                                              "Section 5: background estimation (TODO)"]),
            "application/pdf")
        f_csv = clients["bob"].upload(
            ttbar["id"], "cutflow_2018.csv",
            b"cut,events,eff\nall,10000000,1.000\ntrigger,4012331,0.401\n2 AK8 jets,1821200,0.182\n"
            b"top tag (1),210331,0.021\ntop tag (2),12004,0.0012\n",
            "text/csv")
        p_files = post("bob", ttbar, "Draft AN and the cutflow table attached, comments welcome by Friday.",
                       t1 + 400_000, files=[f_pdf, f_csv])
        post("alice", ttbar, "Will read tonight.", t1 + 500_000, root=p_files["id"])
        post("carol", ttbar, "Section 5 needs the alphabet fit description, I can draft it.", t1 + 560_000,
             root=p_files["id"])

        t2 = now - 2 * DAY
        imgs = [clients["carol"].upload(ttbar["id"], f"pulls_{i}.png", histogram_png(10 + i, 700, 440), "image/png")
                for i in range(3)]
        post("carol", ttbar, "Fit diagnostics: three plots from the latest toys.", t2, files=imgs)
        p_edit = post("dave", ttbar, "Condor jobs finished, 98% success rate.", t2 + 600_000)
        clients["dave"].put(f"/posts/{p_edit['id']}/patch",
                            {"message": "Condor jobs finished, 99.4% success rate (resubmitted the failures)."})
        react("aritra", p_edit, "tada")
        post("aritra", ttbar, "@alice @bob can we freeze the selection by Monday? ~zjet-substructure folks need the "
                              "same jet IDs.", t2 + 900_000)
        t3 = now - 3 * 3_600_000
        post("bob", ttbar, "Freeze is fine with me.", t3)
        post("alice", ttbar, "Same. I'll tag v2.1 of the skimmer.", t3 + 120_000)
        p_last = post("alice", ttbar, "Tagged: `v2.1.0`. Release notes in the repo.", t3 + 240_000)
        react("aritra", p_last, "white_check_mark")
        print("seeded ttbar-analysis")

    if empty(zjet):
        t0 = now - 4 * DAY
        f = clients["carol"].upload(zjet["id"], "rho_unfolded.png", histogram_png(7), "image/png")
        p = post("carol", zjet, "Unfolded $\\rho = \\log(m^2/p_T^2)$ with the iterative Bayes method, 4 iterations.",
                 t0, files=[f])
        post("aritra", zjet, "Closure looks good. What's the regularization strength for TUnfold?", t0 + 60_000,
             root=p["id"])
        post("carol", zjet, "tau = 1e-4 from the L-curve.", t0 + 120_000, root=p["id"])
        post("dave", zjet, "Herwig vs Pythia difference is the dominant systematic again.", t0 + 3_600_000)
        print("seeded zjet-substructure")

    if empty(conveners):
        post("carol", conveners, "Agenda for the next B2G meeting: ttbar status, Z+jet unfolding approval.", now - DAY)
        post("aritra", conveners, "I'll present the ttbar update, 10 minutes.", now - DAY + 300_000)

    if empty(random_ch):
        photos = [clients["dave"].upload(random_ch["id"], f"geneva_{i}.png", photo_png(i), "image/png") for i in range(2)]
        post("dave", random_ch, "Sunset over the lake yesterday :sunrise:", now - 3 * DAY, files=photos)
        post("bob", random_ch, "Who's up for lunch at R1?", now - 2 * DAY)

    if empty(firehose):
        rng = random.Random(42)
        users = [u for u, *_ in USERS]
        start = now - 30 * DAY
        roots = []
        for i in range(600):
            at = start + i * (30 * DAY // 600)
            user = rng.choice(users)
            msg = rng.choice(FIREHOSE_LINES)
            if rng.random() < 0.15:
                msg += " " + " ".join(rng.choice(FIREHOSE_LINES) for _ in range(3))
            root = ""
            if roots and rng.random() < 0.1:
                root = rng.choice(roots[-20:])
            p = post(user, firehose, f"[{i}] {msg}", at, root=root)
            if not root:
                roots.append(p["id"])
        print("seeded firehose (600 posts)")

    if empty(lpc):
        post("bob", lpc, "Jobs held with `memory usage exceeded`, bump request_memory to 4 GB.", now - DAY)
        post("aritra", lpc, "Thanks, that fixed it.", now - DAY + 600_000)

    # Direct and group messages.
    dm = admin.post("/channels/direct", [ids["aritra"], ids["alice"]])
    if empty(dm):
        post("alice", dm, "Hey, do you have a minute to look at the JMS fit?", now - 7_200_000)
        post("aritra", dm, "Sure, send me the workspace path.", now - 7_000_000)
        post("alice", dm, "`/eos/user/a/alice/jms/fit_v3/` thanks!", now - 6_900_000)
        post("alice", dm, "Also :pray: for the review comments", now - 600_000)
    dm2 = admin.post("/channels/direct", [ids["aritra"], ids["bob"]])
    if empty(dm2):
        post("bob", dm2, "Your proxy expired on the LPC node btw", now - 3 * DAY)
    gm = admin.post("/channels/group", [ids["aritra"], ids["carol"], ids["dave"]])
    if empty(gm):
        post("carol", gm, "Dinner on Friday after the meeting?", now - 5 * 3_600_000)
        post("dave", gm, "I'm in.", now - 4 * 3_600_000)

    # Demote non-admins again.
    for username, _, _, is_admin in USERS:
        if not is_admin:
            admin.put(f"/users/{ids[username]}/roles", {"roles": "system_user"})

    # Mark the older channels read for aritra so only the newest posts are unread.
    for ch in (town, zjet, firehose, lpc, conveners):
        admin.post("/channels/members/me/view", {"channel_id": ch["id"]})

    # Profile pictures for some users, so avatars are real images.
    for i, username in enumerate(("alice", "bob", "carol")):
        if admin.get(f"/users/{ids[username]}").get("last_picture_update", 0) > 0:
            continue
        boundary = "----seed" + secrets.token_hex(8)
        img = photo_png(100 + i, 256, 256)
        body = (f'--{boundary}\r\nContent-Disposition: form-data; name="image"; filename="{username}.png"\r\n'
                f"Content-Type: image/png\r\n\r\n").encode() + img + f"\r\n--{boundary}--\r\n".encode()
        admin.request("POST", f"/users/{ids[username]}/image", raw=body,
                      content_type=f"multipart/form-data; boundary={boundary}")
        print(f"profile picture for {username}")

    # A personal access token for the client.
    if not creds["users"]["aritra"].get("access_token"):
        tok = admin.post(f"/users/{ids['aritra']}/tokens", {"description": "mattermost-fast dev"})
        creds["users"]["aritra"]["access_token"] = tok["token"]
        print("created personal access token for aritra")
    creds["team"] = "cms-physics"
    CREDS.write_text(json.dumps(creds, indent=2))
    os.chmod(CREDS, 0o600)
    print(f"done. credentials in {CREDS}")


if __name__ == "__main__":
    try:
        main()
    except RuntimeError as err:
        print("error:", err, file=sys.stderr)
        sys.exit(1)
