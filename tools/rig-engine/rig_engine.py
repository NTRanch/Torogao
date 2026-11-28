#!/usr/bin/env python3
"""torogao-rig-engine — the primo rig adapter.

A builder-service behind the mobile facade (ARCHITECTURE
§5, route B1). It wraps the estate's gated fleet builders
and A10 instruments; it contains no gate logic of its own
beyond defence-in-depth sanity checks (the facade computes
verdicts with torogao-mobile-rig's gate module over the raw
instrument output this service returns untouched).

Engine contract (the facade's, rig flavour):
  GET  /health              -> {"engine": ..., "model_loaded": true}
  POST /jobs                -> {"id": ...}   (a rig job document)
  GET  /jobs/<id>           -> {"state": ..., ...result...}
  POST /jobs/<id>/interrupt -> {}

Job kinds:
  derive         twin build (official Blender), A10.1 battery,
                 A10.2 census (derived vs twin), face stills;
                 files the gated artefacts under artefacts/<glb sha>/
  pose-test      applies morph weights on the gated blend,
                 measures per-morph vertex displacement from the
                 shape-key data, renders face + front stills
  preview-render renders face + front stills of the gated
                 blend with the record's expressions applied

State lives under TOROGAO_RIG_ENGINE_HOME
(default ~/.local/share/torogao-rig-engine); the builder and
instrument copies of record live in its tools/ directory,
hash-pinned by tools/MANIFEST.sha256, verified at startup.
"""

import base64
import hashlib
import json
import os
import queue
import shutil
import subprocess
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ENGINE_ID = "torogao-rig-engine/1"
HOME_DIR = os.environ.get(
    "TOROGAO_RIG_ENGINE_HOME",
    os.path.expanduser("~/.local/share/torogao-rig-engine"),
)
TOOLS_DIR = os.environ.get("TOROGAO_RIG_ENGINE_TOOLS", os.path.join(HOME_DIR, "tools"))
BLENDER = os.environ.get(
    "TOROGAO_BLENDER",
    os.path.expanduser(
        "~/.cache/azoth/blender-official/blender-5.3.0-linux-x64/blender"
    ),
)
PORT = int(os.environ.get("TOROGAO_RIG_ENGINE_PORT", "8193"))
BLENDER_FLAGS = ["-b", "-Y", "--factory-startup", "--offline-mode", "-noaudio"]
STEP_TIMEOUT_SECS = 1800
# Renders are preview artefacts, never gate inputs. On this
# estate the GPU driver can wedge for hours (nvidia-smi in
# D-state); a render step is therefore bounded tightly and
# its failure degrades the job to "no stills", recorded in
# the result, instead of hanging the derivation.
RENDER_TIMEOUT_SECS = 300

BUILDERS = {
    "party": "build_character_party.py",
    "torogao": "build_character.py",
    "lana": "build_character.py",
    "strobe": "build_character.py",
}


def canonical_bytes(value):
    """serde_json-compatible canonical bytes: sorted keys,
    compact separators, ryu-style float formatting."""
    def fmt_float(f):
        if f != f or f in (float("inf"), float("-inf")):
            return "null"
        if f == int(f) and abs(f) < 1e16:
            return f"{f:.1f}"
        r = repr(f)
        if "e" in r:
            mant, exp = r.split("e")
            r = f"{mant}e{int(exp)}"
        return r

    def enc(v):
        if v is None:
            return "null"
        if v is True:
            return "true"
        if v is False:
            return "false"
        if isinstance(v, int):
            return str(v)
        if isinstance(v, float):
            return fmt_float(v)
        if isinstance(v, str):
            return json.dumps(v, ensure_ascii=False)
        if isinstance(v, list):
            return "[" + ",".join(enc(x) for x in v) + "]"
        if isinstance(v, dict):
            return "{" + ",".join(
                json.dumps(k, ensure_ascii=False) + ":" + enc(v[k])
                for k in sorted(v.keys())
            ) + "}"
        raise TypeError(f"uncanonicalisable {type(v)}")

    return enc(value).encode("utf-8")


def sha256_hex(data):
    return hashlib.sha256(data).hexdigest()


def verify_tools_manifest():
    manifest = os.path.join(TOOLS_DIR, "MANIFEST.sha256")
    problems = []
    if not os.path.exists(manifest):
        return [f"tools manifest missing: {manifest}"]
    for line in open(manifest):
        line = line.strip()
        if not line:
            continue
        want, name = line.split(None, 1)
        path = os.path.join(TOOLS_DIR, name.strip().lstrip("*"))
        if not os.path.exists(path):
            problems.append(f"tool missing: {name}")
        elif sha256_hex(open(path, "rb").read()) != want:
            problems.append(f"tool hash mismatch: {name}")
    return problems


class Job:
    def __init__(self, doc):
        self.id = uuid.uuid4().hex[:12]
        self.doc = doc
        self.state = "queued"
        self.result = {}
        self.error = None
        self.cancelled = False
        self.proc = None


JOBS = {}
JOB_QUEUE = queue.Queue()
LOCK = threading.Lock()


def run_step(job, argv, cwd=None, timeout=STEP_TIMEOUT_SECS):
    """Run one subprocess step; honour interrupt."""
    if job.cancelled:
        raise RuntimeError("cancelled")
    proc = subprocess.Popen(
        argv,
        cwd=cwd,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        start_new_session=True,
    )
    job.proc = proc
    try:
        out, _ = proc.communicate(timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        raise RuntimeError(f"step timed out after {timeout}s: {argv[0]}")
    finally:
        job.proc = None
    if job.cancelled:
        raise RuntimeError("cancelled")
    if proc.returncode != 0:
        tail = "\n".join((out or "").splitlines()[-8:])
        raise RuntimeError(f"step failed (rc={proc.returncode}): {' '.join(argv[:3])}...\n{tail}")
    return out


def blender_script(job, script, args, cwd=None, timeout=STEP_TIMEOUT_SECS):
    return run_step(
        job,
        [BLENDER] + BLENDER_FLAGS + ["--python-exit-code", "1", "-P",
                                     os.path.join(TOOLS_DIR, script), "--"] + args,
        cwd=cwd,
        timeout=timeout,
    )


def file_artefacts(paths):
    stills = []
    for name, path in paths:
        with open(path, "rb") as fh:
            stills.append({
                "filename": name,
                "png_base64": base64.b64encode(fh.read()).decode("ascii"),
            })
    return stills


def do_derive(job, workdir):
    doc = job.doc
    record = doc["record"]
    spec = doc["derived_spec"]
    family = record.get("rig", {}).get("builder_family", "")
    builder_name = BUILDERS.get(family)
    if builder_name is None:
        raise RuntimeError(f"unknown builder family {family!r}")
    builder_path = os.path.join(TOOLS_DIR, builder_name)
    builder_sha = sha256_hex(open(builder_path, "rb").read())

    spec_path = os.path.join(workdir, "spec.json")
    with open(spec_path, "w") as fh:
        fh.write(canonical_bytes(spec).decode("utf-8"))

    builds = {}
    for twin in ("a", "b"):
        bdir = os.path.join(workdir, f"build_{twin}")
        os.makedirs(bdir)
        shutil.copy(spec_path, os.path.join(bdir, "spec.json"))
        run_step(job, [BLENDER] + BLENDER_FLAGS + ["--python-exit-code", "1",
                 "-P", builder_path, "--", bdir, "spec.json"])
        glbs = [f for f in os.listdir(bdir) if f.endswith(".glb")]
        blends = [f for f in os.listdir(bdir) if f.endswith(".blend")]
        if len(glbs) != 1 or len(blends) != 1:
            raise RuntimeError(f"twin {twin}: expected one .glb and one .blend, got {glbs} {blends}")
        builds[twin] = (os.path.join(bdir, glbs[0]), os.path.join(bdir, blends[0]))

    glb_a, blend_a = builds["a"]
    glb_b, _ = builds["b"]
    glb_sha = sha256_hex(open(glb_a, "rb").read())
    twin_sha = sha256_hex(open(glb_b, "rb").read())

    battery_path = os.path.join(workdir, "battery.json")
    blender_script(job, "measure_battery.py", [glb_a, battery_path, "none", "full"])
    census_path = os.path.join(workdir, "census.json")
    run_step(job, [sys.executable, os.path.join(TOOLS_DIR, "census_glb.py"),
                   glb_a, glb_b, census_path])
    stills_dir = os.path.join(workdir, "stills")
    prefix = record["id"]
    stills_error = None
    try:
        blender_script(job, "face_stills.py", [blend_a, spec_path, stills_dir, prefix],
                       timeout=RENDER_TIMEOUT_SECS)
    except RuntimeError as e:
        stills_error = str(e)
    still_files = []
    if os.path.isdir(stills_dir):
        still_files = sorted(
            (f, os.path.join(stills_dir, f)) for f in os.listdir(stills_dir)
            if f.endswith(".png")
        )

    # File the artefacts under the GLB hash for later pose
    # tests and previews.
    art_dir = os.path.join(HOME_DIR, "artefacts", glb_sha)
    os.makedirs(art_dir, exist_ok=True)
    shutil.copy(glb_a, os.path.join(art_dir, "char.glb"))
    shutil.copy(blend_a, os.path.join(art_dir, "char.blend"))
    shutil.copy(spec_path, os.path.join(art_dir, "spec.json"))
    with open(os.path.join(art_dir, "record.json"), "w") as fh:
        fh.write(canonical_bytes(record).decode("utf-8"))

    with open(glb_a, "rb") as fh:
        glb_b64 = base64.b64encode(fh.read()).decode("ascii")
    result = {
        "battery": json.load(open(battery_path)),
        "census": json.load(open(census_path)),
        "glb_sha256": glb_sha,
        "twin_sha256": twin_sha,
        "builder_sha256": builder_sha,
        "derived_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "artifacts": {
            "glb_base64": glb_b64,
            "stills": file_artefacts(still_files),
        },
    }
    if stills_error:
        result["stills_error"] = stills_error
    return result


def gated_blend_for(job):
    record = job.doc["record"]
    glb_sha = record.get("rig", {}).get("gated_glb_sha256")
    if not glb_sha:
        raise RuntimeError("record carries no gated GLB sha")
    art_dir = os.path.join(HOME_DIR, "artefacts", glb_sha)
    blend = os.path.join(art_dir, "char.blend")
    spec = os.path.join(art_dir, "spec.json")
    if not (os.path.exists(blend) and os.path.exists(spec)):
        raise RuntimeError(f"no filed artefacts for gated GLB {glb_sha[:12]}...")
    return blend, spec


def do_pose(job, workdir, morphs, prefix_suffix):
    blend, spec = gated_blend_for(job)
    morphs_path = os.path.join(workdir, "morphs.json")
    with open(morphs_path, "w") as fh:
        json.dump(morphs, fh)
    out_dir = os.path.join(workdir, "stills")
    prefix = job.doc["record"]["id"] + prefix_suffix
    stills_error = None
    try:
        blender_script(job, "pose_render.py", [blend, spec, morphs_path, out_dir, prefix],
                       timeout=RENDER_TIMEOUT_SECS)
    except RuntimeError as e:
        stills_error = str(e)
    # pose_render writes result.json (the measurements)
    # before it renders; a wedged render must not eat them.
    result = json.load(open(os.path.join(out_dir, "result.json")))
    still_files = []
    if os.path.isdir(out_dir):
        still_files = sorted(
            (f, os.path.join(out_dir, f)) for f in os.listdir(out_dir)
            if f.endswith(".png")
        )
    out = {
        "measured_morphs": result["measured"],
        "artifacts": {"stills": file_artefacts(still_files)},
    }
    if stills_error:
        out["stills_error"] = stills_error
    return out


def execute(job):
    doc = job.doc
    record = doc.get("record", {})
    # Defence in depth only: the facade has already verified
    # these hashes with the crate; a mismatch here means the
    # document changed hands wrongly.
    if doc.get("record_sha256") != sha256_hex(canonical_bytes(record)):
        raise RuntimeError("record hash mismatch")
    kind = doc.get("kind")
    workdir = os.path.join(HOME_DIR, "jobs", job.id)
    os.makedirs(workdir, exist_ok=True)
    with open(os.path.join(workdir, "doc.json"), "w") as fh:
        fh.write(canonical_bytes(doc).decode("utf-8"))
    if kind == "derive":
        if doc.get("derived_spec_sha256") != sha256_hex(canonical_bytes(doc["derived_spec"])):
            raise RuntimeError("derived spec hash mismatch")
        lock = record.get("identity_lock", {})
        if lock.get("status") != "locked":
            raise RuntimeError("identity lock not held (adapter sanity check)")
        return do_derive(job, workdir)
    if kind == "pose-test":
        return do_pose(job, workdir, doc.get("pose_morphs", []), "-pose")
    if kind == "preview-render":
        return do_pose(job, workdir, record.get("expressions", []), "-preview")
    raise RuntimeError(f"unknown job kind {kind!r}")


def worker():
    while True:
        job = JOB_QUEUE.get()
        with LOCK:
            if job.cancelled:
                job.state = "cancelled"
                continue
            job.state = "running"
        try:
            result = execute(job)
            with LOCK:
                job.result = result
                job.state = "cancelled" if job.cancelled else "done"
        except RuntimeError as e:
            with LOCK:
                job.error = str(e)
                job.state = "cancelled" if job.cancelled else "error"
        except Exception as e:  # the service reports, never dies
            with LOCK:
                job.error = f"{type(e).__name__}: {e}"
                job.state = "error"


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def reply(self, code, obj):
        body = json.dumps(obj).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def read_body(self):
        n = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(n) if n else b""

    def do_GET(self):
        if self.path == "/health":
            return self.reply(200, {"engine": ENGINE_ID, "model_loaded": True})
        if self.path.startswith("/jobs/"):
            job_id = self.path[len("/jobs/"):]
            with LOCK:
                job = JOBS.get(job_id)
                if job is None:
                    return self.reply(404, {"error": "no such job"})
                out = {"state": job.state}
                out.update(job.result)
                if job.error:
                    out["error"] = job.error
                return self.reply(200, out)
        return self.reply(404, {"error": "not found"})

    def do_POST(self):
        if self.path == "/jobs":
            try:
                doc = json.loads(self.read_body() or b"{}")
            except json.JSONDecodeError as e:
                return self.reply(400, {"error": f"bad json: {e}"})
            job = Job(doc)
            with LOCK:
                JOBS[job.id] = job
            JOB_QUEUE.put(job)
            return self.reply(200, {"id": job.id})
        if self.path.startswith("/jobs/") and self.path.endswith("/interrupt"):
            job_id = self.path[len("/jobs/"):-len("/interrupt")]
            self.read_body()
            with LOCK:
                job = JOBS.get(job_id)
                if job is None:
                    return self.reply(404, {"error": "no such job"})
                job.cancelled = True
                proc = job.proc
            if proc is not None:
                try:
                    os.killpg(proc.pid, 9)
                except ProcessLookupError:
                    pass
            return self.reply(200, {"interrupted": job_id})
        return self.reply(404, {"error": "not found"})


def main():
    problems = verify_tools_manifest()
    if problems:
        for p in problems:
            print(f"torogao-rig-engine: {p}", file=sys.stderr)
        sys.exit(1)
    os.makedirs(os.path.join(HOME_DIR, "jobs"), exist_ok=True)
    os.makedirs(os.path.join(HOME_DIR, "artefacts"), exist_ok=True)
    threading.Thread(target=worker, daemon=True).start()
    server = ThreadingHTTPServer(("127.0.0.1", PORT), Handler)
    print(f"torogao-rig-engine: listening on 127.0.0.1:{PORT}, tools at {TOOLS_DIR}")
    server.serve_forever()


if __name__ == "__main__":
    main()
