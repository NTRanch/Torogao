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
    /// Audio-reactive binding rows (A3), sorted by (track, marker).
    pub audio_binds: Vec<AudioBind>,
    /// Named audio tracks of event markers (A3), sorted by id.
    pub audio_tracks: Vec<AudioTrack>,
    /// The authored game clock (AUTH-G), when one is defined.
    pub clock: Option<GameClock>,
    /// Deformers, sorted by id.
    pub deformers: Vec<Deformer>,
    /// Expressions, sorted by id.
    pub expressions: Vec<Expression>,
    /// The project as a game (AUTH-G), when defined.
    pub game: Option<GameDef>,
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
    /// Named randomness streams (AUTH-G), sorted by id.
    pub rng_streams: Vec<RngStream>,
    /// The authored save schema (AUTH-G), when defined.
    pub save_schema: Option<SaveSchema>,
    /// Named variant stacks from separated art, sorted by id.
    pub variant_sets: Vec<VariantSet>,
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

// ---- AUTH-G game records (Amendment 2) --------------------------

/// The game's time structure (`clock_define`): the runtime tick
/// is one slot advance (§G3.1).
#[derive(Clone, Debug, PartialEq)]
pub struct GameClock {
    /// Ordered named slots per day, in tick order (authored
    /// order is semantic, never re-sorted).
    pub slots: Vec<String>,
    /// Days per week, when the game uses weeks.
    pub days_per_week: Option<u32>,
    /// Weekday names, in order; exactly `days_per_week` long
    /// when present.
    pub weekdays: Vec<String>,
    /// Optional day limit; the run cannot pass it.
    pub day_limit: Option<u32>,
}

/// The project as a game (`game_define`).
#[derive(Clone, Debug, PartialEq)]
pub struct GameDef {
    /// The game id.
    pub id: String,
    /// The start scene id: a forward reference at define time,
    /// checked by the validation sweep once scenes exist (§G2.1).
    pub start_scene: String,
    /// Stat/flag values at run start, sorted by target. Each is
    /// validated against its definition at define time (§G2.1).
    pub initial: Vec<InitialAssignment>,
    /// The alter-ego veto rule: an effect-set id applied when
    /// the player declines an option marked `origin: alter_ego`
    /// (§G2.1, §G5). A forward reference until effect sets land.
    pub alter_ego_veto: Option<String>,
    /// Save-context policy default for the game (the schema
    /// carries its own; §G3.3).
    pub save_policy: Option<SaveContextPolicy>,
}

/// One initial assignment: a stat/flag target and its run-start
/// value (stats are integers; flags are booleans — §G2.2).
#[derive(Clone, Debug, PartialEq)]
pub struct InitialAssignment {
    /// The stat or flag id.
    pub target: String,
    /// The value at run start.
    pub value: AssignValue,
}

/// A run-start value: an integer for a stat, a boolean for a
/// flag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AssignValue {
    /// An integer stat value.
    Int(i64),
    /// A boolean flag value.
    Bool(bool),
}

/// Where a manual save may be taken (§G3.3).
#[derive(Clone, Debug, PartialEq)]
pub enum SaveContextPolicy {
    /// At any tick boundary (the first-class default posture).
    AnyTickBoundary,
    /// Only at scene boundaries.
    SceneBoundary,
    /// Only inside the listed safe scenes.
    SafeScenesOnly(Vec<String>),
}

/// A named randomness stream (`rng_stream_define`): the
/// algorithm is part of the project's format record, so an
/// algorithm change is a project migration, not a silent drift.
#[derive(Clone, Debug, PartialEq)]
pub struct RngStream {
    /// Stream id.
    pub id: String,
    /// Named, versioned algorithm (e.g. `torogao-rng/1`).
    pub algorithm: String,
}

/// The authored save schema (`game_save_schema_define`, §G3.3):
/// saving and loading are runtime acts; what a save must
/// contain is authored data.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveSchema {
    /// Schema version: strictly increases on every change.
    pub version: u32,
    /// Slot count, 1..=64.
    pub slots: u32,
    /// Whether a slot holds one run or a chapter list.
    pub slot_holds: SlotHolds,
    /// State fields in the per-slot partition, sorted.
    pub slot_fields: Vec<String>,
    /// State fields in the globals partition, sorted.
    pub globals_fields: Vec<String>,
    /// The named hash algorithm version saves carry.
    pub hash_algorithm: String,
    /// Where a manual save may be taken.
    pub policy: SaveContextPolicy,
    /// The migration chain: named steps, sorted by `from`; each
    /// step goes `from` → `from + 1` and the chain covers every
    /// prior version the project still accepts (§G3.3).
    pub migrations: Vec<Migration>,
}

/// Whether a save slot holds one run or a chapter list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotHolds {
    /// One run per slot.
    OneRun,
    /// A chapter list per slot.
    ChapterList,
}

/// One named migration step in a save schema's chain.
#[derive(Clone, Debug, PartialEq)]
pub struct Migration {
    /// The version this step migrates from.
    pub from: u32,
    /// The migration's name (the field mapping is authored data
    /// the interpreter executes stepwise; the name is its id).
    pub name: String,
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

    /// The RNG stream with this id.
    pub fn rng_stream(&self, id: &str) -> Option<&RngStream> {
        find(&self.rng_streams, id, |stream| &stream.id)
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
