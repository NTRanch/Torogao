//! The canonical style record: the wai checkpoint and the estate's
//! proven stills recipe, shared by every shell and every cast member.
//!
//! D-1525: wai is the single generator for SFW and NSFW image work.
//! The recipe constants are the asset-factory lane's proven wai
//! stills recipe (euler_ancestral / normal, 28 steps, CFG 6.0,
//! 832x1344 portrait, quality style prefix). Changing the house
//! style means changing this record — not a shell, not a prompt box.

/// The generator checkpoint of record (waiIllustriousSDXL v170,
/// sha256 f116b0c7… in the factory registry), resolved by the seat
/// through its strobe model tree.
pub const CHECKPOINT: &str = "waiIllustriousSDXL_v170.safetensors";

/// Quality prefix prepended to every composed positive prompt.
pub const STYLE_PREFIX: &str = "masterpiece, best quality, amazing quality, very aesthetic, ";

/// Shared negative prompt; cast records append their own guards.
pub const DEFAULT_NEGATIVE: &str = "lowres, bad anatomy, bad hands, text, error, \
    missing fingers, extra digit, fewer digits, cropped, worst quality, low quality, \
    jpeg artifacts, signature, watermark, username, blurry";

/// Sampler name in the seat's vocabulary.
pub const SAMPLER: &str = "euler_ancestral";
/// Scheduler name in the seat's vocabulary.
pub const SCHEDULER: &str = "normal";
/// Sampling steps per still.
pub const STEPS: u32 = 28;
/// Classifier-free guidance scale.
pub const CFG: f32 = 6.0;
/// Portrait width in pixels.
pub const WIDTH: u32 = 832;
/// Portrait height in pixels.
pub const HEIGHT: u32 = 1344;
/// File-name prefix the seat saves results under.
pub const FILENAME_PREFIX: &str = "torogao_mobile";
