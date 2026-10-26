#!/usr/bin/env python3
"""
MCP harness for `torogao-cli mcp` (Torogao P1.5 gate).

Drives the server as a stdio child and exercises:
  initialize, tools/list (diffed against `registry-dump`),
  one call per registered tool against the rich fixture,
  disabled-tool call, unknown tool, malformed frame,
  oversized frame, blank lines, and clean EOF shutdown.

Every check has an overall timeout; the server must answer
each frame and exit 0 at EOF without being killed.

Usage: mcp_harness.py <torogao-cli-binary> <fixtures-dir> <work-dir>
Exit code 0 = all checks pass.
"""
import json
import os
import subprocess
import sys

TIMEOUT = 60


def main() -> int:
    binary, fixtures, work = sys.argv[1], sys.argv[2], sys.argv[3]
    rich = os.path.join(fixtures, "rich.inx")
    export_dir = os.path.join(work, "mcp-export")
    failures = []

    def check(name, cond, detail=""):
        status = "PASS" if cond else "FAIL"
        print(f"[{status}] {name}" + (f" -- {detail}" if detail and not cond else ""))
        if not cond:
            failures.append(name)

    # The registry of record, straight from the binary.
    dump = subprocess.run([binary, "registry-dump"], capture_output=True,
                          text=True, timeout=TIMEOUT)
    registry = json.loads(dump.stdout)
    reg_tools = {t["name"]: t for t in registry["tools"]}

    proc = subprocess.Popen([binary, "mcp"], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, text=True)
    transcript = []

    def send(obj, raw=None):
        line = raw if raw is not None else json.dumps(obj)
        transcript.append(">>> " + line)
        proc.stdin.write(line + "\n")
        proc.stdin.flush()

    def recv():
        line = proc.stdout.readline()
        if not line:
            raise RuntimeError("server closed stdout unexpectedly")
        transcript.append("<<< " + line.rstrip())
        return json.loads(line)

    # -- initialize --------------------------------------------------
    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2024-11-05", "capabilities": {},
        "clientInfo": {"name": "p15-harness", "version": "1.0"}}})
    resp = recv()
    check("initialize: result", resp.get("id") == 1 and "result" in resp)
    check("initialize: protocolVersion",
          resp["result"]["protocolVersion"] == "2024-11-05")
    check("initialize: serverInfo name",
          resp["result"]["serverInfo"]["name"] == "torogao")
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    # -- tools/list must equal the registry exactly ------------------
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    resp = recv()
    listed = {t["name"]: t for t in resp["result"]["tools"]}
    check("tools/list: names equal registry",
          set(listed) == set(reg_tools),
          f"listed={sorted(listed)} registry={sorted(reg_tools)}")
    for name, spec in reg_tools.items():
        t = listed.get(name, {})
        check(f"tools/list[{name}]: description equals registry",
              t.get("description") == spec["description"])
        check(f"tools/list[{name}]: inputSchema equals registry",
              t.get("inputSchema") == spec["inputSchema"])
        check(f"tools/list[{name}]: posture annotation",
              t.get("annotations", {}).get("torogao:posture")
              == spec["posture"])

    # -- one call per enabled tool, against the rich fixture ---------
    calls = {
        "inspect": {"project": rich},
        "validate": {"project": rich},
        "project-info": {"project": rich},
        "export-rig": {"project": rich, "out": export_dir},
    }
    next_id = 10
    results = {}
    for name, args in calls.items():
        next_id += 1
        send({"jsonrpc": "2.0", "id": next_id, "method": "tools/call",
              "params": {"name": name, "arguments": args}})
        resp = recv()
        res = resp.get("result", {})
        check(f"call {name}: not an error", res.get("isError") is False,
              json.dumps(resp)[:200])
        results[name] = res.get("structuredContent", {})
    check("call validate: rich fixture is clean",
          results["validate"].get("result") == "clean")
    check("call inspect: census meta name",
          results["inspect"].get("census", {}).get("meta", {}).get("name")
          == "Rich Fixture")
    check("call project-info: container INP + payload",
          results["project-info"].get("container") == "INP"
          and results["project-info"].get("payload") is True)
    check("call export-rig: model file written",
          os.path.isfile(os.path.join(export_dir, "rich.model.json")))

    # -- execution-level error is a tool result, not a crash ---------
    next_id += 1
    send({"jsonrpc": "2.0", "id": next_id, "method": "tools/call",
          "params": {"name": "validate",
                     "arguments": {"project": "/nonexistent/x.inx"}}})
    resp = recv()
    check("call validate missing project: isError result",
          resp.get("result", {}).get("isError") is True
          and "project_not_found" in resp["result"]["content"][0]["text"])

    # -- disabled tool -> typed protocol error naming the gate -------
    next_id += 1
    send({"jsonrpc": "2.0", "id": next_id, "method": "tools/call",
          "params": {"name": "apply-fixes", "arguments": {"project": rich}}})
    resp = recv()
    err = resp.get("error", {})
    check("call apply-fixes: tool_disabled with gate",
          err.get("data", {}).get("code") == "tool_disabled"
          and "mutation gate" in err.get("data", {}).get("message", "")
          or "mutation gate" in err.get("message", ""))

    # -- unknown tool -------------------------------------------------
    next_id += 1
    send({"jsonrpc": "2.0", "id": next_id, "method": "tools/call",
          "params": {"name": "no-such-tool", "arguments": {}}})
    resp = recv()
    check("call unknown tool: unknown_tool error",
          resp.get("error", {}).get("data", {}).get("code") == "unknown_tool")

    # -- malformed frame ----------------------------------------------
    send(None, raw="{not json")
    resp = recv()
    check("malformed frame: parse_error, id null",
          resp.get("error", {}).get("data", {}).get("code") == "parse_error"
          and resp.get("id") is None)

    # -- unknown method ------------------------------------------------
    next_id += 1
    send({"jsonrpc": "2.0", "id": next_id, "method": "bogus/method"})
    resp = recv()
    check("unknown method: method_not_found",
          resp.get("error", {}).get("data", {}).get("code")
          == "method_not_found")

    # -- blank line is ignored (next response is the ping's) ----------
    proc.stdin.write("\n")
    proc.stdin.flush()
    transcript.append(">>> (blank line)")
    next_id += 1
    send({"jsonrpc": "2.0", "id": next_id, "method": "ping"})
    resp = recv()
    check("blank line ignored, ping answered",
          resp.get("id") == next_id and "result" in resp)

    # -- oversized frame: typed error, then stream still in sync ------
    big = json.dumps({"jsonrpc": "2.0", "id": 777, "method": "ping",
                      "padding": "x" * (2 * 1024 * 1024)})
    send(None, raw=big)
    resp = recv()
    check("oversized frame: frame_too_large",
          resp.get("error", {}).get("data", {}).get("code")
          == "frame_too_large")
    next_id += 1
    send({"jsonrpc": "2.0", "id": next_id, "method": "ping"})
    resp = recv()
    check("stream in sync after oversized frame",
          resp.get("id") == next_id and "result" in resp)

    # -- EOF: clean exit -----------------------------------------------
    proc.stdin.close()
    rc = proc.wait(timeout=TIMEOUT)
    check("EOF: server exits 0", rc == 0, f"rc={rc}")

    with open(os.path.join(work, "mcp-transcript.txt"), "w") as f:
        f.write("\n".join(transcript) + "\n")

    print()
    if failures:
        print(f"MCP HARNESS: FAIL ({len(failures)} checks)")
        return 1
    print("MCP HARNESS: PASS")
    return 0


if __name__ == "__main__":
    sys.exit(main())
