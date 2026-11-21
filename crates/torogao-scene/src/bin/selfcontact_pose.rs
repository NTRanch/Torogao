//! Prints the solved finger curls (and the squash weight at the
//! baked depth) for one self-contact pose, as JSON on stdout.
//!
//! Usage: `selfcontact_pose <radius_m> <press_m> <finger>:<root_m>:<l1>,<l2>,<l3> ...`
//! Every number is passed in; nothing is read from disk. Exit code 2
//! on bad arguments, 1 on a refused solve.
//!
//! Section mode (P08 option 1): one finger against the real surface,
//! `selfcontact_pose section <clearance_m> <lo>:<hi>,... <finger>:<l1>,<l2>,...`
//! with the joint limits in degrees and the section's pieces on stdin,
//! one per line: `ax ay bx by` (bend-plane metres). Nothing else is read.

use std::io::Read;
use std::process::ExitCode;

use torogao_scene::{curl, follow, squash};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let res = if args.first().map(String::as_str) == Some("section") {
        section(&args[1..])
    } else {
        run(&args)
    };
    match res {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err((code, msg)) => {
            eprintln!("selfcontact_pose: {msg}");
            ExitCode::from(code)
        }
    }
}

fn num(s: &str, what: &str) -> Result<f64, (u8, String)> {
    s.parse::<f64>()
        .map_err(|_| (2, format!("{what}: not a number: {s}")))
}

fn run(args: &[String]) -> Result<String, (u8, String)> {
    if args.len() < 3 {
        return Err((
            2,
            "usage: selfcontact_pose <radius_m> <press_m> <finger>:<root_m>:<l1>,<l2>,... ..."
                .into(),
        ));
    }
    let radius = num(&args[0], "radius_m")?;
    let press = num(&args[1], "press_m")?;
    let mut fingers = Vec::new();
    for spec in &args[2..] {
        let parts: Vec<&str> = spec.split(':').collect();
        if parts.len() != 3
            || parts[0].is_empty()
            || !parts[0].chars().all(|c| c.is_ascii_alphanumeric())
        {
            return Err((
                2,
                format!("finger spec must be name:root_m:l1,l2,...: {spec}"),
            ));
        }
        let root = num(parts[1], "root_m")?;
        let segs = parts[2]
            .split(',')
            .map(|x| num(x, "segment"))
            .collect::<Result<Vec<_>, _>>()?;
        let c = curl::wrap(radius, press, root, &segs)
            .map_err(|e| (1, format!("{}: {e}", parts[0])))?;
        let deg: Vec<String> = c
            .joint_rad
            .iter()
            .map(|a| format!("{:.4}", a.to_degrees()))
            .collect();
        let hit: Vec<&str> = c
            .on_surface
            .iter()
            .map(|&h| if h { "true" } else { "false" })
            .collect();
        fingers.push((parts[0].to_string(), deg.join(", "), hit.join(", ")));
    }
    fingers.sort();
    // the squash key's weight at its baked depth (null when the palm line stands clear)
    let w = if press > 0.0 {
        squash::weight(press, press)
            .map_err(|e| (1, e.to_string()))?
            .to_string()
    } else {
        "null".to_string()
    };
    let curls: Vec<String> = fingers
        .iter()
        .map(|(n, d, _)| format!("  \"{n}\": [{d}]"))
        .collect();
    let hits: Vec<String> = fingers
        .iter()
        .map(|(n, _, h)| format!("  \"{n}\": [{h}]"))
        .collect();
    Ok(format!(
        "{{\n \"finger_curl_deg\": {{\n{}\n }},\n \"segment_on_surface\": {{\n{}\n }},\n \"press_m\": {press},\n \"radius_m\": {radius},\n \"squash_weight_at_baked_depth\": {w}\n}}",
        curls.join(",\n "),
        hits.join(",\n ")
    ))
}

fn section(args: &[String]) -> Result<String, (u8, String)> {
    if args.len() != 3 {
        return Err((
            2,
            "usage: selfcontact_pose section <clearance_m> <lo>:<hi>,... <finger>:<l1>,<l2>,... < pieces"
                .into(),
        ));
    }
    let clearance = num(&args[0], "clearance_m")?;
    let limits = args[1]
        .split(',')
        .map(|lh| {
            let (lo, hi) = lh
                .split_once(':')
                .ok_or((2, format!("limit must be lo:hi: {lh}")))?;
            Ok((
                num(lo, "limit")?.to_radians(),
                num(hi, "limit")?.to_radians(),
            ))
        })
        .collect::<Result<Vec<_>, (u8, String)>>()?;
    let (name, segs) = args[2]
        .split_once(':')
        .ok_or((2, format!("finger must be name:l1,l2,...: {}", args[2])))?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err((2, format!("bad finger name: {name}")));
    }
    let segs = segs
        .split(',')
        .map(|x| num(x, "segment"))
        .collect::<Result<Vec<_>, _>>()?;
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|e| (2, format!("stdin: {e}")))?;
    let mut pieces = Vec::new();
    for (k, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let v = line
            .split_whitespace()
            .map(|x| num(x, "piece"))
            .collect::<Result<Vec<_>, _>>()?;
        if v.len() != 4 {
            return Err((2, format!("piece line {k} needs 4 numbers")));
        }
        pieces.push([v[0], v[1], v[2], v[3]]);
    }
    let f = follow::follow(&pieces, &segs, &limits, clearance)
        .map_err(|e| (1, format!("{name}: {e}")))?;
    let deg: Vec<String> = f
        .joint_rad
        .iter()
        .map(|a| format!("{:.4}", a.to_degrees()))
        .collect();
    let touch: Vec<String> = f
        .touch
        .iter()
        .map(|t| format!("\"{}\"", t.name()))
        .collect();
    let gap: Vec<String> = f.gap_m.iter().map(|g| format!("{g:.6}")).collect();
    Ok(format!(
        "{{\"finger\": \"{name}\", \"curl_deg\": [{}], \"touch\": [{}], \"gap_m\": [{}], \"pieces\": {}, \"clearance_m\": {clearance}}}",
        deg.join(", "),
        touch.join(", "),
        gap.join(", "),
        pieces.len()
    ))
}
