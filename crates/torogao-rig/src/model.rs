//! The parsed INP1 puppet record: an arena of nodes in file
//! (pre-order) order, parameters in file order, and decoded textures.
//!
//! Indices into `Puppet::nodes` are the crate's node handles; a
//! parent's index always precedes its children's, which the evaluator
//! relies on for its single-pass transform update.

use crate::math::{Vec2, Vec3};
use crate::texture::Texture;

/// A node's base transform offset, as authored in the record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransformOffset {
    /// Translation (x, y, z).
    pub translation: Vec3,
    /// Euler rotation in radians (x, y, z), XYZ order.
    pub rotation: Vec3,
    /// Scale (x, y).
    pub scale: Vec2,
    /// Whether the record asks for pixel snapping. Carried for census
    /// fidelity; the reference render route does not snap.
    pub pixel_snap: bool,
}

impl Default for TransformOffset {
    fn default() -> Self {
        Self {
            translation: Vec3::new(0.0, 0.0, 0.0),
            rotation: Vec3::new(0.0, 0.0, 0.0),
            scale: Vec2::new(1.0, 1.0),
            pixel_snap: false,
        }
    }
}

impl TransformOffset {
    /// The relative matrix `T * R * S`.
    pub fn to_matrix(self) -> crate::math::Mat4 {
        crate::math::Mat4::from_translation(self.translation)
            .mul_mat(crate::math::Mat4::from_euler_xyz(self.rotation))
            .mul_mat(crate::math::Mat4::from_scale_xy(self.scale))
    }
}

/// How a part's fragments blend into the canvas. Names match the
/// INP1 payload strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendMode {
    /// `(ONE, ONE_MINUS_SRC_ALPHA)`, add.
    Normal,
    /// `(DST_COLOR, ONE_MINUS_SRC_ALPHA)`, add.
    Multiply,
    /// `(DST_COLOR, ONE)`, add.
    ColorDodge,
    /// `(ONE, ONE)`, add.
    LinearDodge,
    /// `(ONE, ONE_MINUS_SRC_COLOR)`, add.
    Screen,
    /// `(DST_ALPHA, ONE_MINUS_SRC_ALPHA)`, add.
    ClipToLower,
    /// `(ONE_MINUS_DST_ALPHA, ONE_MINUS_SRC_ALPHA)`, subtract.
    SliceFromLower,
}

impl BlendMode {
    /// Parse a payload blend-mode string; unknown names return
    /// `None` and the importer falls back to `Normal`, matching the
    /// reference payload parser.
    pub fn from_name(name: &str) -> Option<BlendMode> {
        Some(match name {
            "Normal" => BlendMode::Normal,
            "Multiply" => BlendMode::Multiply,
            "ColorDodge" => BlendMode::ColorDodge,
            "LinearDodge" => BlendMode::LinearDodge,
            "Screen" => BlendMode::Screen,
            "ClipToLower" => BlendMode::ClipToLower,
            "SliceFromLower" => BlendMode::SliceFromLower,
            _ => return None,
        })
    }
}

/// Whether a mask source adds or removes coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaskMode {
    /// Source coverage sets the mask value to 1.
    Mask,
    /// Source coverage sets the mask value to 0.
    DodgeMask,
}

/// One mask entry on a drawable.
#[derive(Debug, Clone, Copy)]
pub struct Mask {
    /// Arena index of the source node.
    pub source: usize,
    /// Add or remove coverage.
    pub mode: MaskMode,
}

/// A triangle mesh: positions and UVs per vertex, indices in
/// triples.
#[derive(Debug, Clone, Default)]
pub struct Mesh {
    /// Vertex positions in the node's local space.
    pub verts: Vec<Vec2>,
    /// Per-vertex UVs.
    pub uvs: Vec<Vec2>,
    /// Triangle indices (length is a multiple of 3).
    pub indices: Vec<u32>,
    /// The authored mesh origin. Carried for census fidelity; the
    /// reference render route never subtracts it (its `offset`
    /// uniform is unset), so evaluation does not apply it either.
    pub origin: Vec2,
}

/// Drawable fields shared by parts and composites.
#[derive(Debug, Clone)]
pub struct Drawable {
    /// Blend mode for this drawable.
    pub blend_mode: BlendMode,
    /// Multiply tint (rgb).
    pub tint: [f32; 3],
    /// Screen tint (rgb).
    pub screen_tint: [f32; 3],
    /// Base opacity; parameter `opacity` bindings multiply it.
    pub opacity: f32,
    /// Mask entries, in record order.
    pub masks: Vec<Mask>,
    /// Alpha threshold for mask sources (default 0.5).
    pub mask_threshold: f32,
}

impl Default for Drawable {
    fn default() -> Self {
        Self {
            blend_mode: BlendMode::Normal,
            tint: [1.0, 1.0, 1.0],
            screen_tint: [0.0, 0.0, 0.0],
            opacity: 1.0,
            masks: Vec::new(),
            mask_threshold: 0.5,
        }
    }
}

/// A textured mesh node.
#[derive(Debug, Clone)]
pub struct Part {
    /// The mesh.
    pub mesh: Mesh,
    /// Drawable fields.
    pub drawable: Drawable,
    /// Texture slots [albedo, emissive, bump]: indices into
    /// `Puppet::textures`, or `None`. Only albedo feeds the measured
    /// frame; the other slots are carried for census fidelity.
    pub texture_slots: [Option<usize>; 3],
}

/// A composite node: its children render into an offscreen buffer
/// that is then drawn with the composite's own drawable fields.
#[derive(Debug, Clone)]
pub struct Composite {
    /// Drawable fields.
    pub drawable: Drawable,
}

/// How a physics node's output maps onto its parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicsMapMode {
    /// `(angle / pi, relative length)`.
    AngleLength,
    /// `(relative length, angle / pi)`.
    LengthAngle,
    /// Local direction times relative length, minus (0, 1), y negated.
    Xy,
    /// The XY mapping with components swapped.
    Yx,
}

/// A simple-physics node (pendulum family).
#[derive(Debug, Clone, Copy)]
pub struct PhysicsNode {
    /// Index of the parameter this node drives.
    pub param: usize,
    /// Spring pendulum when true, rigid pendulum when false.
    pub spring: bool,
    /// Output mapping.
    pub map_mode: PhysicsMapMode,
    /// Gravity scale for this node.
    pub gravity: f32,
    /// Pendulum length in model units.
    pub length: f32,
    /// Spring frequency in Hz (spring model only).
    pub frequency: f32,
    /// Angular damping coefficient.
    pub angle_damping: f32,
    /// Length damping coefficient (spring model only).
    pub length_damping: f32,
    /// Output scale, applied component-wise after mapping.
    pub output_scale: Vec2,
    /// When true, the anchor and output are in parent-local space.
    pub local_only: bool,
}

/// The payload of one arena node.
#[derive(Debug, Clone)]
pub enum NodeKind {
    /// A plain transform group (payload type `Node`).
    Group,
    /// Payload type `MeshGroup`: the reference route evaluates it as
    /// a plain group (its group mesh is not applied), and so does
    /// this evaluator; the census counts it separately.
    MeshGroup,
    /// A textured mesh (payload type `Part`).
    Part(Part),
    /// A composite (payload type `Composite`).
    Composite(Composite),
    /// A physics driver (payload type `SimplePhysics`).
    Physics(PhysicsNode),
}

/// One node of the puppet tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// Record uuid.
    pub uuid: u32,
    /// Record name.
    pub name: String,
    /// Authored enabled flag. Carried for fidelity; the reference
    /// evaluation never consults it, and neither does this one.
    pub enabled: bool,
    /// Base z-sort; evaluation accumulates it down the tree.
    pub zsort: f32,
    /// Base transform offset.
    pub offset: TransformOffset,
    /// When true, this node's transform base is the root's absolute
    /// transform instead of its parent's.
    pub lock_to_root: bool,
    /// Parent arena index; `None` for the root.
    pub parent: Option<usize>,
    /// Child arena indices, in file order.
    pub children: Vec<usize>,
    /// Kind payload.
    pub kind: NodeKind,
}

impl Node {
    /// The drawable fields, for parts and composites.
    pub fn drawable(&self) -> Option<&Drawable> {
        match &self.kind {
            NodeKind::Part(part) => Some(&part.drawable),
            NodeKind::Composite(comp) => Some(&comp.drawable),
            _ => None,
        }
    }

    /// True for parts and composites.
    pub fn is_drawable(&self) -> bool {
        self.drawable().is_some()
    }
}

/// How a binding interpolates between grid points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpolateMode {
    /// Bilinear interpolation.
    Linear,
    /// Nearest grid point (ties resolve to the lower point).
    Nearest,
}

/// Which node property a binding drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingKind {
    /// `zSort` (scalar, added).
    ZSort,
    /// `transform.t` (vector, added).
    Translate,
    /// `transform.t.x` (scalar, added).
    TranslateX,
    /// `transform.t.y` (scalar, added).
    TranslateY,
    /// `transform.s` (vector, multiplied).
    Scale,
    /// `transform.s.x` (scalar, multiplied).
    ScaleX,
    /// `transform.s.y` (scalar, multiplied).
    ScaleY,
    /// `transform.r.x` (scalar, added).
    RotateX,
    /// `transform.r.y` (scalar, added).
    RotateY,
    /// `transform.r.z` (scalar, added).
    RotateZ,
    /// `deform` (per-vertex vector offsets, accumulated).
    Deform,
    /// `opacity` (scalar, multiplied into the drawable opacity).
    Opacity,
}

impl BindingKind {
    /// The payload `param_name` for this kind.
    pub fn from_param_name(name: &str) -> Option<BindingKind> {
        Some(match name {
            "zSort" => BindingKind::ZSort,
            "transform.t" => BindingKind::Translate,
            "transform.t.x" => BindingKind::TranslateX,
            "transform.t.y" => BindingKind::TranslateY,
            "transform.s" => BindingKind::Scale,
            "transform.s.x" => BindingKind::ScaleX,
            "transform.s.y" => BindingKind::ScaleY,
            "transform.r.x" => BindingKind::RotateX,
            "transform.r.y" => BindingKind::RotateY,
            "transform.r.z" => BindingKind::RotateZ,
            "deform" => BindingKind::Deform,
            "opacity" => BindingKind::Opacity,
            _ => return None,
        })
    }

    /// True when grid cells are 2D vectors (translate/scale/deform).
    pub fn is_vector(self) -> bool {
        matches!(
            self,
            BindingKind::Translate | BindingKind::Scale | BindingKind::Deform
        )
    }
}

/// A binding's value grid, indexed `[x_point][y_point]`.
#[derive(Debug, Clone)]
pub enum BindingValues {
    /// Scalar cells.
    Scalars(Vec<Vec<f32>>),
    /// Vector cells (translate / scale bindings).
    Vectors(Vec<Vec<Vec2>>),
    /// Deform cells: one offset per mesh vertex of the target part.
    Deforms(Vec<Vec<Vec<Vec2>>>),
}

/// One parameter-to-node binding.
#[derive(Debug, Clone)]
pub struct Binding {
    /// Target node arena index.
    pub node: usize,
    /// What the binding drives.
    pub kind: BindingKind,
    /// Interpolation between grid points.
    pub interpolate: InterpolateMode,
    /// The value grid.
    pub values: BindingValues,
    /// The record's `isSet` grid. The reference apply path never
    /// consults it; it is carried so a census can report it.
    pub is_set: Vec<Vec<bool>>,
}

/// One puppet parameter.
#[derive(Debug, Clone)]
pub struct Param {
    /// Record uuid.
    pub uuid: u32,
    /// Record name; drivers set parameters by this name.
    pub name: String,
    /// True when the parameter is 2D.
    pub is_vec2: bool,
    /// Range minimum (per component).
    pub min: Vec2,
    /// Range maximum (per component).
    pub max: Vec2,
    /// Rest value; every frame starts from it.
    pub defaults: Vec2,
    /// Normalized axis points per component: `axis_points[0]` for x,
    /// `axis_points[1]` for y. Values are in normalized [0, 1] space
    /// across [min, max] (the estate record convention, LRP-304).
    pub axis_points: [Vec<f32>; 2],
    /// Bindings, in file order.
    pub bindings: Vec<Binding>,
}

/// A fully parsed puppet record.
#[derive(Debug, Clone)]
pub struct Puppet {
    /// Payload meta name, when present.
    pub name: Option<String>,
    /// Payload meta version string.
    pub version: String,
    /// Physics global: model units per meter.
    pub pixels_per_meter: f32,
    /// Physics global: gravity scale.
    pub gravity: f32,
    /// Node arena in file (pre-order) order; index 0 is the root.
    pub nodes: Vec<Node>,
    /// Parameters in file order.
    pub params: Vec<Param>,
    /// Decoded textures (RGBA8, rows top-down).
    pub textures: Vec<Texture>,
    /// Vendor extension names from `EXT_SECT`, in file order.
    pub extensions: Vec<String>,
    /// Import accounting: texture slots beyond albedo that carried
    /// the null marker (u32::MAX) and were dropped (albedo is the
    /// only slot the evaluator samples).
    pub dropped_texture_slots: usize,
    /// Import accounting: bindings dropped because their
    /// interpolation mode is not Linear/Nearest (the reference
    /// payload parser drops the same bindings; Midori carries
    /// cubic-interpolated ones).
    pub dropped_unsupported_bindings: usize,
    /// Import accounting: bindings dropped for an unknown
    /// `param_name` or an unknown target node.
    pub skipped_bindings: usize,
    /// Import accounting: RLE packets in TGA textures that crossed a
    /// scanline boundary (decoded with the reference reader's
    /// line-buffered semantics either way).
    pub tga_rle_line_crossings: usize,
}

impl Puppet {
    /// Arena index of a parameter by name.
    pub fn param_index(&self, name: &str) -> Option<usize> {
        self.params.iter().position(|p| p.name == name)
    }

    /// Arena index of a node by name (first match).
    pub fn node_index(&self, name: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.name == name)
    }

    /// Structural census counts for gate reports.
    pub fn census(&self) -> Census {
        let mut census = Census {
            textures: self.textures.len(),
            params: self.params.len(),
            vec2_params: self.params.iter().filter(|p| p.is_vec2).count(),
            bindings: self.params.iter().map(|p| p.bindings.len()).sum(),
            ..Census::default()
        };
        for node in &self.nodes {
            match &node.kind {
                NodeKind::Group => census.groups += 1,
                NodeKind::MeshGroup => census.mesh_groups += 1,
                NodeKind::Part(part) => {
                    census.parts += 1;
                    census.masks += part.drawable.masks.len();
                }
                NodeKind::Composite(comp) => {
                    census.composites += 1;
                    census.masks += comp.drawable.masks.len();
                }
                NodeKind::Physics(_) => census.physics += 1,
            }
        }
        census
    }
}

/// Structural counts of a puppet record.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Census {
    /// Plain group nodes.
    pub groups: usize,
    /// MeshGroup nodes.
    pub mesh_groups: usize,
    /// Part nodes.
    pub parts: usize,
    /// Composite nodes.
    pub composites: usize,
    /// SimplePhysics nodes.
    pub physics: usize,
    /// Mask entries across all drawables.
    pub masks: usize,
    /// Parameters.
    pub params: usize,
    /// Parameters with `is_vec2` set.
    pub vec2_params: usize,
    /// Bindings that survived import.
    pub bindings: usize,
    /// Textures.
    pub textures: usize,
}

impl Census {
    /// Total node count.
    pub fn nodes(&self) -> usize {
        self.groups + self.mesh_groups + self.parts + self.composites + self.physics
    }
}
