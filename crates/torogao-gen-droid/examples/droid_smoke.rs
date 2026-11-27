//! Host/device smoke for the native backend: runs the canonical
//! Prima prompt (composed exactly as `torogao-mobile-core` composes
//! it — style prefix + Prima identity + signature + fixed scene)
//! through the engine's submit → poll → fetch contract and writes
//! the still plus a timing line as JSON.
//!
//! Environment:
//! - `DROID_MODEL`   path to the checkpoint (GGUF or safetensors)
//! - `DROID_BACKEND` ggml backend string (default: empty = library
//!   default; `vulkan0` on the bench device)
//! - `DROID_MODE`    `recipe` (28 steps, CFG 6, euler_a) or
//!   `fast4` / `fast8` (Lightning modes: euler, CFG 1) — the model
//!   file must match the mode (a Lightning-merged checkpoint for
//!   the fast modes)
//! - `DROID_OUT`     output PNG path (default `./droid-smoke.png`)
//! - `DROID_W` / `DROID_H`  size override (default 832×1344)

use std::time::{Duration, Instant};

use torogao_gen_droid::native::NativeBackend;
use torogao_gen_droid::{Engine, GenParams, JobState};

const POSITIVE: &str = "masterpiece, best quality, amazing quality, very aesthetic, \
    1girl, solo, very long wavy silver-white hair, lavender-grey hair shading, \
    centre part, face-framing locks, deep red eyes, fair skin, pink dress, \
    frilled dress, standing in a rose garden, soft evening light";
const NEGATIVE: &str = "lowres, bad anatomy, bad hands, text, error, missing fingers, \
    extra digit, fewer digits, cropped, worst quality, low quality, jpeg artifacts, \
    signature, watermark, username, blurry";
const SEED: i64 = 1795737960925571585;

fn main() {
    let model = std::env::var("DROID_MODEL").expect("DROID_MODEL is required");
    let backend = std::env::var("DROID_BACKEND").unwrap_or_default();
    let mode = std::env::var("DROID_MODE").unwrap_or_else(|_| "recipe".to_owned());
    let out = std::env::var("DROID_OUT").unwrap_or_else(|_| "droid-smoke.png".to_owned());

    let mut params = GenParams::recipe(POSITIVE.to_owned(), NEGATIVE.to_owned(), SEED);
    match mode.as_str() {
        "recipe" => {}
        "fast4" => {
            params.steps = 4;
            params.cfg = 1.0;
            params.sampler = "euler".to_owned();
            params.scheduler = None;
        }
        "fast8" => {
            params.steps = 8;
            params.cfg = 1.0;
            params.sampler = "euler".to_owned();
            params.scheduler = None;
        }
        other => panic!("unknown DROID_MODE {other}"),
    }
    if let Ok(w) = std::env::var("DROID_W") {
        params.width = w.parse().expect("DROID_W");
    }
    if let Ok(h) = std::env::var("DROID_H") {
        params.height = h.parse().expect("DROID_H");
    }

    let engine = Engine::new(NativeBackend::new(model.into(), backend));
    let started = Instant::now();
    let id = engine.submit(params).expect("submit");
    let state = loop {
        match engine.poll(id).expect("poll") {
            JobState::Done => break "done",
            JobState::Failed(why) => {
                println!("{{\"state\":\"failed\",\"error\":{:?}}}", why);
                std::process::exit(1);
            }
            _ => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    let img = engine.fetch(id).expect("fetch");
    std::fs::write(&out, &img.png).expect("write png");
    println!(
        "{{\"state\":\"{state}\",\"mode\":\"{mode}\",\"seconds\":{:.2},\"width\":{},\"height\":{},\"png_bytes\":{},\"out\":\"{out}\"}}",
        started.elapsed().as_secs_f64(),
        img.width,
        img.height,
        img.png.len()
    );
}
