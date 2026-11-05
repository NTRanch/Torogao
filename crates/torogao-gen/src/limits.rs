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
/// CLIP sequence length for SDXL (one encoder block).
pub const TOKEN_LEN: usize = 77;
/// Content tokens per encoder block: `TOKEN_LEN` minus
/// the BOS/EOS frame (the oracle's batching rule, D-980).
pub const TEXT_BLOCK_CONTENT: usize = TOKEN_LEN - 2;
/// Maximum encoder blocks per prompt (D-980): a prompt
/// whose content exceeds `TEXT_CONTENT_MAX` tokens is
/// refused with a typed error, never truncated.
pub const TEXT_BLOCKS_MAX: usize = 16;
/// Maximum content tokens per prompt (D-980).
pub const TEXT_CONTENT_MAX: usize = TEXT_BLOCKS_MAX * TEXT_BLOCK_CONTENT;
/// Maximum aligned CFG context length in blocks (D-980):
/// the bound on the oracle's LCM repeat-alignment of the
/// conditional and unconditional contexts.
pub const CONTEXT_BLOCKS_MAX: usize = 32;
/// The SDXL discrete noise schedule length.
pub const SCHEDULE_LEN: usize = 1000;
