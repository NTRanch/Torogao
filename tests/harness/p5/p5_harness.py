#!/usr/bin/env python3
"""
P5 harness for torogao-cli (Torogao P5 gate).

Exercises the P5 surface end to end against the freshly built
CLI: the adversarial-surge flips (Item 0.5 — every finding case
from SURGE-REPORT.md must now produce the typed refusal),
scenes + the conditioning lab (Item 1), the Playroom and
preference sets (Items 2/3), Addition S paths (Item 5),
tracking takes + the gaze generator (Item 6), controllers
and the authored-motion override (Item 7), the effects stack
(Item 8), the GLB governance surface (Item 9), the video
core (Item 10), the Scenes tutorial track (Item 11), and
registry accounting (5.0.0 / 57 tools) + one MCP call into
the new family.

Usage: p5_harness.py <torogao-cli> <p3-fixtures-dir> <work-dir>
Exit code 0 = all checks pass.
"""
import hashlib
import json
import os
import struct
import subprocess
import sys

FAILURES = []
CHECKS = [0]


def check(name, cond, detail=""):
    CHECKS[0] += 1
    if cond:
        print(f"  ok  {name}")
    else:
        print(f"FAIL  {name}  {detail}")
        FAILURES.append(name)


def run(cli, args, expect_rc=None, env=None):
    full_env = dict(os.environ)
    if env:
        full_env.update(env)
    proc = subprocess.run([cli] + args, capture_output=True, text=True,
                          timeout=600, env=full_env)
    if expect_rc is not None:
        check(f"rc {args[0]} == {expect_rc}", proc.returncode == expect_rc,
              f"got {proc.returncode}; stderr={proc.stderr[:300]}")
    return proc


def run_json(cli, args, expect_rc=None, env=None):
    proc = run(cli, args, expect_rc, env=env)
    try:
        return json.loads(proc.stdout)
    except Exception:
        check(f"json output of {args[0]}", False,
              f"stdout={proc.stdout[:300]} stderr={proc.stderr[:300]}")
        return {}


def sha256(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest()


# ---------------------------------------------------------- INP surgery
def be32(n):
    return struct.pack(">I", n)


def read_inp(path):
    data = open(path, "rb").read()
    assert data[:8] == b"TRNSRTS\0", "bad magic"
    plen = struct.unpack(">I", data[8:12])[0]
    payload = data[12:12 + plen]
    at = 12 + plen
    assert data[at:at + 8] == b"TEX_SECT", "no tex section"
    tex_start = at
    at += 8
    count = struct.unpack(">I", data[at:at + 4])[0]
    at += 4
    for _ in range(count):
        ln = struct.unpack(">I", data[at:at + 4])[0]
        at += 4 + 1 + ln
    tex_part = data[tex_start:at]
    sections = []
    if at < len(data):
        assert data[at:at + 8] == b"EXT_SECT", "trailing garbage"
        at += 8
        n = struct.unpack(">I", data[at:at + 4])[0]
        at += 4
        for _ in range(n):
            nl = struct.unpack(">I", data[at:at + 4])[0]
            at += 4
            name = data[at:at + nl].decode()
            at += nl
            dl = struct.unpack(">I", data[at:at + 4])[0]
            at += 4
            sections.append((name, data[at:at + dl]))
            at += dl
    return payload, tex_part, sections


def write_inp(path, payload, tex_part, sections):
    out = b"TRNSRTS\0" + be32(len(payload)) + payload + tex_part
    if sections:
        out += b"EXT_SECT" + be32(len(sections))
        for name, blob in sections:
            nb = name.encode()
            out += be32(len(nb)) + nb + be32(len(blob)) + blob
    open(path, "wb").write(out)


TOROGAO = "io.ntranch.torogao"


def ext_of(sections):
    for name, blob in sections:
        if name == TOROGAO:
            return json.loads(blob)
    return None


def with_ext(sections, ext):
    out = []
    done = False
    for name, blob in sections:
        if name == TOROGAO:
            out.append((name, json.dumps(ext).encode()))
            done = True
        else:
            out.append((name, blob))
    if not done:
        out.append((TOROGAO, json.dumps(ext).encode()))
    return out


def codes(doc):
    return {f.get("code") for f in doc.get("findings", [])}


# ---------------------------------------------------------- GLB fixture
def make_glb(path):
    gltf = {
        "asset": {"version": "2.0", "generator": "p5-harness"},
        "nodes": [{"name": "Root"}, {"name": "Head", "mesh": 0}],
        "meshes": [{
            "name": "Face",
            "primitives": [{"attributes": {"POSITION": 0},
                            "targets": [{"POSITION": 1},
                                        {"POSITION": 2}]}],
            "extras": {"targetNames": ["Smile", "EyesClosed"]},
        }],
        "skins": [{"name": "BodySkin", "joints": [0, 1]}],
        "animations": [{"name": "Idle", "channels": [], "samplers": []}],
    }
    json_bytes = json.dumps(gltf).encode()
    json_bytes += b" " * ((4 - len(json_bytes) % 4) % 4)
    total = 12 + 8 + len(json_bytes)
    out = b"glTF" + struct.pack("<III", 2, total, len(json_bytes))
    out += struct.pack("<I", 0x4E4F534A) + json_bytes
    open(path, "wb").write(out)


def main():
    cli, p3_fixtures, work = sys.argv[1:4]
    cli = os.path.abspath(cli)
    os.makedirs(work, exist_ok=True)

    def W(name):
        return os.path.join(work, name)

    identity = os.path.join(p3_fixtures, "identity-maren.json")
    hitareas = os.path.join(p3_fixtures, "hitareas.json")

    # -- base character ----------------------------------------
    created = W("maren.inx")
    run_json(cli, ["create-character", "--identity", identity,
                   "--pack", "adult", "--preset", "hair-sway-2step",
                   "--out", created], 0)
    # with hit areas authored in (a second file, so the plain
    # project stays available too)
    maren_ha = W("maren-ha.inx")
    run_json(cli, ["hitareas-author", created, "--spec", hitareas,
                   "--out", maren_ha], 0)

    payload, tex, sections = read_inp(created)
    base_ext = ext_of(sections)

    def mutated(name, mutate):
        ext = json.loads(json.dumps(base_ext))
        mutate(ext)
        path = W(name)
        write_inp(path, payload, tex, with_ext(sections, ext))
        return path

    # -- Item 0.5: the surge flips ------------------------------
    print("== surge F-1: malformed containers are typed ==")
    raw = open(created, "rb").read()
    trunc = W("trunc.inx")
    open(trunc, "wb").write(raw[:len(raw) // 2])
    for label, path in (("truncated", trunc),):
        proc = run(cli, ["validate", path], 2)
        doc = json.loads(proc.stdout)
        check(f"F-1 {label}: typed TG-CONTAINER-MALFORMED, clean stderr",
              doc.get("code") == "TG-CONTAINER-MALFORMED"
              and "inochi2d" not in proc.stderr
              and ".d:" not in proc.stderr
              and "core.exception" not in proc.stdout.split(
                  "malformed INP container")[0],
              f"doc={doc} stderr={proc.stderr[:200]}")
    empty = W("empty.inx")
    open(empty, "wb").write(b"")
    proc = run(cli, ["inspect", empty], 2)
    check("F-1 empty file: typed unloadable JSON",
          json.loads(proc.stdout).get("code") == "TG-CONTAINER-MALFORMED")
    zeros = W("zeros.inx")
    open(zeros, "wb").write(b"\0" * 512)
    proc = run(cli, ["validate", zeros], 2)
    check("F-1 zero file: typed unloadable JSON",
          json.loads(proc.stdout).get("code") == "TG-CONTAINER-MALFORMED")

    print("== surge F-3: the output guard ==")
    proc = run(cli, ["create-character", "--identity", identity,
                     "--pack", "adult", "--out", created], 1)
    doc = json.loads(proc.stdout)
    check("F-3 existing output refuses TG-OUT-EXISTS",
          doc.get("code") == "TG-OUT-EXISTS", f"doc={doc}")
    proc = run(cli, ["hitareas-author", created, "--spec", hitareas,
                     "--out", created], 1)
    doc = json.loads(proc.stdout)
    check("F-3 output == input refuses TG-OUT-IS-INPUT (D01)",
          doc.get("code") == "TG-OUT-IS-INPUT", f"doc={doc}")
    proc = run(cli, ["create-character", "--identity", identity,
                     "--pack", "adult", "--out", identity], 1)
    check("F-3 create over its own identity refuses TG-OUT-IS-INPUT",
          json.loads(proc.stdout).get("code") == "TG-OUT-IS-INPUT")
    run_json(cli, ["create-character", "--identity", identity,
                   "--pack", "adult", "--out", created, "--force"], 0)
    export_dir = W("export1")
    run_json(cli, ["export-rig", created, "--out", export_dir], 0)
    proc = run(cli, ["export-rig", created, "--out", export_dir], 4)
    check("F-3 export-rig re-export refuses TG-OUT-EXISTS at rc 4",
          json.loads(proc.stdout).get("code") == "TG-OUT-EXISTS")
    run_json(cli, ["export-rig", created, "--out", export_dir,
                   "--force"], 0)

    print("== surge F-2: one rating verdict ==")
    def stamp_case(name, meta):
        def m(ext):
            ext["meta"] = meta
        return mutated(name, m)
    a12 = stamp_case("a12.inx", {
        "pack": "adult", "lane": "all-ages", "rating": "all-ages",
        "basis": "surge A12: lane field disagrees with the pack"})
    proc = run(cli, ["rating-check", a12], 1)
    doc = json.loads(proc.stdout)
    check("F-2/A12 stamp lane != pack lane refuses (TG-RATING-LANE-MISMATCH)",
          doc.get("result") == "mismatch"
          and doc.get("code") == "TG-RATING-LANE-MISMATCH", f"doc={doc}")
    val = run_json(cli, ["validate", a12], 1)
    check("F-2/A12 validate agrees with rating-check",
          "TG-RATING-LANE-MISMATCH" in codes(val))
    nostamp = mutated("nostamp.inx", lambda e: e.pop("meta"))
    proc = run(cli, ["rating-check", nostamp], 1)
    doc = json.loads(proc.stdout)
    check("F-2/B03 unstamped project: rating-check 'missing' rc 1",
          doc.get("result") == "missing"
          and doc.get("code") == "TG-RATING-STAMP-MISSING", f"doc={doc}")
    val = run_json(cli, ["validate", nostamp], 1)
    check("F-2/B03 validate agrees (TG-RATING-STAMP-MISSING)",
          "TG-RATING-STAMP-MISSING" in codes(val))
    badext = W("badext.inx")
    write_inp(badext, payload, tex,
              [(TOROGAO, b"{not json")] + [(n, b) for n, b in sections
                                           if n != TOROGAO])
    proc = run(cli, ["rating-check", badext], 1)
    doc = json.loads(proc.stdout)
    check("F-2/B04 malformed EXT: rating-check 'malformed' TG-EXT-MALFORMED",
          doc.get("result") == "malformed"
          and doc.get("code") == "TG-EXT-MALFORMED", f"doc={doc}")

    print("== surge F-4/F-5/F-6: container + section discipline ==")
    dup = W("dup-ext.inx")
    write_inp(dup, payload, tex,
              sections + [(TOROGAO, json.dumps(base_ext).encode())])
    val = run_json(cli, ["validate", dup], 1)
    check("F-4/A09 duplicate Torogao EXT section -> TG-EXT-DUPLICATE",
          "TG-EXT-DUPLICATE" in codes(val), f"codes={codes(val)}")
    trailing = W("trailing.inx")
    open(trailing, "wb").write(raw + b"GARBAGE!")
    val = run_json(cli, ["validate", trailing], None)
    alltext = val.get("findings", [])
    check("F-4/A10 trailing bytes -> TG-CONTAINER-TRAILING",
          "TG-CONTAINER-TRAILING" in codes(val)
          or val.get("code") == "TG-CONTAINER-MALFORMED",
          f"doc={str(val)[:200]}")
    wrong = W("wrongtype.inx")
    write_inp(wrong, payload, tex,
              [(TOROGAO, b"\"just a string\"")]
              + [(n, b) for n, b in sections if n != TOROGAO])
    val = run_json(cli, ["validate", wrong], 1)
    check("F-5/A08+D03 EXT section of wrong type -> TG-EXT-TYPE",
          "TG-EXT-TYPE" in codes(val), f"codes={codes(val)}")
    dupstate = mutated("dupstate.inx",
                        lambda e: e["states"].append(e["states"][0]))
    val = run_json(cli, ["validate", dupstate], 1)
    check("F-6/A11+C03 duplicate state name -> TG-STATE-DUPLICATE",
          "TG-STATE-DUPLICATE" in codes(val), f"codes={codes(val)}")
    track1 = W("track-1s.json")
    json.dump({"format": "torogao-input-track/1", "fps": 60,
               "steps": 60, "inputs": {}}, open(track1, "w"))
    proc = run(cli, ["apply-state", dupstate, "--track", track1,
                     "--state", "neutral", "--out",
                     W("dup-curves.json")], 3)
    check("F-6 apply-state refuses the ambiguous name",
          "TG-STATE-DUPLICATE" in proc.stdout, proc.stdout[:200])

    print("== surge F-8: field-named errors, right nouns ==")
    def bad_hit(ext):
        ext["consumer"]["hitAreas"] = [{
            "name": "bad", "binding": {"part": "Head"},
            "shape": {"kind": "rect", "x": 0, "y": 0, "w": -5, "h": 10}}]
    badhit = mutated("badhit.inx", bad_hit)
    val = run_json(cli, ["validate", badhit], 1)
    msgs = " ".join(f.get("message", "") for f in val.get("findings", []))
    check("F-8/D02 negative width names the field and the value",
          "TG-HITAREA-SHAPE" in codes(val) and "'w'" in msgs
          and "-5" in msgs, f"msgs={msgs[:200]}")
    proc = run(cli, ["instantiate-template", "base-humanoid",
                     "--preset", "nope-preset", "--out", W("x.inx")], None)
    check("F-8/G05 unknown preset is called a preset",
          "preset 'nope-preset' not found" in proc.stdout,
          f"stdout={proc.stdout[:200]}")
    proc = run(cli, ["instantiate-template", "base-humanoid",
                     "--preset", "base-humanoid", "--out", W("x.inx")],
               None)
    check("F-8 wrong-kind preset names the kind problem",
          "is not a physics-chain preset" in proc.stdout,
          f"stdout={proc.stdout[:200]}")

    print("== surge F-7: CLI parity ==")
    info = run_json(cli, ["project-info", created], 0)
    check("F-7 project-info answers on the CLI",
          info.get("container") == "INP" and info.get("payload") is True)
    track = W("track.json")
    json.dump({"format": "torogao-input-track/1", "fps": 60,
               "steps": 30, "inputs": {}}, open(track, "w"))
    curves = W("apply-state-curves.json")
    doc = run_json(cli, ["apply-state", created, "--track", track,
                         "--state", "bliss", "--out", curves], 0)
    check("F-7 apply-state composes on the CLI",
          doc.get("series", 0) > 0 and os.path.isfile(curves))

    # -- Item 5: paths -------------------------------------------
    print("== paths (Addition S) ==")
    env = {"TOROGAO_DATA_PATH": W("forced-data"),
           "TOROGAO_CONFIG_PATH": W("forced-config"),
           "TOROGAO_STATE_PATH": W("forced-state"),
           "TOROGAO_CACHE_PATH": W("forced-cache")}
    doc = run_json(cli, ["paths"], 0, env=env)
    check("forced-path overrides win on every kind",
          doc.get("data") == W("forced-data")
          and doc.get("config") == W("forced-config")
          and doc.get("state") == W("forced-state")
          and doc.get("cache") == W("forced-cache"), f"doc={doc}")
    check("playroom defaults ride the data dir",
          doc.get("playroomSets", "").endswith(
              os.path.join("playroom", "sets")))

    # -- Item 1: scenes -------------------------------------------
    print("== scenes ==")
    doc = run_json(cli, ["scene-info", "conditioning-lab"], 0)
    check("conditioning lab census",
          doc.get("fluidEmitters") == 1 and doc.get("fixtures") == 2
          and doc.get("props") == 2, f"doc={doc}")
    doc = run_json(cli, ["scene-validate", "conditioning-lab",
                         "--project", created, "--pack", "adult"], 0)
    check("conditioning lab validates grounded",
          doc.get("result") == "valid", f"doc={str(doc)[:300]}")
    scene = json.load(open(os.path.join(
        os.path.dirname(cli), "..", "..", "..", "scenes",
        "conditioning-lab.json"))) \
        if os.path.exists(os.path.join(os.path.dirname(cli), "..",
                                       "..", "..", "scenes",
                                       "conditioning-lab.json")) \
        else None
    # (the scene file is located by the CLI itself; mutations are
    # written to the work dir and named by path)
    repo_scene = None
    for cand in (os.path.join(os.path.dirname(cli), "..", "..", "..",
                              "scenes", "conditioning-lab.json"),
                 "scenes/conditioning-lab.json"):
        if os.path.exists(cand):
            repo_scene = os.path.abspath(cand)
            break
    scene = json.load(open(repo_scene))
    bad_anchor = json.loads(json.dumps(scene))
    bad_anchor["fixtures"][0]["anchors"][0]["part"] = "NoSuchPart"
    p = W("scene-bad-anchor.json")
    json.dump(bad_anchor, open(p, "w"))
    doc = run_json(cli, ["scene-validate", p, "--project", created,
                         "--pack", "adult"], 1)
    check("unknown anchor part -> TG-SCENE-UNKNOWN-PART",
          "TG-SCENE-UNKNOWN-PART" in codes(doc), f"codes={codes(doc)}")
    bad_consent = json.loads(json.dumps(scene))
    bad_consent["characters"][0]["consent"]["adult"] = False
    p = W("scene-bad-consent.json")
    json.dump(bad_consent, open(p, "w"))
    doc = run_json(cli, ["scene-validate", p, "--project", created,
                         "--pack", "adult"], 1)
    check("consent record failing the pack model -> TG-SCENE-CONSENT",
          "TG-SCENE-CONSENT" in codes(doc), f"codes={codes(doc)}")

    print("== fluids ==")
    fluid_out = W("fluid.json")
    run_json(cli, ["fluid-sim", "conditioning-lab", "--project",
                   maren_ha, "--seconds", "4", "--out", fluid_out], 0)
    runs = json.load(open(fluid_out))["runs"]
    check("milk run lands and touches the face as 'trance'",
          len(runs) == 1 and runs[0]["landings"]
          and runs[0]["faceContacts"]
          and runs[0]["faceContacts"][0]["expression"] == "trance",
          f"runs={str(runs)[:200]}")
    shas = set()
    for i in range(3):
        o = W(f"fluid-det-{i}.json")
        run_json(cli, ["fluid-sim", "conditioning-lab", "--project",
                       maren_ha, "--seconds", "4", "--out", o], 0)
        shas.add(sha256(o))
    check("fluid record byte-identical x3", len(shas) == 1)

    # -- Item 8: effects -------------------------------------------
    print("== effects ==")
    fxdoc_path = W("fx.json")
    json.dump(scene["effects"], open(fxdoc_path, "w"))
    doc = run_json(cli, ["effects-validate", fxdoc_path,
                         "--project", created], 0)
    check("the lab's stack validates against the project",
          doc.get("result") == "valid", f"doc={str(doc)[:200]}")
    manifest = W("fx-manifest.json")
    doc = run_json(cli, ["effects-compile", fxdoc_path, "--project",
                         created, "--out", manifest], 0)
    man = json.load(open(manifest))
    dash = [e for e in man["effects"] if e["kind"] == "dashboard"][0]
    check("compile resolves dashboard bindings to live parameters",
          all(b.get("resolved") is True for b in dash["bindings"]))
    badfx = json.loads(json.dumps(scene["effects"]))
    badfx["effects"][1]["bindings"][0]["parameter"] = "ParamNope"
    p = W("fx-bad.json")
    json.dump(badfx, open(p, "w"))
    doc = run_json(cli, ["effects-validate", p, "--project", created], 1)
    check("unknown dashboard parameter -> TG-FX-BINDING",
          "TG-FX-BINDING" in codes(doc), f"codes={codes(doc)}")

    # -- Item 7: controllers + override ------------------------------
    print("== controllers + override ==")
    ctrl = W("ctrl.json")
    json.dump({"format": "torogao-controller-values/1",
               "values": {"trance-depth": 1}}, open(ctrl, "w"))
    out = W("ctrl-curves.json")
    run_json(cli, ["physics-preview", created, "--track", track,
                   "--controllers", ctrl, "--out", out], 0)
    series = {s["parameter"]: s["values"]
              for s in json.load(open(out))["series"]}
    check("controller fans out (eyes close toward trance)",
          abs(series["ParamEyeLOpen"][0] - 0.15) < 1e-4,
          f"eye={series.get('ParamEyeLOpen', [None])[0]}")
    badctrl_ext = mutated("badctrl.inx", lambda e: e.update(
        {"controllers": [{"id": "x", "label": "X", "min": 0, "max": 1,
                          "default": 0,
                          "bindings": [{"parameter": "ParamNope",
                                          "gain": 1, "offset": 0}]}]}))
    val = run_json(cli, ["validate", badctrl_ext], 1)
    check("controller binding an unknown parameter refuses validation",
          "TG-CONTROLLER-UNKNOWN-PARAM" in codes(val),
          f"codes={codes(val)}")
    badvalues = W("ctrl-bad.json")
    json.dump({"format": "torogao-controller-values/1",
               "values": {"nope": 1}}, open(badvalues, "w"))
    proc = run(cli, ["physics-preview", created, "--track", track,
                     "--controllers", badvalues, "--out", W("z.json")], 3)
    check("values naming an unknown controller refuse",
          "unknown controller" in proc.stderr, f"stderr={proc.stderr[:150]}")

    # -- Item 6: tracking ----------------------------------------------
    print("== tracking ==")
    targets = W("targets.json")
    json.dump({"targets": [{"time": 0, "x": 0, "y": 0},
                           {"time": 2, "x": 20, "y": -10},
                           {"time": 4, "x": -15, "y": 5}]},
              open(targets, "w"))
    gaze = W("gaze-take.json")
    run_json(cli, ["gaze-take", "--targets", targets, "--seed", "7",
                   "--seconds", "4", "--out", gaze], 0)
    doc = run_json(cli, ["tracking-validate", gaze, "--pack", "adult"], 0)
    check("generated gaze take validates against its set",
          doc.get("result") == "valid", f"doc={str(doc)[:200]}")
    baked = W("gaze-curves.json")
    run_json(cli, ["tracking-bake", gaze, "--pack", "adult",
                   "--seconds", "1", "--out", baked], 0)
    bdoc = json.load(open(baked))
    check("bake emits the two gaze series at the step grid",
          len(bdoc["series"]) == 2 and bdoc["steps"] == 60,
          f"doc={str(bdoc)[:150]}")
    bad_take = {"format": "torogao-tracking-take/1", "source": "x",
                "set": "face-standard", "fps": 60,
                "frames": [{"time": 0, "values": {"ParamNope": 1}}]}
    p = W("take-bad.json")
    json.dump(bad_take, open(p, "w"))
    doc = run_json(cli, ["tracking-validate", p, "--pack", "adult"], 1)
    check("unknown tracking ID -> TG-TRACK-UNKNOWN-ID",
          "TG-TRACK-UNKNOWN-ID" in codes(doc), f"codes={codes(doc)}")
    bad_take["frames"][0]["values"] = {"ParamAngleX": 45}
    json.dump(bad_take, open(p, "w"))
    doc = run_json(cli, ["tracking-validate", p, "--pack", "adult"], 1)
    check("out-of-range value -> TG-TRACK-RANGE",
          "TG-TRACK-RANGE" in codes(doc), f"codes={codes(doc)}")
    # override: baked gaze curves replace the track per parameter
    empty_track = W("empty-track.json")
    json.dump({"format": "torogao-input-track/1", "fps": 60,
               "steps": 60, "inputs": {}}, open(empty_track, "w"))
    out = W("override-curves.json")
    run_json(cli, ["physics-preview", created, "--track", empty_track,
                   "--override", baked, "--out", out], 0)
    oseries = {s["parameter"] for s in json.load(open(out))["series"]}
    check("override take composes into the preview",
          "ParamAngleX" in oseries)
    rich_track = W("rich-track.json")
    json.dump({"format": "torogao-input-track/1", "fps": 60,
               "steps": 60,
               "inputs": {"ParamAngleX": [0.0] * 60}},
              open(rich_track, "w"))
    proc = run(cli, ["physics-preview", created, "--track", rich_track,
                     "--override", baked, "--out", W("z2.json")], None)
    check("track + take on one parameter refuses (two authored layers)",
          proc.returncode != 0
          and "both drive parameter" in (proc.stdout + proc.stderr),
          f"rc={proc.returncode} out={proc.stdout[:150]}")

    # -- Items 2/3: Playroom + prefsets ----------------------------------
    print("== playroom ==")
    session = W("session.json")
    doc = run_json(cli, ["playroom-create", "--identity", identity,
                         "--pack", "adult",
                         "--out", W("pr-character.inx"),
                         "--session", session,
                         "--scene", "conditioning-lab",
                         "--activity", "conditioning"], 0)
    check("playroom-create makes character + session in one call",
          doc.get("result") == "created" and os.path.isfile(session))
    run_json(cli, ["playroom-set", session, "--field", "vocalMode",
                   "--value", "dirty-talk"], 0)
    sdoc = json.load(open(session))
    check("playroom-set updates the vocal lane",
          sdoc["vocalLane"]["mode"] == "dirty-talk")
    proc = run(cli, ["playroom-set", session, "--field", "nonsense",
                     "--value", "x"], 1)
    check("playroom-set refuses an unknown field", proc.returncode == 1)
    # point the session at the hit-area project and compose
    run_json(cli, ["playroom-open", "--character", maren_ha,
                   "--pack", "adult", "--scene", "conditioning-lab",
                   "--activity", "conditioning",
                   "--session", session, "--force"], 0)
    snap = W("snapshot.json")
    doc = run_json(cli, ["playroom-compose", session, "--out", snap], 0)
    check("compose fires the fluid face-contact expression (trance)",
          "trance" in doc.get("expressionsFired", []),
          f"fired={doc.get('expressionsFired')}")
    sets_dir = W("sets")
    good_set = {"format": "torogao-preference-set/1", "name": "lab-kit",
                "wardrobe": [{"article": "trance-visor",
                              "state": "worn"}],
                "morphs": {}, "appearance": {}}
    set_file = W("lab-kit.json")
    json.dump(good_set, open(set_file, "w"))
    run_json(cli, ["prefset-save", "--name", "lab-kit", "--dir",
                   sets_dir, "--set", set_file], 0)
    doc = run_json(cli, ["prefset-list", "--dir", sets_dir], 0)
    check("prefset round-trips through the sets dir",
          doc.get("sets") == ["lab-kit"], f"doc={doc}")
    run_json(cli, ["prefset-apply", session, "--set", "lab-kit",
                   "--dir", sets_dir], 0)
    sdoc = json.load(open(session))
    check("applying the set lands the wardrobe on the session",
          sdoc.get("wardrobe") == good_set["wardrobe"])
    bad_set = {"format": "torogao-preference-set/1", "name": "bad",
               "wardrobe": [{"article": "jetpack", "state": "worn"}]}
    json.dump(bad_set, open(W("bad.json"), "w"))
    run_json(cli, ["prefset-save", "--name", "bad", "--dir", sets_dir,
                   "--set", W("bad.json")], 0)
    proc = run(cli, ["prefset-apply", session, "--set", "bad",
                     "--dir", sets_dir], 1)
    check("unknown wardrobe article refuses TG-WARDROBE-UNKNOWN-ARTICLE",
          "TG-WARDROBE-UNKNOWN-ARTICLE" in proc.stdout,
          f"stdout={proc.stdout[:200]}")

    # -- Item 9: GLB -------------------------------------------------------
    print("== glb ==")
    glb = W("maren.glb")
    make_glb(glb)
    doc = run_json(cli, ["glb-info", glb], 0)
    check("GLB census reads morphs, skins, animations",
          doc.get("morphTargets") == ["Smile", "EyesClosed"]
          and doc.get("animations") == 1
          and doc.get("skins") == ["BodySkin"], f"doc={doc}")
    governed = ["neutral", "smile", "joy", "playful", "longing",
                "desire", "arousal", "trance", "overwhelmed", "bliss"]
    expr_map = {"format": "torogao-expression-map-3d/1",
                "character": "Maren", "glb": "maren.glb",
                "expressions": {
                    name: {"morphs": {"Smile": 0.5, "EyesClosed": 0.25}}
                    for name in governed}}
    map_path = W("expr-map.json")
    json.dump(expr_map, open(map_path, "w"))
    doc = run_json(cli, ["glb-expression-check", map_path, "--glb", glb,
                         "--pack", "adult"], 0)
    check("every governed expression resolves in 3D",
          doc.get("result") == "complete", f"doc={doc}")
    del expr_map["expressions"]["bliss"]
    json.dump(expr_map, open(map_path, "w"))
    doc = run_json(cli, ["glb-expression-check", map_path, "--glb", glb,
                         "--pack", "adult"], 1)
    check("a missing governed expression -> TG-EXPR-MISSING",
          any("TG-EXPR-MISSING" in p for p in doc.get("problems", [])),
          f"doc={doc}")
    doc = run_json(cli, ["glb-resolve-expression", map_path,
                         "--name", "trance"], 0)
    check("resolve returns the morph weights",
          doc.get("morphs", {}).get("Smile") == 0.5)

    # -- Item 10: video ------------------------------------------------------
    print("== video ==")
    clips = W("clips.json")
    json.dump({"tempo": {"bpm": 120},
               "clips": [{"id": "intro", "source": "a.mp4",
                          "in": 0, "out": 2},
                         {"id": "main", "source": "b.mp4",
                          "in": 1, "out": 5}]}, open(clips, "w"))
    timeline = W("timeline.json")
    doc = run_json(cli, ["video-build", clips, "--out", timeline], 0)
    check("video-build sequences clips back-to-back (6 s)",
          doc.get("duration") == 6, f"doc={doc}")
    doc = run_json(cli, ["video-validate", timeline], 0)
    check("built timeline validates", doc.get("result") == "valid")
    doc = run_json(cli, ["video-tempo", timeline], 0)
    check("tempo report: every boundary on the 120 bpm grid",
          doc.get("onBeat") == doc.get("total") == 4, f"doc={doc}")
    overlap = json.load(open(timeline))
    overlap["tracks"][0]["clips"][1]["start"] = 1
    p = W("timeline-overlap.json")
    json.dump(overlap, open(p, "w"))
    doc = run_json(cli, ["video-validate", p], 1)
    check("overlapping clips on one track -> TG-VIDEO-OVERLAP",
          "TG-VIDEO-OVERLAP" in codes(doc), f"codes={codes(doc)}")
    edl_dir = W("edl")
    doc = run_json(cli, ["video-export", timeline, "--out", edl_dir], 0)
    check("export writes the three edit-decision artifacts",
          all(os.path.isfile(os.path.join(edl_dir, n))
              for n in ("concat.txt", "edl.json", "chapters.txt")))

    # -- Item 11: Scenes track -------------------------------------------
    print("== scenes track ==")
    listing = run_json(cli, ["tutorial-list", "adult", "--track",
                             "scenes"], 0)
    check("adult Scenes track lists 6 lessons",
          listing.get("track") == "scenes"
          and len(listing.get("lessons", [])) == 6)
    progress = W("scenes-progress.json")
    for lesson in ("scenes-meet-the-lab", "scenes-room-that-breathes",
                   "scenes-machines-and-rigs", "scenes-the-visor",
                   "scenes-fluids", "scenes-capstone"):
        doc = run_json(cli, ["tutorial-check", "adult", lesson, created,
                             "--track", "scenes",
                             "--progress", progress], 0)
        check(f"scenes lesson {lesson} passes",
              doc.get("passed") is True, f"doc={str(doc)[:200]}")
    status = run_json(cli, ["tutorial-status", "--pack", "adult",
                            "--track", "scenes",
                            "--progress", progress], 0)
    check("Scenes track totals 110 XP",
          status.get("xpTotal") == 110, f"status={status}")

    # -- registry + MCP -----------------------------------------------------
    print("== registry ==")
    dump = run_json(cli, ["registry-dump"], 0)
    names = {t["name"] for t in dump["tools"]}
    p5 = {"scene-info", "scene-validate", "fluid-sim",
          "playroom-open", "playroom-set", "playroom-compose",
          "playroom-create", "prefset-list", "prefset-save",
          "prefset-apply", "tracking-validate", "tracking-bake",
          "effects-validate", "effects-compile", "glb-info",
          "glb-expression-check", "glb-resolve-expression",
          "video-validate", "video-build", "video-export",
          "video-tempo"}
    check("registry v5.0.0 carries 57 tools incl. the 21 P5 tools",
          dump.get("version") == "5.0.0" and len(names) == 57
          and p5 <= names,
          f"version={dump.get('version')} count={len(names)}")
    proc = subprocess.Popen([cli, "mcp"], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, text=True)

    def send(obj):
        proc.stdin.write(json.dumps(obj) + "\n")
        proc.stdin.flush()

    def recv():
        return json.loads(proc.stdout.readline())

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2024-11-05", "capabilities": {},
        "clientInfo": {"name": "p5-harness", "version": "1.0"}}})
    recv()
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
        "name": "scene-validate",
        "arguments": {"scene": "conditioning-lab", "project": created,
                      "pack": "adult"}}})
    resp = recv()
    content = resp.get("result", {}).get("content", [])
    text = content[0]["text"] if content else ""
    called = json.loads(text) if text else {}
    check("MCP tools/call reaches the p5 family (scene-validate)",
          called.get("result") == "valid", f"resp={str(resp)[:200]}")
    proc.stdin.close()
    proc.wait(timeout=30)

    print(f"\n{CHECKS[0]} checks, {len(FAILURES)} failures")
    if FAILURES:
        print("FAILURES:", *FAILURES, sep="\n  ")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
