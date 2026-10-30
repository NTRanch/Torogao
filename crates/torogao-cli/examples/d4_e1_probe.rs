//! D-4 arm E1 probe: evaluate one parameter vector on a
//! repertoire rig through the real INP engine and report the
//! evaluated gaze/blush response — the iris nodes'
//! deformation centroids and the blush nodes' effective
//! opacity (base opacity x binding-driven offset).
//!
//! Usage: `d4_e1_probe <rig.inx> <probe.json>`
//!
//! The probe file is `{"label": "...", "set": {"ParamGaze":
//! [x, y], "ParamCheek": [v], ...}}`. Values are written to
//! the parameters by name, the puppet is updated once from
//! its loaded rest state, and one JSON object is printed.
//! An unknown parameter name is a hard error (that is the
//! arm-E0 evidence: the channel does not exist).

use std::env;
use std::fs;
use std::process::ExitCode;

use serde_json::{Value, json};
use torogao_engine::{Puppet, Vec2};
use torogao_format::container::read_inp;
use torogao_format::payload::decode_payload;

const IRIS_L: u32 = 3001;
const IRIS_R: u32 = 3002;
const BLUSH_L: u32 = 3301;
const BLUSH_R: u32 = 3302;

fn centroid(puppet: &Puppet, uuid: u32) -> Option<[f64; 2]> {
    let node = puppet.nodes.nodes.iter().find(|node| node.uuid == uuid)?;
    if node.deformation.is_empty() {
        return Some([0.0, 0.0]);
    }
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for offset in &node.deformation {
        x += f64::from(offset.x);
        y += f64::from(offset.y);
    }
    let count = node.deformation.len() as f64;
    Some([x / count, y / count])
}

fn effective_opacity(puppet: &Puppet, uuid: u32) -> Option<f64> {
    let node = puppet.nodes.nodes.iter().find(|node| node.uuid == uuid)?;
    Some(f64::from(node.opacity * node.offset_opacity))
}

fn run() -> Result<Value, String> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: d4_e1_probe <rig.inx> <probe.json>".to_string());
    }
    let bytes = fs::read(&args[1]).map_err(|error| error.to_string())?;
    let inp = read_inp(&bytes).map_err(|error| error.to_string())?;
    let doc = decode_payload(&inp.payload).map_err(|error| error.to_string())?;
    let mut puppet = Puppet::load(&doc, &inp).map_err(|error| error.to_string())?;

    let probe_text = fs::read_to_string(&args[2]).map_err(|error| error.to_string())?;
    let probe: Value = serde_json::from_str(&probe_text).map_err(|error| error.to_string())?;
    let set = probe
        .get("set")
        .and_then(Value::as_object)
        .ok_or("probe carries no 'set' object")?;
    for (name, value) in set {
        let idx = puppet
            .param_index(name)
            .ok_or_else(|| format!("parameter '{name}' does not exist on this rig"))?;
        let pair = value.as_array().ok_or("probe value is not an array")?;
        let x = pair.first().and_then(Value::as_f64).unwrap_or(0.0) as f32;
        let y = pair.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        puppet.params[idx].value = Vec2::new(x, y);
    }
    puppet.update().map_err(|error| error.to_string())?;

    Ok(json!({
        "label": probe.get("label").cloned().unwrap_or(Value::Null),
        "iris_l_centroid": centroid(&puppet, IRIS_L),
        "iris_r_centroid": centroid(&puppet, IRIS_R),
        "blush_l_opacity": effective_opacity(&puppet, BLUSH_L),
        "blush_r_opacity": effective_opacity(&puppet, BLUSH_R),
    }))
}

fn main() -> ExitCode {
    match run() {
        Ok(report) => {
            println!("{report}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("probe failed: {error}");
            ExitCode::FAILURE
        }
    }
}
