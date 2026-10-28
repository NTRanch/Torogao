//! The `torogao-gear/1` document model (AMENDMENT-T4 §2.2):
//! the typed form a gear document takes after extraction, its
//! canonical RON emission, and the device waveform contract
//! (§2.5) with its deterministic sampler.
//!
//! Field vocabulary is fixed and complete: the validator's
//! unknown-field census (§2.7 step 2) knows every field name
//! used here, at every nesting level, and refuses any other.

use torogao_author::ron::Ron;

use crate::refusal::{GearRefusal, TG_GEAR_CLASS_INCOMPLETE, TG_GEAR_SCHEMA};

/// The one supported gear format token.
pub const GEAR_FORMAT: &str = "torogao-gear/1";

/// The five gear classes (§2.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GearClass {
    /// Worn clothing: a rigged part-set.
    Apparel,
    /// Attached, non-vibratory devices (clamps, clips, plugs).
    AttachedDevice,
    /// State-driven effects and emitters.
    Effect,
    /// Restraints and fixtures (the Addition C family).
    RestraintFixture,
    /// Vibratory devices (wand, wearable, plug).
    VibratoryDevice,
}

impl GearClass {
    /// Parses the RON ident naming a class.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Apparel" => Some(Self::Apparel),
            "AttachedDevice" => Some(Self::AttachedDevice),
            "Effect" => Some(Self::Effect),
            "RestraintFixture" => Some(Self::RestraintFixture),
            "VibratoryDevice" => Some(Self::VibratoryDevice),
            _ => None,
        }
    }

    /// The RON ident naming this class.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Apparel => "Apparel",
            Self::AttachedDevice => "AttachedDevice",
            Self::Effect => "Effect",
            Self::RestraintFixture => "RestraintFixture",
            Self::VibratoryDevice => "VibratoryDevice",
        }
    }

    /// The state variants this class must declare (§2.3 read
    /// with §2.6); completeness is validation step 3.
    pub fn required_variants(&self) -> &'static [StateVariant] {
        match self {
            Self::Apparel => &[
                StateVariant::Worn,
                StateVariant::Displaced,
                StateVariant::Removed,
            ],
            Self::AttachedDevice | Self::RestraintFixture => {
                &[StateVariant::Attached, StateVariant::Removed]
            }
            Self::Effect => &[StateVariant::Active, StateVariant::Removed],
            Self::VibratoryDevice => &[
                StateVariant::Attached,
                StateVariant::Displaced,
                StateVariant::Removed,
            ],
        }
    }
}

/// A state variant (§2.6): worn/attached, displaced, removed
/// are distinct data states, never synonyms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateVariant {
    /// An effect emitter running.
    Active,
    /// Attached to the body.
    Attached,
    /// Still attached, shifted; contact is recomputed.
    Displaced,
    /// Not on the body; no physical input.
    Removed,
    /// Worn (apparel vocabulary for the attached state).
    Worn,
}

impl StateVariant {
    /// Parses the RON ident naming a variant.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Active" => Some(Self::Active),
            "Attached" => Some(Self::Attached),
            "Displaced" => Some(Self::Displaced),
            "Removed" => Some(Self::Removed),
            "Worn" => Some(Self::Worn),
            _ => None,
        }
    }

    /// The RON ident naming this variant.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Active => "Active",
            Self::Attached => "Attached",
            Self::Displaced => "Displaced",
            Self::Removed => "Removed",
            Self::Worn => "Worn",
        }
    }
}

/// Which side of the body/part an attachment occupies (§2.4),
/// so draw order and occlusion are computable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Facing {
    /// Behind the body part.
    Back,
    /// In front of the body part.
    Front,
    /// The body's left side.
    Left,
    /// The body's right side.
    Right,
}

impl Facing {
    /// Parses the RON ident naming a facing.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Back" => Some(Self::Back),
            "Front" => Some(Self::Front),
            "Left" => Some(Self::Left),
            "Right" => Some(Self::Right),
            _ => None,
        }
    }

    /// The RON ident naming this facing.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Back => "Back",
            Self::Front => "Front",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

/// One attachment declaration (§2.4): an anchor, a local
/// offset, and the contact/spring parameters of the solve.
#[derive(Clone, Debug, PartialEq)]
pub struct Attachment {
    /// Canonical anchor name (§2.4) or `fixture.<id>`.
    pub anchor: String,
    /// Local offset x in rig pixels (1024 convention).
    pub offset_x_px: f64,
    /// Local offset y in rig pixels.
    pub offset_y_px: f64,
    /// Contact radius of this attachment in pixels.
    pub contact_radius_px: i64,
    /// Spring stiffness, permille; maps monotonically onto the
    /// solver's spring constant (§4.2).
    pub stiffness_permille: i64,
    /// Class-bounded maximum contact pressure, permille.
    pub max_pressure_permille: i64,
    /// Which side of the part the item occupies.
    pub facing: Facing,
}

/// A part/art payload reference with its content hash (§2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartRef {
    /// Payload name inside the pack.
    pub name: String,
    /// Lowercase hex sha256 of the payload bytes.
    pub hash: String,
    /// Texture dimensions (width, height) when the payload is
    /// a texture; each axis is bounded at 8192 (§2.7).
    pub texture: Option<(i64, i64)>,
}

/// Whether a draw layer sits above or below its target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawRelation {
    /// Above the named layer.
    Above,
    /// Below the named layer.
    Below,
}

impl DrawRelation {
    /// Parses the RON ident naming a relation.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Above" => Some(Self::Above),
            "Below" => Some(Self::Below),
            _ => None,
        }
    }

    /// The RON ident naming this relation.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Above => "Above",
            Self::Below => "Below",
        }
    }
}

/// One explicit draw-order relation (§2.2): no implicit
/// ordering exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrawOrderEntry {
    /// The layer this entry places.
    pub layer: String,
    /// Above or below the target.
    pub relation: DrawRelation,
    /// The named body/clothing layer it relates to.
    pub target: String,
}

/// The hook kinds the §2.8 whitelist is expressed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookKind {
    /// Arousal-engine inputs through the sensation profile.
    ArousalInput,
    /// The item's own attachment solve.
    AttachmentSolve,
    /// Named body-parameter channels the rig template
    /// publishes.
    BodyParam,
    /// The item's own device cycle.
    DeviceCycle,
    /// Whitelisted effect parameters (§6).
    EffectParam,
    /// Named governed expression names (Addition G).
    Expression,
    /// Named fluid emitters (§6.4).
    Fluid,
}

impl HookKind {
    /// Parses the RON ident naming a hook kind.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "ArousalInput" => Some(Self::ArousalInput),
            "AttachmentSolve" => Some(Self::AttachmentSolve),
            "BodyParam" => Some(Self::BodyParam),
            "DeviceCycle" => Some(Self::DeviceCycle),
            "EffectParam" => Some(Self::EffectParam),
            "Expression" => Some(Self::Expression),
            "Fluid" => Some(Self::Fluid),
            _ => None,
        }
    }

    /// The RON ident naming this hook kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::ArousalInput => "ArousalInput",
            Self::AttachmentSolve => "AttachmentSolve",
            Self::BodyParam => "BodyParam",
            Self::DeviceCycle => "DeviceCycle",
            Self::EffectParam => "EffectParam",
            Self::Expression => "Expression",
            Self::Fluid => "Fluid",
        }
    }
}

/// One declared hook (§2.8). Whether the (kind, name) pair is
/// legal is the validator's step 7, not this type's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hook {
    /// The hook kind.
    pub kind: HookKind,
    /// The governed name / channel / emitter hooked.
    pub name: String,
}

/// Attachment/displacement parameters (§2.2 `physics`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Physics {
    /// Pre-projection spring error (px) past which the item
    /// may displace (§4.5).
    pub displacement_threshold_px: i64,
    /// Whether this item can remain on the body in a shifted
    /// position at all.
    pub displaceable: bool,
}

/// How contact/pressure feeds the arousal engine (§2.2
/// `sensation`). Site *weights* are character-profile data
/// (§5.3) and are deliberately not declarable here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sensation {
    /// The contact site name (a profile-seam key).
    pub site: String,
    /// Static contact pressure, permille (AttachedDevice
    /// requires it).
    pub pressure_permille: Option<i64>,
    /// Persistent arousal floor while attached, permille,
    /// bounded at 600 (§5.4).
    pub arousal_floor_permille: Option<i64>,
}

/// Waveform shapes (§2.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveKind {
    /// Constant level.
    Constant,
    /// Steps through a bounded pattern.
    Pattern,
    /// On/off at a duty cycle.
    Pulse,
    /// Rises to amplitude over one period, then holds.
    Ramp,
    /// Triangle wave.
    Wave,
}

impl WaveKind {
    /// Parses the RON ident naming a wave kind.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Constant" => Some(Self::Constant),
            "Pattern" => Some(Self::Pattern),
            "Pulse" => Some(Self::Pulse),
            "Ramp" => Some(Self::Ramp),
            "Wave" => Some(Self::Wave),
            _ => None,
        }
    }

    /// The RON ident naming this wave kind.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Constant => "Constant",
            Self::Pattern => "Pattern",
            Self::Pulse => "Pulse",
            Self::Ramp => "Ramp",
            Self::Wave => "Wave",
        }
    }
}

/// The device waveform contract (§2.5): output is a
/// deterministic function of tick index and device state. No
/// wall clock and no system RNG enter the sample.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaveSpec {
    /// Waveform shape.
    pub kind: WaveKind,
    /// Peak amplitude, permille 0..=1000.
    pub amplitude_permille: i64,
    /// Cycle frequency, millihertz 0..=20000.
    pub frequency_millihz: i64,
    /// Duty cycle, permille; Pulse/Pattern only.
    pub duty_permille: Option<i64>,
    /// Pattern steps, permille, at most 64; Pattern only.
    pub pattern: Option<Vec<i64>>,
    /// Ticks per pattern step, 1..=3600; Pattern only.
    pub pattern_step_ticks: Option<i64>,
    /// Initial phase in ticks; runtime phase is state.
    pub phase_ticks: i64,
}

impl WaveSpec {
    /// Samples the drive at a tick, in permille. Pure and
    /// deterministic; a zero amplitude is a zero drive *level*
    /// (§2.5), never a stop.
    pub fn sample(&self, tick: u64) -> u32 {
        let amplitude = self.amplitude_permille.clamp(0, 1000) as u32;
        if amplitude == 0 {
            return 0;
        }
        let phase = (tick as i64).saturating_add(self.phase_ticks).max(0) as u64;
        match self.kind {
            WaveKind::Constant => amplitude,
            WaveKind::Pattern => {
                let (Some(pattern), Some(step_ticks)) = (&self.pattern, self.pattern_step_ticks)
                else {
                    return 0;
                };
                if pattern.is_empty() || step_ticks <= 0 {
                    return 0;
                }
                let index = ((phase / step_ticks as u64) % pattern.len() as u64) as usize;
                let step = pattern[index].clamp(0, 1000) as u32;
                amplitude * step / 1000
            }
            WaveKind::Pulse => {
                let Some(period) = self.period_ticks() else {
                    return amplitude;
                };
                let duty = self.duty_permille.unwrap_or(500).clamp(0, 1000) as u64;
                let at = phase % period;
                if at * 1000 < period * duty {
                    amplitude
                } else {
                    0
                }
            }
            WaveKind::Ramp => {
                let Some(period) = self.period_ticks() else {
                    return amplitude;
                };
                let at = phase.min(period);
                (amplitude as u64 * at / period) as u32
            }
            WaveKind::Wave => {
                let Some(period) = self.period_ticks() else {
                    return amplitude;
                };
                let at = phase % period;
                let half = period / 2;
                let tri = if half == 0 {
                    1000
                } else if at <= half {
                    at * 1000 / half
                } else {
                    (period - at) * 1000 / half.max(1)
                };
                amplitude * (tri as u32) / 1000
            }
        }
    }

    /// The cycle period in 60 Hz ticks, or `None` when the
    /// frequency is zero (no cycling).
    pub fn period_ticks(&self) -> Option<u64> {
        if self.frequency_millihz <= 0 {
            return None;
        }
        Some((60_000 / self.frequency_millihz as u64).max(1))
    }
}

/// The device block (VibratoryDevice only, §2.3 C).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// The waveform contract.
    pub waveform: WaveSpec,
    /// Capability declaration only (§2.1): the item *can* be
    /// mirrored. Enablement is never file data.
    pub mirror_capable: bool,
    /// The contact site this device stimulates.
    pub contact_site: String,
}

/// The apparel block (§2.3 A).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApparelBlock {
    /// Anchors this garment covers when worn (§4.4).
    pub covers: Vec<String>,
    /// Contact transmission through the garment, permille:
    /// 0 is opaque, 1000 is a fully authored opening.
    pub transmission_permille: i64,
}

/// One bounded pose constraint of a fixture (§2.3 D): a range
/// on a named rig pose parameter, never a per-frame write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PoseConstraint {
    /// Named rig pose parameter.
    pub param: String,
    /// Range minimum, permille of the parameter's range.
    pub min_permille: i64,
    /// Range maximum, permille of the parameter's range.
    pub max_permille: i64,
}

/// The restraint-fixture block (§2.3 D).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureBlock {
    /// Fixture anchor ids this fixture declares; other items
    /// may anchor at `fixture.<id>` only if declared here or
    /// by a pack companion (§2.4).
    pub declares_anchors: Vec<String>,
    /// Bounded pose constraints.
    pub pose_constraints: Vec<PoseConstraint>,
}

/// The effect block (§2.3 E): inputs are engine state fields
/// or contact events; outputs are whitelisted effect
/// parameters. An effect cannot write arousal state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectBlock {
    /// Named §5.2 state fields or contact events read.
    pub inputs: Vec<String>,
    /// Named §6 effect parameters written.
    pub outputs: Vec<String>,
}

/// The honesty classes of the estate record schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Honesty {
    /// Measured from a primary source.
    Measured,
    /// Authored as estate code/spec data.
    Authored,
    /// Docs-guided construction, not measured.
    Inferred,
    /// Placeholder-class art/data awaiting a keeper ruling.
    Placeholder,
}

impl Honesty {
    /// Parses the RON ident naming an honesty class.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "Authored" => Some(Self::Authored),
            "Inferred" => Some(Self::Inferred),
            "Measured" => Some(Self::Measured),
            "Placeholder" => Some(Self::Placeholder),
            _ => None,
        }
    }

    /// The RON ident naming this honesty class.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Authored => "Authored",
            Self::Inferred => "Inferred",
            Self::Measured => "Measured",
            Self::Placeholder => "Placeholder",
        }
    }
}

/// Provenance (§2.2): author, source pack, honesty class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    /// Author label (studio attribution only).
    pub author: String,
    /// The pack this document ships in.
    pub source_pack: String,
    /// Honesty class of the document's derived values.
    pub honesty: Honesty,
}

/// A `torogao-gear/1` document, fully extracted. Construction
/// from untrusted text goes through
/// [`crate::validate::validate_document`], which runs the
/// §2.7 pipeline; this struct is the validated form.
#[derive(Clone, Debug, PartialEq)]
pub struct GearDocument {
    /// Exactly [`GEAR_FORMAT`].
    pub format: String,
    /// Stable namespaced item id, unique inside its pack.
    pub id: String,
    /// Document schema/revision version.
    pub version: i64,
    /// User-visible name.
    pub display_name: String,
    /// The gear class.
    pub class: GearClass,
    /// Ordered attachment/contact points (§2.4).
    pub anchors: Vec<Attachment>,
    /// Part/art payload references with content hashes.
    pub parts: Vec<PartRef>,
    /// Material references (lumen/atelier lane names).
    pub materials: Vec<String>,
    /// Explicit draw-order relations.
    pub draw_order: Vec<DrawOrderEntry>,
    /// Declared state variants.
    pub state_variants: Vec<StateVariant>,
    /// Declared hooks (§2.8).
    pub hooks: Vec<Hook>,
    /// Attachment/displacement parameters.
    pub physics: Physics,
    /// Sensation profile, when the class carries one.
    pub sensation: Option<Sensation>,
    /// Device block (VibratoryDevice only).
    pub device: Option<Device>,
    /// Apparel block (Apparel only).
    pub apparel: Option<ApparelBlock>,
    /// Fixture block (RestraintFixture only).
    pub fixture: Option<FixtureBlock>,
    /// Effect block (Effect only).
    pub effect: Option<EffectBlock>,
    /// Pack-internal dependencies by item id (§2.7 step 10).
    pub depends_on: Vec<String>,
    /// Provenance.
    pub provenance: Provenance,
}

impl GearDocument {
    /// Whether this item is a clamp-style pair: two or more
    /// attachments that apply/remove as one user act (§2.3 B).
    pub fn is_pair(&self) -> bool {
        self.anchors.len() >= 2
    }

    /// The item's declared arousal floor, if any (§5.4).
    pub fn arousal_floor_permille(&self) -> Option<i64> {
        self.sensation
            .as_ref()
            .and_then(|s| s.arousal_floor_permille)
    }
}

/// Field lookup over a RON struct's field list.
pub(crate) fn field<'a>(fields: &'a [(String, Ron)], name: &str) -> Option<&'a Ron> {
    fields.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// A refusal for a missing required field (completeness).
pub(crate) fn missing(doc: &str, path: &str, what: &str) -> GearRefusal {
    GearRefusal::new(
        TG_GEAR_CLASS_INCOMPLETE,
        doc,
        path,
        format!("required {what} is missing"),
    )
}

/// A refusal for a value of the wrong shape (malformed).
pub(crate) fn malformed(doc: &str, path: &str, what: &str) -> GearRefusal {
    GearRefusal::new(TG_GEAR_SCHEMA, doc, path, format!("expected {what}"))
}
