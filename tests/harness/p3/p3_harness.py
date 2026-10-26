#!/usr/bin/env python3
"""
P3 harness for torogao-cli (Torogao P3 gate).

Exercises the P3 surface end to end against the freshly
built CLI: lane packs, rating stamps + the two purpose-broken
rating fixtures, expression states + coverage (incl. the
state-removed fixture), stage variants, blend shapes, glue
visibility, tracking sets, ambient drivers (60 s idle replay
x3 byte-stability + per-driver on/off deltas), hit areas
(round-trip + unknown-part refusal), the Character Creator
gate (create -> validate -> coverage -> tracking -> export
through the model validator -> physics preview), the writer
core (scaffold + the founding-counterexample gap report +
rupture slot + verbatim refrain), voice/SFX data, and the
Item 0 stepping regression (the corrected rich fixture,
stepped — the path that used to segfault).

Usage: p3_harness.py <torogao-cli> <fixtures-dir> <work-dir>
       <p2-fixtures-dir> <rig-check-binary>
Exit code 0 = all checks pass.
"""
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


def run(cli, args, expect_rc=None):
    proc = subprocess.run([cli] + args, capture_output=True, text=True,
                          timeout=300)
    if expect_rc is not None:
        check(f"rc {args[0]} == {expect_rc}", proc.returncode == expect_rc,
              f"got {proc.returncode}; stderr={proc.stderr[:300]}")
    return proc


def run_json(cli, args, expect_rc=None):
    proc = run(cli, args, expect_rc)
    try:
        return json.loads(proc.stdout)
    except Exception:
        check(f"json output of {args[0]}", False,
              f"stdout={proc.stdout[:300]} stderr={proc.stderr[:300]}")
        return {}


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
    prefix = data[:at]           # magic + payload + tex section
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


def main():
    cli, fixdir, workdir, p2fix, rigcheck = sys.argv[1:6]
    os.makedirs(workdir, exist_ok=True)

    def W(name):
        return os.path.join(workdir, name)

    identity = os.path.join(fixdir, "identity-maren.json")
    created = W("maren.inx")

    # -- registry growth ------------------------------------
    print("== registry ==")
    reg = run_json(cli, ["registry-dump"], 0)
    names = {t["name"] for t in reg["tools"]}
    new_tools = {"pack-info", "rating-check", "list-states",
                 "apply-state", "expression-check", "tracking-check",
                 "hitareas-list", "hitareas-author", "create-character",
                 "writer-scaffold", "voice-check", "sfx-list"}
    check("registry version (3.0.0 at P3; 4.0.0 at P4; 5.0.0 since P5)",
          reg["version"] == "5.0.0")
    check("registry holds 57 tools (25 at P3, +11 in P4, +21 in P5)",
          len(reg["tools"]) == 57,
          f"got {len(reg['tools'])}")
    check("all 12 P3 tools registered", new_tools <= names,
          f"missing {new_tools - names}")

    # -- Item 1: packs ---------------------------------------
    print("== packs ==")
    adult = run_json(cli, ["pack-info", "adult"], 0)["pack"]
    ages = run_json(cli, ["pack-info", "all-ages"], 0)["pack"]
    check("adult pack: 10 governed expressions",
          len(adult["expressions"]) == 10)
    check("all-ages pack: 8 governed expressions",
          len(ages["expressions"]) == 8)
    check("packs share one format",
          adult["format"] == ages["format"] == "torogao-pack/1")
    check("adult pack consent model is schema",
          adult["consentModel"]["requiredFields"]
          == {"adult": True, "wantsSelectedScenes": True})
    fb = [s for s in adult["trackingSets"] if s["id"] == "full-body"][0]
    check("full-body set completed from the mocopi table (P4 item 7)",
          fb["completeness"] == "complete"
          and fb.get("groupsPendingSource") == []
          and len(fb["ids"]) == 86)

    # -- Item 8: Creator gate --------------------------------
    print("== creator ==")
    summary = run_json(cli, ["create-character", "--identity", identity,
                             "--pack", "adult", "--preset",
                             "hair-sway-2step", "--out", created], 0)
    check("create-character result", summary.get("result") == "created")
    check("created project carries 10 states",
          summary.get("states") == 10)
    run(cli, ["validate", created], 0)
    expr = run_json(cli, ["expression-check", created, "--pack", "adult"], 0)
    check("created project passes expression coverage",
          expr.get("result") == "covered" and expr.get("missing") == [])
    trk = run_json(cli, ["tracking-check", created, "--pack", "adult",
                         "--set", "face-standard"], 0)
    check("created project passes face-standard tracking",
          trk.get("result") == "covered")
    trkfb = run_json(cli, ["tracking-check", created, "--pack", "adult",
                           "--set", "full-body"], 1)
    check("full-body set is complete; a template rig covers its 16 "
          "and reports the 70 mocopi IDs missing (P4 item 7)",
          trkfb.get("result") == "findings"
          and trkfb.get("completeness") == "complete"
          and len(trkfb.get("missing", [])) == 70)
    export_dir = W("export-maren")
    report = run_json(cli, ["export-rig", created, "--out", export_dir], 0)
    check("export carries the adult rating stamp",
          report.get("ratingStamp", {}).get("rating") == "adult"
          and report.get("ratingStamp", {}).get("stamped") is True)
    check("stamp sidecar written",
          os.path.isfile(os.path.join(export_dir, "maren.stamp.json")))
    model = os.path.join(export_dir, "maren.model.json")
    proc = subprocess.run([rigcheck, model], capture_output=True,
                          text=True, timeout=120)
    check("export passes the model validator", proc.returncode == 0,
          proc.stdout[:200] + proc.stderr[:200])

    # -- Item 2: states in preview ----------------------------
    print("== states / variants / blend shapes ==")
    track_small = W("track-small.json")
    json.dump({"format": "torogao-input-track/1", "fps": 60,
               "steps": 120, "inputs": {}}, open(track_small, "w"))
    curves = W("curves-trance.json")
    run(cli, ["physics-preview", created, "--track", track_small,
              "--state", "trance", "--out", curves], 0)
    doc = json.load(open(curves))
    series = {s["parameter"]: s["values"] for s in doc["series"]}
    check("state trance drives ParamEyeLOpen to 0.3",
          all(abs(v - 0.3) < 1e-6 for v in series["ParamEyeLOpen"]))
    check("curve document records the state layer",
          doc.get("state") == "trance")
    proc = run(cli, ["physics-preview", created, "--track", track_small,
                     "--state", "no-such-state"], None)
    check("unknown state is a typed preview error",
          proc.returncode == 3 and "not stored" in proc.stdout)

    # variant + blend shape fixture (EXT surgery on a copy)
    payload, tex, sections = read_inp(created)
    ext = ext_of(sections)
    ext["variants"] = [{
        "id": "stage-3-trance",
        "label": "Stage 3",
        "recipe": {"ParamEyeLOpen": 0.3, "ParamEyeROpen": 0.3},
        "visibility": {"HairAnchor": True},
        "appearance": {"Head": {"opacity": 0.9}},
    }]
    ext["blendshapes"] = [{
        "name": "CheekPuff", "parameter": "ParamMouthX",
        "deltas": [{"part": "Mouth", "amount": 0.5}],
    }]
    ext["states"].append({"name": "puff-test",
                          "recipe": {"CheekPuff": 0.7}})
    varianted = W("maren-variant.inx")
    write_inp(varianted, payload, tex, with_ext(sections, ext))
    run(cli, ["validate", varianted], 0)
    curves = W("curves-variant.json")
    run(cli, ["physics-preview", varianted, "--track", track_small,
              "--variant", "stage-3-trance", "--out", curves], 0)
    doc = json.load(open(curves))
    series = {s["parameter"]: s["values"] for s in doc["series"]}
    check("variant recipe drives ParamEyeROpen to 0.3",
          all(abs(v - 0.3) < 1e-6 for v in series["ParamEyeROpen"]))
    curves = W("curves-blend.json")
    run(cli, ["physics-preview", varianted, "--track", track_small,
              "--state", "puff-test", "--out", curves], 0)
    doc = json.load(open(curves))
    series = {s["parameter"]: s["values"] for s in doc["series"]}
    check("state targets a blend shape, folded to ParamMouthX 0.7",
          all(abs(v - 0.7) < 1e-6 for v in series["ParamMouthX"]))

    # variant referencing an unknown part -> typed error
    ext2 = json.loads(json.dumps(ext))
    ext2["variants"] = [{"id": "bad", "recipe": {},
                         "visibility": {"NoSuchPart": True}}]
    bad_variant = W("maren-bad-variant.inx")
    write_inp(bad_variant, payload, tex, with_ext(sections, ext2))
    val = run_json(cli, ["validate", bad_variant], 1)
    codes = {f["code"] for f in val["findings"]}
    check("unknown-part variant fires TG-VARIANT-UNKNOWN-PART",
          "TG-VARIANT-UNKNOWN-PART" in codes, str(codes))

    # -- Item 3: coverage failure fixture ----------------------
    print("== expression coverage failure ==")
    ext3 = json.loads(json.dumps(ext))
    ext3["states"] = [s for s in ext3["states"] if s["name"] != "bliss"]
    ext3["variants"] = []
    ext3["blendshapes"] = []
    broken = W("maren-missing-bliss.inx")
    write_inp(broken, payload, tex, with_ext(sections, ext3))
    expr = run_json(cli, ["expression-check", broken, "--pack", "adult"], 1)
    check("missing governed name is listed",
          expr.get("missing") == ["bliss"]
          and expr.get("code") == "TG-EXPR-MISSING", json.dumps(expr)[:200])

    # -- Item 1 gates: rating fixtures -------------------------
    print("== rating gates ==")
    ext4 = json.loads(json.dumps(ext))
    ext4["meta"]["lane"] = "all-ages"   # rating stays "adult"
    mismatch = W("maren-lane-mismatch.inx")
    write_inp(mismatch, payload, tex, with_ext(sections, ext4))
    val = run_json(cli, ["validate", mismatch], 1)
    codes = {f["code"] for f in val["findings"]}
    check("adult under all-ages lane fires TG-RATING-LANE-MISMATCH",
          "TG-RATING-LANE-MISMATCH" in codes, str(codes))
    run(cli, ["rating-check", mismatch], 1)
    run(cli, ["export-rig", mismatch, "--out", W("export-mismatch")], 4)

    ext5 = json.loads(json.dumps(ext))
    del ext5["meta"]["basis"]
    nostamp = W("maren-stamp-missing.inx")
    write_inp(nostamp, payload, tex, with_ext(sections, ext5))
    val = run_json(cli, ["validate", nostamp], 1)
    codes = {f["code"] for f in val["findings"]}
    check("missing stamp field fires TG-RATING-STAMP-MISSING",
          "TG-RATING-STAMP-MISSING" in codes, str(codes))

    # -- Item 4: glue visibility --------------------------------
    print("== glue ==")
    payload_obj = json.loads(payload)
    payload_obj["glue"] = [{"a": 1, "b": 2, "weight": 0.5}]
    glued = W("maren-glued.inx")
    write_inp(glued, json.dumps(payload_obj).encode(), tex, sections)
    val = run_json(cli, ["validate", glued], 1)
    codes = {f["code"] for f in val["findings"]}
    check("glue is reported, never silently dropped",
          "TG-GLUE-UNMAPPED" in codes, str(codes))
    insp = run_json(cli, ["inspect", glued], 0)
    check("inspect counts the glue records",
          insp["extension"]["glueRecords"] == 1)

    # -- Item 6: ambient drivers --------------------------------
    print("== ambient drivers ==")
    track60 = W("track-60s.json")
    json.dump({"format": "torogao-input-track/1", "fps": 60,
               "steps": 3600, "inputs": {}}, open(track60, "w"))
    drivers = os.path.join(fixdir, "drivers-idle.json")
    hashes = []
    for i in range(3):
        out = W(f"curves-idle-{i}.json")
        run(cli, ["physics-preview", created, "--track", track60,
                  "--drivers", drivers, "--out", out], 0)
        import hashlib
        hashes.append(hashlib.sha256(open(out, "rb").read()).hexdigest())
    check("60 s idle replay is byte-stable across 3 runs",
          hashes[0] == hashes[1] == hashes[2], str(hashes))
    idle = json.load(open(W("curves-idle-0.json")))
    idle_series = {s["parameter"]: s["values"] for s in idle["series"]}
    check("breath driver moves ParamBreath",
          max(idle_series["ParamBreath"]) - min(idle_series["ParamBreath"])
          > 0.5)
    check("blink driver closes ParamEyeLOpen sometimes",
          min(idle_series["ParamEyeLOpen"]) < 0.05
          and max(idle_series["ParamEyeLOpen"]) > 0.95)
    check("sway driver moves ParamAngleZ",
          max(idle_series["ParamAngleZ"]) - min(idle_series["ParamAngleZ"])
          > 1.0)

    delta_calls = [0]

    def delta(driver_names, param, track=None):
        cfg = W("drivers-one.json")
        json.dump({"drivers": driver_names, "seed": 7}, open(cfg, "w"))
        # Unique output per call: since P5 (surge F-3) the CLI
        # refuses to overwrite an existing output without
        # --force, by design.
        delta_calls[0] += 1
        out = W(f"curves-one-{delta_calls[0]}.json")
        run(cli, ["physics-preview", created, "--track",
                  track or track_small,
                  "--drivers", cfg, "--out", out], 0)
        d = json.load(open(out))
        s = {x["parameter"]: x["values"] for x in d["series"]}
        return max(s[param]) - min(s[param])

    def plain(param):
        out = W("curves-plain.json")
        run(cli, ["physics-preview", created, "--track", track_small,
                  "--out", out], 0)
        d = json.load(open(out))
        s = {x["parameter"]: x["values"] for x in d["series"]}
        return max(s[param]) - min(s[param])

    check("driver delta: breath alone moves ParamBreath",
          delta(["breath"], "ParamBreath", track60) > 0.5
          and plain("ParamBreath") == 0)
    check("driver delta: blink alone closes the eyes",
          delta(["blink"], "ParamEyeLOpen") > 0.9)
    check("driver delta: sway alone moves ParamAngleZ",
          delta(["sway"], "ParamAngleZ") > 1.0)

    # saccade needs eyeball parameters: add them by payload
    # surgery (preview-only fixture) and measure the delta.
    payload_obj = json.loads(payload)
    payload_obj["param"].append({
        "uuid": 777001, "name": "ParamEyeBallX", "is_vec2": False,
        "min": [-1.0, 0.0], "max": [1.0, 0.0], "defaults": [0.0, 0.0],
        "axis_points": [[-1.0, 0.0, 1.0], [0.0]], "bindings": []})
    payload_obj["param"].append({
        "uuid": 777002, "name": "ParamEyeBallY", "is_vec2": False,
        "min": [-1.0, 0.0], "max": [1.0, 0.0], "defaults": [0.0, 0.0],
        "axis_points": [[-1.0, 0.0, 1.0], [0.0]], "bindings": []})
    eyeball = W("maren-eyeball.inx")
    write_inp(eyeball, json.dumps(payload_obj).encode(), tex, sections)
    cfg = W("drivers-saccade.json")
    json.dump({"drivers": ["saccade"], "seed": 7}, open(cfg, "w"))
    out = W("curves-saccade.json")
    run(cli, ["physics-preview", eyeball, "--track", track60,
              "--drivers", cfg, "--out", out], 0)
    d = json.load(open(out))
    s = {x["parameter"]: x["values"] for x in d["series"]}
    check("driver delta: saccade moves ParamEyeBallX in steps",
          max(s["ParamEyeBallX"]) - min(s["ParamEyeBallX"]) > 0.3
          and len(set(s["ParamEyeBallX"])) > 2)

    # -- Item 7: hit areas ---------------------------------------
    print("== hit areas ==")
    authored = W("maren-hit.inx")
    spec = os.path.join(fixdir, "hitareas.json")
    run(cli, ["hitareas-author", created, "--spec", spec,
              "--out", authored], 0)
    back = run_json(cli, ["hitareas-list", authored], 0)
    want = json.load(open(spec))["hitAreas"]
    check("hit areas round-trip identical",
          back.get("hitAreas") == want,
          json.dumps(back.get("hitAreas"))[:200])
    run(cli, ["validate", authored], 0)
    proc = run(cli, ["hitareas-author", created, "--spec",
                     os.path.join(fixdir, "hitareas-broken.json"),
                     "--out", W("maren-hit-bad.inx")], 1)
    check("unknown-part hit area refuses the write",
          "TG-HITAREA-UNKNOWN-PART" in proc.stdout)

    # -- Item 10: writer core -------------------------------------
    print("== writer ==")
    scaffold_path = W("scaffold.json")
    scaf = run_json(cli, ["writer-scaffold", "--character", identity,
                          "--pack", "adult", "--kink", "conditioning",
                          "--stage", "0", "--sfx", "cue-chime",
                          "--out", scaffold_path], 0)
    check("scaffold offered for an in-vector kink",
          scaf.get("result") == "scaffold")
    check("scaffold checks all pass",
          all(v == "pass" for v in scaf["checks"].values()),
          json.dumps(scaf.get("checks")))
    check("refrain is character-exact",
          scaf.get("refrain") == "Steady hands, steady heart.")
    cue = scaf["sceneScript"]["audioCues"][0]
    check("SFX cue attaches with a complete manifest hash",
          cue["sfx"] == "cue-chime" and len(cue["recipeSha256"]) == 64)
    check("scaffold voice ref carries the anchor version",
          scaf["sceneScript"]["voiceRef"]["anchorVersion"] == 1)
    gap = run_json(cli, ["writer-scaffold", "--character", identity,
                         "--pack", "adult", "--kink", "feeding",
                         "--stage", "0"], 1)
    check("out-of-vector kink produces the gap report",
          gap.get("result") == "gap-report"
          and gap.get("code") == "TG-WRITER-GAP"
          and any("feeding" in m for m in gap["missingCanon"]),
          json.dumps(gap)[:300])
    scaf4 = run_json(cli, ["writer-scaffold", "--character", identity,
                           "--pack", "adult", "--kink", "hypnosis",
                           "--stage", "4"], 0)
    check("rupture line lands at its structural slot, verbatim",
          scaf4.get("ruptureLine")
          == "The hands everyone trusts are mine to steady — or to still.")
    ident_obj = json.load(open(identity))
    del ident_obj["refrain"]
    noref = W("identity-norefrain.json")
    json.dump(ident_obj, open(noref, "w"))
    gap = run_json(cli, ["writer-scaffold", "--character", noref,
                         "--pack", "adult", "--kink", "hypnosis",
                         "--stage", "0"], 1)
    check("missing refrain is a named gap, not an invention",
          gap.get("result") == "gap-report"
          and any("refrain" in m for m in gap["missingCanon"]))

    # -- Item 11: voice + SFX --------------------------------------
    print("== voice / sfx ==")
    run(cli, ["voice-check", identity], 0)
    bad = run_json(cli, ["voice-check",
                         os.path.join(fixdir, "voice-bad.json")], 1)
    codes = {f["code"] for f in bad["findings"]}
    check("non-estate provenance is a validation error",
          "TG-VOICE-PROVENANCE" in codes, str(codes))
    index = run_json(cli, ["sfx-list"], 0)
    ids = {e["id"] for e in index["entries"]}
    check("SFX library lists its entries",
          ids == {"cue-chime", "room-bed"}, str(ids))

    # -- Item 0: stepping regression --------------------------------
    print("== item 0 regression ==")
    rich = os.path.join(p2fix, "rich.inx")
    track = os.path.join(p2fix, "rich-track.json")
    out = W("curves-rich.json")
    run(cli, ["physics-preview", rich, "--track", track,
              "--out", out], 0)
    d = json.load(open(out))
    s = {x["parameter"]: x["values"] for x in d["series"]}
    check("corrected rich fixture steps cleanly, HeadTurn moves",
          max(s["HeadTurn"]) - min(s["HeadTurn"]) > 1.0)
    check("HairSway moves under its automation (D16 landed, P4)",
          max(s["HairSway"]) - min(s["HairSway"]) > 0.5)

    print(f"\n{CHECKS[0]} checks, {len(FAILURES)} failures")
    if FAILURES:
        print("FAILED:", ", ".join(FAILURES))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
