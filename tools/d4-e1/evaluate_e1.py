#!/usr/bin/env python3
"""D-4 comparison, arm E1 measurement harness.

Runs every COMPARISONS.md section 3 metric that is evaluable
without a rasteriser, against the frozen inputs:

- X1: lumen-rig-core `evaluate_motion` trace hashes for the
  landed motions on E0 (conformance fixture) and E1
  (extended export) models, 3 repetitions per arm in ABBA
  order.
- X2: the Marsha 11-state gaze/cheek channels, checked by
  EVALUATED output at two layers — the INP engine
  (`d4_e1_probe`: iris deformation centroid, blush effective
  opacity) and the lumen keyform layer (multilinear
  interpolation of the exported lattices, per SPEC 3.7).
- X3: gaze probes (9 levels per axis + diagonal) and blush
  probes (5 levels) at both layers; monotonicity and
  absolute error against the ideal linear response. The
  render-readback leg (MediaPipe / face-state tooling) is
  RENDER-1-BLOCKED and is reported, not approximated.
- X4: export-report accounting (no E1 binding dropped),
  parameter survival with ranges/defaults, export
  determinism across repetitions.
- X6 data: parameter counts per rig per arm.

Usage: evaluate_e1.py <runs_dir> <torogao_bin_dir> <lumen_example_bin>
"""

import hashlib
import json
import subprocess
import sys
from pathlib import Path

TRAVEL_X = 5.5
TRAVEL_Y = 5.8
TOL = 1e-4

# Marsha section C gaze/cheek specifications (Cubism-normalised).
# gaze None = the state's gaze is temporal (drift / darting),
# not a state vector. cheek is the blush opacity target.
STATES = {
    "F-01": {"gaze": (0.0, 0.0), "cheek": 0.1},
    "F-03": {"gaze": (0.0, -0.1), "cheek": 0.55},
    "F-06": {"gaze": None, "cheek": 0.85},
    "F-07": {"gaze": (0.0, 0.25), "cheek": 0.8},
    "F-10": {"gaze": (0.2, 0.3), "cheek": 1.0},
    "N-01": {"gaze": (0.0, 0.0), "cheek": 0.15},
    "N-02": {"gaze": (0.6, 0.0), "cheek": 0.0},
    "N-03": {"gaze": None, "cheek": 0.9},
    "N-04": {"gaze": (0.0, -0.4), "cheek": 0.8},
    "N-05": {"gaze": "roll", "cheek": 0.9},
    "N-06": {"gaze": (0.0, 0.0), "cheek": 0.6},
}

GAZE_LEVELS = [-1.0, -0.75, -0.5, -0.25, 0.0, 0.25, 0.5, 0.75, 1.0]
CHEEK_LEVELS = [0.0, 0.25, 0.5, 0.75, 1.0]


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(cmd):
    return subprocess.run(cmd, capture_output=True, text=True)


# ---------------------------------------------------------------- lumen layer

def lattice_value(model, mesh_id, vector):
    """Multilinear interpolation of a mesh lattice at a
    parameter vector {param_id: value}. Returns (centroid,
    opacity) of the interpolated form; opacity falls back to
    the mesh base opacity when forms carry none."""
    mesh = next(m for m in model["meshes"] if m["id"] == mesh_id)
    base_centroid = centroid_of(mesh["vertices"])
    lattice = mesh.get("keyforms")
    if lattice is None:
        return base_centroid, mesh["opacity"]
    axes = lattice["axes"]
    keys = lattice["keys"]
    forms = {tuple(form["at"]): form for form in lattice["forms"]}
    defaults = {p["id"]: p.get("default", 0.0) for p in model["parameters"]}
    # Per-axis bracketing (index, fraction). An axis the
    # vector does not set sits at its parameter default.
    brackets = []
    for axis, row in zip(axes, keys):
        value = vector.get(axis, defaults.get(axis, 0.0))
        if value <= row[0]:
            brackets.append((0, 0.0))
        elif value >= row[-1]:
            brackets.append((len(row) - 2, 1.0))
        else:
            for i in range(len(row) - 1):
                if row[i] <= value <= row[i + 1]:
                    frac = (value - row[i]) / (row[i + 1] - row[i])
                    brackets.append((i, frac))
                    break
    corners = 1 << len(axes)
    centroid = [0.0, 0.0]
    opacity = 0.0
    for mask in range(corners):
        at = []
        weight = 1.0
        for axis_i, (lo, frac) in enumerate(brackets):
            if mask & (1 << axis_i):
                at.append(lo + 1)
                weight *= frac
            else:
                at.append(lo)
                weight *= 1.0 - frac
        form = forms[tuple(at)]
        form_centroid = centroid_of(form["vertices"])
        centroid[0] += weight * form_centroid[0]
        centroid[1] += weight * form_centroid[1]
        opacity += weight * form.get("opacity", mesh["opacity"])
    return centroid, opacity


def centroid_of(vertices):
    n = len(vertices)
    return [sum(v[0] for v in vertices) / n, sum(v[1] for v in vertices) / n]


def param_map(model):
    return {p["id"]: p for p in model["parameters"]}


# ---------------------------------------------------------------- engine layer

PROBE_DIR = None


def engine_probe(probe_bin, inx: Path, label, assignments):
    probe = {"label": label, "set": assignments}
    probe_path = PROBE_DIR / f"probe-{label}.json"
    probe_path.write_text(json.dumps(probe))
    result = run([str(probe_bin), str(inx), str(probe_path)])
    if result.returncode != 0:
        return {"error": result.stderr.strip()}
    return json.loads(result.stdout)


def state_assignments(spec):
    assignments = {}
    if isinstance(spec["gaze"], tuple):
        # Section C gaze [-1, 1] -> parameter value [0, 1].
        assignments["ParamGaze"] = [(spec["gaze"][0] + 1) / 2, (spec["gaze"][1] + 1) / 2]
    elif spec["gaze"] == "roll":
        assignments["ParamEyeRoll"] = [1.0]
    assignments["ParamCheek"] = [spec["cheek"]]
    return assignments


def expected_iris_inp(spec):
    """Expected iris centroid offset in INP space (y-down)."""
    if isinstance(spec["gaze"], tuple):
        gx, gy = spec["gaze"]
        return [gx * TRAVEL_X, -gy * TRAVEL_Y]
    if spec["gaze"] == "roll":
        return [0.0, -TRAVEL_Y]
    return None


def main() -> int:
    global PROBE_DIR
    runs_dir = Path(sys.argv[1])
    PROBE_DIR = runs_dir / "probes"
    PROBE_DIR.mkdir(parents=True, exist_ok=True)
    torogao_bin = Path(sys.argv[2])
    lumen_eval = Path(sys.argv[3])
    probe_bin = torogao_bin / "examples" / "d4_e1_probe"
    cli_bin = torogao_bin / "torogao"

    fixtures = runs_dir.parent.parent / "lumen-d4-e1-20261029" / "crates" / "fixtures" / "conformance"
    e0_inx_dir = Path.home() / "workspace/torogao-rust-port-20261027/phase-f-gates/fixtures"
    e1_inx_dir = runs_dir / "inputs-extended"
    e1_export_dir = runs_dir / "exports-e1"

    results = {"x1": {}, "x2": {}, "x3": {}, "x4": {}, "x6": {}}

    models_e0, models_e1 = {}, {}
    for rig in ("prima", "yui", "alma"):
        models_e0[rig] = json.loads((fixtures / f"{rig}-repertoire.model.json").read_text())
        models_e1[rig] = json.loads(
            (e1_export_dir / rig / f"{rig}-repertoire-extended.model.json").read_text()
        )

    # ---- X1: trace invariance, ABBA, 3 reps per arm.
    motions = {}
    for performance in ("Greeting", "Daydream"):
        motions[performance] = fixtures / f"prima-repertoire.motion-{performance}.json"
    x1_runs = []
    order = ["E0", "E1", "E1", "E0", "E0", "E1"]
    for arm in order:
        model_path = (
            fixtures / "prima-repertoire.model.json"
            if arm == "E0"
            else e1_export_dir / "prima" / "prima-repertoire-extended.model.json"
        )
        row = {"arm": arm}
        for performance, motion_path in motions.items():
            out = run([str(lumen_eval), str(model_path), str(motion_path)])
            token = [t for t in out.stdout.split() if t.startswith("trace_hash=")]
            row[performance] = token[0].split("=", 1)[1] if token else f"ERROR: {out.stderr.strip()}"
        x1_runs.append(row)
    results["x1"]["prima_runs_abba"] = x1_runs
    # Supporting: Yui/Alma E0 vs E1 single-run hashes.
    support = {}
    for rig in ("yui", "alma"):
        support[rig] = {}
        for arm, model_path in (
            ("E0", fixtures / f"{rig}-repertoire.model.json"),
            ("E1", e1_export_dir / rig / f"{rig}-repertoire-extended.model.json"),
        ):
            support[rig][arm] = {}
            for performance in ("Greeting", "Daydream"):
                motion_path = fixtures / f"{rig}-repertoire.motion-{performance}.json"
                out = run([str(lumen_eval), str(model_path), str(motion_path)])
                token = [t for t in out.stdout.split() if t.startswith("trace_hash=")]
                support[rig][arm][performance] = (
                    token[0].split("=", 1)[1] if token else f"ERROR: {out.stderr.strip()}"
                )
    results["x1"]["yui_alma_supporting"] = support

    # ---- X2 + X3.
    for arm, inx_dir, models in (
        ("E0", e0_inx_dir, models_e0),
        ("E1", e1_inx_dir, models_e1),
    ):
        arm_states = {}
        for state, spec in STATES.items():
            assignments = state_assignments(spec)
            engine = engine_probe(
                probe_bin, inx_dir / f"prima-repertoire{'-extended' if arm == 'E1' else ''}.inx",
                f"x2-{arm}-{state}", assignments,
            )
            # Lumen layer (Prima).
            model = models["prima"]
            params = param_map(model)
            vector = {}
            if isinstance(spec["gaze"], tuple):
                if "ParamGaze_2201_x" in params and "ParamGaze_2201_y" in params:
                    vector["ParamGaze_2201_x"] = (spec["gaze"][0] + 1) / 2
                    vector["ParamGaze_2201_y"] = (spec["gaze"][1] + 1) / 2
            elif spec["gaze"] == "roll":
                vector["ParamEyeRoll_2101_x"] = 1.0
            cheek_vector = dict(vector)
            if "ParamCheek_2203_x" in params:
                cheek_vector["ParamCheek_2203_x"] = spec["cheek"]
            iris = lattice_value(model, "mesh_IrisL_3001", vector) if vector or arm == "E0" else None
            blush = lattice_value(model, "mesh_BlushL_3301", cheek_vector) if "ParamCheek_2203_x" in params else None
            arm_states[state] = {
                "spec": {"gaze": spec["gaze"], "cheek": spec["cheek"]},
                "engine": engine,
                "expected_iris_offset_inp": expected_iris_inp(spec),
                "lumen_iris": iris,
                "lumen_blush": blush,
            }
        results["x2"][arm] = arm_states

    # X3 probes (E1 only; E0 is not measurable by definition).
    inx = e1_inx_dir / "prima-repertoire-extended.inx"
    model = models_e1["prima"]
    gaze_probe_rows = []
    for axis in ("x", "y"):
        for level in GAZE_LEVELS:
            mapped = (level + 1) / 2
            assignments = {"ParamGaze": [mapped, 0.5] if axis == "x" else [0.5, mapped]}
            engine = engine_probe(probe_bin, inx, f"x3-gaze-{axis}-{level}", assignments)
            vector = {
                "ParamGaze_2201_x": mapped if axis == "x" else 0.5,
                "ParamGaze_2201_y": mapped if axis == "y" else 0.5,
                "ParamEyeRoll_2101_x": 0.0,
                "ParamEyeCross_2102_x": 0.0,
            }
            lumen = lattice_value(model, "mesh_IrisL_3001", vector)
            gaze_probe_rows.append(
                {"axis": axis, "level": level, "engine": engine, "lumen_iris": lumen}
            )
    cheek_probe_rows = []
    for level in CHEEK_LEVELS:
        engine = engine_probe(probe_bin, inx, f"x3-cheek-{level}", {"ParamCheek": [level]})
        lumen = lattice_value(model, "mesh_BlushL_3301", {"ParamCheek_2203_x": level})
        cheek_probe_rows.append({"level": level, "engine": engine, "lumen_blush": lumen})
    # Noise floor at the geometry layer: repeated neutral
    # evaluations must be bit-identical (deterministic eval).
    neutral_reps = [
        engine_probe(probe_bin, inx, f"x3-neutral-{i}", {}) for i in range(3)
    ]
    results["x3"]["gaze_probes"] = gaze_probe_rows
    results["x3"]["cheek_probes"] = cheek_probe_rows
    results["x3"]["neutral_repetitions"] = neutral_reps

    # ---- X4: report accounting + parameter survival + determinism.
    x4 = {}
    for rig in ("prima", "yui", "alma"):
        report = json.loads(
            (e1_export_dir / rig / f"{rig}-repertoire-extended.export-report.json").read_text()
        )
        entries = report["entries"] if isinstance(report, dict) else report
        dropped_e1 = [
            e for e in entries
            if e.get("verdict") == "dropped"
            and ("ParamGaze" in e.get("construct", "") or "ParamCheek" in e.get("construct", "")
                 or "Blush" in e.get("construct", ""))
        ]
        params = param_map(models_e1[rig])
        x4[rig] = {
            "dropped_e1_entries": dropped_e1,
            "report_entries_total": len(entries),
            "params_present": {
                key: params.get(key)
                for key in ("ParamGaze_2201_x", "ParamGaze_2201_y", "ParamCheek_2203_x")
            },
        }
    results["x4"] = x4
    # Export determinism: two more exports, hashes compared.
    det = {}
    for rep in (2, 3):
        for rig in ("prima", "yui", "alma"):
            out_dir = runs_dir / f"exports-e1-rep{rep}" / rig
            run([str(cli_bin), "export-rig", str(e1_inx_dir / f"{rig}-repertoire-extended.inx"),
                 "--out", str(out_dir)])
            det[f"{rig}-rep{rep}"] = sha256(out_dir / f"{rig}-repertoire-extended.model.json")
    for rig in ("prima", "yui", "alma"):
        det[f"{rig}-rep1"] = sha256(e1_export_dir / rig / f"{rig}-repertoire-extended.model.json")
    results["x4"]["export_determinism_sha256"] = det

    # ---- X6 data: parameter counts.
    for rig in ("prima", "yui", "alma"):
        results["x6"][rig] = {
            "e0_named_parameters": len(models_e0[rig]["parameters"]),
            "e1_named_parameters": len(models_e1[rig]["parameters"]),
            "e0_meshes": len(models_e0[rig]["meshes"]),
            "e1_meshes": len(models_e1[rig]["meshes"]),
        }

    out_path = runs_dir / "results.json"
    out_path.write_text(json.dumps(results, indent=2, sort_keys=True))
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
