//! Deterministic fixed-step fluid particle simulation,
//! ported from the D `torogao.cli.fluids` module.

use serde_json::{Value, json};

use crate::audioproc::Xoshiro256;
use crate::packs::{pack_num, pack_str};
use crate::util::jget;

pub const FLUID_DT: f64 = 1.0 / 120.0;
pub const FLUID_FORMAT: &str = "torogao-fluid/1";
pub const FLUID_MAX_PARTICLES: usize = 4096;
pub const FLUID_MAX_SECONDS: f64 = 120.0;

pub fn round4(v: f64) -> Value {
    json!((v * 10000.0).round() / 10000.0)
}

pub fn round6(v: f64) -> Value {
    json!((v * 1000000.0).round() / 1000000.0)
}

fn floating_or(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

struct Particle {
    born_step: i64,
    strand_recorded: bool,
    vx: f64,
    vy: f64,
    x: f64,
    y: f64,
}

fn pressure_at(behavior: &str, cycle: f64, t: f64) -> f64 {
    if behavior != "milk" {
        return 1.0;
    }
    let phase = (t / cycle) % 1.0;
    let s = (2.0 * std::f64::consts::PI * phase).sin();
    if s < 0.0 {
        return 0.0;
    }
    s * s * s
}

pub fn point_in_area(px: f64, py: f64, area: &Value) -> bool {
    let shape = jget(area, "shape");
    let kind = pack_str(shape, "kind");
    let ax = pack_num(shape, "x", 0.0);
    let ay = pack_num(shape, "y", 0.0);
    if kind == "circle" {
        let r = pack_num(shape, "r", 0.0);
        let dx = px - ax;
        let dy = py - ay;
        return dx * dx + dy * dy <= r * r;
    }
    if kind == "rect" {
        let w = pack_num(shape, "w", 0.0);
        let h = pack_num(shape, "h", 0.0);
        return px >= ax - w / 2.0
            && px <= ax + w / 2.0
            && py >= ay - h / 2.0
            && py <= ay + h / 2.0;
    }
    false
}

pub fn hit_areas_from_ext(ext: &Value) -> Vec<Value> {
    match jget(jget(ext, "consumer"), "hitAreas").as_array() {
        Some(a) => a.clone(),
        None => Vec::new(),
    }
}

pub fn is_face_area(area: &Value) -> bool {
    let part = pack_str(jget(area, "binding"), "part");
    let name = pack_str(area, "name");
    part.to_lowercase().contains("face") || name.to_lowercase().contains("face") || part == "Head"
}

pub fn simulate_fluid(
    emitter: &Value,
    hit_areas: &[Value],
    seconds: f64,
    face_expression: &str,
) -> Value {
    let behavior = pack_str(emitter, "behavior");
    let source = jget(emitter, "source");
    let sx = pack_num(source, "x", 0.0);
    let sy = pack_num(source, "y", 0.0);
    let rate = pack_num(emitter, "rate", 30.0);
    let mut cycle = pack_num(emitter, "cycleSeconds", 2.4);
    if cycle <= 0.0 {
        cycle = 2.4;
    }
    let lactation = pack_num(emitter, "lactation", 1.0);
    let floor_y = pack_num(emitter, "floorY", -400.0);
    let (mut dx, mut dy) = (0.25f64, -1.0f64);
    if let Some(dir) = jget(emitter, "direction").as_array() {
        if dir.len() == 2 {
            if let (Some(a), Some(b)) = (floating_or(&dir[0]), floating_or(&dir[1])) {
                dx = a;
                dy = b;
            }
        }
    }
    let len = (dx * dx + dy * dy).sqrt();
    if len > 0.0 {
        dx /= len;
        dy /= len;
    }
    let seed: i64 = match jget(emitter, "seed") {
        Value::Number(n) => n
            .as_i64()
            .unwrap_or_else(|| n.as_u64().map_or(0, |u| u as i64)),
        _ => 0,
    };
    let mut rng = Xoshiro256::from_seed(seed as u64);
    let seconds = seconds.clamp(0.0, FLUID_MAX_SECONDS);
    let steps = (seconds / FLUID_DT + 0.5) as i64;
    let mut live: Vec<Particle> = Vec::new();
    let mut emit_acc = 0.0f64;
    let mut spawned: i64 = 0;
    let mut escaped: i64 = 0;
    let mut landings: Vec<Value> = Vec::new();
    let mut strands: Vec<Value> = Vec::new();
    let mut face_contacts: Vec<Value> = Vec::new();
    let mut accumulation: Vec<(String, f64)> = Vec::new();
    let gravity = if behavior == "saliva" { -340.0 } else { -980.0 };
    for step in 0..steps {
        let t = step as f64 * FLUID_DT;
        let mut eff_rate = rate;
        if behavior == "milk" {
            eff_rate = rate * lactation * (0.15 + pressure_at(&behavior, cycle, t));
        }
        if behavior == "saliva" && eff_rate > 6.0 {
            eff_rate = 6.0;
        }
        emit_acc += eff_rate * FLUID_DT;
        while emit_acc >= 1.0 && live.len() < FLUID_MAX_PARTICLES {
            emit_acc -= 1.0;
            let u1 = (rng.uniform() + 1.0) / 2.0;
            let u2 = (rng.uniform() + 1.0) / 2.0;
            let spread = (u1 - 0.5) * 0.7;
            let cs = spread.cos();
            let sn = spread.sin();
            let rx = dx * cs - dy * sn;
            let ry = dx * sn + dy * cs;
            let speed = if behavior == "spray" {
                210.0 + 90.0 * u2
            } else if behavior == "milk" {
                (30.0 + 120.0 * u2) * (0.35 + pressure_at(&behavior, cycle, t))
            } else {
                12.0 + 10.0 * u2
            };
            live.push(Particle {
                born_step: step,
                strand_recorded: false,
                vx: rx * speed,
                vy: ry * speed,
                x: sx,
                y: sy,
            });
            spawned += 1;
        }
        if emit_acc >= 1.0 {
            emit_acc = 0.0;
        }
        let mut kept: Vec<Particle> = Vec::with_capacity(live.len());
        for mut p in live {
            p.vy += gravity * FLUID_DT;
            p.x += p.vx * FLUID_DT;
            p.y += p.vy * FLUID_DT;
            let age = (step - p.born_step) as f64 * FLUID_DT;
            if behavior == "saliva" && !p.strand_recorded && age >= 0.5 {
                p.strand_recorded = true;
                strands.push(json!({
                    "step": step,
                    "time": round6(t),
                    "x": round4(p.x),
                    "y": round4(p.y),
                }));
            }
            let mut landed = false;
            for area in hit_areas {
                if point_in_area(p.x, p.y, area) {
                    let part = pack_str(jget(area, "binding"), "part");
                    landings.push(json!({
                        "part": part,
                        "step": step,
                        "time": round6(t),
                        "x": round4(p.x),
                        "y": round4(p.y),
                    }));
                    let amount = if behavior == "milk" {
                        1.0
                    } else if behavior == "saliva" {
                        0.8
                    } else {
                        0.6
                    };
                    match accumulation.iter_mut().find(|(name, _)| *name == part) {
                        Some(entry) => entry.1 += amount,
                        None => accumulation.push((part.clone(), amount)),
                    }
                    if !face_expression.is_empty() && is_face_area(area) {
                        face_contacts.push(json!({
                            "expression": face_expression,
                            "part": part,
                            "step": step,
                            "time": round6(t),
                        }));
                    }
                    landed = true;
                    break;
                }
            }
            if landed {
                continue;
            }
            if p.y < floor_y {
                escaped += 1;
                continue;
            }
            kept.push(p);
        }
        live = kept;
    }
    accumulation.sort_by(|a, b| a.0.cmp(&b.0));
    let accumulation_json: Vec<Value> = accumulation
        .iter()
        .map(|(part, amount)| json!({"amount": round4(*amount), "part": part}))
        .collect();
    let mut doc = json!({
        "accumulation": accumulation_json,
        "behavior": behavior,
        "durationSeconds": round6(steps as f64 * FLUID_DT),
        "emitter": pack_str(emitter, "id"),
        "faceContacts": face_contacts,
        "format": FLUID_FORMAT,
        "landings": landings,
        "particlesEscaped": escaped,
        "particlesSpawned": spawned,
        "seed": seed,
        "sourcePart": pack_str(source, "part"),
        "steps": steps,
        "strands": strands,
    });
    if behavior == "milk" {
        doc["cycleSeconds"] = json!(cycle);
        doc["lactation"] = json!(lactation);
    }
    doc
}
