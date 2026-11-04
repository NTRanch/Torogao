//! The session record: `torogao-scene-record/1` (D-838, the
//! LIFT layer, Stage 1).
//!
//! A generation session is a brokered, logged authoring act
//! (D-077 capture discipline), and this record is its product:
//! the media a session emits is legally a *render of this
//! record*. The record carries three things:
//!
//! 1. **The render recipe** — the conditioning record's hash,
//!    the compiled graph's hash, and the seed, plus the
//!    backend and execution context the render ran under.
//!    Re-rendering is byte-identical only where the backend
//!    allows it, and the record says which case it is in its
//!    `rerender` field: `PinnedContextByteIdentical` is a
//!    claim with the D-761 meaning (same graph + same build +
//!    same machine + pinned execution context), and
//!    `BestEffort` must carry the reason it cannot claim more.
//!    There is no third, vaguer state.
//! 2. **Typed medium-extension blocks** — one per medium the
//!    estate emits: `puppet2d`, `armature3d`, `sprite_cels`,
//!    `audio_events`. Every record carries all four, each in
//!    an explicit state. Absence would be ambiguous (not yet
//!    built? lost? not applicable?), so a block that holds
//!    nothing says `NotYetPopulated` — a typed state, not a
//!    missing field. In this stage only `puppet2d` can be
//!    populated: it references a lumen rig model by content
//!    hash and format version (Torogao consumes lumen's
//!    format downstream, D-835 — a hash and a version
//!    constant, never a dependency, never vendored code).
//!    The other three kinds have no populated state in this
//!    schema version; a record claiming one does not parse.
//! 3. **The custody map and the freeze ledger** — who holds
//!    custody of which nodes (see `custody`), and the
//!    provenance of every freeze applied to the scene (see
//!    `freeze`): operation, source hash, frozen hash, and the
//!    contract value the freeze ran under.
//!
//! The record's RON tree form and the shared semantics lumen
//! checks against are specified in `SCENE-RECORD-SPEC.md`
//! (lift workdir) and in this module's validation rules.

use crate::custody::CustodyMap;
use crate::error::GencondResult;
use crate::error::{
    CODE_CONFLICT, CODE_MALFORMED, CODE_RANGE, CODE_UNKNOWN_KIND, CODE_UNSUPPORTED_SCHEMA, refuse,
};
use crate::freeze::{FrozenTrack, PaletteSnap};
use crate::limits::{
    FROZEN_TRACKS_MAX, PALETTE_SNAPS_MAX, PROVENANCE_ENTRIES_MAX, SCENE_TEXT_BYTES_MAX,
};
use serde::{Deserialize, Serialize};

/// The scene record's format token.
pub const SCENE_FORMAT: &str = "torogao-scene-record";
/// The scene record format version this build reads/writes.
pub const SCENE_FORMAT_VERSION: u32 = 1;

/// The format token of the lumen rig model a `puppet2d`
/// block references. A reference, not a dependency (D-835):
/// lumen owns the format; Torogao names it to point at it.
pub const LUMEN_RIG_MODEL_FORMAT: &str = "lumen-rig-model";
/// The lumen rig-model format version this build references.
pub const LUMEN_RIG_MODEL_FORMAT_VERSION: u32 = 1;

/// One generation session's record: recipe, medium blocks,
/// custody, and freeze provenance (see the module docs).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SceneRecord {
    /// Format version; must equal [`SCENE_FORMAT_VERSION`].
    pub schema: u32,
    /// The scene's id (a source-id-shaped name).
    pub id: String,
    /// The render recipe: what produced the media.
    pub recipe: RenderRecipe,
    /// The four medium-extension blocks, each exactly once.
    pub extensions: Vec<MediumExtension>,
    /// Custody over the scene's nodes.
    pub custody: CustodyMap,
    /// Provenance of every freeze applied to this scene,
    /// in application order (append-only history).
    pub provenance: Vec<FreezeProvenance>,
    /// Tracks frozen into this scene, by parameter.
    pub frozen_tracks: Vec<FrozenTrack>,
    /// Palette snaps frozen into this scene.
    pub palette_snaps: Vec<PaletteSnap>,
}

/// The render recipe: sufficient to re-render the session's
/// media where the backend allows, honest where it does not.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderRecipe {
    /// SHA-256 of the conditioning record the session ran
    /// (canonical JSON of the record; see
    /// [`crate::Conditioning`] and `conditioning_sha256`).
    pub conditioning_sha256: String,
    /// SHA-256 of the compiled `torogao-gen-graph/1` graph
    /// (the compile report's `graph_sha256`).
    pub graph_sha256: String,
    /// The session's seed (the generation record's seed plan;
    /// batch position is part of identity where a batch ran,
    /// DESIGN §1.2 — recorded by the emitter in the context).
    pub seed: i64,
    /// The executing backend's name (an S-7-style name; the
    /// graph itself never names its executor, S-1 §2.3 —
    /// the recipe is where the deployment fact lives).
    pub backend: String,
    /// The pinned execution context (D-761), verbatim; empty
    /// when the run pinned nothing — in which case `rerender`
    /// may not claim byte identity.
    pub execution_context: String,
    /// What re-rendering this recipe promises.
    pub rerender: RerenderGuarantee,
}

/// What re-rendering a recipe promises — a typed honesty
/// contract, not a comment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RerenderGuarantee {
    /// Same graph + same seam build + same machine + the
    /// recorded pinned context ⇒ byte-identical media
    /// (the D-761 parity definition).
    PinnedContextByteIdentical,
    /// Re-rendering is not promised byte-identical; the
    /// reason is part of the record (e.g. the backend's
    /// execution-context jitter is unpinned, or the backend
    /// is a remote service whose build is not knowable).
    BestEffort {
        /// Why byte identity cannot be claimed. Non-empty:
        /// an unexplained best-effort is not honest data.
        reason: String,
    },
}

/// The media the estate emits, as extension-block kinds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediumKind {
    /// A 2D puppet: a lumen rig model, evaluated by lumen's
    /// rig stack (the D-834/D-835 proving lane).
    Puppet2d,
    /// A 3D armature scene (glTF-lineage). Not yet populable.
    Armature3d,
    /// Sprite cels (Aseprite-lineage). Not yet populable.
    SpriteCels,
    /// Audio events (score-lineage). Not yet populable.
    AudioEvents,
}

impl MediumKind {
    /// Every kind, in record order. A complete scene record
    /// carries exactly these, each exactly once.
    pub const ALL: [MediumKind; 4] = [
        MediumKind::Puppet2d,
        MediumKind::Armature3d,
        MediumKind::SpriteCels,
        MediumKind::AudioEvents,
    ];

    /// The wire/tree identifier.
    pub fn ident(self) -> &'static str {
        match self {
            MediumKind::Puppet2d => "Puppet2d",
            MediumKind::Armature3d => "Armature3d",
            MediumKind::SpriteCels => "SpriteCels",
            MediumKind::AudioEvents => "AudioEvents",
        }
    }

    /// Parses a tree-form identifier; an unknown kind refuses
    /// by name — a record naming a medium this build does not
    /// know is refused at load, never silently dropped.
    pub fn from_ident(name: &str) -> GencondResult<MediumKind> {
        match name {
            "Puppet2d" => Ok(MediumKind::Puppet2d),
            "Armature3d" => Ok(MediumKind::Armature3d),
            "SpriteCels" => Ok(MediumKind::SpriteCels),
            "AudioEvents" => Ok(MediumKind::AudioEvents),
            other => refuse(
                CODE_UNKNOWN_KIND,
                format!("medium extension kind '{other}' is not known to this build"),
            ),
        }
    }
}

/// One medium-extension block: a kind and its typed state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediumExtension {
    /// Which medium this block speaks for.
    pub kind: MediumKind,
    /// The block's state — explicitly populated or
    /// explicitly not yet populated, never absent.
    pub state: ExtensionState,
}

/// A medium block's state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionState {
    /// The block holds nothing yet, and says so. This is the
    /// only state `armature3d`, `sprite_cels`, and
    /// `audio_events` can be in under this schema version.
    NotYetPopulated,
    /// A `puppet2d` block populated with a lumen rig-model
    /// reference. Legal only on the `puppet2d` kind
    /// (validated).
    Puppet2d(Puppet2dRef),
}

/// A reference to a lumen rig model: content hash plus the
/// format token and version it was written under. Torogao
/// stores the reference; lumen owns the model (D-835).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Puppet2dRef {
    /// The referenced format token; must be
    /// [`LUMEN_RIG_MODEL_FORMAT`].
    pub format: String,
    /// The referenced format's version; must be
    /// [`LUMEN_RIG_MODEL_FORMAT_VERSION`] in this build.
    pub format_version: u32,
    /// SHA-256 of the referenced model's canonical form.
    pub model_sha256: String,
}

/// Which freeze operation a provenance entry records.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreezeOp {
    /// `freeze_track`.
    FreezeTrack,
    /// `freeze_palette`.
    FreezePalette,
}

impl FreezeOp {
    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            FreezeOp::FreezeTrack => "FreezeTrack",
            FreezeOp::FreezePalette => "FreezePalette",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<FreezeOp> {
        match name {
            "FreezeTrack" => Ok(FreezeOp::FreezeTrack),
            "FreezePalette" => Ok(FreezeOp::FreezePalette),
            other => refuse(
                CODE_UNKNOWN_KIND,
                format!("freeze operation '{other}' is not known"),
            ),
        }
    }
}

/// Where a freeze's epsilon came from (track freezes only).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpsilonOrigin {
    /// The caller supplied the epsilon as a contract value.
    Caller,
    /// The epsilon was derived from the track's own sample
    /// noise (`track_noise_epsilon`).
    TrackNoise,
}

impl EpsilonOrigin {
    /// The tree-form identifier.
    pub fn ident(self) -> &'static str {
        match self {
            EpsilonOrigin::Caller => "Caller",
            EpsilonOrigin::TrackNoise => "TrackNoise",
        }
    }

    /// Parses a tree-form identifier.
    pub fn from_ident(name: &str) -> GencondResult<EpsilonOrigin> {
        match name {
            "Caller" => Ok(EpsilonOrigin::Caller),
            "TrackNoise" => Ok(EpsilonOrigin::TrackNoise),
            other => refuse(
                CODE_UNKNOWN_KIND,
                format!("epsilon origin '{other}' is not known"),
            ),
        }
    }
}

/// One freeze's provenance: source hash -> frozen hash, the
/// operation, and the contract value it ran under. Entries
/// are history: applying a freeze appends, never rewrites.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreezeProvenance {
    /// The operation that ran.
    pub op: FreezeOp,
    /// SHA-256 of the freeze input's canonical form.
    pub source_sha256: String,
    /// SHA-256 of the frozen output's canonical form.
    pub frozen_sha256: String,
    /// The epsilon a track freeze ran under (track freezes
    /// only; exactly one of `epsilon`/`tolerance` is set).
    pub epsilon: Option<f64>,
    /// Where that epsilon came from (present iff `epsilon`).
    pub epsilon_origin: Option<EpsilonOrigin>,
    /// The tolerance a palette freeze ran under (palette
    /// freezes only).
    pub tolerance: Option<u32>,
}

impl SceneRecord {
    /// A record whose medium blocks are all explicitly
    /// not-yet-populated and whose custody and freeze ledger
    /// are empty: the shape a session emits before any medium
    /// lands in it.
    pub fn new(id: String, recipe: RenderRecipe) -> SceneRecord {
        SceneRecord {
            schema: SCENE_FORMAT_VERSION,
            id,
            recipe,
            extensions: MediumKind::ALL
                .into_iter()
                .map(|kind| MediumExtension {
                    kind,
                    state: ExtensionState::NotYetPopulated,
                })
                .collect(),
            custody: CustodyMap::default(),
            provenance: Vec::new(),
            frozen_tracks: Vec::new(),
            palette_snaps: Vec::new(),
        }
    }

    /// Validates the record whole: version, id, recipe
    /// honesty, the four-block completeness law, custody,
    /// and every frozen artifact it carries.
    pub fn validate(&self) -> GencondResult<()> {
        if self.schema != SCENE_FORMAT_VERSION {
            return refuse(
                CODE_UNSUPPORTED_SCHEMA,
                format!(
                    "scene record schema {} is not supported (this build reads {})",
                    self.schema, SCENE_FORMAT_VERSION
                ),
            );
        }
        crate::schema::check_source_id(&self.id)?;
        self.recipe.validate()?;
        self.validate_extensions()?;
        self.custody.validate()?;
        if self.provenance.len() > PROVENANCE_ENTRIES_MAX {
            return refuse(CODE_RANGE, "scene record exceeds the provenance bound");
        }
        for entry in &self.provenance {
            entry.validate()?;
        }
        if self.frozen_tracks.len() > FROZEN_TRACKS_MAX {
            return refuse(CODE_RANGE, "scene record exceeds the frozen-track bound");
        }
        let mut parameters: Vec<&str> = Vec::new();
        for track in &self.frozen_tracks {
            track.validate()?;
            if parameters.contains(&track.parameter.as_str()) {
                return refuse(
                    CODE_CONFLICT,
                    format!(
                        "parameter '{}' is frozen twice in one scene",
                        track.parameter
                    ),
                );
            }
            parameters.push(track.parameter.as_str());
        }
        if self.palette_snaps.len() > PALETTE_SNAPS_MAX {
            return refuse(CODE_RANGE, "scene record exceeds the palette-snap bound");
        }
        let mut palettes: Vec<&str> = Vec::new();
        for snap in &self.palette_snaps {
            snap.validate()?;
            if palettes.contains(&snap.palette_sha256.as_str()) {
                return refuse(
                    CODE_CONFLICT,
                    "one palette is snapped twice in one scene; a re-snap replaces, it does not stack",
                );
            }
            palettes.push(snap.palette_sha256.as_str());
        }
        Ok(())
    }

    fn validate_extensions(&self) -> GencondResult<()> {
        if self.extensions.len() != MediumKind::ALL.len() {
            return refuse(
                CODE_MALFORMED,
                "a scene record carries exactly the four medium-extension blocks; \
                 absence is ambiguous, so every block is present and states its state",
            );
        }
        for kind in MediumKind::ALL {
            let count = self
                .extensions
                .iter()
                .filter(|ext| ext.kind == kind)
                .count();
            if count != 1 {
                return refuse(
                    CODE_CONFLICT,
                    format!(
                        "medium extension '{}' appears {count} times; each kind appears exactly once",
                        kind.ident()
                    ),
                );
            }
        }
        for extension in &self.extensions {
            if let ExtensionState::Puppet2d(reference) = &extension.state {
                if extension.kind != MediumKind::Puppet2d {
                    return refuse(
                        CODE_CONFLICT,
                        "a puppet2d reference sits in a block of another kind",
                    );
                }
                reference.validate()?;
            }
        }
        Ok(())
    }

    /// Records a freeze in the record's provenance (the
    /// append-only history) — the act the freeze verbs
    /// perform when they land their output here.
    pub fn record_freeze(&mut self, entry: FreezeProvenance) {
        self.provenance.push(entry);
    }

    /// Lands a frozen track, replacing any earlier freeze of
    /// the same parameter (provenance keeps the history; the
    /// scene keeps the current curve).
    pub fn land_frozen_track(&mut self, track: FrozenTrack) {
        if let Some(existing) = self
            .frozen_tracks
            .iter_mut()
            .find(|held| held.parameter == track.parameter)
        {
            *existing = track;
        } else {
            self.frozen_tracks.push(track);
        }
    }

    /// Lands a palette snap, replacing any earlier snap of
    /// the same locked palette.
    pub fn land_palette_snap(&mut self, snap: PaletteSnap) {
        if let Some(existing) = self
            .palette_snaps
            .iter_mut()
            .find(|held| held.palette_sha256 == snap.palette_sha256)
        {
            *existing = snap;
        } else {
            self.palette_snaps.push(snap);
        }
    }
}

impl RenderRecipe {
    /// Validates the recipe: hash shapes, bounded names, and
    /// the honesty contract — byte identity may be claimed
    /// only with a pinned context recorded, and best-effort
    /// must carry its reason.
    pub fn validate(&self) -> GencondResult<()> {
        crate::schema::check_sha256(&self.conditioning_sha256, "conditioning sha256")?;
        crate::schema::check_sha256(&self.graph_sha256, "graph sha256")?;
        crate::schema::check_model_name(&self.backend)?;
        if self.execution_context.len() > SCENE_TEXT_BYTES_MAX {
            return refuse(CODE_RANGE, "execution context exceeds the byte bound");
        }
        match &self.rerender {
            RerenderGuarantee::PinnedContextByteIdentical => {
                if self.execution_context.is_empty() {
                    return refuse(
                        CODE_MALFORMED,
                        "a byte-identical re-render claim requires the pinned execution \
                         context to be recorded; an unpinned run is BestEffort with its reason",
                    );
                }
            }
            RerenderGuarantee::BestEffort { reason } => {
                if reason.is_empty() {
                    return refuse(
                        CODE_MALFORMED,
                        "a best-effort re-render guarantee must carry its reason",
                    );
                }
                if reason.len() > SCENE_TEXT_BYTES_MAX {
                    return refuse(CODE_RANGE, "re-render reason exceeds the byte bound");
                }
            }
        }
        Ok(())
    }
}

impl Puppet2dRef {
    /// Validates the reference: it must name lumen's rig
    /// format at the version this build references, and a
    /// well-formed content hash.
    pub fn validate(&self) -> GencondResult<()> {
        if self.format != LUMEN_RIG_MODEL_FORMAT {
            return refuse(
                CODE_UNKNOWN_KIND,
                format!(
                    "puppet2d block references format '{}'; the puppet lane references {} \
                     (D-835: lumen's format, consumed downstream)",
                    self.format, LUMEN_RIG_MODEL_FORMAT
                ),
            );
        }
        if self.format_version != LUMEN_RIG_MODEL_FORMAT_VERSION {
            return refuse(
                CODE_UNSUPPORTED_SCHEMA,
                format!(
                    "puppet2d block references {} version {}; this build references version {}",
                    LUMEN_RIG_MODEL_FORMAT, self.format_version, LUMEN_RIG_MODEL_FORMAT_VERSION
                ),
            );
        }
        crate::schema::check_sha256(&self.model_sha256, "rig model sha256")?;
        Ok(())
    }
}

impl FreezeProvenance {
    /// Validates one provenance entry: hash shapes and the
    /// contract-value discipline — a track freeze carries an
    /// epsilon and its origin and no tolerance; a palette
    /// freeze carries a tolerance and no epsilon.
    pub fn validate(&self) -> GencondResult<()> {
        crate::schema::check_sha256(&self.source_sha256, "freeze source sha256")?;
        crate::schema::check_sha256(&self.frozen_sha256, "frozen output sha256")?;
        match self.op {
            FreezeOp::FreezeTrack => {
                let Some(epsilon) = self.epsilon else {
                    return refuse(
                        CODE_MALFORMED,
                        "a track freeze's provenance carries its epsilon",
                    );
                };
                if !epsilon.is_finite() || epsilon < 0.0 {
                    return refuse(
                        CODE_RANGE,
                        "provenance epsilon must be finite and non-negative",
                    );
                }
                if self.epsilon_origin.is_none() {
                    return refuse(
                        CODE_MALFORMED,
                        "a track freeze's provenance carries its epsilon's origin",
                    );
                }
                if self.tolerance.is_some() {
                    return refuse(
                        CODE_CONFLICT,
                        "a track freeze's provenance carries no tolerance",
                    );
                }
            }
            FreezeOp::FreezePalette => {
                if self.tolerance.is_none() {
                    return refuse(
                        CODE_MALFORMED,
                        "a palette freeze's provenance carries its tolerance",
                    );
                }
                if self.tolerance.is_some_and(|tolerance| tolerance > 255) {
                    return refuse(CODE_RANGE, "provenance tolerance must lie in [0, 255]");
                }
                if self.epsilon.is_some() || self.epsilon_origin.is_some() {
                    return refuse(
                        CODE_CONFLICT,
                        "a palette freeze's provenance carries no epsilon",
                    );
                }
            }
        }
        Ok(())
    }
}

/// The conditioning record hash a recipe carries: SHA-256 of
/// the record's canonical JSON (the sorted-key compact form,
/// the graph identity rule applied to the record). This is
/// the function a session emitter uses to fill
/// `RenderRecipe.conditioning_sha256` mechanically, so the
/// hash is derived, never transcribed.
pub fn conditioning_sha256(record: &crate::schema::Conditioning) -> GencondResult<String> {
    crate::freeze::canonical_sha256(record)
}
