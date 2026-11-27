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

/// User-adjustable recipe settings, bounded by the recipe of
/// record above. The defaults ARE the recipe of record; the
/// bounds are the range a user may tune in the app's settings.
/// The facade re-checks the same bounds seat-side, so a client
/// that lies about them is refused at the seat.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecipeSettings {
    /// Sampler steps.
    pub steps: u32,
    /// Classifier-free guidance scale.
    pub cfg: f32,
    /// Output width in pixels.
    pub width: u32,
    /// Output height in pixels.
    pub height: u32,
}

/// Lowest sampler steps the settings may choose.
pub const STEPS_MIN: u32 = 10;
/// Highest sampler steps the settings may choose.
pub const STEPS_MAX: u32 = 60;
/// Lowest CFG the settings may choose.
pub const CFG_MIN: f32 = 1.0;
/// Highest CFG the settings may choose.
pub const CFG_MAX: f32 = 12.0;
/// Smallest axis (either direction) the settings may choose.
pub const AXIS_MIN: u32 = 512;
/// Largest width the settings may choose.
pub const WIDTH_MAX: u32 = 1024;
/// Largest height the settings may choose.
pub const HEIGHT_MAX: u32 = 1536;

impl RecipeSettings {
    /// The recipe of record, as constants above.
    pub fn of_record() -> Self {
        Self {
            steps: STEPS,
            cfg: CFG,
            width: WIDTH,
            height: HEIGHT,
        }
    }

    /// Check the settings against the recipe bounds. Returns a
    /// presentable reason for the first bound violated.
    pub fn validate(&self) -> Result<(), String> {
        if !(STEPS_MIN..=STEPS_MAX).contains(&self.steps) {
            return Err(format!(
                "steps {} is outside {STEPS_MIN}..={STEPS_MAX}",
                self.steps
            ));
        }
        if !self.cfg.is_finite() || !(CFG_MIN..=CFG_MAX).contains(&self.cfg) {
            return Err(format!("cfg {} is outside {CFG_MIN}..={CFG_MAX}", self.cfg));
        }
        if self.width < AXIS_MIN || self.width > WIDTH_MAX || self.width % 8 != 0 {
            return Err(format!(
                "width {} is outside {AXIS_MIN}..={WIDTH_MAX} in steps of 8",
                self.width
            ));
        }
        if self.height < AXIS_MIN || self.height > HEIGHT_MAX || self.height % 8 != 0 {
            return Err(format!(
                "height {} is outside {AXIS_MIN}..={HEIGHT_MAX} in steps of 8",
                self.height
            ));
        }
        Ok(())
    }
}
