//! Hard bounds for torogao-gen runs. The Name Rule: every
//! bound lives here, named once, and is checked before any
//! allocation or GPU work happens.

/// Maximum image width/height in pixels (atelier's ceiling).
pub const AXIS_MAX_PX: u32 = 2048;
/// Maximum total pixels per image (atelier's ceiling).
pub const TOTAL_MAX_PX: u32 = 2_097_152;
/// Maximum sampler steps (atelier's ceiling).
pub const STEPS_MAX: usize = 60;
/// Minimum sampler steps.
pub const STEPS_MIN: usize = 1;
/// CFG scale bounds.
pub const CFG_MIN: f32 = 0.0;
/// CFG scale bounds.
pub const CFG_MAX: f32 = 30.0;
/// CLIP sequence length for SDXL.
pub const TOKEN_LEN: usize = 77;
/// The SDXL discrete noise schedule length.
pub const SCHEDULE_LEN: usize = 1000;
