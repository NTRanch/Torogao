//! The verb manifest and the typed op inputs.
//!
//! `MANIFEST` is the brief's §Item 4 surface, all 47 verbs in
//! pipeline group order, alphabetical within each group, each
//! tagged with the slice that delivers it. This slice (A1)
//! implements the verbs tagged `A1`; every other verb is refused
//! by name (`TG-AUTH-UNKNOWN-KIND`), never silently ignored.
//!
//! Op inputs are JSON objects (the wire format). Each verb has
//! one input struct with `deny_unknown_fields`; the op log records
//! the normalized inputs (`Op::inputs_json`), so replay reads
//! back exactly what was applied.

use crate::model::{Consent, Point, Stamp};
use crate::refusal::{Refusal, RefusalClass, refuse};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The slice that delivers a verb.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Slice {
    /// Format-level authoring ops + CLI skeleton (this slice).
    A1,
    /// A1's group, but conditional on an RP2 ruling (AUTH-D3).
    A1Conditional,
    /// Engine evaluation + physics bake.
    A2,
    /// Motion authoring + render goldens.
    A3,
    /// Export (interfaces with the moc3 lane; AUTH-D4).
    Export,
}

impl Slice {
    /// The slice label used in listings.
    pub fn label(self) -> &'static str {
        match self {
            Slice::A1 => "A1",
            Slice::A1Conditional => "A1-conditional (AUTH-D3)",
            Slice::A2 => "A2",
            Slice::A3 => "A3",
            Slice::Export => "export (AUTH-D4 boundary)",
        }
    }
}

/// One manifest row: verb name, group, delivering slice, and
/// whether the verb mutates the project.
#[derive(Clone, Copy, Debug)]
pub struct VerbSpec {
    /// Canonical verb name.
    pub name: &'static str,
    /// Manifest group.
    pub group: &'static str,
    /// Delivering slice.
    pub slice: Slice,
    /// True when the verb changes `project.ron`.
    pub mutating: bool,
}

const fn verb(name: &'static str, group: &'static str, slice: Slice, mutating: bool) -> VerbSpec {
    VerbSpec {
        name,
        group,
        slice,
        mutating,
    }
}

/// The §Item 4 manifest: 47 verbs in 11 groups.
pub const MANIFEST: [VerbSpec; 47] = [
    verb("project_diff", "project", Slice::A1, false),
    verb("project_import", "project", Slice::A1, true),
    verb("project_init", "project", Slice::A1, true),
    verb("project_inspect", "project", Slice::A1, false),
    verb("project_replay", "project", Slice::A1, false),
    verb("project_validate", "project", Slice::A1, false),
    verb("intake_image", "art intake", Slice::A1, true),
    verb("intake_psd", "art intake", Slice::A1, true),
    verb("mesh_set", "parts & meshes", Slice::A1, true),
    verb("mesh_trim", "parts & meshes", Slice::A1, true),
    verb("part_art_bind", "parts & meshes", Slice::A1, true),
    verb("part_clip_set", "parts & meshes", Slice::A1, true),
    verb("part_create", "parts & meshes", Slice::A1, true),
    verb("part_draw_order_set", "parts & meshes", Slice::A1, true),
    verb("part_parent_set", "parts & meshes", Slice::A1, true),
    verb("part_remove", "parts & meshes", Slice::A1, true),
    verb("part_visibility_set", "parts & meshes", Slice::A1, true),
    verb("deformer_glue_set", "deformers", Slice::A1, true),
    verb("deformer_parent_set", "deformers", Slice::A1, true),
    verb("deformer_remove", "deformers", Slice::A1, true),
    verb("deformer_rotation_create", "deformers", Slice::A1, true),
    verb("deformer_warp_create", "deformers", Slice::A1, true),
    verb("keyform_set", "parameters", Slice::A1, true),
    verb("param_define", "parameters", Slice::A1, true),
    verb("param_grid_define", "parameters", Slice::A1, true),
    verb(
        "param_link_define",
        "parameters",
        Slice::A1Conditional,
        true,
    ),
    verb("param_pose_default_set", "parameters", Slice::A1, true),
    verb("param_remove", "parameters", Slice::A1, true),
    verb("expression_define", "expressions", Slice::A1, true),
    verb("expression_remove", "expressions", Slice::A1, true),
    verb("expression_test", "expressions", Slice::A1, false),
    verb("physics_bake", "physics", Slice::A2, true),
    verb("physics_forces_set", "physics", Slice::A2, true),
    verb("physics_group_define", "physics", Slice::A2, true),
    verb("physics_group_remove", "physics", Slice::A2, true),
    verb("curve_from_samples", "motion", Slice::A3, true),
    verb("curve_set", "motion", Slice::A3, true),
    verb("motion_bake", "motion", Slice::A3, true),
    verb("motion_create", "motion", Slice::A3, true),
    verb("audio_bind_set", "audio-reactive hooks", Slice::A3, true),
    verb(
        "audio_track_define",
        "audio-reactive hooks",
        Slice::A3,
        true,
    ),
    verb("render_range_sweep", "render", Slice::A3, false),
    verb("render_still", "render", Slice::A3, false),
    verb("render_take", "render", Slice::A3, false),
    verb("export_bundle", "export", Slice::Export, false),
    verb("export_manifest", "export", Slice::Export, false),
    verb("export_native", "export", Slice::Export, false),
];

/// The manifest row for a verb name.
pub fn verb_spec(name: &str) -> Option<&'static VerbSpec> {
    MANIFEST.iter().find(|spec| spec.name == name)
}

/// Refuses a manifest verb this slice does not implement, or a
/// name the manifest does not carry. A descoped verb refuses
/// with its recorded ruling and its capability's home.
pub fn refuse_unimplemented<T>(name: &str) -> Result<T, Refusal> {
    if let Some(descope) = descope(name) {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "verb '{name}' is descoped from the author surface by recorded ruling (G-10): {} \
                 The capability lives in {}.",
                descope.ruling, descope.home
            ),
        );
    }
    match verb_spec(name) {
        Some(spec) => refuse(
            RefusalClass::UnknownKind,
            format!(
                "verb '{name}' belongs to slice {}; it is not implemented in A1",
                spec.slice.label()
            ),
        ),
        None => refuse(
            RefusalClass::UnknownKind,
            format!("verb '{name}' is not in the authoring manifest"),
        ),
    }
}

/// The rig-manifest verbs the G-10 completion implements in
/// this crate (the A2 physics records, the A3 motion and audio
/// records, and the export group's manifest read), alphabetical.
/// Together with the A1 slice (30) these bring the author
/// surface to 39 of 47; the remaining eight are descoped by
/// recorded ruling ([`DESCOPED`]), so the census is 47/47
/// accounted — implemented here, or named with its capability's
/// home and the ruling that put it there.
pub const RIGX_IMPLEMENTED: &[&str] = &[
    "audio_bind_set",
    "audio_track_define",
    "curve_from_samples",
    "curve_set",
    "export_manifest",
    "motion_create",
    "physics_forces_set",
    "physics_group_define",
    "physics_group_remove",
];

/// A verb descoped from the author surface by recorded ruling:
/// what it would have done, where the capability actually
/// lives, and the ruling.
#[derive(Clone, Copy, Debug)]
pub struct Descope {
    /// The verb name.
    pub verb: &'static str,
    /// Where the capability lives instead.
    pub home: &'static str,
    /// The ruling that descoped it.
    pub ruling: &'static str,
}

/// The recorded descopes (G-10). A descoped verb is refused by
/// name with its ruling and its capability's home — never
/// silently, and never by pretending the act is an authored
/// record when it is an engine or file act.
pub const DESCOPED: &[Descope] = &[
    Descope {
        verb: "export_bundle",
        home: "the CLI export face (torogao-cli), over the records this crate authors",
        ruling: "bundle assembly writes a release layout — a file act, not an authored \
                 record; the author crate authors records and emits none of its own files \
                 beyond the project and its op log",
    },
    Descope {
        verb: "export_native",
        home: "torogao-format's INP writer, driven by the CLI export face",
        ruling: "INP emission is the format crate's writer; the author crate references \
                 torogao-format's records and must not grow a second writer (AUTH-D4's \
                 no-second-moc3-writer rule, applied to INP)",
    },
    Descope {
        verb: "motion_bake",
        home: "torogao-engine's per-tick evaluation (parameters + physics + audio bindings)",
        ruling: "a bake is an evaluation act over authored records, not an authored \
                 record; torogao-author sits below the engine in the dependency order \
                 (AUTH-D2) and must not grow an evaluator",
    },
    Descope {
        verb: "param_link_define",
        home: "no home: the construct it would author does not exist in the format",
        ruling: "AUTH-D3: the verb exists iff RP2 adopts ParameterParameterBinding; \
                 torogao-format carries no such construct (censused 2026-11-01), so the \
                 brief's own condition is unmet and the verb is descoped until the \
                 construct is adopted",
    },
    Descope {
        verb: "physics_bake",
        home: "torogao-engine's fixed-step physics integrator",
        ruling: "a bake is an evaluation act over authored records, not an authored \
                 record; torogao-author sits below the engine in the dependency order \
                 (AUTH-D2) and must not grow an integrator",
    },
    Descope {
        verb: "render_range_sweep",
        home: "the CLI render tools over torogao-engine's rasterizer",
        ruling: "rendering is an engine act producing golden images; the author crate \
                 holds no renderer and its answers are documents, not images",
    },
    Descope {
        verb: "render_still",
        home: "the CLI render tools over torogao-engine's rasterizer",
        ruling: "rendering is an engine act producing golden images; the author crate \
                 holds no renderer and its answers are documents, not images",
    },
    Descope {
        verb: "render_take",
        home: "the CLI render/video tools over torogao-engine's rasterizer",
        ruling: "rendering is an engine act producing frame sequences; the author crate \
                 holds no renderer and its answers are documents, not images",
    },
];

/// The descope record for a verb, if it is descoped.
pub fn descope(name: &str) -> Option<&'static Descope> {
    DESCOPED.iter().find(|entry| entry.verb == name)
}

// ---- input records ----------------------------------------------

/// `project_init` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitInput {
    /// Project name (id syntax).
    pub name: String,
    /// Lane stamp.
    pub stamp: Stamp,
    /// Consent record.
    pub consent: Consent,
}

/// `project_import` inputs: a native rig-model file plus the
/// provenance its textures need.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ImportInput {
    /// Path to the `.model.json` file (normalized to canonical).
    pub path: String,
    /// License statement for the textures.
    pub license: String,
    /// Origin statement for the textures.
    pub origin: String,
    /// sha256 of the model file; filled at apply time, checked
    /// when given.
    #[serde(default)]
    pub source_sha256: Option<String>,
    /// The model's textures as resolved (canonical path + sha256),
    /// in model order; filled at apply time, checked when given.
    /// Replay reads texture bytes from the asset store by these
    /// hashes, so the original files need not survive.
    #[serde(default)]
    pub textures: Option<Vec<TextureSource>>,
}

/// One resolved texture source of an imported model.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextureSource {
    /// Canonical path.
    pub path: String,
    /// sha256 of the PNG bytes.
    pub sha256: String,
}

/// `intake_image` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeImageInput {
    /// Asset id.
    pub id: String,
    /// Source PNG path (normalized to canonical).
    pub path: String,
    /// License statement.
    pub license: String,
    /// Origin statement.
    pub origin: String,
    /// sha256 of the source; filled at apply time, checked when
    /// given.
    #[serde(default)]
    pub source_sha256: Option<String>,
}

/// Completion-painting declaration, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionInput {
    /// Completion-painted.
    Painted,
    /// No completion painting needed.
    NotNeeded,
}

/// One PSD layer declaration under the separation standard.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LayerDecl {
    /// Layer path: group names and the layer name joined by `/`.
    pub layer: String,
    /// The part this layer becomes.
    pub part: String,
    /// Completion declaration (pixel layers only).
    #[serde(default)]
    pub completion: Option<CompletionInput>,
    /// The clip base part (required exactly when the PSD layer is
    /// clipped).
    #[serde(default)]
    pub clip_base: Option<String>,
    /// Variant set this layer belongs to.
    #[serde(default)]
    pub variant_set: Option<String>,
}

/// `intake_psd` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IntakePsdInput {
    /// Source PSD path (normalized to canonical).
    pub path: String,
    /// License statement.
    pub license: String,
    /// Origin statement.
    pub origin: String,
    /// sha256 of the source; filled at apply time, checked when
    /// given.
    #[serde(default)]
    pub source_sha256: Option<String>,
    /// One declaration per PSD layer, groups included.
    pub layers: Vec<LayerDecl>,
}

/// An inputs record carrying one id.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct IdInput {
    /// The id.
    pub id: String,
}

/// An inputs record naming one part.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PartInput {
    /// The part id.
    pub part: String,
}

/// `mesh_set` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MeshSetInput {
    /// The part id.
    pub part: String,
    /// Vertex positions.
    pub vertices: Vec<Point>,
    /// UVs, one per vertex.
    pub uvs: Vec<Point>,
    /// Triangle indices.
    pub indices: Vec<u32>,
}

/// A rectangle, as input.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RectInput {
    /// Left, pixels.
    pub x: u32,
    /// Top, pixels.
    pub y: u32,
    /// Width, pixels.
    pub width: u32,
    /// Height, pixels.
    pub height: u32,
}

/// `part_art_bind` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtBindInput {
    /// The part id.
    pub part: String,
    /// The asset id.
    pub asset: String,
    /// UV region within the asset.
    pub region: RectInput,
}

/// `part_clip_set` inputs (`source: null` clears the mask).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClipInput {
    /// The part id.
    pub part: String,
    /// The masking part, or null.
    pub source: Option<String>,
    /// Inverted mask.
    #[serde(default)]
    pub inverted: bool,
}

/// `part_draw_order_set` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DrawInput {
    /// The part id.
    pub part: String,
    /// Band, 0..=99.
    pub band: u32,
    /// Index within the band, 0..=99.
    pub index: u32,
    /// Optional render group.
    #[serde(default)]
    pub group: Option<String>,
}

/// `part_parent_set` inputs (`parent: null` makes a root).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PartParentInput {
    /// The part id.
    pub part: String,
    /// The parent part, or null.
    pub parent: Option<String>,
}

/// `part_visibility_set` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisibilityInput {
    /// The part id.
    pub part: String,
    /// Default visibility.
    pub visible: bool,
}

/// `deformer_glue_set` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GlueInput {
    /// Glue id.
    pub id: String,
    /// First part.
    pub a: String,
    /// Second part.
    pub b: String,
    /// Vertex index pairs.
    pub pairs: Vec<[u32; 2]>,
    /// Strength, 0..=1.
    pub weight: f64,
}

/// `deformer_parent_set` inputs: target is a deformer (deformer
/// parenting) or a part (the deformer that deforms its mesh).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeformerParentInput {
    /// Deformer or part id.
    pub target: String,
    /// Parent deformer, or null.
    pub parent: Option<String>,
}

/// `deformer_remove` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeformerInput {
    /// The deformer id.
    pub deformer: String,
}

/// `deformer_rotation_create` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RotationCreateInput {
    /// Deformer id.
    pub id: String,
    /// Pivot.
    pub origin: Point,
    /// Rest angle, degrees.
    #[serde(default)]
    pub base_angle_degrees: f64,
}

/// `deformer_warp_create` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WarpCreateInput {
    /// Deformer id.
    pub id: String,
    /// Grid columns, 1..=32.
    pub columns: u32,
    /// Grid rows, 1..=32.
    pub rows: u32,
    /// Rest rectangle `[x0, y0, x1, y1]`.
    pub bounds: [f64; 4],
}

/// One lattice coordinate.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Coordinate {
    /// Axis parameter.
    pub parameter: String,
    /// Value on that axis.
    pub value: f64,
}

/// A keyform payload, as input (externally tagged by kind).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum FormInput {
    /// Mesh vertex positions.
    Mesh {
        /// One per mesh vertex.
        vertices: Vec<Point>,
    },
    /// Rotation transform.
    Rotation {
        /// Angle, degrees.
        angle_degrees: f64,
        /// Scale per axis.
        scale: Point,
    },
    /// Warp control points.
    Warp {
        /// One per control point.
        positions: Vec<Point>,
    },
}

/// `keyform_set` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct KeyformSetInput {
    /// Part (mesh) or deformer id.
    pub target: String,
    /// One coordinate, or two for a declared grid (in grid order).
    pub at: Vec<Coordinate>,
    /// The payload.
    pub form: FormInput,
}

/// `param_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParamDefineInput {
    /// Parameter id.
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

/// `param_grid_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GridInput {
    /// Part (mesh) or deformer id.
    pub target: String,
    /// The two axis parameters, in grid order.
    pub axes: [String; 2],
}

/// `param_pose_default_set` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PoseInput {
    /// The parameter.
    pub parameter: String,
    /// Its neutral value.
    pub value: f64,
}

/// `param_remove` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParameterInput {
    /// The parameter id.
    pub parameter: String,
}

/// Expression blend, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendInput {
    /// Add to the pose.
    Additive,
    /// Replace the pose.
    Override,
}

/// One expression visibility row, as input.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisibilityRow {
    /// The part.
    pub part: String,
    /// Visibility while active.
    pub visible: bool,
}

/// `expression_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExpressionDefineInput {
    /// Expression id.
    pub id: String,
    /// Blend mode.
    pub blend: BlendInput,
    /// Parameter rows.
    #[serde(default)]
    pub values: Vec<Coordinate>,
    /// Visibility rows.
    #[serde(default)]
    pub visibility: Vec<VisibilityRow>,
}

/// A mutating op: one verb with its typed inputs.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `audio_bind_set` (A3).
    AudioBindSet(crate::audio::AudioBindSetInput),
    /// `audio_track_define` (A3).
    AudioTrackDefine(crate::audio::AudioTrackDefineInput),
    /// `curve_from_samples` (A3).
    CurveFromSamples(crate::motion::CurveFromSamplesInput),
    /// `curve_set` (A3).
    CurveSet(crate::motion::CurveSetInput),
    /// `deformer_glue_set`.
    DeformerGlueSet(GlueInput),
    /// `deformer_parent_set`.
    DeformerParentSet(DeformerParentInput),
    /// `deformer_remove`.
    DeformerRemove(DeformerInput),
    /// `deformer_rotation_create`.
    DeformerRotationCreate(RotationCreateInput),
    /// `deformer_warp_create`.
    DeformerWarpCreate(WarpCreateInput),
    /// `expression_define`.
    ExpressionDefine(ExpressionDefineInput),
    /// `expression_remove`.
    ExpressionRemove(IdInput),
    /// An AUTH-G game verb (see `game::op::GameOp`).
    Game(crate::game::op::GameOp),
    /// `intake_image`.
    IntakeImage(IntakeImageInput),
    /// `intake_psd`.
    IntakePsd(IntakePsdInput),
    /// `keyform_set`.
    KeyformSet(KeyformSetInput),
    /// `mesh_set`.
    MeshSet(MeshSetInput),
    /// `mesh_trim`.
    MeshTrim(PartInput),
    /// `motion_create` (A3).
    MotionCreate(crate::motion::MotionCreateInput),
    /// `param_define`.
    ParamDefine(ParamDefineInput),
    /// `param_grid_define`.
    ParamGridDefine(GridInput),
    /// `param_pose_default_set`.
    ParamPoseDefaultSet(PoseInput),
    /// `param_remove`.
    ParamRemove(ParameterInput),
    /// `part_art_bind`.
    PartArtBind(ArtBindInput),
    /// `part_clip_set`.
    PartClipSet(ClipInput),
    /// `part_create`.
    PartCreate(IdInput),
    /// `part_draw_order_set`.
    PartDrawOrderSet(DrawInput),
    /// `part_parent_set`.
    PartParentSet(PartParentInput),
    /// `part_remove`.
    PartRemove(PartInput),
    /// `part_visibility_set`.
    PartVisibilitySet(VisibilityInput),
    /// `physics_forces_set` (A2).
    PhysicsForcesSet(crate::physics::PhysicsForcesSetInput),
    /// `physics_group_define` (A2).
    PhysicsGroupDefine(crate::physics::PhysicsGroupDefineInput),
    /// `physics_group_remove` (A2).
    PhysicsGroupRemove(IdInput),
    /// `project_import`.
    ProjectImport(ImportInput),
    /// `project_init`.
    ProjectInit(InitInput),
}

fn typed<T: for<'de> Deserialize<'de>>(verb: &str, inputs: &Value) -> Result<T, Refusal> {
    serde_json::from_value(inputs.clone()).or_else(|err| {
        refuse(
            RefusalClass::Malformed,
            format!("inputs for '{verb}' do not match its schema: {err}"),
        )
    })
}

impl Op {
    /// Parses a mutating verb and its JSON inputs. Read-only and
    /// non-A1 verbs are refused by name.
    pub fn parse(verb: &str, inputs: &Value) -> Result<Op, Refusal> {
        if crate::game::op::game_verb_spec(verb).is_some() {
            return crate::game::op::parse_game_op(verb, inputs);
        }
        let op = match verb {
            "audio_bind_set" => Op::AudioBindSet(typed(verb, inputs)?),
            "audio_track_define" => Op::AudioTrackDefine(typed(verb, inputs)?),
            "curve_from_samples" => Op::CurveFromSamples(typed(verb, inputs)?),
            "curve_set" => Op::CurveSet(typed(verb, inputs)?),
            "deformer_glue_set" => Op::DeformerGlueSet(typed(verb, inputs)?),
            "deformer_parent_set" => Op::DeformerParentSet(typed(verb, inputs)?),
            "deformer_remove" => Op::DeformerRemove(typed(verb, inputs)?),
            "deformer_rotation_create" => Op::DeformerRotationCreate(typed(verb, inputs)?),
            "deformer_warp_create" => Op::DeformerWarpCreate(typed(verb, inputs)?),
            "expression_define" => Op::ExpressionDefine(typed(verb, inputs)?),
            "expression_remove" => Op::ExpressionRemove(typed(verb, inputs)?),
            "intake_image" => Op::IntakeImage(typed(verb, inputs)?),
            "intake_psd" => Op::IntakePsd(typed(verb, inputs)?),
            "keyform_set" => Op::KeyformSet(typed(verb, inputs)?),
            "mesh_set" => Op::MeshSet(typed(verb, inputs)?),
            "mesh_trim" => Op::MeshTrim(typed(verb, inputs)?),
            "motion_create" => Op::MotionCreate(typed(verb, inputs)?),
            "param_define" => Op::ParamDefine(typed(verb, inputs)?),
            "param_grid_define" => Op::ParamGridDefine(typed(verb, inputs)?),
            "param_pose_default_set" => Op::ParamPoseDefaultSet(typed(verb, inputs)?),
            "param_remove" => Op::ParamRemove(typed(verb, inputs)?),
            "part_art_bind" => Op::PartArtBind(typed(verb, inputs)?),
            "part_clip_set" => Op::PartClipSet(typed(verb, inputs)?),
            "part_create" => Op::PartCreate(typed(verb, inputs)?),
            "part_draw_order_set" => Op::PartDrawOrderSet(typed(verb, inputs)?),
            "part_parent_set" => Op::PartParentSet(typed(verb, inputs)?),
            "part_remove" => Op::PartRemove(typed(verb, inputs)?),
            "part_visibility_set" => Op::PartVisibilitySet(typed(verb, inputs)?),
            "physics_forces_set" => Op::PhysicsForcesSet(typed(verb, inputs)?),
            "physics_group_define" => Op::PhysicsGroupDefine(typed(verb, inputs)?),
            "physics_group_remove" => Op::PhysicsGroupRemove(typed(verb, inputs)?),
            "project_import" => Op::ProjectImport(typed(verb, inputs)?),
            "project_init" => Op::ProjectInit(typed(verb, inputs)?),
            other => return refuse_unimplemented(other),
        };
        Ok(op)
    }

    /// The canonical verb name.
    pub fn verb(&self) -> &'static str {
        match self {
            Op::AudioBindSet(_) => "audio_bind_set",
            Op::AudioTrackDefine(_) => "audio_track_define",
            Op::CurveFromSamples(_) => "curve_from_samples",
            Op::CurveSet(_) => "curve_set",
            Op::DeformerGlueSet(_) => "deformer_glue_set",
            Op::DeformerParentSet(_) => "deformer_parent_set",
            Op::DeformerRemove(_) => "deformer_remove",
            Op::DeformerRotationCreate(_) => "deformer_rotation_create",
            Op::DeformerWarpCreate(_) => "deformer_warp_create",
            Op::ExpressionDefine(_) => "expression_define",
            Op::ExpressionRemove(_) => "expression_remove",
            Op::Game(game_op) => game_op.verb(),
            Op::IntakeImage(_) => "intake_image",
            Op::IntakePsd(_) => "intake_psd",
            Op::KeyformSet(_) => "keyform_set",
            Op::MeshSet(_) => "mesh_set",
            Op::MeshTrim(_) => "mesh_trim",
            Op::MotionCreate(_) => "motion_create",
            Op::ParamDefine(_) => "param_define",
            Op::ParamGridDefine(_) => "param_grid_define",
            Op::ParamPoseDefaultSet(_) => "param_pose_default_set",
            Op::ParamRemove(_) => "param_remove",
            Op::PartArtBind(_) => "part_art_bind",
            Op::PartClipSet(_) => "part_clip_set",
            Op::PartCreate(_) => "part_create",
            Op::PartDrawOrderSet(_) => "part_draw_order_set",
            Op::PartParentSet(_) => "part_parent_set",
            Op::PartRemove(_) => "part_remove",
            Op::PartVisibilitySet(_) => "part_visibility_set",
            Op::PhysicsForcesSet(_) => "physics_forces_set",
            Op::PhysicsGroupDefine(_) => "physics_group_define",
            Op::PhysicsGroupRemove(_) => "physics_group_remove",
            Op::ProjectImport(_) => "project_import",
            Op::ProjectInit(_) => "project_init",
        }
    }

    /// The normalized inputs as JSON (what the op log records).
    pub fn inputs_json(&self) -> Result<Value, Refusal> {
        if let Op::Game(game_op) = self {
            return game_op.inputs_json();
        }
        let result = match self {
            Op::Game(_) => unreachable!("handled above"),
            Op::AudioBindSet(inputs) => serde_json::to_value(inputs),
            Op::AudioTrackDefine(inputs) => serde_json::to_value(inputs),
            Op::CurveFromSamples(inputs) => serde_json::to_value(inputs),
            Op::CurveSet(inputs) => serde_json::to_value(inputs),
            Op::DeformerGlueSet(inputs) => serde_json::to_value(inputs),
            Op::DeformerParentSet(inputs) => serde_json::to_value(inputs),
            Op::DeformerRemove(inputs) => serde_json::to_value(inputs),
            Op::DeformerRotationCreate(inputs) => serde_json::to_value(inputs),
            Op::DeformerWarpCreate(inputs) => serde_json::to_value(inputs),
            Op::ExpressionDefine(inputs) => serde_json::to_value(inputs),
            Op::ExpressionRemove(inputs) => serde_json::to_value(inputs),
            Op::IntakeImage(inputs) => serde_json::to_value(inputs),
            Op::IntakePsd(inputs) => serde_json::to_value(inputs),
            Op::KeyformSet(inputs) => serde_json::to_value(inputs),
            Op::MeshSet(inputs) => serde_json::to_value(inputs),
            Op::MeshTrim(inputs) => serde_json::to_value(inputs),
            Op::MotionCreate(inputs) => serde_json::to_value(inputs),
            Op::ParamDefine(inputs) => serde_json::to_value(inputs),
            Op::ParamGridDefine(inputs) => serde_json::to_value(inputs),
            Op::ParamPoseDefaultSet(inputs) => serde_json::to_value(inputs),
            Op::ParamRemove(inputs) => serde_json::to_value(inputs),
            Op::PartArtBind(inputs) => serde_json::to_value(inputs),
            Op::PartClipSet(inputs) => serde_json::to_value(inputs),
            Op::PartCreate(inputs) => serde_json::to_value(inputs),
            Op::PartDrawOrderSet(inputs) => serde_json::to_value(inputs),
            Op::PartParentSet(inputs) => serde_json::to_value(inputs),
            Op::PartRemove(inputs) => serde_json::to_value(inputs),
            Op::PartVisibilitySet(inputs) => serde_json::to_value(inputs),
            Op::PhysicsForcesSet(inputs) => serde_json::to_value(inputs),
            Op::PhysicsGroupDefine(inputs) => serde_json::to_value(inputs),
            Op::PhysicsGroupRemove(inputs) => serde_json::to_value(inputs),
            Op::ProjectImport(inputs) => serde_json::to_value(inputs),
            Op::ProjectInit(inputs) => serde_json::to_value(inputs),
        };
        result.or_else(|err| {
            refuse(
                RefusalClass::Malformed,
                format!("inputs for '{}' cannot be recorded: {err}", self.verb()),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MANIFEST, Op, Slice};
    use serde_json::json;

    #[test]
    fn manifest_has_47_verbs_in_11_groups_alphabetical_within_group() {
        assert_eq!(MANIFEST.len(), 47);
        let mut groups: Vec<&str> = Vec::new();
        for spec in MANIFEST {
            if groups.last() != Some(&spec.group) {
                assert!(!groups.contains(&spec.group), "group {} split", spec.group);
                groups.push(spec.group);
            }
        }
        assert_eq!(groups.len(), 11);
        for pair in MANIFEST.windows(2) {
            if pair[0].group == pair[1].group {
                assert!(
                    pair[0].name < pair[1].name,
                    "{} !< {}",
                    pair[0].name,
                    pair[1].name
                );
            }
        }
        let a1 = MANIFEST
            .iter()
            .filter(|spec| spec.slice == Slice::A1)
            .count();
        assert_eq!(a1, 30);
    }

    #[test]
    fn census_is_47_accounted_implemented_or_descoped() {
        use super::{DESCOPED, RIGX_IMPLEMENTED, descope, verb_spec};
        // Every RIGX verb is a real manifest verb, not A1, and
        // not descoped; every descoped verb is a real manifest
        // verb, not A1, and not implemented.
        for name in RIGX_IMPLEMENTED {
            let spec = verb_spec(name).expect("rigx verb in manifest");
            assert_ne!(spec.slice, Slice::A1, "{name}");
            assert!(descope(name).is_none(), "{name} both ways");
        }
        for entry in DESCOPED {
            let spec = verb_spec(entry.verb).expect("descoped verb in manifest");
            assert_ne!(spec.slice, Slice::A1, "{}", entry.verb);
            assert!(!RIGX_IMPLEMENTED.contains(&entry.verb), "{}", entry.verb);
        }
        // The three sets partition the manifest: 30 + 9 + 8 = 47.
        let a1 = MANIFEST
            .iter()
            .filter(|spec| spec.slice == Slice::A1)
            .count();
        assert_eq!(a1 + RIGX_IMPLEMENTED.len() + DESCOPED.len(), MANIFEST.len());
        for spec in MANIFEST {
            let accounted = spec.slice == Slice::A1
                || RIGX_IMPLEMENTED.contains(&spec.name)
                || descope(spec.name).is_some();
            assert!(accounted, "{} unaccounted", spec.name);
        }
        // Descope refusals name the ruling, not a slice.
        let err = Op::parse("render_still", &json!({})).expect_err("x");
        assert!(
            err.message.contains("descoped"),
            "descope refusal: {}",
            err.message
        );
    }

    #[test]
    fn unknown_fields_and_unimplemented_verbs_refuse_typed() {
        let err = Op::parse("part_create", &json!({"id": "a", "force": true})).expect_err("x");
        assert_eq!(err.code(), "TG-AUTH-MALFORMED");
        let err = Op::parse("param_link_define", &json!({})).expect_err("x");
        assert_eq!(err.code(), "TG-AUTH-UNKNOWN-KIND");
        let err = Op::parse("physics_bake", &json!({})).expect_err("x");
        assert_eq!(err.code(), "TG-AUTH-UNKNOWN-KIND");
        let err = Op::parse("no_such_verb", &json!({})).expect_err("x");
        assert_eq!(err.code(), "TG-AUTH-UNKNOWN-KIND");
    }
}
