#!/usr/bin/env python3
"""
A2A harness for `torogao-cli a2a` (Torogao P1.5 gate).

Starts the surface on a loopback port and exercises:
  GET the Agent Card (skills must equal the tool registry,
  straight from `registry-dump`), one task round-trip
  (message/send -> tasks/get) running validate on the rich
  fixture, a failing task (missing project), unknown tool,
  unknown task, tasks/cancel on a terminal task, bad JSON,
  and an unknown route.

Usage: a2a_harness.py <torogao-cli-binary> <fixtures-dir> <work-dir> <port>
Exit code 0 = all checks pass.
"""
import json
import os
import subprocess
import sys
import time
import urllib.request
import urllib.error

TIMEOUT = 60


def http(method, url, body=None):
    data = None
    headers = {}
    if body is not None:
        data = body if isinstance(body, bytes) else json.dumps(body).encode()
        headers["Content-Type"] = "application/json"
    req = urllib.request.Request(url, data=data, headers=headers,
                                 method=method)
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            return resp.status, resp.read().decode()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()


def main() -> int:
    binary, fixtures, work = sys.argv[1], sys.argv[2], sys.argv[3]
    port = int(sys.argv[4]) if len(sys.argv) > 4 else 8377
    base = f"http://127.0.0.1:{port}"
    rich = os.path.join(fixtures, "rich.inx")
    failures = []
    transcript = []

    def check(name, cond, detail=""):
        status = "PASS" if cond else "FAIL"
        print(f"[{status}] {name}"
              + (f" -- {detail}" if detail and not cond else ""))
        if not cond:
            failures.append(name)

    dump = subprocess.run([binary, "registry-dump"], capture_output=True,
                          text=True, timeout=TIMEOUT)
    registry = json.loads(dump.stdout)
    reg_tools = {t["name"]: t for t in registry["tools"]}

    proc = subprocess.Popen([binary, "a2a", "--port", str(port)],
                            stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, text=True)
    try:
        # Wait for the listener.
        card = None
        deadline = time.time() + TIMEOUT
        while time.time() < deadline:
            try:
                status, text = http("GET",
                                    base + "/.well-known/agent-card.json")
                if status == 200:
                    card = json.loads(text)
                    transcript.append("GET /.well-known/agent-card.json"
                                      f" -> {status}\n{text}")
                    break
            except Exception:
                pass
            time.sleep(0.25)
        check("agent card served", card is not None)
        if card is None:
            return 1
        check("card name", card.get("name") == "Torogao")
        check("card provider is the studio",
              card.get("provider", {}).get("organization")
              == "Nice Thoughts Ranch")
        check("card url", card.get("url") == base + "/")
        skills = {s["id"]: s for s in card.get("skills", [])}
        check("card skills equal registry tools",
              set(skills) == set(reg_tools),
              f"skills={sorted(skills)} registry={sorted(reg_tools)}")
        for name, spec in reg_tools.items():
            check(f"card skill[{name}]: description equals registry",
                  skills.get(name, {}).get("description")
                  == spec["description"])

        def rpc(method, params, id_):
            body = {"jsonrpc": "2.0", "id": id_, "method": method,
                    "params": params}
            status, text = http("POST", base + "/", body)
            transcript.append(f"POST / {method} -> {status}\n{text}")
            return status, json.loads(text)

        def send_task(tool, arguments, id_):
            return rpc("message/send", {"message": {
                "role": "user", "messageId": f"m{id_}",
                "parts": [{"kind": "data", "data": {
                    "tool": tool, "arguments": arguments}}]}}, id_)

        # -- one full task round-trip: validate the rich fixture -----
        status, resp = send_task("validate", {"project": rich}, 1)
        task = resp.get("result", {})
        check("message/send validate: task completed",
              task.get("status", {}).get("state") == "completed",
              json.dumps(resp)[:200])
        artifact = (task.get("artifacts") or [{}])[0]
        payload = (artifact.get("parts") or [{}])[0].get("data", {})
        check("task artifact: validate payload clean",
              payload.get("result") == "clean")
        task_id = task.get("id")

        status, resp = rpc("tasks/get", {"id": task_id}, 2)
        check("tasks/get: same task, same state",
              resp.get("result", {}).get("id") == task_id
              and resp["result"]["status"]["state"] == "completed")

        # -- cancel of a terminal task returns it unchanged ----------
        status, resp = rpc("tasks/cancel", {"id": task_id}, 3)
        check("tasks/cancel terminal: state stays completed",
              resp.get("result", {}).get("status", {}).get("state")
              == "completed")

        # -- failing task: missing project ---------------------------
        status, resp = send_task("validate",
                                 {"project": "/nonexistent/x.inx"}, 4)
        task = resp.get("result", {})
        payload = ((task.get("artifacts") or [{}])[0]
                   .get("parts") or [{}])[0].get("data", {})
        check("message/send missing project: task failed w/ typed error",
              task.get("status", {}).get("state") == "failed"
              and payload.get("error", {}).get("code")
              == "project_not_found")

        # -- unknown tool task -> failed task naming unknown_tool ----
        status, resp = send_task("no-such-tool", {}, 5)
        task = resp.get("result", {})
        payload = ((task.get("artifacts") or [{}])[0]
                   .get("parts") or [{}])[0].get("data", {})
        check("message/send unknown tool: failed w/ unknown_tool",
              task.get("status", {}).get("state") == "failed"
              and payload.get("error", {}).get("code") == "unknown_tool")

        # -- unknown task ----------------------------------------------
        status, resp = rpc("tasks/get", {"id": "task-9999"}, 6)
        check("tasks/get unknown: task_not_found",
              resp.get("error", {}).get("data", {}).get("code")
              == "task_not_found")

        # -- bad JSON body ------------------------------------------------
        status, text = http("POST", base + "/", b"{not json")
        transcript.append(f"POST / (bad json) -> {status}\n{text}")
        check("bad json: 400 parse_error",
              status == 400
              and json.loads(text)["error"]["data"]["code"]
              == "parse_error")

        # -- unknown route --------------------------------------------
        status, text = http("GET", base + "/nope")
        check("unknown route: 404", status == 404)

        with open(os.path.join(work, "agent-card.json"), "w") as f:
            json.dump(card, f, indent=2)
            f.write("\n")
    finally:
        proc.terminate()
        try:
            out, _ = proc.communicate(timeout=10)
        except subprocess.TimeoutExpired:
            proc.kill()
            out, _ = proc.communicate()
        transcript.append("server log:\n" + (out or ""))

    with open(os.path.join(work, "a2a-transcript.txt"), "w") as f:
        f.write("\n\n".join(transcript) + "\n")

    print()
    if failures:
        print(f"A2A HARNESS: FAIL ({len(failures)} checks)")
        return 1
    print("A2A HARNESS: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
