//! Stage 2 conditioning inputs and their provenance
//! (D-936): ControlNet pose conditioning and IP-Adapter
//! reference-image conditioning.
//!
//! Every conditioned run records its conditioning inputs
//! and their SHA-256 hashes in the run record — a Stage 2
//! output without provenance is not a proof artifact.
//! Conditioning inputs are validated before any GPU work
//! and refused with the typed `gen_conditioning` error:
//! a uniform/blank pose map (the gencond precedent: a
//! conditioning map with no signal is an upstream failure,
//! never a silent no-op), a pose map whose size differs
//! from the generation size, and any missing input file
//! (including a missing reference embedding source).

use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::Digest;

use crate::error::{GenError, Result};

/// A pose map whose per-channel standard deviation (0..=1
/// pixel scale) falls below this on every channel carries
/// no pose signal and is refused. A real pose map — rig
/// render or estimator output — draws saturated-colour
/// strokes on a black field, so its deviation is orders of
/// magnitude above this floor; a blank map sits at exactly
/// zero. The floor exists to also refuse near-uniform
/// maps (a failed render at 1-2 LSB of noise).
pub const POSE_MIN_CHANNEL_STD: f64 = 0.004;

/// ControlNet union type, by task-embedding row.
///
/// The row order is the xinsir ControlNet-Union type order
/// as exposed by the oracle's own type selector (the
/// SetUnionControlNetType option list, minus its leading
/// "auto" entry, which selects no embedding row): openpose,
/// depth, hed/pidi/scribble/ted, canny/lineart/mlsd,
/// normal, segment, tile, repaint. This is a documented
/// derivation from measured evidence, and the parity gate
/// is its judge: a wrong row would show as a pose map
/// interpreted as another modality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CnType {
    /// OpenPose body pose (the Stage 2 scope).
    OpenPose,
    /// Depth map.
    Depth,
    /// HED / PiDi / scribble / TEED edge maps.
    Hed,
    /// Canny / lineart / MLSD edge maps.
    Canny,
    /// Normal map.
    Normal,
    /// Segmentation map.
    Segment,
    /// Tile (detail enhancement; not Stage 2 scope).
    Tile,
    /// Repaint / inpaint mask.
    Repaint,
}

impl CnType {
    /// The task-embedding row for this type.
    pub fn index(self) -> usize {
        match self {
            Self::OpenPose => 0,
            Self::Depth => 1,
            Self::Hed => 2,
            Self::Canny => 3,
            Self::Normal => 4,
            Self::Segment => 5,
            Self::Tile => 6,
            Self::Repaint => 7,
        }
    }

    /// The stable name used in run records and CLI flags.
    pub fn name(self) -> &'static str {
        match self {
            Self::OpenPose => "openpose",
            Self::Depth => "depth",
            Self::Hed => "hed",
            Self::Canny => "canny",
            Self::Normal => "normal",
            Self::Segment => "segment",
            Self::Tile => "tile",
            Self::Repaint => "repaint",
        }
    }

    /// Parse a type name (the CLI/record spelling).
    ///
    /// # Errors
    /// Returns `gen_conditioning` on an unknown name.
    pub fn parse(name: &str) -> Result<Self> {
        match name {
            "openpose" => Ok(Self::OpenPose),
            "depth" => Ok(Self::Depth),
            "hed" => Ok(Self::Hed),
            "canny" => Ok(Self::Canny),
            "normal" => Ok(Self::Normal),
            "segment" => Ok(Self::Segment),
            "tile" => Ok(Self::Tile),
            "repaint" => Ok(Self::Repaint),
            other => Err(GenError::Conditioning {
                detail: format!("unknown controlnet type {other:?}"),
            }),
        }
    }
}

/// ControlNet conditioning request.
#[derive(Debug, Clone)]
pub struct ControlSpec {
    /// ControlNet weights file (xinsir union, native format).
    pub controlnet_path: PathBuf,
    /// Pose/conditioning map (PNG, generation size).
    pub pose_path: PathBuf,
    /// Residual scale applied to the ControlNet outputs.
    pub strength: f64,
    /// Union type (task-embedding row).
    pub cn_type: CnType,
    /// First sampler-step fraction the ControlNet applies at.
    pub start: f64,
    /// Step fraction at which the ControlNet stops applying.
    pub end: f64,
}

/// IP-Adapter conditioning request.
#[derive(Debug, Clone)]
pub struct IpSpec {
    /// IP-Adapter weights file (plus / ViT-H layout).
    pub ipadapter_path: PathBuf,
    /// CLIP ViT-H vision weights file.
    pub clip_vision_path: PathBuf,
    /// Reference image (any size; resized to the vision
    /// encoder's 224x224 input).
    pub reference_path: PathBuf,
    /// Scale of the decoupled image branch.
    pub scale: f64,
}

/// A full Stage 2 conditioning request; either half may be
/// absent, and both absent is the Stage 1 path.
#[derive(Debug, Clone, Default)]
pub struct ConditioningSpec {
    /// ControlNet half.
    pub control: Option<ControlSpec>,
    /// IP-Adapter half.
    pub ip: Option<IpSpec>,
}

/// Per-channel statistics of a pose map (0..=1 scale).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ChannelStats {
    /// Mean per channel (R, G, B).
    pub mean: [f64; 3],
    /// Standard deviation per channel (R, G, B).
    pub std: [f64; 3],
}

/// ControlNet half of a run's conditioning provenance.
#[derive(Debug, Clone, Serialize)]
pub struct ControlRecord {
    /// Pose map path as given.
    pub pose_path: String,
    /// SHA-256 of the pose map file bytes.
    pub pose_sha256: String,
    /// SHA-256 of the ControlNet weights file bytes.
    pub controlnet_sha256: String,
    /// Residual strength applied.
    pub strength: f64,
    /// Union type name.
    pub cn_type: String,
    /// Start step fraction.
    pub start: f64,
    /// End step fraction.
    pub end: f64,
    /// Pose map channel statistics.
    pub pose_stats: ChannelStats,
}

/// IP-Adapter half of a run's conditioning provenance.
#[derive(Debug, Clone, Serialize)]
pub struct IpRecord {
    /// Reference image path as given.
    pub reference_path: String,
    /// SHA-256 of the reference image file bytes.
    pub reference_sha256: String,
    /// SHA-256 of the IP-Adapter weights file bytes.
    pub ipadapter_sha256: String,
    /// SHA-256 of the CLIP vision weights file bytes.
    pub clip_vision_sha256: String,
    /// Decoupled-branch scale applied.
    pub scale: f64,
    /// RMS of the raw reference-image tokens (pre-normalisation).
    pub embed_rms_raw: f64,
    /// Reference token count (the penalty's basis:
    /// factor = 1 / sqrt(token_count)).
    pub token_count: u32,
    /// Factor the reference tokens were rescaled by.
    pub embed_norm_factor: f64,
    /// RMS of the reference tokens after normalisation.
    pub embed_rms_after: f64,
}

/// A run's full conditioning provenance; `None` halves are
/// absent from the record exactly as they were from the run.
#[derive(Debug, Clone, Serialize)]
pub struct ConditioningRecord {
    /// ControlNet provenance, when the run used it.
    pub control: Option<ControlRecord>,
    /// IP-Adapter provenance, when the run used it.
    pub ip: Option<IpRecord>,
}

/// SHA-256 of a file's bytes, read in bounded chunks.
///
/// # Errors
/// Returns `gen_conditioning` when the file cannot be
/// opened or read: a conditioning input that cannot be
/// hashed cannot enter a provenance record, so the run
/// refuses here rather than downstream.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path).map_err(|e| GenError::Conditioning {
        detail: format!("cannot open {}: {e}", path.display()),
    })?;
    let mut hasher = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| GenError::Conditioning {
            detail: format!("cannot read {}: {e}", path.display()),
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Per-channel mean/std of packed RGB bytes (0..=1 scale).
pub fn channel_stats(rgb: &[u8]) -> ChannelStats {
    assert!(rgb.len() % 3 == 0, "rgb buffer must be whole pixels");
    let n = (rgb.len() / 3) as f64;
    if n == 0.0 {
        return ChannelStats {
            mean: [0.0; 3],
            std: [0.0; 3],
        };
    }
    let mut mean = [0.0f64; 3];
    for px in rgb.chunks_exact(3) {
        for (c, v) in px.iter().enumerate() {
            mean[c] += f64::from(*v) / 255.0;
        }
    }
    for m in &mut mean {
        *m /= n;
    }
    let mut var = [0.0f64; 3];
    for px in rgb.chunks_exact(3) {
        for (c, v) in px.iter().enumerate() {
            let d = f64::from(*v) / 255.0 - mean[c];
            var[c] += d * d;
        }
    }
    let mut std = [0.0f64; 3];
    for c in 0..3 {
        std[c] = (var[c] / n).sqrt();
    }
    ChannelStats { mean, std }
}

/// Refuse a pose map that carries no signal.
///
/// # Errors
/// Returns `gen_conditioning` when every channel's
/// standard deviation is below `POSE_MIN_CHANNEL_STD`.
pub fn check_pose_signal(stats: &ChannelStats, path: &Path) -> Result<()> {
    if stats.std.iter().all(|s| *s < POSE_MIN_CHANNEL_STD) {
        return Err(GenError::Conditioning {
            detail: format!(
                "pose map {} is uniform/blank (per-channel std {:?}); refusing to condition on a map with no pose signal",
                path.display(),
                stats.std
            ),
        });
    }
    Ok(())
}

/// A pose map decoded and validated for one generation:
/// RGB pixels at exactly the generation size, with the
/// statistics the provenance record carries.
pub struct LoadedPose {
    /// Packed RGB bytes, row-major.
    pub rgb: Vec<u8>,
    /// Channel statistics of the map.
    pub stats: ChannelStats,
}

/// Decode a pose map and validate it against the
/// generation size and the signal floor.
///
/// # Errors
/// Returns `gen_conditioning` when the file is missing or
/// undecodable, its size differs from the generation size,
/// or it is uniform/blank.
pub fn load_pose(path: &Path, width: u32, height: u32) -> Result<LoadedPose> {
    if !path.exists() {
        return Err(GenError::Conditioning {
            detail: format!("pose map {} does not exist", path.display()),
        });
    }
    let img = image::open(path).map_err(|e| GenError::Conditioning {
        detail: format!("cannot decode pose map {}: {e}", path.display()),
    })?;
    let (w, h) = (img.width(), img.height());
    if w != width || h != height {
        return Err(GenError::Conditioning {
            detail: format!(
                "pose map {} is {w}x{h}, generation is {width}x{height}; sizes must match",
                path.display()
            ),
        });
    }
    let rgb = img.to_rgb8().into_raw();
    let stats = channel_stats(&rgb);
    check_pose_signal(&stats, path)?;
    Ok(LoadedPose { rgb, stats })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cn_type_rows_match_the_union_selector_order() {
        // Validation half: the adopted row derivation.
        assert_eq!(CnType::OpenPose.index(), 0);
        assert_eq!(CnType::Depth.index(), 1);
        assert_eq!(CnType::Repaint.index(), 7);
        assert_eq!(CnType::parse("openpose").ok(), Some(CnType::OpenPose));
        assert_eq!(CnType::parse("canny").ok(), Some(CnType::Canny));
    }

    #[test]
    fn cn_type_unknown_name_is_refused() {
        // Adversarial half: a typo'd type must fail closed.
        let err = CnType::parse("openpos").err();
        assert!(matches!(err, Some(GenError::Conditioning { .. })));
    }

    #[test]
    fn uniform_map_is_refused() {
        // Adversarial: the blank-map refusal (gencond precedent).
        let stats = channel_stats(&[0u8; 3 * 64]);
        assert!(matches!(
            check_pose_signal(&stats, Path::new("blank.png")),
            Err(GenError::Conditioning { .. })
        ));
        // A flat mid-grey map is equally signal-free.
        let stats = channel_stats(&[128u8; 3 * 64]);
        assert!(check_pose_signal(&stats, Path::new("grey.png")).is_err());
    }

    #[test]
    fn pose_shaped_map_passes_the_signal_floor() {
        // Validation: saturated strokes on black pass.
        let mut rgb = vec![0u8; 3 * 64];
        rgb[0] = 255;
        rgb[4] = 255;
        rgb[8] = 255;
        let stats = channel_stats(&rgb);
        assert!(check_pose_signal(&stats, Path::new("pose.png")).is_ok());
        assert!(stats.std[0] > 0.1);
    }

    #[test]
    fn channel_stats_of_a_known_buffer() {
        // Validation: two pixels, one black one white.
        let stats = channel_stats(&[0, 0, 0, 255, 255, 255]);
        for c in 0..3 {
            assert!((stats.mean[c] - 0.5).abs() < 1e-9);
            assert!((stats.std[c] - 0.5).abs() < 1e-9);
        }
    }

    #[test]
    fn sha256_of_missing_file_is_a_conditioning_refusal() {
        // Adversarial: missing inputs refuse with the typed code.
        let err = sha256_file(Path::new("/nonexistent/definitely/missing.png")).err();
        match err {
            Some(e @ GenError::Conditioning { .. }) => {
                assert_eq!(e.code(), "gen_conditioning");
            }
            other => panic!("expected gen_conditioning, got {other:?}"),
        }
    }
}
