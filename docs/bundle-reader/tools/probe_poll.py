import json, os, subprocess, sys, select, time

BIN = os.path.expanduser("~/.local/bin/theodolite")
CFG = os.path.expanduser("~/.local/share/theodolite/bridge.toml")

server, workspace, path = sys.argv[1], sys.argv[2], sys.argv[3]
poll_secs = int(os.environ.get("POLL_SECONDS", "300"))

proc = subprocess.Popen([BIN, "--config", CFG],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                        stderr=subprocess.PIPE, text=True)
_next = [0]

def send(o):
    proc.stdin.write(json.dumps(o) + "\n"); proc.stdin.flush()

def read_for(want_id, timeout_s):
    end = time.time() + timeout_s
    while time.time() < end:
        r, _, _ = select.select([proc.stdout], [], [], max(1, end - time.time()))
        if not r: break
        line = proc.stdout.readline()
        if not line.strip(): continue
        try: msg = json.loads(line)
        except Exception: continue
        if msg.get("id") == want_id:
            return msg
    return None

def call(name, args, timeout_s):
    _next[0] += 1
    i = _next[0]
    send({"jsonrpc": "2.0", "id": i, "method": "tools/call",
          "params": {"name": name, "arguments": args}})
    return read_for(i, timeout_s)

def diag_summary(resp):
    if resp is None: return "TIMEOUT"
    r = resp.get("result", {})
    if r.get("isError"): return "TOOL-ERROR"
    try:
        text = r["content"][0]["text"]
        payload = json.loads(text)
        ds = payload.get("diagnostics", [])
        return "count=%d sevs=%s codes=%s" % (
            payload.get("count", -1),
            sorted({d.get("severity") for d in ds}),
            sorted({d.get("code") for d in ds}))
    except Exception as e:
        return "UNPARSEABLE %s" % e

try:
    _next[0] = 1
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2026-07-28", "capabilities": {},
        "clientInfo": {"name": "pax-wiring-probe-poll", "version": "0.1"}}})
    init = read_for(1, 30)
    if not init or "result" not in init:
        print("INIT FAILED"); sys.exit(1)
    send({"jsonrpc": "2.0", "method": "notifications/initialized", "params": {}})
    st = call("session_status", {"server": server, "workspace": workspace}, 180)
    print("session_status ok:", bool(st and "result" in st))
    t0 = time.time()
    attempt = 0
    while True:
        attempt += 1
        dg = call("diagnostics", {"server": server, "workspace": workspace, "path": path}, 180)
        summary = diag_summary(dg)
        print(f"[{time.time()-t0:.0f}s attempt {attempt}] {path}: {summary}")
        if "1" in summary.split("sevs=")[-1].split("]")[0] if "sevs=" in summary else False:
            print("SEVERITY-1 SURFACED")
            break
        if time.time() - t0 > poll_secs:
            print("POLL WINDOW EXHAUSTED")
            break
        time.sleep(20)
finally:
    proc.terminate()
    try: proc.wait(timeout=5)
    except Exception: proc.kill()
