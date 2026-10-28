//! The video bridge: played frames → the CLI video
//! family's documents, so play → video is a one-command
//! path.
//!
//! The bridge plays a named animation through the play
//! host (the performance of record: the player's fixed
//! 1/60 s step trajectory, per the play-parity ruling),
//! renders every frame offscreen, encodes the performance
//! clip with ffmpeg, and then builds, validates, and
//! exports the `torogao-timeline/1` document through the
//! CLI crate's own video implementation — the same code
//! the `video-build` / `video-validate` / `video-export`
//! tools serve, called in-process. The final clip is the
//! concat demuxer applied to the export's own concat
//! list. Determinism: the same play produces the same
//! timeline bytes on re-run (asserted by the gate).

use std::path::PathBuf;
use std::process::Command;

use serde_json::json;
use torogao_format::jsonfmt::to_d_json;

use crate::error::{AppError, AppResult};
use crate::player::PlayHost;
use crate::render::Renderer;
use crate::scene::{STEP_SECONDS, Scene};
use crate::textures::encode_png;

/// Largest frame count one bridge run will play.
pub const VIDEO_FRAMES_MAX: usize = 36_000;
/// The performance clip's file name inside the output dir.
pub const PERFORMANCE_CLIP: &str = "performance.mp4";
/// The timeline document's file name.
pub const TIMELINE_FILE: &str = "timeline.json";
/// The final concatenated clip's file name.
pub const FINAL_CLIP: &str = "final.mp4";

/// Options for one bridge run.
#[derive(Debug, Clone)]
pub struct BridgeOptions {
    /// The model file to load.
    pub model: PathBuf,
    /// The animation to play.
    pub animation: String,
    /// The output directory (created; must be empty or
    /// absent unless `force`).
    pub out_dir: PathBuf,
    /// Whether existing outputs may be overwritten.
    pub force: bool,
}

/// What one bridge run produced.
#[derive(Debug, Clone)]
pub struct BridgeReport {
    /// Frames played and rendered.
    pub frames: usize,
    /// The clip rate (the player's fixed step rate).
    pub fps: f64,
    /// The performance duration in seconds.
    pub duration_seconds: f64,
    /// The animation's authored timestep (recorded for
    /// the report; the play trajectory is the fixed-step
    /// one — see the module note).
    pub authored_timestep: f32,
    /// SHA-256 of the timeline document bytes.
    pub timeline_sha256: String,
    /// The video codec that encoded the performance clip.
    pub codec: String,
    /// Output file names, in production order.
    pub files: Vec<String>,
}

/// SHA-256 of a byte string, hex (the same digest the
/// workspace's session stack reports).
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Runs one ffmpeg invocation with separate argv values,
/// captured output, and a typed failure carrying a
/// bounded stderr excerpt.
fn run_ffmpeg(args: &[String]) -> AppResult<()> {
    let output = Command::new("ffmpeg")
        .args(args)
        .output()
        .map_err(|err| AppError::Io {
            context: "run ffmpeg",
            message: err.to_string(),
        })?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: String = stderr
        .chars()
        .rev()
        .take(400)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    Err(AppError::Render(format!(
        "ffmpeg failed ({}) on {}: {tail}",
        output.status,
        args.join(" ")
    )))
}

/// Whether the local ffmpeg offers libx264.
fn ffmpeg_has_libx264() -> bool {
    Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).contains("libx264"))
        .unwrap_or(false)
}

/// Plays the animation and produces the full bridge
/// output set in `options.out_dir`.
pub fn run_bridge(options: &BridgeOptions) -> AppResult<BridgeReport> {
    let mut scene = Scene::load_file(&options.model)?;
    scene.evaluate_fresh(&[])?;
    let (_, animation) = scene
        .puppet
        .animations
        .iter()
        .find(|(name, _)| name == &options.animation)
        .ok_or_else(|| {
            AppError::Param(format!(
                "model carries no animation named {:?}",
                options.animation
            ))
        })?;
    let frames = usize::try_from(animation.length).map_err(|_| {
        AppError::Format(format!(
            "animation {:?} has a negative length {}",
            options.animation, animation.length
        ))
    })?;
    if frames == 0 || frames > VIDEO_FRAMES_MAX {
        return Err(AppError::Format(format!(
            "animation {:?} length {frames} is outside 1..={VIDEO_FRAMES_MAX}",
            options.animation
        )));
    }
    let authored_timestep = animation.timestep;

    if options.out_dir.exists() && !options.force {
        let occupied = std::fs::read_dir(&options.out_dir)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if occupied {
            return Err(AppError::Param(format!(
                "bridge output dir {} is not empty; pass force to reuse it",
                options.out_dir.display()
            )));
        }
    }
    let frames_dir = options.out_dir.join("frames");
    std::fs::create_dir_all(&frames_dir).map_err(|err| AppError::Io {
        context: "create bridge output",
        message: err.to_string(),
    })?;

    // Play + render: frame k is the evaluated state after
    // k fixed steps of the play trajectory (frame 0 is the
    // state play() installs).
    let mut renderer = Renderer::new()?;
    let mut host = PlayHost::new();
    host.play(&scene, Some(&options.animation), false, None)?;
    for frame in 0..frames {
        let (image, _) = renderer.render(&mut scene)?;
        let encoded = encode_png(&image)?;
        let name = format!("frame-{frame:05}.png");
        std::fs::write(frames_dir.join(&name), encoded).map_err(|err| AppError::Io {
            context: "write frame",
            message: err.to_string(),
        })?;
        if frame + 1 < frames {
            host.step(&mut scene, 1)?;
        }
    }

    // Encode the performance clip from the frame sequence.
    let fps = 1.0 / STEP_SECONDS;
    let performance_path = options.out_dir.join(PERFORMANCE_CLIP);
    let (codec, codec_args): (&str, Vec<String>) = if ffmpeg_has_libx264() {
        (
            "libx264",
            vec![
                "-c:v".into(),
                "libx264".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ],
        )
    } else {
        (
            "mpeg4",
            vec![
                "-c:v".into(),
                "mpeg4".into(),
                "-pix_fmt".into(),
                "yuv420p".into(),
            ],
        )
    };
    let mut encode_args: Vec<String> = vec![
        "-y".into(),
        "-framerate".into(),
        format!("{fps}"),
        "-i".into(),
        frames_dir.join("frame-%05d.png").display().to_string(),
        "-vf".into(),
        "pad=ceil(iw/2)*2:ceil(ih/2)*2".into(),
    ];
    encode_args.extend(codec_args);
    encode_args.push(performance_path.display().to_string());
    run_ffmpeg(&encode_args)?;

    // Build + validate the timeline through the CLI
    // crate's own video implementation.
    let duration = frames as f64 * STEP_SECONDS;
    let clip_list = json!({
        "clips": [{
            "id": "performance",
            "in": 0.0,
            "out": duration,
            "source": PERFORMANCE_CLIP,
        }],
    });
    let timeline = torogao_cli::video::build_timeline(&clip_list);
    let base_dir = options.out_dir.display().to_string();
    let findings = torogao_cli::video::validate_timeline(&timeline, &base_dir);
    let errors: Vec<String> = findings
        .iter()
        .filter(|finding| finding.severity == "error")
        .map(|finding| format!("{}: {}", finding.code, finding.message))
        .collect();
    if !errors.is_empty() {
        return Err(AppError::Format(format!(
            "the built timeline does not validate: {}",
            errors.join("; ")
        )));
    }
    let timeline_bytes = format!("{}\n", to_d_json(&timeline));
    let timeline_path = options.out_dir.join(TIMELINE_FILE);
    std::fs::write(&timeline_path, &timeline_bytes).map_err(|err| AppError::Io {
        context: "write timeline",
        message: err.to_string(),
    })?;

    // Export artifacts, byte-shaped like the CLI's own
    // video-export writes.
    let concat = torogao_cli::video::concat_list(&timeline);
    std::fs::write(options.out_dir.join("concat.txt"), concat).map_err(|err| AppError::Io {
        context: "write concat list",
        message: err.to_string(),
    })?;
    let edl = json!({"format": "torogao-edl/1", "timeline": timeline});
    std::fs::write(
        options.out_dir.join("edl.json"),
        format!("{}\n", to_d_json(&edl)),
    )
    .map_err(|err| AppError::Io {
        context: "write edl",
        message: err.to_string(),
    })?;
    let chapters = torogao_cli::video::chapters_file(&timeline);
    std::fs::write(options.out_dir.join("chapters.txt"), chapters).map_err(|err| AppError::Io {
        context: "write chapters",
        message: err.to_string(),
    })?;

    // The finished clip: the concat demuxer over the
    // export's own concat list, stream copy.
    let final_path = options.out_dir.join(FINAL_CLIP);
    run_ffmpeg(&[
        "-y".into(),
        "-f".into(),
        "concat".into(),
        "-safe".into(),
        "0".into(),
        "-i".into(),
        options.out_dir.join("concat.txt").display().to_string(),
        "-c".into(),
        "copy".into(),
        final_path.display().to_string(),
    ])?;

    Ok(BridgeReport {
        frames,
        fps,
        duration_seconds: torogao_cli::video::timeline_duration(&timeline),
        authored_timestep,
        timeline_sha256: sha256_hex(timeline_bytes.as_bytes()),
        codec: codec.to_string(),
        files: vec![
            PERFORMANCE_CLIP.to_string(),
            TIMELINE_FILE.to_string(),
            "concat.txt".to_string(),
            "edl.json".to_string(),
            "chapters.txt".to_string(),
            FINAL_CLIP.to_string(),
        ],
    })
}

/// The bridge's headless entry: prints the report as
/// `key: value` lines (the gate reads them) and returns
/// the process exit code.
pub fn run_play_export(options: &BridgeOptions) -> AppResult<i32> {
    let report = run_bridge(options)?;
    println!("bridge-model: {}", options.model.display());
    println!("bridge-animation: {}", options.animation);
    println!("bridge-frames: {}", report.frames);
    println!("bridge-fps: {}", report.fps);
    println!("bridge-duration-seconds: {}", report.duration_seconds);
    println!("bridge-authored-timestep: {}", report.authored_timestep);
    println!("bridge-codec: {}", report.codec);
    println!("bridge-timeline-sha256: {}", report.timeline_sha256);
    for file in &report.files {
        println!("bridge-file: {file}");
    }
    println!("bridge-out: {}", options.out_dir.display());
    Ok(0)
}
