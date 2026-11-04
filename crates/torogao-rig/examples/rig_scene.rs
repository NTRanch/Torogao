//! Two-body acceptance scene driver (Prima lifting Alma on the
//! D-859 full rigs): runs the 12 s scene through the Torogao
//! evaluator, writes per-step records and the verdict summary as
//! JSON, and renders stills at the acceptance keyframe steps.
//!
//! Usage: rig_scene --prima FILE.inx --alma FILE.inx --out-dir DIR

use std::process::ExitCode;

use torogao_rig::scene::{SceneStepView, run_scene, summarize};
use torogao_rig::{Canvas, load_inp, render_onto};

const STILL_STEPS: [usize; 6] = [120, 180, 240, 480, 570, 660];
const CANVAS_WIDTH: usize = 900;
const CANVAS_HEIGHT: usize = 800;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut prima_path: Option<String> = None;
    let mut alma_path: Option<String> = None;
    let mut out_dir: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let take = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match args[i].as_str() {
            "--prima" => prima_path = take(&mut i),
            "--alma" => alma_path = take(&mut i),
            "--out-dir" => out_dir = take(&mut i),
            other => {
                eprintln!("rig_scene: unknown argument '{other}'");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    let (Some(prima_path), Some(alma_path), Some(out_dir)) = (prima_path, alma_path, out_dir)
    else {
        eprintln!("usage: rig_scene --prima FILE --alma FILE --out-dir DIR");
        return ExitCode::from(2);
    };
    let prima = match std::fs::read(&prima_path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| load_inp(&bytes).map_err(|e| e.to_string()))
    {
        Ok(m) => m,
        Err(e) => {
            eprintln!("rig_scene: prima import failed: {e}");
            return ExitCode::from(2);
        }
    };
    let alma = match std::fs::read(&alma_path)
        .map_err(|e| e.to_string())
        .and_then(|bytes| load_inp(&bytes).map_err(|e| e.to_string()))
    {
        Ok(m) => m,
        Err(e) => {
            eprintln!("rig_scene: alma import failed: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = std::fs::create_dir_all(&out_dir) {
        eprintln!("rig_scene: cannot create {out_dir}: {e}");
        return ExitCode::from(2);
    }
    let out_dir_stills = out_dir.clone();
    let mut on_step = |view: &SceneStepView<'_>| {
        if STILL_STEPS.contains(&view.step) {
            let mut canvas = Canvas::new(CANVAS_WIDTH, CANVAS_HEIGHT);
            render_onto(view.prima, &mut canvas, 1.0, view.prima_placement);
            render_onto(view.alma, &mut canvas, 1.0, view.alma_placement);
            let path = format!("{out_dir_stills}/frame-{:03}.png", view.step);
            if let Some(image) =
                image::RgbaImage::from_raw(CANVAS_WIDTH as u32, CANVAS_HEIGHT as u32, canvas.pixels)
            {
                let _ = image.save(&path);
            }
        }
    };
    let (records, context) = match run_scene(prima, alma, &mut on_step) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("rig_scene: scene failed: {e}");
            return ExitCode::from(2);
        }
    };
    let summary = summarize(&records, &context);
    let records_json: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "step": r.step,
                "t": r.t,
                "engaged": r.engaged,
                "slip_left": r.slip_left,
                "slip_right": r.slip_right,
                "band_left": r.band_left,
                "band_right": r.band_right,
                "c2_margin": r.c2_margin,
                "c2_pair": [r.c2_pair.0, r.c2_pair.1],
                "prima_root_y": r.prima_root_y,
                "alma_clearance": r.alma_clearance,
            })
        })
        .collect();
    let summary_json = serde_json::json!({
        "prima_height": summary.prima_height,
        "alma_height": summary.alma_height,
        "c1_p50": summary.c1_p50,
        "c1_p95": summary.c1_p95,
        "c1_max": summary.c1_max,
        "c1_p95_pct_alma_height": summary.c1_p95 / summary.alma_height * 100.0,
        "c1_max_pct_alma_height": summary.c1_max / summary.alma_height * 100.0,
        "c1_anchor_p95": summary.c1_anchor_p95,
        "c1_anchor_max": summary.c1_anchor_max,
        "c1_support_p95": summary.c1_support_p95,
        "c1_support_max": summary.c1_support_max,
        "contact_fraction": summary.contact_fraction,
        "c2_engaged_min": summary.c2_engaged_min,
        "c2_scene_min": summary.c2_scene_min,
        "c2_scene_min_step": summary.c2_scene_min_step,
        "c4_dip_units": summary.c4_dip_units,
        "c4_dip_pct_height": summary.c4_dip_pct_height,
        "c4_hold_offset": summary.c4_hold_offset,
        "clearance_hold": summary.clearance_hold,
        "clearance_lift_min": summary.clearance_lift_min,
    });
    let records_path = format!("{out_dir}/records.json");
    let summary_path = format!("{out_dir}/summary.json");
    if let Err(e) = std::fs::write(
        &records_path,
        serde_json::to_string_pretty(&records_json).unwrap_or_default(),
    ) {
        eprintln!("rig_scene: cannot write {records_path}: {e}");
        return ExitCode::from(2);
    }
    if let Err(e) = std::fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary_json).unwrap_or_default(),
    ) {
        eprintln!("rig_scene: cannot write {summary_path}: {e}");
        return ExitCode::from(2);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&summary_json).unwrap_or_default()
    );
    ExitCode::SUCCESS
}
