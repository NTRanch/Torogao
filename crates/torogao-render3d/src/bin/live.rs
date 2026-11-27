//! `render3d-live` — the spike renderer for the ache GUI live
//! route (SCOPE.md §§4/6 in
//! `~/workspace/ache-live-render3d-scope-20261127/`).
//!
//! A dumb display server: it loads one character GLB and a set of
//! frozen `torogao-motion-clip/2` reaction clips, then streams
//! RGBA frames. Drive states arrive as JSON lines on stdin;
//! frames leave as length-prefixed binary packets on stdout;
//! logs and statistics go to stderr (stdout is pure protocol).
//!
//! ```text
//! in : {"seq":N,"clip":"lana/react/building.t2","expr":{...per-mille...}}
//! in : {"seq":N,"stop":true}
//! out: {"ready":true,...}\n                      (one text line)
//! out: u32le len | seq u64 | state_seq u64 | w u32 | h u32 | rgba…
//! ```
//!
//! Determinism posture (the crate's contract, kept): the render
//! path is a pure function of (clip, clip time, expression
//! vector). Wall clock lives only in this host's frame scheduler,
//! which supplies clip time as *input state* — the same host
//! policy `pose.rs` documents for the arc plates. `--probe`
//! renders one evaluated state twice and prints both SHA-256
//! hashes as the twin witness.
//!
//! The clip evaluator here is SPIKE-GRADE: a local implementation
//! of DESIGN Appendix C semantics (clamp, keyframe exactness,
//! step, componentwise linear, shortest-arc slerp, Hermite with
//! per-frame tangents scaled by segment length) covering exactly
//! the interp census of the 32 frozen Lana clips (cubic occurs
//! only on scalar parameter channels; rotations are linear/step).
//! The evaluator of record is `torogao-runtime::clip_eval` on the
//! hand-animation lineage; production converges the lineages
//! (SCOPE §4 item 2) and deletes this copy.
//!
//! Composition law for joint channels (SCOPE §2.1, translated
//! from the rung-1 Blender binder `bind_rung1.py`): rotation keys
//! are absolute pose quaternions composed over the bone's rest
//! orientation (local = rest ∘ q); the Hips translation is
//! absolute in clip space with rest anchor (0, 0.98, 0), applied
//! as a delta over the GLB rest translation in the hips' parent
//! frame. Bones the clip does not key hold their GLB rest locals.
//!
//! Expression: the clip's 13 parameter channels (per-mille)
//! become an `ExprVector`; `expr::resolve` applies the stock
//! morph bindings, then the Lana adapter lands the five rung-1
//! carrier morphs the stock table does not know: Fac_Pupil_Dilate
//! (above-neutral half of pupil_dilate, linear), Fac_Eye_Gloss
//! (linear), and the patch carriers Fac_Blush / Fac_Blush_Spread
//! / Fac_Tear_Sheen with the binder's v^0.6 emergence gamma.
//! `skin_flush` / `skin_sheen` have no GLB carrier and stay
//! unlanded on this legacy shading path (SCOPE §0).

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use torogao_render3d::backend::vulkan::VulkanBackend;
use torogao_render3d::backend::{DrawFrame, RenderBackend, pack_scene};
use torogao_render3d::compare::{Image, sha256_hex, write_png};
use torogao_render3d::expr::{ExprVector, PARAM_NAMES, resolve};
use torogao_render3d::math::{Mat4, Quat};
use torogao_render3d::model::{Model, load_model};
use torogao_render3d::setscene::plate_rig_scene;
use torogao_render3d::toon::ToonSettings;

// ----------------------------------------------------------------
// Clip documents (spike-grade parse of the frozen /2 form)

#[derive(Clone, Copy, PartialEq, Debug)]
enum Interp {
    Linear,
    Step,
    Cubic,
}

#[derive(Clone, Debug)]
struct Key {
    frame: f64,
    interp: Interp,
    value: Vec<f64>,
    in_tangent: Vec<f64>,
    out_tangent: Vec<f64>,
}

#[derive(Clone, Debug)]
struct Channel {
    kind: String, // "joint" | "parameter"
    name: String,
    property: String, // "rotation" | "translation" | "value"
    keys: Vec<Key>,
}

#[derive(Clone, Debug)]
struct Clip {
    rate: f64,
    loop_start: f64,
    loop_end: f64,
    channels: Vec<Channel>,
    freeze_sha: Option<String>,
}

fn value_vec(v: &serde_json::Value) -> Vec<f64> {
    match v {
        serde_json::Value::Number(n) => vec![n.as_f64().unwrap_or(0.0)],
        serde_json::Value::Array(a) => a.iter().map(|x| x.as_f64().unwrap_or(0.0)).collect(),
        _ => Vec::new(),
    }
}

fn parse_clip(key: &str, bytes: &[u8]) -> Result<Clip, String> {
    let doc: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| format!("clip {key}: {e}"))?;
    if doc["format"].as_str() != Some("torogao-motion-clip/2") {
        return Err(format!("clip {key}: not a torogao-motion-clip/2 document"));
    }
    let rate =
        doc["rate"]["num"].as_f64().unwrap_or(24.0) / doc["rate"]["den"].as_f64().unwrap_or(1.0);
    let loop_start = doc["loop"]["start"].as_f64().unwrap_or(0.0);
    let loop_end = doc["loop"]["end"]
        .as_f64()
        .unwrap_or(doc["range"]["end"].as_f64().unwrap_or(0.0));
    let mut channels = Vec::new();
    for ch in doc["channels"].as_array().cloned().unwrap_or_default() {
        let target = &ch["target"];
        let mut keys = Vec::new();
        for k in ch["keys"].as_array().cloned().unwrap_or_default() {
            let interp = match k["interp"].as_str().unwrap_or("linear") {
                "step" => Interp::Step,
                "cubic" => Interp::Cubic,
                _ => Interp::Linear,
            };
            keys.push(Key {
                frame: k["frame"].as_f64().unwrap_or(0.0),
                interp,
                value: value_vec(&k["value"]),
                in_tangent: value_vec(&k["in_tangent"]),
                out_tangent: value_vec(&k["out_tangent"]),
            });
        }
        keys.sort_by(|a, b| a.frame.partial_cmp(&b.frame).unwrap());
        channels.push(Channel {
            kind: target["kind"].as_str().unwrap_or("").to_string(),
            name: target["name"].as_str().unwrap_or("").to_string(),
            property: target["property"].as_str().unwrap_or("").to_string(),
            keys,
        });
    }
    Ok(Clip {
        rate,
        loop_start,
        loop_end,
        channels,
        freeze_sha: doc["freeze"]["sha256"].as_str().map(str::to_string),
    })
}

/// Evaluate one channel at an absolute clip-frame position.
/// Keyframe exactness: a position on a key returns the key value.
fn eval_channel(ch: &Channel, pos: f64) -> Vec<f64> {
    let keys = &ch.keys;
    if keys.is_empty() {
        return Vec::new();
    }
    if pos <= keys[0].frame {
        return keys[0].value.clone();
    }
    let last = keys.len() - 1;
    if pos >= keys[last].frame {
        return keys[last].value.clone();
    }
    // Find the segment [i, i+1] containing pos.
    let mut i = 0;
    while i + 1 < keys.len() && keys[i + 1].frame <= pos {
        i += 1;
    }
    let a = &keys[i];
    let b = &keys[i + 1];
    if (pos - a.frame).abs() < 1e-9 {
        return a.value.clone();
    }
    let span = b.frame - a.frame;
    let t = if span > 0.0 {
        (pos - a.frame) / span
    } else {
        0.0
    };
    let is_quat = ch.kind == "joint" && ch.property == "rotation" && a.value.len() == 4;
    match a.interp {
        Interp::Step => a.value.clone(),
        Interp::Cubic if !is_quat => {
            // Hermite, tangents per-frame scaled by segment length.
            let n = a.value.len().max(b.value.len());
            let t2 = t * t;
            let t3 = t2 * t;
            (0..n)
                .map(|c| {
                    let v0 = a.value.get(c).copied().unwrap_or(0.0);
                    let v1 = b.value.get(c).copied().unwrap_or(0.0);
                    let m0 = a.out_tangent.get(c).copied().unwrap_or(0.0) * span;
                    let m1 = b.in_tangent.get(c).copied().unwrap_or(0.0) * span;
                    (2.0 * t3 - 3.0 * t2 + 1.0) * v0
                        + (t3 - 2.0 * t2 + t) * m0
                        + (-2.0 * t3 + 3.0 * t2) * v1
                        + (t3 - t2) * m1
                })
                .collect()
        }
        _ => {
            if is_quat {
                let qa = Quat::from_array([a.value[0], a.value[1], a.value[2], a.value[3]]);
                let qb = Quat::from_array([b.value[0], b.value[1], b.value[2], b.value[3]]);
                let q = Quat::slerp(qa, qb, t);
                vec![q.x, q.y, q.z, q.w]
            } else {
                let n = a.value.len().max(b.value.len());
                (0..n)
                    .map(|c| {
                        let v0 = a.value.get(c).copied().unwrap_or(0.0);
                        let v1 = b.value.get(c).copied().unwrap_or(0.0);
                        v0 + (v1 - v0) * t
                    })
                    .collect()
            }
        }
    }
}

/// One evaluated clip state: bone poses by node name, parameter
/// values (per-mille) by name.
#[derive(Clone, Default)]
struct EvalFrame {
    rotations: BTreeMap<String, [f64; 4]>,
    hips_translation: Option<[f64; 3]>,
    params: BTreeMap<String, f64>,
}

fn eval_clip(clip: &Clip, pos: f64) -> EvalFrame {
    let mut out = EvalFrame::default();
    for ch in &clip.channels {
        let v = eval_channel(ch, pos);
        match (ch.kind.as_str(), ch.property.as_str()) {
            ("joint", "rotation") if v.len() == 4 => {
                out.rotations
                    .insert(ch.name.clone(), [v[0], v[1], v[2], v[3]]);
            }
            ("joint", "translation") if v.len() == 3 => {
                out.hips_translation = Some([v[0], v[1], v[2]]);
            }
            ("parameter", _) => {
                if let Some(s) = v.first() {
                    out.params.insert(ch.name.clone(), *s);
                }
            }
            _ => {}
        }
    }
    out
}

/// Blend two evaluated frames (band/tier transitions): slerp
/// rotations, lerp translations and parameters — the
/// `pose.rs::blend_frames` pattern over this bin's frame shape.
fn blend_eval(a: &EvalFrame, b: &EvalFrame, t: f64) -> EvalFrame {
    let mut out = EvalFrame::default();
    let names: std::collections::BTreeSet<&String> =
        a.rotations.keys().chain(b.rotations.keys()).collect();
    for name in names {
        let qa = a.rotations.get(name);
        let qb = b.rotations.get(name);
        let q = match (qa, qb) {
            (Some(x), Some(y)) => {
                let s = Quat::slerp(Quat::from_array(*x), Quat::from_array(*y), t);
                [s.x, s.y, s.z, s.w]
            }
            (Some(x), None) => *x,
            (None, Some(y)) => *y,
            (None, None) => unreachable!(),
        };
        out.rotations.insert(name.clone(), q);
    }
    out.hips_translation = match (a.hips_translation, b.hips_translation) {
        (Some(x), Some(y)) => Some([
            x[0] + (y[0] - x[0]) * t,
            x[1] + (y[1] - x[1]) * t,
            x[2] + (y[2] - x[2]) * t,
        ]),
        (x, y) => y.or(x),
    };
    let pnames: std::collections::BTreeSet<&String> =
        a.params.keys().chain(b.params.keys()).collect();
    for name in pnames {
        let va = a.params.get(name).copied().unwrap_or(0.0);
        let vb = b.params.get(name).copied().unwrap_or(0.0);
        out.params.insert(name.clone(), va + (vb - va) * t);
    }
    out
}

// ----------------------------------------------------------------
// Pose + expression composition against the loaded model

const HIPS_ANCHOR: [f64; 3] = [0.0, 0.98, 0.0];

struct Rig {
    model: Model,
    /// Parents-first node walk (pose.rs pattern).
    walk_order: Vec<usize>,
    /// Node index per skin joint is `model.skin.joints` itself;
    /// this maps node name -> node index for clip channels.
    node_by_name: BTreeMap<String, usize>,
    hips_node: Option<usize>,
}

impl Rig {
    fn new(model: Model) -> Rig {
        let mut node_by_name = BTreeMap::new();
        for (i, n) in model.nodes.iter().enumerate() {
            node_by_name.insert(n.name.clone(), i);
        }
        let hips_node = node_by_name.get("mixamorig:Hips").copied();
        let mut walk_order = Vec::with_capacity(model.nodes.len());
        let mut state = vec![0_u8; model.nodes.len()];
        fn visit(
            node: usize,
            nodes: &[torogao_render3d::model::Node],
            state: &mut [u8],
            out: &mut Vec<usize>,
        ) {
            if state[node] != 0 {
                return;
            }
            state[node] = 1;
            if let Some(parent) = nodes[node].parent {
                visit(parent, nodes, state, out);
            }
            out.push(node);
        }
        for n in 0..model.nodes.len() {
            visit(n, &model.nodes, &mut state, &mut walk_order);
        }
        Rig {
            model,
            walk_order,
            node_by_name,
            hips_node,
        }
    }

    /// Joint matrices (world * inverse-bind), skin order, for an
    /// evaluated frame. The composition law is in the header.
    fn joint_matrices(&self, eval: &EvalFrame) -> Vec<[f32; 16]> {
        let model = &self.model;
        let mut locals: Vec<Mat4> = model.nodes.iter().map(|n| n.local).collect();
        for (name, q) in &eval.rotations {
            if let Some(&node) = self.node_by_name.get(name) {
                let n = &model.nodes[node];
                let rest = Quat::from_array(n.rotation);
                let posed = rest.multiply(Quat::from_array(*q));
                let mut t = n.translation;
                if Some(node) == self.hips_node {
                    if let Some(ht) = eval.hips_translation {
                        t = [
                            t[0] + (ht[0] - HIPS_ANCHOR[0]),
                            t[1] + (ht[1] - HIPS_ANCHOR[1]),
                            t[2] + (ht[2] - HIPS_ANCHOR[2]),
                        ];
                    }
                }
                locals[node] = Mat4::from_trs(t, [posed.x, posed.y, posed.z, posed.w], n.scale);
            }
        }
        // Hips translation with no rotation channel (defensive;
        // every shipped clip keys both).
        if let (Some(ht), Some(node)) = (eval.hips_translation, self.hips_node) {
            if !eval.rotations.contains_key("mixamorig:Hips") {
                let n = &model.nodes[node];
                let t = [
                    n.translation[0] + (ht[0] - HIPS_ANCHOR[0]),
                    n.translation[1] + (ht[1] - HIPS_ANCHOR[1]),
                    n.translation[2] + (ht[2] - HIPS_ANCHOR[2]),
                ];
                locals[node] = Mat4::from_trs(t, n.rotation, n.scale);
            }
        }
        let mut worlds = vec![Mat4::IDENTITY; model.nodes.len()];
        for &node in &self.walk_order {
            worlds[node] = match model.nodes[node].parent {
                Some(parent) => worlds[parent].multiply(locals[node]),
                None => locals[node],
            };
        }
        model
            .skin
            .joints
            .iter()
            .enumerate()
            .map(|(joint, &node)| {
                worlds[node]
                    .multiply(model.skin.inverse_bind[joint])
                    .to_f32_cols()
            })
            .collect()
    }

    /// Per-mesh morph weights for an evaluated frame: the clip's
    /// expression vector through `expr::resolve`, plus the Lana
    /// carrier adapter (see header).
    fn mesh_weights(&self, eval: &EvalFrame) -> Result<Vec<Vec<f32>>, String> {
        let mut vector = ExprVector::NEUTRAL;
        for name in PARAM_NAMES {
            if let Some(pm) = eval.params.get(name) {
                vector
                    .set(name, (pm / 1000.0).clamp(0.0, 1.0) as f32)
                    .map_err(|e| e.to_string())?;
            }
        }
        let resolved =
            resolve(&self.model, &ToonSettings::default(), &vector).map_err(|e| e.to_string())?;
        let mut weights = resolved.mesh_weights;
        let above_mid = |v: f32| ((v - 0.5) * 2.0).max(0.0);
        let gamma = |v: f32| v.clamp(0.0, 1.0).powf(0.6);
        let carriers: [(&str, f32); 5] = [
            ("Fac_Pupil_Dilate", above_mid(vector.pupil_dilate)),
            ("Fac_Eye_Gloss", vector.eye_gloss),
            ("Fac_Tear_Sheen", gamma(vector.tear_sheen)),
            ("Fac_Blush", gamma(vector.blush_intensity)),
            ("Fac_Blush_Spread", gamma(vector.blush_spread)),
        ];
        for (mesh, w) in self.model.meshes.iter().zip(weights.iter_mut()) {
            for (target, value) in &carriers {
                if let Some(idx) = mesh.target_names.iter().position(|t| t == target) {
                    if idx < w.len() {
                        w[idx] = *value;
                    }
                }
            }
        }
        Ok(weights)
    }

    /// The expression vector an evaluated frame carries (for the
    /// probe sidecar and logs).
    fn vector_of(eval: &EvalFrame) -> ExprVector {
        let mut vector = ExprVector::NEUTRAL;
        for name in PARAM_NAMES {
            if let Some(pm) = eval.params.get(name) {
                let _ = vector.set(name, (pm / 1000.0).clamp(0.0, 1.0) as f32);
            }
        }
        vector
    }
}

// ----------------------------------------------------------------
// Camera / scene

fn look_at(eye: [f64; 3], target: [f64; 3], up: [f64; 3]) -> Mat4 {
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let norm = |a: [f64; 3]| {
        let l = dot(a, a).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let forward = norm(sub(target, eye));
    let right = norm(cross(forward, up));
    let true_up = cross(right, forward);
    Mat4::from_rows([
        [right[0], right[1], right[2], -dot(right, eye)],
        [true_up[0], true_up[1], true_up[2], -dot(true_up, eye)],
        [-forward[0], -forward[1], -forward[2], dot(forward, eye)],
        [0.0, 0.0, 0.0, 1.0],
    ])
}

// ----------------------------------------------------------------
// Drive state from the GUI

#[derive(Clone, Debug)]
struct StateMsg {
    seq: u64,
    clip: Option<String>,
    stop: bool,
    expr_pm: BTreeMap<String, f64>,
}

fn parse_state(line: &str) -> Option<StateMsg> {
    let doc: serde_json::Value = serde_json::from_str(line).ok()?;
    let mut expr_pm = BTreeMap::new();
    if let Some(obj) = doc["expr"].as_object() {
        for (k, v) in obj {
            if let Some(f) = v.as_f64() {
                expr_pm.insert(k.clone(), f);
            }
        }
    }
    Some(StateMsg {
        seq: doc["seq"].as_u64().unwrap_or(0),
        clip: doc["clip"].as_str().map(str::to_string),
        stop: doc["stop"].as_bool().unwrap_or(false),
        expr_pm,
    })
}

// ----------------------------------------------------------------

struct Args {
    glb: PathBuf,
    clips_dir: PathBuf,
    width: u32,
    height: u32,
    fps: f64,
    eye: [f64; 3],
    target: [f64; 3],
    tan_half_y: f64,
    probe: Option<ProbeArgs>,
}

#[derive(Clone)]
struct ProbeArgs {
    clip: Option<String>,
    frame: f64,
    out: PathBuf,
    rest: bool,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn parse_csv3(text: &str) -> Result<[f64; 3], String> {
    let parts: Vec<f64> = text
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("vector {text:?}: {e}"))?;
    if parts.len() != 3 {
        return Err(format!("vector {text:?} wants 3 components"));
    }
    Ok([parts[0], parts[1], parts[2]])
}

fn parse_args() -> Result<Args, String> {
    let args: Vec<String> = std::env::args().collect();
    let glb = PathBuf::from(
        arg_value(&args, "--glb").ok_or("usage: render3d-live --glb FILE --clips DIR")?,
    );
    let clips_dir = PathBuf::from(arg_value(&args, "--clips").ok_or("--clips DIR is required")?);
    let width: u32 = arg_value(&args, "--width")
        .unwrap_or_else(|| "480".into())
        .parse()
        .map_err(|e| format!("--width: {e}"))?;
    let height: u32 = arg_value(&args, "--height")
        .unwrap_or_else(|| "960".into())
        .parse()
        .map_err(|e| format!("--height: {e}"))?;
    let fps: f64 = arg_value(&args, "--fps")
        .unwrap_or_else(|| "30".into())
        .parse()
        .map_err(|e| format!("--fps: {e}"))?;
    let eye = match arg_value(&args, "--eye") {
        Some(t) => parse_csv3(&t)?,
        None => [0.0, 1.02, 3.05],
    };
    let target = match arg_value(&args, "--target") {
        Some(t) => parse_csv3(&t)?,
        None => [0.0, 0.90, 0.0],
    };
    let tan_half_y: f64 = arg_value(&args, "--tan-half-y")
        .unwrap_or_else(|| "0.335".into())
        .parse()
        .map_err(|e| format!("--tan-half-y: {e}"))?;
    let probe = if args.iter().any(|a| a == "--probe") {
        Some(ProbeArgs {
            clip: arg_value(&args, "--clip"),
            frame: arg_value(&args, "--frame")
                .unwrap_or_else(|| "0".into())
                .parse()
                .map_err(|e| format!("--frame: {e}"))?,
            out: PathBuf::from(arg_value(&args, "--out").ok_or("--probe wants --out FILE")?),
            rest: args.iter().any(|a| a == "--rest"),
        })
    } else {
        None
    };
    Ok(Args {
        glb,
        clips_dir,
        width,
        height,
        fps,
        eye,
        target,
        tan_half_y,
        probe,
    })
}

fn load_clips(clips_dir: &Path) -> BTreeMap<String, Clip> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(clips_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(stem) = name.strip_suffix(".clip.json") else {
            continue;
        };
        // File names are the clip keys with '/' flattened to '_'
        // (lana_react_building.t2 -> lana/react/building.t2).
        let Some(rest) = stem.strip_prefix("lana_react_") else {
            continue;
        };
        let key = format!("lana/react/{rest}");
        match std::fs::read(&path) {
            Ok(bytes) => match parse_clip(&key, &bytes) {
                Ok(clip) => {
                    out.insert(key, clip);
                }
                Err(e) => eprintln!("render3d-live: skipping {name}: {e}"),
            },
            Err(e) => eprintln!("render3d-live: cannot read {name}: {e}"),
        }
    }
    out
}

fn build_backend(rig: &Rig, args: &Args) -> Result<VulkanBackend, String> {
    let view = look_at(args.eye, args.target, [0.0, 1.0, 0.0]);
    let mut scene = plate_rig_scene(view.inverse());
    scene.view = view;
    scene.tan_half_y = args.tan_half_y;
    scene.tan_half_x = args.tan_half_y * (args.width as f64 / args.height as f64);
    let mut packed = pack_scene(&rig.model, &scene);
    // Compositing: the stage provides the backdrop; the figure
    // must arrive with a true silhouette alpha. Opaque fragments
    // write alpha 1 and the overlay pass keeps readback alpha a
    // correct coverage composite, so clearing alpha 0 is the
    // whole mechanism (SCOPE §3.3).
    packed.clear_rgba[3] = 0.0;
    let mut backend = VulkanBackend::new().map_err(|e| e.to_string())?;
    backend
        .upload_scene(&packed, args.width, args.height)
        .map_err(|e| e.to_string())?;
    Ok(backend)
}

fn draw_frame(rig: &Rig, eval: &EvalFrame, frame: u32) -> Result<DrawFrame, String> {
    let identity = Mat4::from_rows([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    Ok(DrawFrame {
        frame,
        joint_matrices: rig.joint_matrices(eval),
        mesh_weights: rig.mesh_weights(eval)?,
        visible_meshes: vec![true; rig.model.meshes.len()],
        include_set: false,
        toy_model: identity,
    })
}

fn run_probe(
    rig: &Rig,
    clips: &BTreeMap<String, Clip>,
    args: &Args,
    probe: &ProbeArgs,
) -> Result<(), String> {
    let mut backend = build_backend(rig, args)?;
    eprintln!("render3d-live probe: backend {}", backend.describe());
    let eval = if probe.rest {
        EvalFrame::default()
    } else {
        let key = probe
            .clip
            .clone()
            .ok_or("--probe wants --clip KEY (or --rest)")?;
        let clip = clips
            .get(&key)
            .ok_or_else(|| format!("clip {key} not loaded"))?;
        let span = clip.loop_end - clip.loop_start;
        let pos = if span > 0.0 {
            clip.loop_start + probe.frame.rem_euclid(span)
        } else {
            probe.frame
        };
        eprintln!(
            "render3d-live probe: clip {key} frame {} (loop pos {pos:.2}), freeze {:?}",
            probe.frame, clip.freeze_sha
        );
        eval_clip(clip, pos)
    };
    let draw = draw_frame(rig, &eval, 0)?;
    let first = backend.render_frame(&draw).map_err(|e| e.to_string())?;
    let second = backend.render_frame(&draw).map_err(|e| e.to_string())?;
    let hash_a = sha256_hex(&first.rgba);
    let hash_b = sha256_hex(&second.rgba);
    // Alpha census: how much of the frame is silhouette?
    let mut alpha_zero = 0_usize;
    let mut alpha_full = 0_usize;
    let mut alpha_mid = 0_usize;
    for px in first.rgba.chunks_exact(4) {
        match px[3] {
            0 => alpha_zero += 1,
            255 => alpha_full += 1,
            _ => alpha_mid += 1,
        }
    }
    let total = (first.width * first.height) as usize;
    println!("probe sha256 render 1: {hash_a}");
    println!("probe sha256 render 2: {hash_b}");
    println!("probe twin identical: {}", hash_a == hash_b);
    println!(
        "probe alpha: zero {alpha_zero} ({:.1}%), full {alpha_full} ({:.1}%), mid {alpha_mid}",
        100.0 * alpha_zero as f64 / total as f64,
        100.0 * alpha_full as f64 / total as f64
    );
    let vector = Rig::vector_of(&eval);
    let mut params = serde_json::Map::new();
    for name in PARAM_NAMES {
        params.insert(
            name.to_string(),
            serde_json::json!(vector.get(name).expect("declared parameter")),
        );
    }
    let sidecar = serde_json::json!({
        "clip": probe.clip,
        "frame": probe.frame,
        "rest": probe.rest,
        "vector": params,
        "sha256_rgba": hash_a,
        "twin_identical": hash_a == hash_b,
        "alpha": {"zero": alpha_zero, "full": alpha_full, "mid": alpha_mid},
        "width": first.width,
        "height": first.height,
        "backend": backend.describe(),
    });
    let image = Image::new(first.width, first.height, first.rgba).map_err(|e| e.to_string())?;
    write_png(&probe.out, &image).map_err(|e| e.to_string())?;
    let sidecar_path = probe.out.with_extension("json");
    std::fs::write(
        &sidecar_path,
        serde_json::to_string_pretty(&sidecar).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    println!(
        "probe wrote {} + {}",
        probe.out.display(),
        sidecar_path.display()
    );
    if hash_a != hash_b {
        return Err("twin-render hashes differ: determinism witness FAILED".into());
    }
    Ok(())
}

fn run_stream(rig: &Rig, clips: &BTreeMap<String, Clip>, args: &Args) -> Result<(), String> {
    let mut backend = build_backend(rig, args)?;
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());
    let handshake = serde_json::json!({
        "ready": true,
        "backend": backend.describe(),
        "width": args.width,
        "height": args.height,
        "clips": clips.keys().collect::<Vec<_>>(),
    });
    writeln!(out, "{handshake}").map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;

    // Drive states from stdin, on a reader thread.
    let (tx, rx) = mpsc::channel::<StateMsg>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut lines = BufReader::new(stdin.lock()).lines();
        while let Some(Ok(line)) = lines.next() {
            if let Some(msg) = parse_state(&line) {
                if tx.send(msg).is_err() {
                    break;
                }
            }
        }
    });

    let frame_dt = Duration::from_secs_f64(1.0 / args.fps);
    let mut current_key: Option<String> = None;
    let mut current_eval = EvalFrame::default();
    let mut from_eval: Option<EvalFrame> = None;
    let mut blend_left = 0.0_f64;
    let mut clip_clock = 0.0_f64; // seconds inside the current clip
    let mut state_seq: u64 = 0;
    let mut fallback_expr: BTreeMap<String, f64> = BTreeMap::new();
    let mut frame_no: u32 = 0;
    let mut last = Instant::now();
    // Rolling stats.
    let mut stat_eval = Duration::ZERO;
    let mut stat_render = Duration::ZERO;
    let mut stat_write = Duration::ZERO;
    let mut stat_count = 0_u32;

    loop {
        // Drain pending drive states; the newest wins.
        let mut newest: Option<StateMsg> = None;
        while let Ok(msg) = rx.try_recv() {
            newest = Some(msg);
        }
        if let Some(msg) = newest {
            state_seq = msg.seq;
            if !msg.expr_pm.is_empty() {
                fallback_expr = msg.expr_pm.clone();
            }
            let want = if msg.stop {
                Some("lana/react/composed.t0".to_string())
            } else {
                msg.clip.clone()
            };
            if let Some(key) = want {
                if clips.contains_key(&key) && current_key.as_deref() != Some(key.as_str()) {
                    from_eval = Some(current_eval.clone());
                    blend_left = if msg.stop { 0.0 } else { 0.15 };
                    current_key = Some(key);
                    clip_clock = 0.0;
                } else if msg.stop {
                    // Already on the stop clip: restart at its loop
                    // point, no blend — the halt is immediate.
                    blend_left = 0.0;
                    from_eval = None;
                    clip_clock = 0.0;
                }
            }
        }

        let now = Instant::now();
        let dt = now.duration_since(last);
        if dt < frame_dt {
            std::thread::sleep(frame_dt - dt);
        }
        let step = Instant::now();
        let dt = step.duration_since(last).as_secs_f64();
        last = step;

        // Evaluate.
        let t_eval = Instant::now();
        clip_clock += dt;
        let eval = match current_key.as_ref().and_then(|k| clips.get(k)) {
            Some(clip) => {
                let span = clip.loop_end - clip.loop_start;
                let pos = if span > 0.0 {
                    clip.loop_start + (clip_clock * clip.rate).rem_euclid(span)
                } else {
                    clip.loop_start
                };
                eval_clip(clip, pos)
            }
            None => {
                // No clip: rest pose, GUI-supplied expression.
                EvalFrame {
                    params: fallback_expr.clone(),
                    ..EvalFrame::default()
                }
            }
        };
        let eval = if blend_left > 0.0 {
            blend_left -= dt;
            let t = (1.0 - blend_left / 0.15).clamp(0.0, 1.0);
            match &from_eval {
                Some(from) => blend_eval(from, &eval, t),
                None => eval,
            }
        } else {
            from_eval = None;
            eval
        };
        current_eval = eval.clone();
        stat_eval += t_eval.elapsed();

        // Render.
        let t_render = Instant::now();
        let draw = draw_frame(rig, &eval, frame_no)?;
        let rendered = match backend.render_frame(&draw) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("render3d-live: render_frame failed: {e}");
                return Err(e.to_string());
            }
        };
        stat_render += t_render.elapsed();

        // Write the frame packet.
        let t_write = Instant::now();
        let payload_len = 8 + 8 + 4 + 4 + rendered.rgba.len();
        let mut packet = Vec::with_capacity(4 + payload_len);
        packet.extend_from_slice(&(payload_len as u32).to_le_bytes());
        packet.extend_from_slice(&(frame_no as u64).to_le_bytes());
        packet.extend_from_slice(&state_seq.to_le_bytes());
        packet.extend_from_slice(&rendered.width.to_le_bytes());
        packet.extend_from_slice(&rendered.height.to_le_bytes());
        packet.extend_from_slice(&rendered.rgba);
        if out.write_all(&packet).and_then(|_| out.flush()).is_err() {
            // The GUI went away; exit quietly.
            eprintln!("render3d-live: stdout closed, exiting");
            return Ok(());
        }
        stat_write += t_write.elapsed();

        frame_no += 1;
        stat_count += 1;
        if stat_count >= 90 {
            eprintln!(
                "render3d-live stats: frames {frame_no}, avg eval {:.2} ms, render {:.2} ms, write {:.2} ms",
                stat_eval.as_secs_f64() * 1000.0 / stat_count as f64,
                stat_render.as_secs_f64() * 1000.0 / stat_count as f64,
                stat_write.as_secs_f64() * 1000.0 / stat_count as f64
            );
            stat_eval = Duration::ZERO;
            stat_render = Duration::ZERO;
            stat_write = Duration::ZERO;
            stat_count = 0;
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let bytes = std::fs::read(&args.glb).map_err(|e| format!("glb: {e}"))?;
    let model = load_model(&bytes).map_err(|e| e.to_string())?;
    let rig = Rig::new(model);
    eprintln!(
        "render3d-live: glb {} ({} meshes, {} joints, hips node {:?})",
        args.glb.display(),
        rig.model.meshes.len(),
        rig.model.skin.joints.len(),
        rig.hips_node
    );
    let clips = load_clips(&args.clips_dir);
    eprintln!(
        "render3d-live: {} clips loaded from {}",
        clips.len(),
        args.clips_dir.display()
    );
    match &args.probe {
        Some(probe) => run_probe(&rig, &clips, &args, probe),
        None => run_stream(&rig, &clips, &args),
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(detail) => {
            eprintln!("render3d-live: {detail}");
            std::process::ExitCode::FAILURE
        }
    }
}
