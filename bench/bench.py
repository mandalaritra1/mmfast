#!/usr/bin/env python3
"""Fresh-launch comparison: Mattermost Desktop vs mmfast, same account, warm caches."""
import json, re, statistics, subprocess, sys, time

S = sys.argv[1]
WINWATCH = f"{S}/winwatch"
APPS = {
    "Mattermost": {"bundle": "/Applications/Mattermost.app", "match": "/Applications/Mattermost.app/", "owner": "Mattermost"},
    "mmfast": {"bundle": "/Users/aritra/Projects/mattermost-fast/dist/mmfast.app", "match": "dist/mmfast.app/", "owner": "mmfast"},
}

def pids(match):
    out = subprocess.run(["pgrep", "-f", match], capture_output=True, text=True).stdout.split()
    return [int(p) for p in out]

def cpu_seconds(ps):
    if not ps:
        return 0.0
    out = subprocess.run(["ps", "-o", "time=", "-p", ",".join(map(str, ps))], capture_output=True, text=True).stdout
    total = 0.0
    for t in out.split():
        parts = t.split(":")
        secs = 0.0
        for p in parts:
            secs = secs * 60 + float(p)
        total += secs
    return total

def footprint_mb(ps):
    total = 0.0
    for p in ps:
        out = subprocess.run(["footprint", "-p", str(p)], capture_output=True, text=True).stdout
        m = re.search(r"Footprint: ([\d.]+) (KB|MB|GB)", out)
        if m:
            v = float(m.group(1)) * {"KB": 1 / 1024, "MB": 1, "GB": 1024}[m.group(2)]
            total += v
    return total

def threads(ps):
    out = subprocess.run(["ps", "-M", "-p", ",".join(map(str, ps))], capture_output=True, text=True).stdout
    return max(0, len(out.strip().splitlines()) - 1)

def quit_app(match):
    for p in pids(match):
        subprocess.run(["kill", "-TERM", str(p)])
    for _ in range(100):
        if not pids(match):
            return
        time.sleep(0.1)
    for p in pids(match):
        subprocess.run(["kill", "-KILL", str(p)])
    time.sleep(1)

def run(name):
    a = APPS[name]
    quit_app(a["match"])
    time.sleep(3)
    watcher = subprocess.Popen([WINWATCH, a["owner"], "60"], stdout=subprocess.PIPE, text=True)
    time.sleep(0.2)
    t0 = time.time()
    subprocess.run(["open", "-n", a["bundle"]])
    t_window = watcher.communicate()[0].strip()
    t_window = (float(t_window) - t0) * 1000 if t_window != "timeout" else None
    # Startup done: CPU of all its processes below 5% of a core over 2 s.
    samples = []
    settled = None
    while time.time() - t0 < 60:
        samples.append((time.time(), cpu_seconds(pids(a["match"]))))
        old = [s for s in samples if s[0] <= samples[-1][0] - 2.0]
        if old and time.time() - t0 > 1.0:
            rate = (samples[-1][1] - old[-1][1]) / (samples[-1][0] - old[-1][0])
            if rate < 0.05:
                settled = old[-1][0] - t0
                break
        time.sleep(0.25)
    cpu_startup = cpu_seconds(pids(a["match"]))
    time.sleep(20)
    ps = pids(a["match"])
    mem = footprint_mb(ps)
    c1 = cpu_seconds(ps)
    time.sleep(30)
    idle = cpu_seconds(pids(a["match"])) - c1
    return {
        "window_ms": round(t_window) if t_window else None,
        "settled_s": round(settled, 2) if settled is not None else None,
        "startup_cpu_s": round(cpu_startup, 2),
        "footprint_mb": round(mem),
        "processes": len(ps),
        "threads": threads(ps),
        "idle_cpu_s_30s": round(idle, 3),
    }

results = {"Mattermost": [], "mmfast": []}
for i in range(3):
    for name in ("Mattermost", "mmfast"):
        r = run(name)
        results[name].append(r)
        print(name, i, json.dumps(r), flush=True)

summary = {}
for name, rs in results.items():
    summary[name] = {k: statistics.median([r[k] for r in rs if r[k] is not None]) for k in rs[0]}
print("MEDIANS", json.dumps(summary, indent=1), flush=True)
json.dump({"runs": results, "medians": summary}, open(f"{S}/bench.json", "w"), indent=1)
# Leave both running for the user.
subprocess.run(["open", "/Applications/Mattermost.app"])
