#!/usr/bin/env python3
"""AssetRipper fixture driver for bundle-native rung 2.

Starts the estate AssetRipper 2.0.0 binary headless (GPL tool use
on RE material only; outputs are test fixtures — data, never code),
loads one title folder, walks its collections, and writes JSONL
fixtures: every MonoBehaviour's /Assets/Json export verbatim, and
for TextAssets only name + byte length + sha256 of the script
bytes (content never enters the diff pipeline).

Usage: arfixtures.py <game-dir> <out.jsonl> [--port N]
"""
import hashlib
import html
import json
import os
import re
import subprocess
import sys
import time
import urllib.parse
import urllib.request

BIN = os.path.expanduser(
    "~/.local/share/pax/assetripper-r1-20261110/bin/AssetRipper.GUI.Free"
)
ROW_RE = re.compile(
    r'<tr data-class="([^"]+)"><td>(-?\d+)</td><td>[^<]*</td><td>(.*?)</td></tr>',
    re.S,
)
HREF_RE = re.compile(r'href="([^"]+)"')


def q(obj):
    return urllib.parse.quote(json.dumps(obj, separators=(",", ":")))


def strip_tags(s):
    return html.unescape(re.sub(r"<[^>]+>", "", s)).strip()


class Client:
    def __init__(self, base):
        self.base = base.rstrip("/")

    def get(self, path, timeout=120):
        with urllib.request.urlopen(self.base + path, timeout=timeout) as r:
            return r.read().decode("utf-8", "replace")

    def post_load(self, folder, timeout):
        data = urllib.parse.urlencode({"Path": folder}).encode()
        req = urllib.request.Request(self.base + "/LoadFolder", data=data)
        try:
            with urllib.request.urlopen(req, timeout=timeout) as r:
                return r.status
        except urllib.error.HTTPError as e:
            return e.code


def parse_path_param(href):
    m = re.search(r"[?&]Path=([^\"&]+)", href)
    if not m:
        return None
    try:
        return json.loads(urllib.parse.unquote(m.group(1)))
    except Exception:
        return None


def walk_collections(client):
    collections = []
    seen_bundles = set()
    seen_collections = set()
    queue = [{"P": []}]
    while queue:
        bp = queue.pop(0)
        key = json.dumps(bp, sort_keys=True)
        if key in seen_bundles:
            continue
        seen_bundles.add(key)
        page = client.get(f"/Bundles/View?Path={q(bp)}")
        for href in HREF_RE.findall(page):
            if href.startswith("/Bundles/View?"):
                child = parse_path_param(href)
                if child and json.dumps(child, sort_keys=True) not in seen_bundles:
                    queue.append(child)
            elif href.startswith("/Collections/View?"):
                cp = parse_path_param(href)
                if not cp:
                    continue
                ckey = json.dumps(cp, sort_keys=True)
                if ckey in seen_collections:
                    continue
                seen_collections.add(ckey)
                collections.append({"bundle": cp["B"]["P"], "index": cp["I"]})
    return collections


def read_collection(client, coll):
    cp = {"B": {"P": coll["bundle"]}, "I": coll["index"]}
    count = int(client.get(f"/Collections/Count?Path={q(cp)}").strip())
    page = client.get(f"/Collections/View?Path={q(cp)}")
    tm = re.search(r"<title>([^<]*)</title>", page)
    name = strip_tags(tm.group(1)) if tm else "?"
    assets = []
    for cls, pid, namecell in ROW_RE.findall(page):
        assets.append(
            {"pathID": int(pid), "class": cls, "name": strip_tags(namecell)}
        )
    assets.sort(key=lambda a: a["pathID"])
    return {"name": name, "count": count, "assets": assets, "cp": cp}


def main():
    game_dir = sys.argv[1]
    out_path = sys.argv[2]
    port = 18742
    if "--port" in sys.argv:
        port = int(sys.argv[sys.argv.index("--port") + 1])
    base = f"http://127.0.0.1:{port}"
    proc = subprocess.Popen(
        [BIN, "--headless", "--port", str(port)],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        client = Client(base)
        deadline = time.time() + 120
        while True:
            try:
                client.get("/", timeout=5)
                break
            except Exception:
                if time.time() > deadline:
                    raise SystemExit("server did not come up")
                time.sleep(1)
        code = client.post_load(game_dir, timeout=1800)
        print(f"load: HTTP {code}", file=sys.stderr)
        if code not in (200, 302):
            raise SystemExit(f"LoadFolder failed: HTTP {code}")
        collections = walk_collections(client)
        print(f"collections: {len(collections)}", file=sys.stderr)
        n_mb = 0
        n_ta = 0
        n_fail = 0
        with open(out_path, "w") as out:
            for coll in collections:
                info = read_collection(client, coll)
                for asset in info["assets"]:
                    if asset["class"] not in ("MonoBehaviour", "TextAsset"):
                        continue
                    ap = {"C": info["cp"], "D": asset["pathID"]}
                    try:
                        raw = client.get(f"/Assets/Json?Path={q(ap)}")
                        parsed = json.loads(raw)
                    except Exception as e:
                        n_fail += 1
                        print(
                            f"fixture fail {info['name']} {asset['pathID']}: {e}",
                            file=sys.stderr,
                        )
                        continue
                    if asset["class"] == "TextAsset":
                        script = parsed.get("m_Script", "")
                        raw_bytes = (
                            script.encode("utf-8")
                            if isinstance(script, str)
                            else bytes(script)
                        )
                        rec = {
                            "collection": info["name"],
                            "path_id": asset["pathID"],
                            "class_id": 49,
                            "name": parsed.get("m_Name", asset["name"]),
                            "bytes": len(raw_bytes),
                            "sha256": hashlib.sha256(raw_bytes).hexdigest(),
                        }
                        n_ta += 1
                    else:
                        rec = {
                            "collection": info["name"],
                            "path_id": asset["pathID"],
                            "class_id": 114,
                            "json": parsed,
                        }
                        n_mb += 1
                    out.write(json.dumps(rec, separators=(",", ":")) + "\n")
        print(
            f"fixtures: mb={n_mb} textasset={n_ta} failed={n_fail}",
            file=sys.stderr,
        )
    finally:
        proc.terminate()
        try:
            proc.wait(timeout=30)
        except Exception:
            proc.kill()


if __name__ == "__main__":
    main()
