//! The authored project model: what `project.ron` holds.
//!
//! The vocabulary follows the rig-model format
//! (`torogao-rig-model`): scalar parameters with ranges,
//! rotation/warp deformers, keyform lattices addressed by one or
//! two parameters, drawable meshes. Authoring adds the records a
//! build needs and a model file does not carry: asset provenance,
//! draw-order bands, default visibility, glue pairs, expressions,
//! the neutral pose, variant sets, and the lane/consent header.
//!
//! Every collection is kept sorted by id, so two scripts that
//! define the same rig in different orders produce the same bytes.
//! Coordinates are model units (pixels of the source art); every
//! float is finite, enforced by `decide()`.

/// The project file's format token.
pub const PROJECT_FORMAT: &str = "torogao-author-project";
/// The project file's format version.
pub const PROJECT_FORMAT_VERSION: i64 = 1;
/// The consent schema every project header carries.
pub const CONSENT_SCHEMA: &str = "torogao-consent/1";

/// A 2-D point or vector in model units.
pub type Point = [f64; 2];

/// One authored project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Project {
    /// Identity, lane stamp, and consent record.
    pub header: Header,
    /// Intaken art, sorted by id.
    pub assets: Vec<Asset>,
    /// Audio-binding rows (the rig family's audio group),
    /// sorted by id. Absent from the file when empty: a project
    /// with no audio bindings carries no section.
    pub audio_bindings: Vec<AudioBinding>,
    /// Audio-reactive binding rows (A3), sorted by (track, marker).
    pub audio_binds: Vec<AudioBind>,
    /// Named audio tracks of event markers (A3), sorted by id.
    pub audio_tracks: Vec<AudioTrack>,
    /// Deformers, sorted by id.
    pub deformers: Vec<Deformer>,
    /// Expressions, sorted by id.
    pub expressions: Vec<Expression>,
    /// Glue pairs between drawables, sorted by id.
    pub glues: Vec<Glue>,
    /// Motion clips (A3), sorted by id.
    pub motions: Vec<Motion>,
    /// Parameters, sorted by id.
    pub parameters: Vec<Parameter>,
    /// Parts (node tree; drawable when meshed), sorted by id.
    pub parts: Vec<Part>,
    /// Physics groups (A2), sorted by id.
    pub physics_groups: Vec<PhysicsGroup>,
    /// The neutral pose: parameter values that differ from the
    /// declared defaults, sorted by parameter id.
    pub pose: Vec<PoseEntry>,
    /// Named variant stacks from separated art, sorted by id.
    pub variant_sets: Vec<VariantSet>,
    /// Visual-binding rows (Q1, §c), sorted by id. Absent from
    /// the file when empty.
    pub visual_bindings: Vec<VisualBinding>,
    /// The AUTH-G game section, when the project authors a game.
    /// Absent means absent: rig-only projects carry no field and
    /// serialize exactly as before the game layer existed.
    pub game: Option<crate::game::model::Game>,
    /// The generation record (D-822 conditioning seam), when
    /// the project authors generation. Its `conditioning`
    /// subtree is carried as opaque RON in every build: in a
    /// build without the `gen-conditioning` feature the record
    /// is inert data — loaded, round-tripped, and re-emitted
    /// untouched, never interpreted.
    pub generation: Option<GenerationRecord>,
    /// Session records (D-838, the LIFT layer), sorted by id.
    /// Absent from the file when empty: a project with no
    /// session records carries no section. Each record's
    /// typed form (`torogao-scene-record/1`) lives in
    /// torogao-gencond behind the `gen-conditioning` feature;
    /// here the whole record is an opaque RON subtree, carried
    /// verbatim in every build — inert data without the
    /// feature, exactly like the conditioning section.
    pub scenes: Vec<SceneRecord>,
    /// The `content.ron` sidecar's prose entries, sorted by id.
    /// Sidecar state: the store merges them in at load and writes
    /// them back at commit; they are never serialized into
    /// `project.ron`, so a prose-only edit cannot churn the
    /// project hash (D-468).
    pub content: Vec<crate::game::model::ContentEntry>,
    /// The sidecar's locale as loaded (the canonical authored
    /// locale is `en`, D-472); preserved verbatim on re-emission
    /// so a translation sidecar stays itself.
    pub content_locale: Option<String>,
}

/// A generation record (D-822): the base plan plus, when set,
/// the conditioning section. The base fields are plain author
/// data in every build; the conditioning subtree's typed form
/// lives in `torogao-gencond` and exists only behind the
/// `gen-conditioning` feature — here it is a `crate::ron::Ron`
/// subtree, carried verbatim.
#[derive(Clone, Debug, PartialEq)]
pub struct GenerationRecord {
    /// Record id.
    pub id: String,
    /// S-7 name of the base checkpoint.
    pub checkpoint: String,
    /// The latent canvas.
    pub latent: GenerationLatent,
    /// The sampler configuration.
    pub sampler: GenerationSampler,
    /// The base seed.
    pub seed: i64,
    /// Save-image filename prefix.
    pub filename_prefix: String,
    /// The conditioning section as an opaque RON subtree
    /// (a `Conditioning(...)` record in the gencond schema),
    /// when the record carries one.
    pub conditioning: Option<crate::ron::Ron>,
}

/// A session record (D-838): one generation session's
/// product — its render recipe, medium-extension blocks,
/// custody map, and freeze provenance. The typed form is
/// `torogao_gencond::SceneRecord`; this is its storage shape:
/// the scene id in the open (collections sort and index by
/// it) and the record itself as an opaque RON subtree,
/// carried verbatim in every build.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneRecord {
    /// Scene id (matches the typed record's id; the
    /// validation sweep checks the match in feature builds).
    pub id: String,
    /// The `torogao-scene-record/1` record as an opaque RON
    /// subtree (a `SceneRecord(...)` tree in the gencond
    /// schema).
    pub record: crate::ron::Ron,
}

/// A generation record's latent canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GenerationLatent {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Batch size.
    pub batch_size: u32,
}

/// A generation record's sampler configuration.
#[derive(Clone, Debug, PartialEq)]
pub struct GenerationSampler {
    /// Steps.
    pub steps: u32,
    /// CFG scale.
    pub cfg: f64,
    /// Sampler name.
    pub name: String,
    /// Scheduler name.
    pub scheduler: String,
    /// Denoise strength.
    pub denoise: f64,
}

/// The project header.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Header {
    /// The project name (an id).
    pub name: String,
    /// The model format this project exports to: always
    /// `torogao-rig-model` (never the legacy token).
    pub model_format: String,
    /// The lane stamp.
    pub stamp: Stamp,
    /// The `torogao-consent/1` record.
    pub consent: Consent,
}

/// The lane stamp, in the existing CLI's rating-meta shape.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Stamp {
    /// Why this stamp was chosen.
    pub basis: String,
    /// The lane (`all-ages` or `adult`).
    pub lane: String,
    /// The lane pack name.
    pub pack: String,
    /// The rating within the lane.
    pub rating: String,
}

/// The consent record, in the lane packs' `torogao-consent/1`
/// shape.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Consent {
    /// The character is a consenting adult.
    pub adult: bool,
    /// The schema token (`torogao-consent/1`).
    pub schema: String,
    /// The character wants the selected scenes.
    pub wants_selected_scenes: bool,
}

/// An intaken image.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    /// Stable id.
    pub id: String,
    /// What kind of intake produced it.
    pub kind: AssetKind,
    /// Project-relative path: `assets/<sha256>.png`.
    pub file: String,
    /// sha256 of the asset file bytes, lowercase hex.
    pub sha256: String,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Painted-content bounds within the asset, when decoded.
    pub content: Option<Rect>,
    /// Position of the asset's top-left on the source canvas
    /// (separated art only).
    pub canvas_origin: Option<[i64; 2]>,
    /// Where the art came from.
    pub provenance: Provenance,
}

/// Asset kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AssetKind {
    /// A PNG intaken as-is (`intake_image`).
    Png,
    /// One layer of a separated PSD, written as PNG (`intake_psd`).
    PsdLayer,
}

/// Provenance for one asset.
#[derive(Clone, Debug, PartialEq)]
pub struct Provenance {
    /// License statement for the source art.
    pub license: String,
    /// Origin statement (who made it, under what commission).
    pub origin: String,
    /// The source file's base name.
    pub source_name: String,
    /// sha256 of the source file, lowercase hex.
    pub source_sha256: String,
    /// The PSD layer path, for separated art.
    pub layer: Option<String>,
}

/// An integer pixel rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rect {
    /// Left edge, pixels.
    pub x: u32,
    /// Top edge, pixels.
    pub y: u32,
    /// Width, pixels (> 0).
    pub width: u32,
    /// Height, pixels (> 0).
    pub height: u32,
}

/// A part: a node in the part tree, drawable once it has a mesh.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    /// Stable id (unique across parts and deformers).
    pub id: String,
    /// Node-tree parent part.
    pub parent: Option<String>,
    /// The deformer that deforms this part's mesh.
    pub deformer: Option<String>,
    /// Art binding.
    pub art: Option<ArtBind>,
    /// Mesh geometry.
    pub mesh: Option<Mesh>,
    /// Clipping mask.
    pub clip: Option<Clip>,
    /// Draw-order placement.
    pub draw: Option<Draw>,
    /// Default visibility.
    pub visible: bool,
    /// Completion-painting declaration from separated intake.
    pub completion: Option<Completion>,
    /// Mesh keyforms (vertex positions per lattice coordinate).
    pub keyforms: Option<Keyforms<MeshForm>>,
}

/// A part's binding to art.
#[derive(Clone, Debug, PartialEq)]
pub struct ArtBind {
    /// The asset id.
    pub asset: String,
    /// The UV region within the asset, pixels.
    pub region: Rect,
}

/// Mesh geometry: vertices, UVs (one per vertex), triangles.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    /// Vertex positions, model units.
    pub vertices: Vec<Point>,
    /// Texture coordinates, one per vertex.
    pub uvs: Vec<Point>,
    /// Triangle list, three indices per triangle.
    pub indices: Vec<u32>,
}

/// A clipping-mask record.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    /// The masking part.
    pub source: String,
    /// Draw outside the mask instead of inside.
    pub inverted: bool,
}

/// Draw-order placement: a band, an index within it, and an
/// optional render group. Exports as `band * 100 + index`.
#[derive(Clone, Debug, PartialEq)]
pub struct Draw {
    /// The band, 0..=99.
    pub band: u32,
    /// Index within the band, 0..=99.
    pub index: u32,
    /// Optional render-group id.
    pub group: Option<String>,
}

/// Completion-painting declaration for separated art.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    /// The hidden region was completion-painted.
    Painted,
    /// The layer needs no completion painting.
    NotNeeded,
}

/// A keyform lattice: one or two parameter axes and the forms
/// authored at lattice coordinates, sorted by coordinate.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyforms<F> {
    /// Axis parameter ids, in lattice order (1 or 2).
    pub axes: Vec<String>,
    /// Authored forms.
    pub forms: Vec<Keyform<F>>,
}

/// One keyform: a coordinate (one value per axis) and a payload.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyform<F> {
    /// Parameter values, one per axis.
    pub at: Vec<f64>,
    /// The payload.
    pub form: F,
}

/// Mesh keyform payload: absolute vertex positions.
#[derive(Clone, Debug, PartialEq)]
pub struct MeshForm {
    /// One position per mesh vertex.
    pub vertices: Vec<Point>,
}

/// Rotation keyform payload.
#[derive(Clone, Debug, PartialEq)]
pub struct RotationForm {
    /// Absolute angle, degrees.
    pub angle_degrees: f64,
    /// Scale per axis (> 0).
    pub scale: Point,
}

/// Warp keyform payload: absolute control-point positions.
#[derive(Clone, Debug, PartialEq)]
pub struct WarpForm {
    /// `(columns + 1) * (rows + 1)` positions, row-major.
    pub positions: Vec<Point>,
}

/// A deformer.
#[derive(Clone, Debug, PartialEq)]
pub struct Deformer {
    /// Stable id (unique across parts and deformers).
    pub id: String,
    /// Parent deformer.
    pub parent: Option<String>,
    /// Kind-specific data.
    pub kind: DeformerKind,
}

/// Deformer kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum DeformerKind {
    /// Rotation about a pivot.
    Rotation(Rotation),
    /// Control-grid warp.
    Warp(Warp),
}

/// A rotation deformer.
#[derive(Clone, Debug, PartialEq)]
pub struct Rotation {
    /// Pivot, model units.
    pub origin: Point,
    /// Rest angle, degrees.
    pub base_angle_degrees: f64,
    /// Keyforms.
    pub keyforms: Option<Keyforms<RotationForm>>,
}

/// A warp deformer over a rectangle.
#[derive(Clone, Debug, PartialEq)]
pub struct Warp {
    /// Grid columns, 1..=32.
    pub columns: u32,
    /// Grid rows, 1..=32.
    pub rows: u32,
    /// Rest rectangle `[x0, y0, x1, y1]`, `x0 < x1`, `y0 < y1`.
    pub bounds: [f64; 4],
    /// Keyforms.
    pub keyforms: Option<Keyforms<WarpForm>>,
}

/// A glue record joining vertices of two drawables.
#[derive(Clone, Debug, PartialEq)]
pub struct Glue {
    /// Stable id.
    pub id: String,
    /// First part.
    pub a: String,
    /// Second part.
    pub b: String,
    /// Vertex index pairs `(index in a, index in b)`.
    pub pairs: Vec<[u32; 2]>,
    /// Glue strength, 0..=1.
    pub weight: f64,
}

/// A parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Parameter {
    /// Stable id.
    pub id: String,
    /// Census class label (id syntax).
    pub class: String,
    /// Lower bound.
    pub min: f64,
    /// Declared default.
    pub default: f64,
    /// Upper bound.
    pub max: f64,
}

/// One neutral-pose entry.
#[derive(Clone, Debug, PartialEq)]
pub struct PoseEntry {
    /// The parameter.
    pub parameter: String,
    /// Its neutral value.
    pub value: f64,
}

/// An expression: an override table over parameters and parts.
#[derive(Clone, Debug, PartialEq)]
pub struct Expression {
    /// Stable id.
    pub id: String,
    /// How values combine with the pose.
    pub blend: Blend,
    /// Parameter rows, sorted by parameter id.
    pub values: Vec<ExpressionValue>,
    /// Visibility rows, sorted by part id.
    pub visibility: Vec<ExpressionVisibility>,
}

/// Expression blend modes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Blend {
    /// Values add to the pose, then clamp to range.
    Additive,
    /// Values replace the pose.
    Override,
}

/// One expression parameter row.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionValue {
    /// The parameter.
    pub parameter: String,
    /// The value (or delta, for additive blends).
    pub value: f64,
}

/// One expression visibility row.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionVisibility {
    /// The part.
    pub part: String,
    /// Visibility while the expression is active.
    pub visible: bool,
}

/// A named variant stack: alternative parts of which one shows.
#[derive(Clone, Debug, PartialEq)]
pub struct VariantSet {
    /// Stable id.
    pub id: String,
    /// Member parts, in declaration order.
    pub variants: Vec<String>,
}

// ---- A2 physics records (BRIEF-AUTHORING, physics group) --------
//
// The authored shape mirrors the physics model torogao-format
// carries (its PhysicsGroup / PhysicsInput / Strand / Particle /
// PhysicsOutput), so an authored group exports without a second
// translation. Field semantics are the format's; the fixed-step
// integrator that evaluates them is the engine's (physics_bake
// is descoped from this surface — see op::DESCOPED).

/// A physics group: anchored particle strands driven by
/// parameter inputs, writing parameter outputs.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsGroup {
    /// Stable id.
    pub id: String,
    /// Gravity vector.
    pub gravity: Point,
    /// Wind vector.
    pub wind: Point,
    /// Turbulence amplitude.
    pub turbulence_amplitude: f64,
    /// Parameter inputs, sorted by parameter.
    pub inputs: Vec<PhysicsInput>,
    /// Particle strands, sorted by id.
    pub strands: Vec<PhysicsStrand>,
}

/// A physics input: a scalar parameter rotating the group's
/// gravity vector by `scale_degrees * normalized_value`.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsInput {
    /// The driving parameter.
    pub parameter: String,
    /// Degrees of rotation at the parameter's maximum.
    pub scale_degrees: f64,
}

/// One anchored particle chain in a physics group.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsStrand {
    /// Stable id (unique within its group).
    pub id: String,
    /// What the strand's root is pinned to.
    pub anchor: PhysicsAnchor,
    /// The strand's particles, root to tip.
    pub particles: Vec<PhysicsParticle>,
    /// The parameter the strand's tip angle writes, when any.
    pub output: Option<PhysicsOutput>,
}

/// What a strand's root is pinned to.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsAnchor {
    /// The collection the anchor names.
    pub kind: AnchorKind,
    /// The deformer or part id.
    pub id: String,
    /// Offset from the anchor's origin.
    pub offset: Point,
}

/// The collection a physics anchor names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnchorKind {
    /// A deformer.
    Deformer,
    /// A part (its mesh).
    Mesh,
}

/// One particle (one segment) of a strand.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsParticle {
    /// Segment length.
    pub length: f64,
    /// Collision radius.
    pub radius: f64,
    /// Mobility (response to forces).
    pub mobility: f64,
    /// Delay (response lag).
    pub delay: f64,
    /// Damping (velocity loss per step).
    pub damping: f64,
}

/// A strand output: writes one scalar parameter as
/// `offset + scale * tip_angle_degrees`.
#[derive(Clone, Debug, PartialEq)]
pub struct PhysicsOutput {
    /// The written parameter.
    pub parameter: String,
    /// Degrees-to-value scale.
    pub scale: f64,
    /// Value offset.
    pub offset: f64,
}

// ---- A3 motion records (BRIEF-AUTHORING, motion group) ----------

/// A motion clip: a length in ticks, a loop flag, and one curve
/// per driven parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct Motion {
    /// Stable id.
    pub id: String,
    /// Clip length in ticks (one tick is the runtime's step).
    pub length_ticks: u32,
    /// Whether the clip loops.
    pub looped: bool,
    /// Parameter curves, sorted by parameter.
    pub curves: Vec<MotionCurve>,
}

/// One parameter's keyframe curve inside a motion.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionCurve {
    /// The driven parameter.
    pub parameter: String,
    /// Keyframe rows, strictly ascending by tick.
    pub keyframes: Vec<MotionKeyframe>,
    /// The cited source when the curve was imported from
    /// measured samples (`curve_from_samples`); authored curves
    /// carry none.
    pub source: Option<String>,
}

/// One keyframe row: a value at a tick.
#[derive(Clone, Debug, PartialEq)]
pub struct MotionKeyframe {
    /// The tick (0-based, inside the clip's length).
    pub tick: u32,
    /// The parameter's value at that tick.
    pub value: f64,
}

// ---- A3 audio-reactive records (BRIEF-AUTHORING, audio group) ---

/// A named audio track: event markers in ticks. The track is
/// the reaction system's sync contract as data; it synthesizes
/// no audio (atelier's lane) and plays none (the engine's).
#[derive(Clone, Debug, PartialEq)]
pub struct AudioTrack {
    /// Stable id.
    pub id: String,
    /// Event markers, sorted by tick then id.
    pub markers: Vec<AudioMarker>,
}

/// One event marker on an audio track.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioMarker {
    /// Stable id (unique within its track).
    pub id: String,
    /// The marker's tick.
    pub tick: u32,
}

/// One audio-reactive binding row: a track marker drives a
/// parameter-curve offset or fires an expression, with the
/// envelope (peak tick, trail length) carried as data.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioBind {
    /// The track id.
    pub track: String,
    /// The marker id within the track.
    pub marker: String,
    /// What the marker drives.
    pub target: AudioBindTarget,
    /// Sync offset in ticks (may be negative: a reaction may
    /// lead its marker).
    pub sync_offset_ticks: i64,
    /// Ticks from the (offset) event to the envelope's peak.
    pub peak_tick: u32,
    /// The envelope's trail length in ticks.
    pub trail_ticks: u32,
}

/// What an audio marker drives.
#[derive(Clone, Debug, PartialEq)]
pub enum AudioBindTarget {
    /// Offset a parameter's curve by a fixed amount.
    ParameterOffset {
        /// The parameter.
        parameter: String,
        /// The offset added while the envelope is hot.
        offset: f64,
    },
    /// Fire a named expression.
    ExpressionTrigger {
        /// The expression.
        expression: String,
    },
}

/// One audio-binding row: what a named sound slot in the rig
/// family's audio group is. A dialogue line's reaction hook
/// names a row by id (Amendment 2 §Item G4); the row says what
/// the sound is. Rows are references only — no audio data, and
/// no foreign record's fields, ever enter the project.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioBinding {
    /// Stable id.
    pub id: String,
    /// What kind of sound the row binds.
    pub kind: AudioBindingKind,
}

/// Audio-binding kinds. The voice kind is the torori seam
/// (voice plan §3); the audio-reactive kinds belong to slice
/// A3's `audio_track_define` / `audio_bind_set` and land with
/// that slice, not here. The hypnosis-track kind lands with
/// the Q1 hypnosis build (D-634): a layered hypnosis track
/// bound into a project (TOROGAO-HYPNO-CAPABILITY §b).
#[derive(Clone, Debug, PartialEq)]
pub enum AudioBindingKind {
    /// A spoken line: one character, voiced by torori from the
    /// delivery record the row references.
    Voice(VoiceBinding),
    /// A layered hypnosis track: a session reference plus the
    /// layer map that names its stack.
    HypnosisTrack(HypnosisTrackBinding),
}

/// A hypnosis track binding's payload (TECHNIQUE-SPEC T3/T8):
/// the session the track renders, and the layer map naming
/// the stack. Layers are kept separate through the whole
/// pipeline; an edition is a mix decision over this map, not
/// a re-render.
#[derive(Clone, Debug, PartialEq)]
pub struct HypnosisTrackBinding {
    /// The session reference (a `torogao-session` document id;
    /// an opaque reference string here, like a delivery ref).
    pub session: String,
    /// The layer stack, sorted by kind label.
    pub layers: Vec<HypnosisLayer>,
}

/// One layer of a hypnosis track's stack.
#[derive(Clone, Debug, PartialEq)]
pub struct HypnosisLayer {
    /// The layer's perceptual job in the stack.
    pub kind: HypnosisLayerKind,
    /// The stem reference this layer renders.
    pub stem: String,
    /// Gain offset in dB against the session's own staging.
    pub gain_offset_db: f64,
    /// The named gain envelope for this layer, when one is
    /// bound (beyond the session's fixed edges).
    pub envelope: Option<String>,
    /// The named editions that include this layer. Empty
    /// means every edition (T8's layer-subset publishing,
    /// decided at authoring time).
    pub editions: Vec<String>,
    /// Whether the layer loops continuously beneath the
    /// session (the T5 drone posture).
    pub looped: bool,
}

/// A hypnosis layer's job in the stack (T3).
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum HypnosisLayerKind {
    /// The audible narration layer.
    Voice,
    /// The binaural carrier layer.
    Carrier,
    /// The noise-bed layer.
    Bed,
    /// The encoded affirmation layer.
    Subliminal,
    /// A sound-effect layer.
    Sfx,
}

impl HypnosisLayerKind {
    /// The kind's wire/record label.
    pub fn label(self) -> &'static str {
        match self {
            HypnosisLayerKind::Voice => "voice",
            HypnosisLayerKind::Carrier => "carrier",
            HypnosisLayerKind::Bed => "bed",
            HypnosisLayerKind::Subliminal => "subliminal",
            HypnosisLayerKind::Sfx => "sfx",
        }
    }

    /// The kind for a wire/record label.
    pub fn from_label(label: &str) -> Option<Self> {
        match label {
            "voice" => Some(HypnosisLayerKind::Voice),
            "carrier" => Some(HypnosisLayerKind::Carrier),
            "bed" => Some(HypnosisLayerKind::Bed),
            "subliminal" => Some(HypnosisLayerKind::Subliminal),
            "sfx" => Some(HypnosisLayerKind::Sfx),
            _ => None,
        }
    }
}

impl HypnosisTrackBinding {
    /// The layers an edition includes, in stack order: a
    /// layer with an empty edition mask rides every edition;
    /// otherwise the edition must be named.
    pub fn layers_for_edition<'a>(&'a self, edition: &str) -> Vec<&'a HypnosisLayer> {
        self.layers
            .iter()
            .filter(|layer| {
                layer.editions.is_empty() || layer.editions.iter().any(|e| e == edition)
            })
            .collect()
    }
}

/// One visual-binding row: a visual induction channel bound
/// into the project beside the audio bindings
/// (TOROGAO-HYPNO-CAPABILITY §c). Rows are references and
/// seeded parameters only — no pixels, ever.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualBinding {
    /// Stable id.
    pub id: String,
    /// What kind of visual channel the row binds.
    pub kind: VisualBindingKind,
}

/// Visual-binding kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum VisualBindingKind {
    /// A looping visual accompaniment bound to a session
    /// (T9's shape: portrait, seamless, fixed duration).
    VisualLoop(VisualLoopBinding),
    /// A procedural overlay hook with seeded parameters.
    OverlayHook(OverlayHookBinding),
}

/// A visual loop binding's payload. The loop starts at phase
/// boundaries — that is the binding's fixed semantics, not a
/// field — and its repetition is declared here, never
/// improvised at runtime.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualLoopBinding {
    /// The loop asset reference.
    pub asset: String,
    /// The session reference the loop accompanies.
    pub session: String,
    /// How the loop covers the session's duration.
    pub sync: LoopSync,
    /// The loop's declared duration in seconds (T9's fixed
    /// duration, as data).
    pub duration_seconds: f64,
    /// The loop's declared frame rate.
    pub frame_rate: u32,
}

/// How a visual loop covers a session's duration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoopSync {
    /// The loop repeats this many times.
    Count(u32),
    /// The loop stretches to the session's duration.
    Stretch,
}

/// A procedural overlay hook's payload: a named hook plus its
/// seeded parameters, all data in the binding. The hook is
/// replaceable per game without code.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayHookBinding {
    /// The named overlay hook (e.g. a spiral).
    pub hook: String,
    /// The session reference the overlay accompanies.
    pub session: String,
    /// The parameter seed: the overlay's motion derives from
    /// it deterministically.
    pub seed: i64,
    /// Rotation rate in radians per second (signed).
    pub rotation_rate: f64,
    /// The named opacity envelope (tied to phase by the
    /// presentation layer).
    pub opacity_envelope: String,
}

impl OverlayHookBinding {
    /// The overlay's deterministic phase at a sample position
    /// on the session's 48 kHz clock: a seed-derived offset
    /// advanced by the bound rotation rate. Same record, same
    /// sample → same phase, on every machine.
    pub fn phase_at(&self, at_sample: u64) -> f64 {
        let mixed = (self.seed as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let offset = ((mixed >> 11) as f64 / 9_007_199_254_740_992.0) * std::f64::consts::TAU;
        let advanced = self.rotation_rate * (at_sample as f64 / 48_000.0);
        (offset + advanced).rem_euclid(std::f64::consts::TAU)
    }
}

/// A voice binding's payload: who speaks, and which torori
/// conditioning-v1 record says how.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceBinding {
    /// The speaking character's id (a game-layer character
    /// record; resolution joins when the game layer lands).
    pub character: String,
    /// The torori delivery reference.
    pub delivery: DeliveryRef,
}

/// A reference into torori's namespace: the estate
/// VoiceIdentity a line is voiced as, and the conditioning-v1
/// record that steers the performance. Both are opaque
/// reference strings — torori owns their meaning; this crate
/// never models a conditioning field.
#[derive(Clone, Debug, PartialEq)]
pub struct DeliveryRef {
    /// The torori VoiceIdentity id (`identity_ref` in torori's
    /// registry).
    pub identity: String,
    /// The torori conditioning-v1 record reference.
    pub record: String,
}

impl Project {
    /// The part with this id.
    pub fn part(&self, id: &str) -> Option<&Part> {
        find(&self.parts, id, |part| &part.id)
    }

    /// The deformer with this id.
    pub fn deformer(&self, id: &str) -> Option<&Deformer> {
        find(&self.deformers, id, |deformer| &deformer.id)
    }

    /// The parameter with this id.
    pub fn parameter(&self, id: &str) -> Option<&Parameter> {
        find(&self.parameters, id, |parameter| &parameter.id)
    }

    /// The asset with this id.
    pub fn asset(&self, id: &str) -> Option<&Asset> {
        find(&self.assets, id, |asset| &asset.id)
    }

    /// The expression with this id.
    pub fn expression(&self, id: &str) -> Option<&Expression> {
        find(&self.expressions, id, |expression| &expression.id)
    }

    /// The glue record with this id.
    pub fn glue(&self, id: &str) -> Option<&Glue> {
        find(&self.glues, id, |glue| &glue.id)
    }

    /// The audio binding row for this (track, marker), if any.
    pub fn audio_bind(&self, track: &str, marker: &str) -> Option<&AudioBind> {
        self.audio_binds
            .iter()
            .find(|bind| bind.track == track && bind.marker == marker)
    }

    /// The audio track with this id.
    pub fn audio_track(&self, id: &str) -> Option<&AudioTrack> {
        find(&self.audio_tracks, id, |track| &track.id)
    }

    /// The motion with this id.
    pub fn motion(&self, id: &str) -> Option<&Motion> {
        find(&self.motions, id, |motion| &motion.id)
    }

    /// The physics group with this id.
    pub fn physics_group(&self, id: &str) -> Option<&PhysicsGroup> {
        find(&self.physics_groups, id, |group| &group.id)
    }

    /// The audio-binding row with this id.
    pub fn audio_binding(&self, id: &str) -> Option<&AudioBinding> {
        find(&self.audio_bindings, id, |binding| &binding.id)
    }

    /// The visual-binding row with this id.
    pub fn visual_binding(&self, id: &str) -> Option<&VisualBinding> {
        find(&self.visual_bindings, id, |binding| &binding.id)
    }

    /// The neutral value of a parameter: its pose entry when one
    /// exists, else its declared default.
    pub fn neutral_value(&self, parameter: &Parameter) -> f64 {
        self.pose
            .iter()
            .find(|entry| entry.parameter == parameter.id)
            .map_or(parameter.default, |entry| entry.value)
    }
}

/// Binary search in an id-sorted collection.
pub fn find<'a, T>(items: &'a [T], id: &str, key: impl Fn(&T) -> &String) -> Option<&'a T> {
    items
        .binary_search_by(|item| key(item).as_str().cmp(id))
        .ok()
        .map(|index| &items[index])
}

/// Index of an id in an id-sorted collection: `Ok` when present,
/// `Err(insertion point)` otherwise.
pub fn locate<T>(items: &[T], id: &str, key: impl Fn(&T) -> &String) -> Result<usize, usize> {
    items.binary_search_by(|item| key(item).as_str().cmp(id))
}

impl Deformer {
    /// The keyform axes, if any lattice is declared.
    pub fn axes(&self) -> Option<&[String]> {
        match &self.kind {
            DeformerKind::Rotation(rotation) => rotation.keyforms.as_ref().map(|k| &k.axes[..]),
            DeformerKind::Warp(warp) => warp.keyforms.as_ref().map(|k| &k.axes[..]),
        }
    }

    /// The kind name used in census output and messages.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            DeformerKind::Rotation(_) => "rotation",
            DeformerKind::Warp(_) => "warp",
        }
    }
}

impl Warp {
    /// Rest control points: an evenly spaced grid over `bounds`,
    /// row-major, `(columns + 1) * (rows + 1)` points.
    pub fn rest_positions(&self) -> Vec<Point> {
        let [x0, y0, x1, y1] = self.bounds;
        let mut points = Vec::with_capacity(self.point_count());
        for row in 0..=self.rows {
            let y = y0 + (y1 - y0) * f64::from(row) / f64::from(self.rows);
            for column in 0..=self.columns {
                let x = x0 + (x1 - x0) * f64::from(column) / f64::from(self.columns);
                points.push([x, y]);
            }
        }
        points
    }

    /// The control-point count.
    pub fn point_count(&self) -> usize {
        (self.columns as usize + 1) * (self.rows as usize + 1)
    }
}
