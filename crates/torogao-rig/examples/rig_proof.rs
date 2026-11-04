//! Corpus proof driver: render one evaluated frame of an `.inx`
//! record to a PNG, mirroring the estate spike driver's CLI
//! contract so gate scripts can diff frames across evaluators.
//!
//! Usage: rig_proof --inx FILE --out FILE.png [--frames N]
//!        [--set Name=x[,y]]... [--scale S] [--width W] [--height H]
//!
//! Each frame: begin, apply the sets, end at dt = 1/60. Prints a
//! JSON line with the canvas size and nontransparent pixel count.

use std::process::ExitCode;

use torogao_rig::math::Vec2;
use torogao_rig::{Evaluator, load_inp, render_frame};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut inx: Option<String> = None;
    let mut out: Option<String> = None;
    let mut frames: usize = 30;
    let mut scale: f32 = 0.15;
    let mut width: usize = 600;
    let mut height: usize = 800;
    let mut sets: Vec<(String, Vec2)> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let take = |i: &mut usize| -> Option<String> {
            *i += 1;
            args.get(*i).cloned()
        };
        match arg.as_str() {
            "--inx" => inx = take(&mut i),
            "--out" => out = take(&mut i),
            "--frames" => frames = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(30),
            "--scale" => scale = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(0.15),
            "--width" => width = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(600),
            "--height" => height = take(&mut i).and_then(|v| v.parse().ok()).unwrap_or(800),
            "--set" => {
                let Some(spec) = take(&mut i) else {
                    return fail("--set needs a value");
                };
                let Some((name, value)) = parse_set(&spec) else {
                    return fail(&format!("bad --set '{spec}' (want Name=x[,y])"));
                };
                sets.push((name, value));
            }
            other => return fail(&format!("unknown argument '{other}'")),
        }
        i += 1;
    }
    let (Some(inx), Some(out)) = (inx, out) else {
        return fail(
            "usage: rig_proof --inx FILE --out FILE.png [--frames N] [--set Name=x[,y]]...",
        );
    };
    let bytes = match std::fs::read(&inx) {
        Ok(b) => b,
        Err(e) => return fail(&format!("cannot read {inx}: {e}")),
    };
    let model = match load_inp(&bytes) {
        Ok(m) => m,
        Err(e) => return fail(&format!("import failed: {e}")),
    };
    let mut evaluator = Evaluator::new(model);
    for _ in 0..frames {
        evaluator.begin_frame();
        for (name, value) in &sets {
            if let Err(e) = evaluator.set_param(name, *value) {
                return fail(&format!("set failed: {e}"));
            }
        }
        evaluator.end_frame(1.0 / 60.0);
    }
    let canvas = render_frame(&evaluator, width, height, scale);
    let image = match image::RgbaImage::from_raw(width as u32, height as u32, canvas.pixels.clone())
    {
        Some(img) => img,
        None => return fail("canvas size mismatch"),
    };
    if let Err(e) = image.save(&out) {
        return fail(&format!("cannot write {out}: {e}"));
    }
    println!(
        "{{\"file\": \"{}\", \"width\": {}, \"height\": {}, \"nontransparent\": {}}}",
        out,
        width,
        height,
        canvas.nontransparent_count()
    );
    ExitCode::SUCCESS
}

fn parse_set(spec: &str) -> Option<(String, Vec2)> {
    let (name, value) = spec.rsplit_once('=')?;
    let mut parts = value.split(',');
    let x: f32 = parts.next()?.parse().ok()?;
    let y: f32 = parts.next().and_then(|v| v.parse().ok()).unwrap_or(0.0);
    Some((name.to_owned(), Vec2::new(x, y)))
}

fn fail(message: &str) -> ExitCode {
    eprintln!("rig_proof: {message}");
    ExitCode::from(2)
}
