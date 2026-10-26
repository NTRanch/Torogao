#!/usr/bin/env python3
"""
P4 harness for torogao-cli (Torogao P4 gate).

Exercises the P4 surface end to end against the freshly built
CLI: the tutorial engine + Characters track (incl. the
Addition S default progress path), the writer model lane
(draft + the three named refusals + the gap short-circuit),
viseme authoring/validation/preview composition, the session
builder (byte-identical double build + both subliminal
refusals + an independent loudness recomputation), motion
events (round-trip + typed refusals), lip-sync analysis
(determinism x3, silence, a known tone, preview fold), the
mocopi completion (Item 7), the M fixture set (Item 8), and
registry accounting + one MCP call into the new family.

Usage: p4_harness.py <torogao-cli> <p4-fixtures-dir> <work-dir>
       <p3-fixtures-dir> <p2-fixtures-dir> <rig-check-binary>
       <repo-root>
Exit code 0 = all checks pass.
"""
import hashlib
import json
import math
import os
import shutil
import struct
import subprocess
import sys
import wave

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


def main():
    (cli, fixtures, work, p3_fixtures, p2_fixtures,
     rigcheck, repo) = sys.argv[1:8]
    cli, fixtures, work = (os.path.abspath(cli),
                           os.path.abspath(fixtures),
                           os.path.abspath(work))
    p3_fixtures, p2_fixtures = (os.path.abspath(p3_fixtures),
                                os.path.abspath(p2_fixtures))
    rigcheck, repo = os.path.abspath(rigcheck), os.path.abspath(repo)
    os.makedirs(work, exist_ok=True)

    def W(name):
        return os.path.join(work, name)

    identity = os.path.join(p3_fixtures, "identity-maren.json")
    rich = os.path.join(p2_fixtures, "rich.inx")
    created = W("maren.inx")
    run_json(cli, ["create-character", "--identity", identity,
                   "--pack", "adult", "--preset", "hair-sway-2step",
                   "--out", created], 0)

    # -- Item 1: tutorials -------------------------------------
    print("== tutorials ==")
    for pack in ("adult", "all-ages"):
        listing = run_json(cli, ["tutorial-list", pack], 0)
        check(f"{pack} lessons list the Characters track",
              listing.get("track") == "characters"
              and len(listing.get("lessons", [])) == 6)
    progress = W("progress.json")
    lessons = ["create-first-character", "know-your-states",
               "expression-coverage", "tracking-basics",
               "export-gate", "capstone-character"]
    for lesson in lessons:
        doc = run_json(cli, ["tutorial-check", "adult", lesson, created,
                             "--progress", progress], 0)
        check(f"lesson {lesson} passes on a creator project",
              doc.get("passed") is True and doc.get("recorded") is True)
    status = run_json(cli, ["tutorial-status", "--pack", "adult",
                            "--progress", progress], 0)
    check("progress totals 130 XP over 6 completions",
          status.get("xpTotal") == 130
          and status.get("completions") == 6
          and status.get("lessonsRemaining") == 0)
    again = run_json(cli, ["tutorial-check", "adult",
                            "create-first-character", created,
                            "--progress", progress], 0)
    check("re-checking a passed lesson records nothing twice",
          again.get("passed") is True and again.get("recorded") is False
          and again.get("xpTotal") == 130)
    failed = run_json(cli, ["tutorial-check", "adult",
                             "create-first-character", rich,
                             "--progress", progress], 1)
    check("a project with no consent record fails the lesson, typed",
          failed.get("passed") is False
          and failed.get("code") == "TG-TUTORIAL-CHECK-FAILED")
    unknown = run_json(cli, ["tutorial-check", "adult", "no-such-lesson",
                              created, "--progress", progress], 1)
    check("unknown lesson is a typed refusal",
          unknown.get("code") == "TG-TUTORIAL-LESSON-UNKNOWN")
    # Addition S: the default progress path is the XDG state dir.
    fake_home = W("fake-home")
    os.makedirs(fake_home, exist_ok=True)
    xdg = run_json(cli, ["tutorial-status"],
                   env={"HOME": fake_home,
                        "XDG_STATE_HOME": os.path.join(fake_home, "xdg")})
    check("default progress path resolves via XDG state (Addition S)",
          xdg.get("progressFile")
          == os.path.join(fake_home, "xdg", "torogao",
                          "tutorial-progress.json"))
    xdg2 = run_json(cli, ["tutorial-status"],
                    env={"HOME": fake_home, "XDG_STATE_HOME": ""})
    check("without XDG_STATE_HOME the fallback is ~/.local/state/torogao",
          xdg2.get("progressFile")
          == os.path.join(fake_home, ".local", "state", "torogao",
                          "tutorial-progress.json"))

    # -- Item 2: writer lane -------------------------------------
    print("== writer lane ==")
    draft_path = W("maren-draft.json")
    draft = run_json(cli, ["writer-draft", "--character", identity,
                           "--pack", "adult", "--kink", "hypnosis",
                           "--stage", "3", "--sfx", "cue-chime",
                           "--lane", os.path.join(fixtures,
                                                  "lane-fixture.json"),
                           "--out", draft_path], 0)
    check("fixture lane produces a gated draft end to end",
          draft.get("result") == "draft"
          and draft.get("checks") == {"denyScan": "pass",
                                      "wordBounds": "pass",
                                      "refrainClosing": "pass"})
    fills = draft.get("fills", {})
    check("draft fills carry measured word counts in bounds",
          20 <= fills.get("d1", {}).get("words", 0) <= 260
          and 150 <= fills.get("prose", {}).get("words", 0) <= 1200)
    check("the scene script rides the draft with its SFX cue hash",
          draft.get("scaffold", {}).get("sceneScript", {})
          .get("audioCues", [{}])[0].get("recipeSha256", "") != "")
    denied = run_json(cli, ["writer-draft", "--character", identity,
                            "--pack", "adult", "--kink", "hypnosis",
                            "--stage", "3",
                            "--lane", os.path.join(fixtures,
                                                   "lane-denied.json")], 1)
    check("out-of-vector vocabulary refuses the whole draft",
          denied.get("result") == "refused"
          and denied.get("code") == "TG-WRITER-DRAFT-DENIED")
    short = run_json(cli, ["writer-draft", "--character", identity,
                           "--pack", "adult", "--kink", "hypnosis",
                           "--stage", "3",
                           "--lane", os.path.join(fixtures,
                                                  "lane-short.json")], 1)
    check("an under-bound fill refuses the whole draft",
          short.get("result") == "refused"
          and short.get("code") == "TG-WRITER-DRAFT-WORDS")
    gap = run_json(cli, ["writer-draft", "--character", identity,
                         "--pack", "adult", "--kink", "bondage",
                         "--stage", "3",
                         "--lane", os.path.join(fixtures,
                                                "lane-gap.json")], 1)
    check("a canon gap returns the gap; the lane is never touched",
          gap.get("result") == "gap-report"
          and gap.get("code") == "TG-WRITER-GAP")

    # -- Item 3: viseme authoring --------------------------------
    print("== viseme authoring ==")
    spec = os.path.join(repo, "contracts",
                        "viseme-greeting-authoring.json")
    track_a, track_b = W("greeting-a.json"), W("greeting-b.json")
    run_json(cli, ["viseme-author", spec, "--out", track_a], 0)
    run_json(cli, ["viseme-author", spec, "--out", track_b], 0)
    check("authored track is byte-identical across runs",
          sha256(track_a) == sha256(track_b))
    clean = run_json(cli, ["viseme-validate", track_a, created], 0)
    check("authored track validates clean against a mouth rig",
          clean.get("result") == "clean")
    overlap = {"format": "torogao-viseme-authoring/1", "events": [
        {"viseme": "aa", "start": 0.0, "end": 0.5},
        {"viseme": "E", "start": 0.3, "end": 0.6}]}
    overlap_path = W("overlap.json")
    json.dump(overlap, open(overlap_path, "w"))
    bad = run_json(cli, ["viseme-author", overlap_path, "--out",
                         W("overlap-track.json")], 1)
    check("overlapping events refuse at authoring",
          bad.get("code") == "TG-VISEME-AUTHOR")
    unordered = {"format": "torogao-viseme-track/1", "fps": 60,
                 "durationSeconds": 1.0, "events": [
                     {"viseme": "aa", "start": 0.5, "end": 0.7},
                     {"viseme": "E", "start": 0.1, "end": 0.3}]}
    unordered_path = W("unordered.json")
    json.dump(unordered, open(unordered_path, "w"))
    flagged = run_json(cli, ["viseme-validate", unordered_path, created], 1)
    codes = [f["code"] for f in flagged.get("findings", [])]
    check("out-of-order track events are flagged, typed",
          "TG-VISEME-TRACK-ORDER" in codes)
    track72 = {"format": "torogao-input-track/1", "fps": 60, "steps": 72,
               "inputs": {}}
    track72_path = W("track72.json")
    json.dump(track72, open(track72_path, "w"))
    curves_path = W("viseme-curves.json")
    run_json(cli, ["physics-preview", created, "--track", track72_path,
                   "--viseme", track_a, "--out", curves_path], 0)
    curves = json.load(open(curves_path))
    mouth = [s for s in curves["series"]
             if s["parameter"] == "ParamMouthOpenY"][0]
    check("viseme layer moves the mouth in preview (aa opens > 0.8)",
          max(mouth["values"]) > 0.8)
    conflict = {"format": "torogao-input-track/1", "fps": 60, "steps": 72,
                "inputs": {"ParamMouthOpenY": [0.5] * 72}}
    conflict_path = W("track-conflict.json")
    json.dump(conflict, open(conflict_path, "w"))
    refused = run_json(cli, ["physics-preview", created, "--track",
                             conflict_path, "--viseme", track_a], 3)
    check("track + viseme driving one parameter refuses, typed",
          "both drive parameter 'ParamMouthOpenY'"
          in refused.get("error", ""))

    # -- Item 4: session builder ---------------------------------
    print("== session builder ==")
    session = os.path.join(fixtures, "session-induction.json")
    valid = run_json(cli, ["audio-validate-session", session], 0)
    check("session validates clean", valid.get("result") == "clean")
    out_a, out_b = W("session-a"), W("session-b")
    built = run_json(cli, ["audio-build-session", session,
                           "--out", out_a], 0)
    run_json(cli, ["audio-build-session", session, "--out", out_b], 0)
    wav_a = os.path.join(out_a, "induction-short.wav")
    check("two builds from one session are byte-identical",
          sha256(wav_a) == sha256(os.path.join(out_b,
                                               "induction-short.wav"))
          and sha256(os.path.join(out_a, "induction-short.recipe.json"))
          == sha256(os.path.join(out_b, "induction-short.recipe.json")))
    recipe = json.load(open(os.path.join(out_a,
                                         "induction-short.recipe.json")))
    beats = [s["beatHz"] for s in recipe["stems"]
             if s["type"] == "binaural"]
    check("phases compile to crossfaded binaural stems in order",
          beats == [10.0, 6.0, 10.0] and len(recipe["stems"]) == 5)
    manifest = built.get("manifest", {})
    check("manifest loudness hits the house target",
          abs(manifest.get("measuredLufs", 0) - (-16.0)) <= 0.5
          and manifest.get("measuredTruePeakDb", 0) <= -1.0 + 1e-9)
    # Independent recomputation: plain integrated RMS loudness
    # (no K-weighting) of the rendered master vs the manifest.
    with wave.open(wav_a, "rb") as w:
        frames = w.getnframes()
        raw = w.readframes(frames)
        ch = w.getnchannels()
    ints = []
    for i in range(0, len(raw), 3):
        v = raw[i] | (raw[i + 1] << 8) | (raw[i + 2] << 16)
        if v >= 1 << 23:
            v -= 1 << 24
        ints.append(v / float(1 << 23))
    ms = sum(v * v for v in ints) / (len(ints) / ch)
    rms_lufs = 10 * math.log10(ms) - 0.691
    check("independent RMS loudness agrees with the manifest (<=2 dB)",
          abs(rms_lufs - manifest.get("measuredLufs", 0)) <= 2.0,
          f"rms={rms_lufs:.2f} manifest={manifest.get('measuredLufs')}")
    for name, code in (("session-bad-level.json",
                        "TG-SESSION-SUBLIMINAL-LEVEL"),
                       ("session-bad-peak.json",
                        "TG-SESSION-SUBLIMINAL-PEAK")):
        refused = run_json(cli, ["audio-build-session",
                                 os.path.join(fixtures, name),
                                 "--out", W("session-x")], 1)
        check(f"{name} refuses typed ({code})",
              refused.get("result") == "refused"
              and refused.get("code") == code)

    # -- Item 5: motion events ------------------------------------
    print("== motion events ==")
    events_spec = {"motionEvents": [
        {"name": "blink-cue", "motion": "Blink", "timeSeconds": 0.5,
         "payloadClass": "sfx", "payload": "cue-chime"}]}
    events_path = W("events.json")
    json.dump(events_spec, open(events_path, "w"))
    authored = W("rich-events.inx")
    run_json(cli, ["events-author", rich, "--spec", events_path,
                   "--out", authored], 0)
    listed = run_json(cli, ["events-list", authored], 0)
    check("authored events list back identically",
          listed.get("motionEvents") == events_spec["motionEvents"])
    authored2 = W("rich-events2.inx")
    run_json(cli, ["events-author", authored, "--spec", events_path,
                    "--out", authored2], 0)
    check("event round-trip through export/re-import is byte-identical",
          sha256(authored) == sha256(authored2))
    for name, spec_events, code in (
            ("late", [{"name": "t", "motion": "Blink",
                       "timeSeconds": 1.5, "payloadClass": "sfx",
                       "payload": "cue-chime"}],
             "TG-EVENT-BEYOND-DURATION"),
            ("unknown", [{"name": "t", "motion": "Nope",
                          "timeSeconds": 0.1, "payloadClass": "sfx",
                          "payload": "cue-chime"}],
             "TG-EVENT-UNKNOWN-MOTION")):
        p = W(f"events-{name}.json")
        json.dump({"motionEvents": spec_events}, open(p, "w"))
        refused = run_json(cli, ["events-author", rich, "--spec", p,
                                 "--out", W("events-x.inx")], 1)
        codes = [f["code"] for f in refused.get("findings", [])]
        check(f"{name} event refuses typed ({code})", code in codes)
    validated = run_json(cli, ["validate", authored], None)
    event_findings = [f for f in validated.get("findings", [])
                      if f["code"].startswith("TG-EVENT")]
    check("validate reports no event findings on the authored file",
          event_findings == [])

    # -- Item 6: lip-sync analysis --------------------------------
    print("== lip-sync analysis ==")
    bed_wav = os.path.join(p2_fixtures, "..", "evidence", "audio",
                           "binaural-1", "binaural-bed.wav")
    proposals = []
    for i in range(3):
        p = W(f"proposal-{i}.json")
        run_json(cli, ["lipsync-analyze", bed_wav, "--out", p], 0)
        proposals.append(sha256(p))
    check("same WAV yields a byte-identical proposal x3",
          proposals[0] == proposals[1] == proposals[2])
    silence = W("silence.wav")
    with wave.open(silence, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(48000)
        w.writeframes(b"\x00\x00" * 48000)
    sil = run_json(cli, ["lipsync-analyze", silence, "--out",
                         W("proposal-silence.json")], 0)
    sil_doc = json.load(open(W("proposal-silence.json")))
    check("a silent WAV proposes a closed-mouth track",
          [e["viseme"] for e in sil_doc["track"]["events"]] == ["sil"])
    tone = W("tone500.wav")
    with wave.open(tone, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(48000)
        w.writeframes(b"".join(
            struct.pack("<h", int(12000 * math.sin(
                2 * math.pi * 500 * i / 48000)))
            for i in range(48000)))
    run_json(cli, ["lipsync-analyze", tone, "--out",
                   W("proposal-tone.json")], 0)
    tone_doc = json.load(open(W("proposal-tone.json")))
    feats = tone_doc["features"]
    check("a steady 500 Hz tone measures its own frequency band",
          len(feats) == 1 and feats[0]["viseme"] == "oh"
          and 450 <= feats[0]["meanCentroidHz"] <= 550,
          f"got {feats}")
    check("proposal curves are a torogao-curves/1 document",
          tone_doc["curves"]["format"] == "torogao-curves/1")
    bed_doc = json.load(open(W("proposal-0.json")))
    bed_track = W("bed-track.json")
    json.dump(bed_doc["track"], open(bed_track, "w"))
    steps = round(bed_doc["track"]["durationSeconds"] * 60)
    input_track = W("bed-input.json")
    json.dump({"format": "torogao-input-track/1", "fps": 60,
               "steps": steps, "inputs": {}}, open(input_track, "w"))
    folded = run_json(cli, ["physics-preview", created, "--track",
                            input_track, "--viseme", bed_track,
                            "--out", W("bed-curves.json")], 0)
    check("the proposal folds into a preview via --viseme",
          folded.get("steps") == steps)

    # -- Item 7: mocopi completion --------------------------------
    print("== mocopi completion ==")
    for pack_name in ("adult", "all-ages"):
        pack = json.load(open(os.path.join(repo, "packs",
                                           f"{pack_name}.json")))
        fb = [s for s in pack["trackingSets"]
              if s["id"] == "full-body"][0]
        groups = {e["group"] for e in fb["ids"]}
        ids = {e["id"] for e in fb["ids"]}
        check(f"{pack_name} full-body set is complete at 86 IDs",
              fb["completeness"] == "complete" and len(fb["ids"]) == 86
              and fb.get("groupsPendingSource") == []
              and {"body", "arms", "hands", "legs", "feet"} <= groups)
        check(f"{pack_name} set keeps the source's verbatim spellings",
              {"UpperLegRIghtAngleY", "LowerLegRIghtAngleX",
               "FootLeftPosisionY"} <= ids)
    trkfb = run_json(cli, ["tracking-check", created, "--pack", "adult",
                           "--set", "full-body"], 1)
    check("coverage re-run: the template rig reports the 70 table "
          "IDs missing",
          trkfb.get("completeness") == "complete"
          and len(trkfb.get("missing", [])) == 70)

    # -- Item 8: M fixture set -------------------------------------
    print("== M fixture set ==")
    msrc = os.path.join(repo, "fixtures", "m-rigs")
    mwork = W("m-rigs")
    shutil.copytree(msrc, mwork,
                    ignore=shutil.ignore_patterns("rigs"))
    gen = subprocess.run([sys.executable,
                          os.path.join(mwork, "generate.py"), cli, mwork],
                         capture_output=True, text=True, timeout=600)
    check("fixture generator runs clean", gen.returncode == 0,
          gen.stdout[-200:] + gen.stderr[-200:])
    manifest = json.load(open(os.path.join(mwork, "manifest.json")))
    fxs = manifest["fixtures"]
    check("manifest carries 8 fixtures, 2 held out, >= 3 face shapes",
          len(fxs) == 8
          and sum(1 for f in fxs if f["heldOut"]) == 2
          and len({f["faceShape"] for f in fxs}) >= 3)
    for fx in fxs:
        rig = os.path.join(mwork, fx["rig"])
        run(cli, ["validate", rig], 0)
        expr = run_json(cli, ["expression-check", rig, "--pack",
                               fx["pack"]], 0)
        check(f"{fx['id']} passes expression coverage for its pack",
              expr.get("result") == "covered")
        insp = run_json(cli, ["inspect", rig], 0)["census"]
        gt = json.load(open(os.path.join(mwork, fx["groundTruth"])))
        params = sorted(p["name"] for p in insp["parameters"]["list"])
        parts = [n["name"] for n in insp["nodes"]["list"]
                 if n["type"] == "Part"]
        outputs = sorted(d["drives"] for d in insp["drivers"]["list"])
        check(f"{fx['id']} ground truth matches fresh inspection",
              params == gt["parameters"] and parts == gt["parts"]
              and outputs == gt["physicsOutputs"])
        out_dir = W(f"export-{fx['id']}")
        run_json(cli, ["export-rig", rig, "--out", out_dir], 0)
        model = os.path.join(
            out_dir, os.path.basename(rig).replace(".inx",
                                                   ".model.json"))
        verdict = subprocess.run([rigcheck, model],
                                 capture_output=True, text=True,
                                 timeout=300)
        check(f"{fx['id']} export passes the lumen validator",
              verdict.returncode == 0
              and "VALID" in verdict.stdout,
              verdict.stdout[-200:])

    # -- Registry accounting + MCP spot check ----------------------
    print("== registry ==")
    dump = run_json(cli, ["registry-dump"], 0)
    names = {t["name"] for t in dump["tools"]}
    p4 = {"tutorial-list", "tutorial-status", "tutorial-check",
          "writer-draft", "viseme-author", "viseme-validate",
          "session-validate", "session-build", "events-list",
          "events-author", "lipsync-analyze"}
    check("registry v5.0.0 carries 57 tools incl. the 11 P4 tools",
          dump.get("version") == "5.0.0" and len(names) == 57
          and p4 <= names)
    proc = subprocess.Popen([cli, "mcp"], stdin=subprocess.PIPE,
                            stdout=subprocess.PIPE, text=True)

    def send(obj):
        proc.stdin.write(json.dumps(obj) + "\n")
        proc.stdin.flush()

    def recv():
        return json.loads(proc.stdout.readline())

    send({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
        "protocolVersion": "2024-11-05", "capabilities": {},
        "clientInfo": {"name": "p4-harness", "version": "1.0"}}})
    recv()
    send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    send({"jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": {
        "name": "tutorial-list", "arguments": {"pack": "adult"}}})
    resp = recv()
    content = resp.get("result", {}).get("content", [])
    text = content[0]["text"] if content else ""
    called = json.loads(text) if text else {}
    check("MCP tools/call reaches the p4 family (tutorial-list)",
          called.get("track") == "characters"
          and len(called.get("lessons", [])) == 6)
    proc.stdin.close()
    proc.wait(timeout=30)

    print(f"\n{CHECKS[0]} checks, {len(FAILURES)} failures")
    if FAILURES:
        print("FAILURES:", *FAILURES, sep="\n  ")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
