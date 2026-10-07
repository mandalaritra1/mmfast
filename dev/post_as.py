#!/usr/bin/env python3
"""Post a message to the local test server as one of the seeded users.

    python3 dev/post_as.py alice ttbar-analysis "hello @aritra"
    python3 dev/post_as.py bob ttbar-analysis "a reply" --reply-to-last-root

Used to check live delivery over the websocket while the app is open.
"""

import json
import sys
import urllib.request
from pathlib import Path

CREDS = json.loads((Path(__file__).resolve().parent / "seed-credentials.json").read_text())
API = CREDS["server"] + "/api/v4"


def call(method, path, token=None, body=None):
    headers = {"Content-Type": "application/json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request(API + path, data=json.dumps(body).encode() if body is not None else None,
                                 method=method, headers=headers)
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read() or b"{}"), resp.headers.get("Token")


def main():
    user, channel, message = sys.argv[1:4]
    reply = "--reply-to-last-root" in sys.argv
    _, token = call("POST", "/users/login", body={"login_id": user, "password": CREDS["users"][user]["password"]})
    team, _ = call("GET", f"/teams/name/{CREDS.get('team', 'cms-physics')}", token)
    ch, _ = call("GET", f"/teams/{team['id']}/channels/name/{channel}", token)
    root = ""
    if reply:
        posts, _ = call("GET", f"/channels/{ch['id']}/posts?per_page=50", token)
        roots = [posts["posts"][i] for i in posts["order"] if not posts["posts"][i]["root_id"] and not posts["posts"][i]["type"]]
        root = roots[0]["id"] if roots else ""
    post, _ = call("POST", "/posts", token, {"channel_id": ch["id"], "message": message, "root_id": root})
    print(post["id"])


if __name__ == "__main__":
    main()
