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
    /// Deformers, sorted by id.
    pub deformers: Vec<Deformer>,
    /// Expressions, sorted by id.
    pub expressions: Vec<Expression>,
    /// Glue pairs between drawables, sorted by id.
    pub glues: Vec<Glue>,
    /// Parameters, sorted by id.
    pub parameters: Vec<Parameter>,
    /// Parts (node tree; drawable when meshed), sorted by id.
    pub parts: Vec<Part>,
    /// The neutral pose: parameter values that differ from the
    /// declared defaults, sorted by parameter id.
    pub pose: Vec<PoseEntry>,
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
