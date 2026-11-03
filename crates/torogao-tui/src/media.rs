//! Media preview machinery: decoding, terminal graphics
//! protocols, and the two estate subprocess feeds (ffmpeg for
//! video frames and foreign stills, `torogao-app
//! --headless-eval` for model renders).
//!
//! Preview is a view-layer capability. It owns no project
//! semantics: it reads files the operator points it at, renders
//! the project's own puppet through the app binary that is the
//! render path of record, and never writes to a project. Every
//! failure is a named [`MediaError`]; nothing here panics on bad
//! input, and every loop and payload is bounded by the caps
//! below.
//!
//! Video preview is visual only — frame extraction passes
//! `-an` to ffmpeg and no audio sink exists anywhere in this
//! crate (the estate audio silence rule).

use image::RgbaImage;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Media files one project scan lists at most.
pub const MEDIA_ITEMS_MAX: usize = 128;
/// Directory depth a project media scan descends at most.
pub const SCAN_DEPTH_MAX: usize = 2;
/// A still larger than this on disk is refused, not decoded.
pub const STILL_BYTES_MAX: u64 = 512 * 1024 * 1024;
/// Longest edge, in pixels, a decoded still is kept at.
pub const STILL_EDGE_MAX: u32 = 2048;
/// Longest edge a video frame extraction is scaled to.
pub const FRAME_EDGE_MAX: u32 = 640;
/// Longest edge a kitty transmission is scaled to.
pub const KITTY_EDGE_MAX: u32 = 1024;
/// Longest edge a sixel transmission is scaled to.
pub const SIXEL_EDGE_MAX: u32 = 384;
/// Highest playback rate the transport advances at, however
/// fast the source's native fps runs.
pub const PLAYBACK_FPS_CAP: f64 = 30.0;
/// ffmpeg/ffprobe subprocess deadline.
pub const TOOL_TIMEOUT: Duration = Duration::from_secs(15);
/// Model render subprocess deadline.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(60);
/// One kitty payload chunk carries at most this many base64
/// characters (the protocol's own bound).
pub const KITTY_CHUNK_MAX: usize = 4096;

/// What a listed media file is, by extension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaKind {
    /// A still image (png/jpg/jpeg/webp/bmp/tif/tiff).
    Image,
    /// A Torogao puppet container (.inx/.inp).
    Model,
    /// A video or animation (mp4/webm/mov/mkv/gif).
    Video,
}

/// One listable media file.
#[derive(Clone, Debug)]
pub struct MediaItem {
    /// Path on disk.
    pub path: PathBuf,
    /// Display label (file name, or the asset id for assets).
    pub label: String,
    /// What kind of media it is.
    pub kind: MediaKind,
}

/// A named preview failure. The `code` is stable and shown
/// verbatim in the media pane; `message` is the plain-English
/// detail.
#[derive(Clone, Debug)]
pub struct MediaError {
    /// Stable machine-readable code (e.g. `TG-TUI-MEDIA-DECODE`).
    pub code: &'static str,
    /// Human detail.
    pub message: String,
}

impl MediaError {
    fn new(code: &'static str, message: impl Into<String>) -> MediaError {
        MediaError {
            code,
            message: message.into(),
        }
    }

    /// The one-line form shown in the media pane.
    pub fn line(&self) -> String {
        format!("{}: {}", self.code, self.message)
    }
}

impl std::fmt::Display for MediaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.line())
    }
}

/// The class of one path, from its extension — `None` when the
/// path is not previewable media at all.
pub fn classify(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tif" | "tiff" => Some(MediaKind::Image),
        "inx" | "inp" => Some(MediaKind::Model),
        "mp4" | "webm" | "mov" | "mkv" | "gif" => Some(MediaKind::Video),
        _ => None,
    }
}

/// Collects the previewable media of a project directory:
/// bounded scan (depth, count), sorted by path, deduplicated.
/// Project asset files are ordinary PNG files under `assets/`
/// and arrive through the same scan.
pub fn collect_media(root: &Path) -> Vec<MediaItem> {
    let mut found: Vec<PathBuf> = Vec::new();
    scan_into(root, 0, &mut found);
    found.sort();
    found.dedup();
    found.truncate(MEDIA_ITEMS_MAX);
    found
        .into_iter()
        .map(|path| MediaItem {
            label: path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            kind: classify(&path).unwrap_or(MediaKind::Image),
            path,
        })
        .collect()
}

fn scan_into(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > SCAN_DEPTH_MAX || out.len() >= MEDIA_ITEMS_MAX {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    entries.sort();
    for path in entries {
        if out.len() >= MEDIA_ITEMS_MAX {
            return;
        }
        if path.is_dir() {
            scan_into(&path, depth + 1, out);
        } else if classify(&path).is_some() {
            out.push(path);
        }
    }
}

/// The scratch directory for extracted frames and model
/// renders: `$TMPDIR/torogao-tui-preview-<pid>`, created on
/// first use. Files inside are single-frame PNGs, replaced as
/// the preview moves.
pub fn preview_tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("torogao-tui-preview-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// -- Stills -------------------------------------------------------

/// Decodes a still to RGBA, downscaled to `STILL_EDGE_MAX`.
/// PNG decodes natively (the format every project asset is);
/// anything else the native decoder refuses is bridged
/// through ffmpeg's still extraction — the same tool the app
/// uses — so a JPEG or WebP on disk still previews.
pub fn decode_still(path: &Path) -> Result<RgbaImage, MediaError> {
    let size = std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0);
    if size > STILL_BYTES_MAX {
        return Err(MediaError::new(
            "TG-TUI-MEDIA-TOO-LARGE",
            format!(
                "{} is {size} bytes; the preview cap is {STILL_BYTES_MAX}",
                path.display()
            ),
        ));
    }
    match decode_native(path) {
        Ok(image) => Ok(fit_edge(&image, STILL_EDGE_MAX)),
        Err(native_err) => match extract_frame(path, 0.0, STILL_EDGE_MAX) {
            Ok(image) => Ok(image),
            Err(ffmpeg_err) => Err(MediaError::new(
                "TG-TUI-MEDIA-DECODE",
                format!(
                    "{}: native decode refused ({native_err}); ffmpeg refused ({ffmpeg_err})",
                    path.display()
                ),
            )),
        },
    }
}

fn decode_native(path: &Path) -> Result<RgbaImage, MediaError> {
    let mut reader = image::ImageReader::open(path)
        .map_err(|err| MediaError::new("TG-TUI-MEDIA-IO", err.to_string()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(STILL_EDGE_MAX * 2);
    limits.max_image_height = Some(STILL_EDGE_MAX * 2);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    reader
        .decode()
        .map(|image| image.to_rgba8())
        .map_err(|err| MediaError::new("TG-TUI-MEDIA-DECODE", err.to_string()))
}

/// Downscales `image` so its longest edge is at most
/// `edge_max`, preserving aspect. Never upscales.
pub fn fit_edge(image: &RgbaImage, edge_max: u32) -> RgbaImage {
    let (width, height) = (image.width(), image.height());
    let longest = width.max(height);
    if longest <= edge_max || longest == 0 {
        return image.clone();
    }
    let scale = f64::from(edge_max) / f64::from(longest);
    let new_width = ((f64::from(width) * scale).round() as u32).max(1);
    let new_height = ((f64::from(height) * scale).round() as u32).max(1);
    image::imageops::resize(
        image,
        new_width,
        new_height,
        image::imageops::FilterType::Triangle,
    )
}

/// Encodes a raster as PNG bytes (kitty transmission payload).
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, MediaError> {
    let mut bytes: Vec<u8> = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut bytes);
    image::ImageEncoder::write_image(
        encoder,
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|err| MediaError::new("TG-TUI-MEDIA-ENCODE", err.to_string()))?;
    Ok(bytes)
}

// -- Half-block sampling ------------------------------------------

/// One RGB colour.
pub type Rgb = (u8, u8, u8);

/// Samples `image` into a `cols` × `rows` grid of
/// (foreground, background) colour pairs for half-block (▀)
/// rendering: each cell averages its source box for the top
/// half into the foreground and the bottom half into the
/// background, composited over black (the Addition U ground).
/// Aspect is preserved by the cell geometry (a cell covers
/// 1×2 pixels), so the caller passes the cell budget and this
/// returns the grid at the image's own aspect within it.
pub fn halfblock_cells(image: &RgbaImage, cols_max: u16, rows_max: u16) -> Vec<Vec<(Rgb, Rgb)>> {
    let (width, height) = (image.width(), image.height());
    if width == 0 || height == 0 || cols_max == 0 || rows_max == 0 {
        return Vec::new();
    }
    // Work in pixel space: the grid is cols × (2*rows) pixels.
    let mut cols = u32::from(cols_max);
    let mut rows_px = u32::from(rows_max) * 2;
    let scale = (f64::from(cols) / f64::from(width)).min(f64::from(rows_px) / f64::from(height));
    if scale < 1.0 {
        cols = ((f64::from(width) * scale).floor() as u32).max(1);
        rows_px = ((f64::from(height) * scale).floor() as u32).max(2) & !1;
    } else {
        cols = width;
        rows_px = height & !1;
        if rows_px == 0 {
            rows_px = 2.min(height);
        }
    }
    let sampled =
        image::imageops::resize(image, cols, rows_px, image::imageops::FilterType::Triangle);
    let mut grid = Vec::with_capacity((rows_px / 2) as usize);
    let mut y = 0;
    while y + 1 < rows_px || (y < rows_px && rows_px == 1) {
        let mut row = Vec::with_capacity(cols as usize);
        for x in 0..cols {
            let top = over_black(sampled.get_pixel(x, y));
            let bottom = if y + 1 < rows_px {
                over_black(sampled.get_pixel(x, y + 1))
            } else {
                (0, 0, 0)
            };
            row.push((top, bottom));
        }
        grid.push(row);
        y += 2;
    }
    grid
}

fn over_black(pixel: &image::Rgba<u8>) -> Rgb {
    let [r, g, b, a] = pixel.0;
    let a = u32::from(a);
    (
        (u32::from(r) * a / 255) as u8,
        (u32::from(g) * a / 255) as u8,
        (u32::from(b) * a / 255) as u8,
    )
}

// -- Terminal graphics --------------------------------------------

/// How the media pane puts pixels on screen.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GraphicsMode {
    /// Kitty graphics protocol (transmit PNG, place by id).
    Kitty,
    /// Sixel graphics.
    Sixel,
    /// Half-block cells inside the ratatui buffer — always
    /// available, lowest fidelity.
    HalfBlock,
}

impl GraphicsMode {
    /// The label shown in the media pane.
    pub fn label(self) -> &'static str {
        match self {
            GraphicsMode::Kitty => "kitty graphics",
            GraphicsMode::Sixel => "sixel",
            GraphicsMode::HalfBlock => "half-block",
        }
    }
}

/// Picks the graphics mode from an explicit override
/// (`TOROGAO_TUI_GRAPHICS` = kitty/sixel/halfblock) or the
/// terminal's environment. Returns the mode and the reason,
/// which the media pane shows — the degradation is stated,
/// never silent.
pub fn detect_graphics(
    override_: Option<&str>,
    term: Option<&str>,
    term_program: Option<&str>,
    kitty_window_id: Option<&str>,
) -> (GraphicsMode, String) {
    match override_
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("kitty") => {
            return (
                GraphicsMode::Kitty,
                "TOROGAO_TUI_GRAPHICS=kitty".to_string(),
            );
        }
        Some("sixel") => {
            return (
                GraphicsMode::Sixel,
                "TOROGAO_TUI_GRAPHICS=sixel".to_string(),
            );
        }
        Some("halfblock") | Some("half-block") => {
            return (
                GraphicsMode::HalfBlock,
                "TOROGAO_TUI_GRAPHICS=halfblock".to_string(),
            );
        }
        Some(other) => {
            return (
                GraphicsMode::HalfBlock,
                format!("TOROGAO_TUI_GRAPHICS={other} is not a mode; half-block fallback"),
            );
        }
        None => {}
    }
    let term = term.unwrap_or("");
    let program = term_program.unwrap_or("");
    if kitty_window_id.is_some_and(|value| !value.is_empty())
        || term.contains("kitty")
        || term.contains("ghostty")
        || matches!(program, "WezTerm" | "kitty" | "ghostty")
    {
        return (
            GraphicsMode::Kitty,
            "terminal advertises kitty graphics".to_string(),
        );
    }
    if term.contains("sixel") || program == "foot" {
        return (GraphicsMode::Sixel, "terminal advertises sixel".to_string());
    }
    (
        GraphicsMode::HalfBlock,
        "no kitty/sixel terminal detected".to_string(),
    )
}

/// Reads the process environment for [`detect_graphics`].
pub fn detect_graphics_env() -> (GraphicsMode, String) {
    let var = |name: &str| std::env::var(name).ok();
    detect_graphics(
        var("TOROGAO_TUI_GRAPHICS").as_deref(),
        var("TERM").as_deref(),
        var("TERM_PROGRAM").as_deref(),
        var("KITTY_WINDOW_ID").as_deref(),
    )
}

/// Standard base64 (RFC 4648, with padding), bounded by input.
pub fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = u32::from(*chunk.get(1).unwrap_or(&0));
        let b2 = u32::from(*chunk.get(2).unwrap_or(&0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Builds the kitty escape text that transmits `png` and
/// places it at 1-based (`col`, `row`) sized `cols` × `rows`
/// cells, under image id `id`. The caller writes it to the
/// terminal after the frame draw; the image persists until a
/// delete (see [`kitty_delete`]).
pub fn kitty_blast(png: &[u8], id: u32, col: u16, row: u16, cols: u16, rows: u16) -> String {
    let payload = base64_encode(png);
    let mut out = String::new();
    // Move to the placement origin (crossterm CUP is 1-based).
    out.push_str(&format!("\x1b[{};{}H", row.max(1), col.max(1)));
    let chunks: Vec<&str> = payload
        .as_bytes()
        .chunks(KITTY_CHUNK_MAX)
        .map(|chunk| std::str::from_utf8(chunk).unwrap_or(""))
        .collect();
    let last = chunks.len().saturating_sub(1);
    for (index, chunk) in chunks.iter().enumerate() {
        let more = u8::from(index < last);
        if index == 0 {
            out.push_str(&format!(
                "\x1b_Ga=T,f=100,t=d,i={id},c={},r={},m={more};{chunk}\x1b\\",
                cols.max(1),
                rows.max(1)
            ));
        } else {
            out.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
        }
    }
    if chunks.is_empty() {
        out.push_str(&format!(
            "\x1b_Ga=T,f=100,t=d,i={id},c={},r={},m=0;\x1b\\",
            cols.max(1),
            rows.max(1)
        ));
    }
    out
}

/// The kitty escape text deleting every placed image.
pub fn kitty_delete() -> &'static str {
    "\x1b_Ga=d\x1b\\"
}

/// Builds the sixel escape text drawing `image` at the
/// cursor's current position: the caller positions the cursor
/// at the placement origin first (this function emits the DCS
/// with raster attributes sized to the image). Colours are
/// quantized to the fixed 216-entry RGB cube — bounded,
/// deterministic, no palette search.
pub fn sixel_blast(image: &RgbaImage) -> String {
    let (width, height) = (image.width(), image.height());
    let mut out = String::new();
    out.push_str(&format!("\x1bPq\"1;1;{width};{height}"));
    // Declare the 216 cube colours once.
    for index in 0..216u32 {
        let (r, g, b) = cube_colour(index);
        out.push_str(&format!(
            "#{index};2;{};{};{}",
            r * 100 / 255,
            g * 100 / 255,
            b * 100 / 255
        ));
    }
    let bands = height.div_ceil(6);
    for band in 0..bands {
        // Group this band's pixels by colour index.
        let mut by_colour: Vec<Vec<u8>> = vec![Vec::new(); 216];
        for sub in 0..6u32 {
            let y = band * 6 + sub;
            if y >= height {
                break;
            }
            for x in 0..width {
                let pixel = over_black(image.get_pixel(x, y));
                let index = cube_index(pixel);
                let bits = &mut by_colour[index as usize];
                if bits.len() <= x as usize {
                    bits.resize(x as usize + 1, 0);
                }
                bits[x as usize] |= 1 << sub;
            }
        }
        let mut first = true;
        for (index, bits) in by_colour.iter().enumerate() {
            if bits.iter().all(|bit| *bit == 0) {
                continue;
            }
            if !first {
                out.push('$');
            }
            first = false;
            out.push_str(&format!("#{index}"));
            out.push_str(&sixel_run(bits, width));
        }
        if band + 1 < bands {
            out.push('-');
        }
    }
    out.push_str("\x1b\\");
    out
}

fn cube_index((r, g, b): Rgb) -> u32 {
    let quantize = |value: u8| u32::from(value) * 5 / 255;
    quantize(r) * 36 + quantize(g) * 6 + quantize(b)
}

fn cube_colour(index: u32) -> (u32, u32, u32) {
    let expand = |level: u32| level * 255 / 5;
    (expand(index / 36), expand(index / 6 % 6), expand(index % 6))
}

/// Run-length-compresses one colour's sixel row: bytes are
/// `bits + 63` as ASCII, runs of 3+ collapse to `!<n><char>`.
fn sixel_run(bits: &[u8], width: u32) -> String {
    let mut out = String::new();
    let mut x = 0u32;
    while x < width {
        let byte = bits.get(x as usize).copied().unwrap_or(0);
        let ch = (byte + 63) as char;
        let mut run = 1u32;
        while x + run < width && bits.get((x + run) as usize).copied().unwrap_or(0) == byte {
            run += 1;
        }
        if run >= 3 {
            out.push_str(&format!("!{run}{ch}"));
        } else {
            for _ in 0..run {
                out.push(ch);
            }
        }
        x += run;
    }
    out
}

// -- Subprocess feeds (ffmpeg / torogao-app) ----------------------

/// Runs `command` with stdout captured, killing it at
/// `deadline`. A spawn failure or a timeout is a named error,
/// never a hang.
fn run_bounded(mut command: Command, deadline: Duration, tool: &str) -> Result<String, MediaError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|err| {
        MediaError::new(
            "TG-TUI-MEDIA-NO-TOOL",
            format!("cannot spawn {tool}: {err}"),
        )
    })?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MediaError::new(
                        "TG-TUI-MEDIA-TIMEOUT",
                        format!("{tool} exceeded its {}s deadline", deadline.as_secs()),
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(err) => {
                return Err(MediaError::new(
                    "TG-TUI-MEDIA-IO",
                    format!("waiting on {tool}: {err}"),
                ));
            }
        }
    }
    let output = child
        .wait_with_output()
        .map_err(|err| MediaError::new("TG-TUI-MEDIA-IO", format!("reading {tool}: {err}")))?;
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// What ffprobe reports about one video file.
#[derive(Clone, Debug)]
pub struct VideoInfo {
    /// Duration in seconds (0 when the container hides it).
    pub duration_s: f64,
    /// Native frames per second.
    pub fps: f64,
    /// Frame width, pixels.
    pub width: u32,
    /// Frame height, pixels.
    pub height: u32,
    /// Total frames: the container's count, or fps × duration.
    pub frames: u64,
}

impl VideoInfo {
    /// Frame index → timestamp in seconds.
    pub fn frame_time(&self, frame: u64) -> f64 {
        if self.fps > 0.0 {
            frame as f64 / self.fps
        } else {
            0.0
        }
    }

    /// Timestamp → nearest frame index, clamped to the range.
    pub fn frame_at(&self, seconds: f64) -> u64 {
        if self.fps <= 0.0 || self.frames == 0 {
            return 0;
        }
        (seconds * self.fps).round() as u64 % self.frames.max(1)
    }
}

/// Probes a video with ffprobe (the estate's media tool,
/// already the app's own dependency). Read-only.
pub fn probe_video(path: &Path) -> Result<VideoInfo, MediaError> {
    let mut command = Command::new("ffprobe");
    command.args([
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=width,height,avg_frame_rate,r_frame_rate,nb_frames",
        "-show_entries",
        "format=duration",
        "-of",
        "json",
    ]);
    command.arg(path);
    let text = run_bounded(command, TOOL_TIMEOUT, "ffprobe")?;
    parse_probe(&text)
}

/// Parses ffprobe's JSON. Factored out of the subprocess so
/// the parse is unit-testable on recorded output.
pub fn parse_probe(text: &str) -> Result<VideoInfo, MediaError> {
    let doc: serde_json::Value = serde_json::from_str(text)
        .map_err(|err| MediaError::new("TG-TUI-MEDIA-PROBE", format!("ffprobe JSON: {err}")))?;
    let stream = doc["streams"]
        .as_array()
        .and_then(|streams| streams.first().cloned())
        .ok_or_else(|| MediaError::new("TG-TUI-MEDIA-PROBE", "no video stream reported"))?;
    let parse_rate = |value: &serde_json::Value| -> f64 {
        let text = value.as_str().unwrap_or("0/1");
        let (num, den) = text.split_once('/').unwrap_or((text, "1"));
        let num: f64 = num.parse().unwrap_or(0.0);
        let den: f64 = den.parse().unwrap_or(1.0);
        if den == 0.0 { 0.0 } else { num / den }
    };
    let mut fps = parse_rate(&stream["avg_frame_rate"]);
    if fps <= 0.0 {
        fps = parse_rate(&stream["r_frame_rate"]);
    }
    let duration_s = doc["format"]["duration"]
        .as_str()
        .and_then(|text| text.parse::<f64>().ok())
        .unwrap_or(0.0);
    let frames = stream["nb_frames"]
        .as_str()
        .and_then(|text| text.parse::<u64>().ok())
        .filter(|count| *count > 0)
        .unwrap_or_else(|| (duration_s * fps).round() as u64);
    Ok(VideoInfo {
        duration_s,
        fps,
        width: stream["width"].as_u64().unwrap_or(0) as u32,
        height: stream["height"].as_u64().unwrap_or(0) as u32,
        frames,
    })
}

/// Extracts the frame at `seconds` from a video as an RGBA
/// raster scaled to `edge_max`, through ffmpeg into the
/// scratch directory. `-an` is always passed: preview is
/// visual only, no audio is decoded, routed, or played.
pub fn extract_frame(path: &Path, seconds: f64, edge_max: u32) -> Result<RgbaImage, MediaError> {
    let out = preview_tmp().join("frame.png");
    let scale = format!(
        "scale='min({edge_max},iw)':'min({edge_max},ih)':force_original_aspect_ratio=decrease"
    );
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-ss"]);
    command.arg(format!("{seconds:.3}"));
    command.arg("-i").arg(path);
    command.args(["-frames:v", "1", "-an", "-vf", &scale, "-y"]);
    command.arg(&out);
    let _ = run_bounded(command, TOOL_TIMEOUT, "ffmpeg")?;
    if !out.is_file() {
        return Err(MediaError::new(
            "TG-TUI-MEDIA-PROBE",
            format!(
                "ffmpeg produced no frame at {seconds:.3}s in {}",
                path.display()
            ),
        ));
    }
    decode_native(&out)
}

// -- Model renders --------------------------------------------------

/// What one headless model render reports.
#[derive(Clone, Debug, Default)]
pub struct ModelRender {
    /// The model's display name line (nodes/parameters/textures).
    pub summary: String,
    /// The evaluated state hash line.
    pub state_hash: String,
    /// The written render PNG, when the render succeeded.
    pub png: Option<PathBuf>,
    /// Rendered dimensions + parts drawn, as reported.
    pub render_line: String,
    /// Why no render exists, when the renderer refused (e.g.
    /// no GPU adapter on this machine).
    pub unavailable: Option<String>,
}

/// Finds the `torogao-app` binary: `$TOROGAO_APP_BIN`, then
/// beside this executable, then PATH.
pub fn app_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("TOROGAO_APP_BIN") {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("torogao-app");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let output = Command::new("which").arg("torogao-app").output().ok()?;
    if output.status.success() {
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !text.is_empty() {
            return Some(PathBuf::from(text));
        }
    }
    None
}

/// Renders a puppet model through `torogao-app
/// --headless-eval <model> --render <out>` — the app's own
/// verification surface, and the render path of record. The
/// author crate's `render_still` verb is manifest-only (slice
/// A3) and `run_verb` refuses it, so no author verb produces
/// pixels today; this subprocess is the feed, bounded by
/// [`RENDER_TIMEOUT`]. A GPU-less machine yields
/// `ModelRender.unavailable`, not a failure of the preview.
pub fn render_model(path: &Path) -> Result<ModelRender, MediaError> {
    let binary = app_binary().ok_or_else(|| {
        MediaError::new(
            "TG-TUI-MEDIA-NO-RENDERER",
            "torogao-app not found ($TOROGAO_APP_BIN, executable directory, PATH)",
        )
    })?;
    let out = preview_tmp().join("model-render.png");
    let _ = std::fs::remove_file(&out);
    // The renderer's stdout is also its report channel; send
    // it to a file so a verbose render cannot fill a pipe.
    let log_path = preview_tmp().join("model-render.log");
    let log = std::fs::File::create(&log_path)
        .map_err(|err| MediaError::new("TG-TUI-MEDIA-IO", err.to_string()))?;
    let mut child = {
        let mut direct = Command::new(&binary);
        direct
            .arg("--headless-eval")
            .arg(path)
            .arg("--render")
            .arg(&out)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::from(log));
        direct.spawn().map_err(|err| {
            MediaError::new(
                "TG-TUI-MEDIA-NO-RENDERER",
                format!("cannot spawn {}: {err}", binary.display()),
            )
        })?
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if started.elapsed() > RENDER_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(MediaError::new(
                        "TG-TUI-MEDIA-TIMEOUT",
                        format!(
                            "torogao-app render exceeded its {}s deadline",
                            RENDER_TIMEOUT.as_secs()
                        ),
                    ));
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(err) => {
                return Err(MediaError::new(
                    "TG-TUI-MEDIA-IO",
                    format!("waiting on torogao-app: {err}"),
                ));
            }
        }
    }
    let text = std::fs::read_to_string(&log_path).unwrap_or_default();
    Ok(parse_render_report(&text, &out))
}

/// Parses the headless report lines into a [`ModelRender`].
/// Factored out so the parse is unit-testable on recorded
/// output from both a GPU machine and a GPU-less one.
pub fn parse_render_report(text: &str, out: &Path) -> ModelRender {
    let mut render = ModelRender::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("name: ") {
            render.summary = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("state-hash: ") {
            render.state_hash = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("render: unavailable") {
            render.unavailable = Some(
                rest.trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .to_string(),
            );
        } else if line.starts_with("render: ") {
            render.render_line = line.trim_start_matches("render: ").to_string();
        }
    }
    if render.unavailable.is_none() && out.is_file() {
        render.png = Some(out.to_path_buf());
    }
    render
}

/// Keeps the scratch directory's existence an internal
/// detail; exported for the app's exit-path cleanup.
pub fn cleanup_preview_tmp() {
    let dir = preview_tmp();
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn graphics_detection() {
        let (mode, _) = detect_graphics(Some("kitty"), Some("xterm"), None, None);
        assert_eq!(mode, GraphicsMode::Kitty);
        let (mode, _) = detect_graphics(Some("bogus"), None, None, None);
        assert_eq!(mode, GraphicsMode::HalfBlock);
        let (mode, _) = detect_graphics(None, Some("xterm-kitty"), None, None);
        assert_eq!(mode, GraphicsMode::Kitty);
        let (mode, _) = detect_graphics(None, Some("xterm-256color"), None, Some("1"));
        assert_eq!(mode, GraphicsMode::Kitty);
        let (mode, _) = detect_graphics(None, Some("xterm"), Some("foot"), None);
        assert_eq!(mode, GraphicsMode::Sixel);
        let (mode, reason) = detect_graphics(None, Some("xterm-256color"), None, None);
        assert_eq!(mode, GraphicsMode::HalfBlock);
        assert!(reason.contains("no kitty/sixel"));
    }

    #[test]
    fn classify_extensions() {
        assert_eq!(classify(Path::new("a/b.PNG")), Some(MediaKind::Image));
        assert_eq!(classify(Path::new("m.inx")), Some(MediaKind::Model));
        assert_eq!(classify(Path::new("v.mp4")), Some(MediaKind::Video));
        assert_eq!(classify(Path::new("a.gif")), Some(MediaKind::Video));
        assert_eq!(classify(Path::new("x.ron")), None);
    }

    #[test]
    fn halfblock_grid_shape_and_colour() {
        // 4x4 image: left half red-opaque, right half transparent.
        let mut image = RgbaImage::new(4, 4);
        for y in 0..4 {
            for x in 0..2 {
                image.put_pixel(x, y, image::Rgba([255, 0, 0, 255]));
            }
        }
        let grid = halfblock_cells(&image, 80, 24);
        assert_eq!(grid.len(), 2);
        assert_eq!(grid[0].len(), 4);
        assert_eq!(grid[0][0], ((255, 0, 0), (255, 0, 0)));
        assert_eq!(grid[0][3], ((0, 0, 0), (0, 0, 0)));
        // A tiny budget still yields a non-empty grid.
        let small = halfblock_cells(&image, 2, 1);
        assert!(!small.is_empty());
    }

    #[test]
    fn sixel_framing() {
        let image = RgbaImage::from_pixel(6, 6, image::Rgba([10, 200, 30, 255]));
        let text = sixel_blast(&image);
        assert!(text.starts_with("\x1bPq\"1;1;6;6"));
        assert!(text.ends_with("\x1b\\"));
        assert!(text.contains("#0;2;"));
    }

    #[test]
    fn kitty_chunking() {
        let png = vec![7u8; 9000];
        let text = kitty_blast(&png, 3, 2, 4, 10, 5);
        assert!(text.starts_with("\x1b[4;2H"));
        // 9000 bytes → 12000 base64 chars → 3 chunks (4096/4096/3808).
        assert_eq!(text.matches("\x1b_G").count(), 3);
        assert!(text.contains("i=3"));
        assert!(text.contains("m=1"));
        assert!(text.contains("m=0;"));
        assert!(text.ends_with("\x1b\\"));
    }

    #[test]
    fn probe_parse() {
        let text = r#"{"streams":[{"width":640,"height":360,"avg_frame_rate":"30000/1001","r_frame_rate":"30000/1001","nb_frames":"180"}],"format":{"duration":"6.006000"}}"#;
        let info = parse_probe(text).expect("probe parses");
        assert_eq!(info.width, 640);
        assert!((info.fps - 29.97).abs() < 0.01);
        assert_eq!(info.frames, 180);
        assert_eq!(info.frame_at(3.0), 90);
        assert!(parse_probe("{}").is_err());
        assert!(parse_probe("not json").is_err());
    }

    #[test]
    fn render_report_parse() {
        let gpu = "model: /x/Aka.inx\nname: /x/Aka.inx nodes: 100 parameters: 34 textures: 76\nstate-hash: 0f5499d3e76f4e74\nrender: /tmp/o.png 3063x5606 parts-drawn: 76 parts-skipped: 0\n";
        let render = parse_render_report(gpu, Path::new("/nonexistent-o.png"));
        // PNG path is honoured only when the file actually exists.
        assert!(render.png.is_none());
        assert!(render.summary.contains("parameters: 34"));
        assert_eq!(render.state_hash, "0f5499d3e76f4e74");
        let no_gpu =
            "state-hash: 00\nrender: unavailable (render failed: no GPU adapter available)\n";
        let render = parse_render_report(no_gpu, Path::new("/nonexistent-o.png"));
        assert!(
            render
                .unavailable
                .as_deref()
                .unwrap_or("")
                .contains("no GPU adapter")
        );
    }
}
