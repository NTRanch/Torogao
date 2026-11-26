//! Named bounds for every loop, read, and allocation the
//! checker performs (Tiger Style: bound everything, name the
//! unit). Inputs past a bound are a typed error, never a
//! truncation the caller cannot see — except findings, whose
//! cap is reported in the summary (`truncated`).

/// Maximum scene-spec size accepted, in bytes (16 MiB).
pub const SPEC_BYTES_MAX: u64 = 16 * 1024 * 1024;

/// Maximum manifest size accepted, in bytes (4 MiB).
pub const MANIFEST_BYTES_MAX: u64 = 4 * 1024 * 1024;

/// Maximum identity-lock record size accepted, in bytes.
pub const LOCK_BYTES_MAX: u64 = 4 * 1024 * 1024;

/// Maximum number of asset references checked per spec.
pub const ASSETS_MAX: usize = 4_096;

/// Maximum number of elements checked per spec.
pub const ELEMENTS_MAX: usize = 65_536;

/// Maximum number of cues checked per spec.
pub const CUES_MAX: usize = 65_536;

/// Maximum asset file size hashed, in bytes (1 GiB, streamed —
/// memory stays at one 64 KiB buffer regardless).
pub const HASH_BYTES_MAX: u64 = 1024 * 1024 * 1024;

/// Buffer size for streamed hashing, in bytes.
pub const HASH_BUFFER_BYTES: usize = 64 * 1024;

/// Maximum findings emitted per report; past the cap the
/// summary reports `truncated: true`.
pub const FINDINGS_MAX: usize = 10_000;

/// Maximum frame files considered per directory in a diff.
pub const FRAMES_MAX: usize = 100_000;

/// Maximum directory depth walked in a diff.
pub const DIFF_DEPTH_MAX: usize = 16;

/// Maximum decoded frame width or height, in pixels.
pub const FRAME_DIM_MAX: u32 = 16_384;

/// Maximum allocation for one decoded frame, in bytes.
pub const FRAME_ALLOC_MAX: u64 = 512 * 1024 * 1024;

/// A frame counts as over threshold when the fraction of
/// pixels whose largest channel delta exceeds the pixel
/// threshold is greater than this value (0.1% of pixels).
pub const FRAME_OVER_FRACTION: f64 = 0.001;

/// How many frames the `worst_frames` list carries.
pub const WORST_FRAMES_MAX: usize = 8;

/// Default per-pixel channel delta threshold for `diff`
/// (0–255). Gates should pass their measured noise floor
/// explicitly; this default only keeps the verb runnable.
pub const PIXEL_THRESHOLD_DEFAULT: u8 = 8;

/// Maximum change regions recorded per frame in a diff
/// (P1); components past the cap are counted into the frame's
/// `regions_dropped` -- counted, never silent.
pub const REGIONS_PER_FRAME_MAX: usize = 64;

/// Maximum accepted value of the diff verb's explicit
/// `region_gap` parameter, in pixels. The gap is a declared,
/// recorded input -- never auto-derived from the image.
pub const REGION_GAP_MAX: u32 = 16;
