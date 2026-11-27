//! Scene-spec compiler for splat layers (DESIGN.md
//! §2.3, splat programme Rung 1).
//!
//! A scene spec names a splat layer:
//!
//! ```json
//! { "kind": "splat",
//!   "asset": { "path": "…/room.ply", "sha256": "…" },
//!   "transform": { "translation": [0, 0, 0],
//!                  "rotation": [1, 0, 0, 0],
//!                  "scale": 1.0 },
//!   "axis": "gltf" }
//! ```
//!
//! This module is the compiler path from that entry to
//! the renderer's packet: the asset bytes are read, the
//! SHA-256 is verified against the spec (a mismatch does
//! not compile — the asset is a frozen input, like every
//! other spec input), the cloud is loaded through the
//! strict `.ply` loader, the axis map is applied as a
//! change of basis, the layer transform is applied, and
//! [`pack_splats`] produces the `PackedSplats` the
//! caller places into `PackedScene::splats`.
//!
//! The spec document this compiler reads is the scene
//! spec's `layers` array; other top-level fields belong
//! to other compilers and are ignored here, so a full
//! scene spec carrying a `layers` array compiles through
//! this path unchanged.
//!
//! Scope at this rung, stated plainly:
//!
//! - **One splat layer.** The boundary packet carries
//!   one layer; a spec naming two fails compilation
//!   rather than silently dropping one.
//! - **Full TRS** (programme Rung 3). The transform's
//!   `rotation` field (glTF quaternion order in the
//!   spec) compiles: centres, covariances and the SH
//!   field all rotate by the same R (Wigner-D bands
//!   1-3, `crate::splat_rotate`). An identity rotation
//!   takes the unrotated path and reproduces the
//!   Rung 1 packet bit-identically.
//! - **Axis maps** are changes of basis applied to
//!   centres and covariances, re-expressed through
//!   the same rotation machinery (Rung 3): their SH
//!   fields rotate too, closing the Rung 1 colour
//!   caveat — an axis-mapped cloud's view-dependent
//!   colour is no longer exact only at band 0. The
//!   maps themselves (which way is up in the asset)
//!   are exact.
//!
//! Everything here is a pure function of (spec, asset
//! bytes, scene camera): the determinism contract of
//! DESIGN.md §3 is inherited from `pack_splats`, and the
//! unit gates below prove the compiler's packet equals
//! the direct packet for the same inputs.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::compare::sha256_hex;
use crate::error::RenderError;
use crate::math::Mat4;
use crate::setscene::ArcScene;
use crate::splat::{PackedSplats, Splat, SplatCloud, SplatTransform, pack_splats};

fn spec_err(detail: impl Into<String>) -> RenderError {
    RenderError::Document(format!("splat spec: {}", detail.into()))
}

/// The axis convention an asset was captured/trained
/// in, declared in the spec (DESIGN.md §2.1: no
/// guessing; absence of the field means the asset is
/// already glTF, the one sanctioned default).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AxisMap {
    /// Already glTF space (Y-up, right-handed).
    #[default]
    Gltf,
    /// COLMAP / OpenCV convention: x right, y down,
    /// z forward. Change of basis diag(1, -1, -1) —
    /// a 180° rotation about X.
    Colmap,
    /// Blender world space, through the setscene axis
    /// map (x, y, z) -> (x, z, -y) — a -90° rotation
    /// about X.
    Blender,
}

impl AxisMap {
    fn parse(name: &str) -> Result<AxisMap, RenderError> {
        match name {
            "gltf" => Ok(AxisMap::Gltf),
            "colmap" => Ok(AxisMap::Colmap),
            "blender" => Ok(AxisMap::Blender),
            other => Err(spec_err(format!(
                "unknown axis map {other:?} (expected \"gltf\", \"colmap\" or \"blender\")"
            ))),
        }
    }

    /// The change-of-basis matrix for centres:
    /// asset point p lands at `matrix * p`.
    fn matrix(&self) -> Mat4 {
        match self {
            AxisMap::Gltf => Mat4::IDENTITY,
            AxisMap::Colmap => Mat4::from_rows([
                [1.0, 0.0, 0.0, 0.0],
                [0.0, -1.0, 0.0, 0.0],
                [0.0, 0.0, -1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ]),
            AxisMap::Blender => crate::setscene::axis_map(),
        }
    }

    /// The same rotation as a quaternion (w, x, y, z),
    /// for the covariance path: a splat's rotation R
    /// becomes A * R (its local frame is carried by the
    /// change of basis).
    fn quaternion(&self) -> [f64; 4] {
        match self {
            AxisMap::Gltf => [1.0, 0.0, 0.0, 0.0],
            AxisMap::Colmap => [0.0, 1.0, 0.0, 0.0],
            // -90° about X: (cos -45°, sin -45°, 0, 0).
            AxisMap::Blender => [
                std::f64::consts::FRAC_1_SQRT_2,
                -std::f64::consts::FRAC_1_SQRT_2,
                0.0,
                0.0,
            ],
        }
    }

    /// Apply the change of basis to a whole cloud:
    /// centres by the matrix, rotations by the
    /// quaternion product q_axis * q_splat (Hamilton,
    /// in f64, cast back to the stored f32 form).
    pub fn apply(&self, cloud: &SplatCloud) -> SplatCloud {
        if *self == AxisMap::Gltf {
            return cloud.clone();
        }
        let m = self.matrix();
        let [aw, ax, ay, az] = self.quaternion();
        // Rung 3: the map is a rotation, so the SH
        // field rotates through the same machinery as
        // a layer rotation (closing the Rung 1 colour
        // caveat). Band 0 is invariant.
        let sh = crate::splat_rotate::ShRotation::new(
            &crate::splat_rotate::Rotation::from_quaternion(self.quaternion())
                .expect("axis maps are unit quaternions")
                .matrix,
        );
        let splats = cloud
            .splats
            .iter()
            .map(|s| {
                let p = [
                    f64::from(s.pos[0]),
                    f64::from(s.pos[1]),
                    f64::from(s.pos[2]),
                    1.0,
                ];
                let mut out = [0.0; 4];
                for i in 0..4 {
                    out[i] =
                        m.m[i] * p[0] + m.m[4 + i] * p[1] + m.m[8 + i] * p[2] + m.m[12 + i] * p[3];
                }
                let (w, x, y, z) = (
                    f64::from(s.rot[0]),
                    f64::from(s.rot[1]),
                    f64::from(s.rot[2]),
                    f64::from(s.rot[3]),
                );
                let mut rest = [0.0_f32; crate::splat::SH_REST_TOTAL];
                for ch in 0..3 {
                    let span = ch * crate::splat::SH_REST_PER_CHANNEL
                        ..(ch + 1) * crate::splat::SH_REST_PER_CHANNEL;
                    let mut c = [0.0_f64; crate::splat::SH_REST_PER_CHANNEL];
                    for (dst, src) in c.iter_mut().zip(&s.rest[span.clone()]) {
                        *dst = f64::from(*src);
                    }
                    for (dst, src) in rest[span].iter_mut().zip(sh.rotate_channel(&c)) {
                        *dst = src as f32;
                    }
                }
                Splat {
                    rest,
                    pos: [out[0] as f32, out[1] as f32, out[2] as f32],
                    rot: [
                        (aw * w - ax * x - ay * y - az * z) as f32,
                        (aw * x + ax * w + ay * z - az * y) as f32,
                        (aw * y - ax * z + ay * w + az * x) as f32,
                        (aw * z + ax * y - ay * x + az * w) as f32,
                    ],
                    ..*s
                }
            })
            .collect();
        SplatCloud { splats }
    }
}

/// A frozen asset reference: path (relative to the
/// spec's base directory, or absolute) + the SHA-256
/// the bytes must hash to.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetRef {
    pub path: String,
    pub sha256: String,
}

/// The layer's placement. `rotation` is the glTF
/// quaternion (x, y, z, w) order used by the spec
/// format's other transforms; at this rung it must be
/// identity when present (see the module docs).
#[derive(Debug, Clone, Deserialize)]
pub struct LayerTransform {
    #[serde(default)]
    pub translation: [f64; 3],
    #[serde(default)]
    pub rotation: Option<[f64; 4]>,
    #[serde(default = "default_scale")]
    pub scale: f64,
}

fn default_scale() -> f64 {
    1.0
}

impl Default for LayerTransform {
    fn default() -> Self {
        LayerTransform {
            translation: [0.0; 3],
            rotation: None,
            scale: 1.0,
        }
    }
}

/// One `layers` entry of kind "splat" (DESIGN §2.3).
#[derive(Debug, Clone, Deserialize)]
pub struct SplatLayerSpec {
    pub asset: AssetRef,
    #[serde(default)]
    pub transform: LayerTransform,
    #[serde(default)]
    pub axis: Option<String>,
}

/// The spec document, as much of it as this compiler
/// owns: the `layers` array. Layer kinds other than
/// "splat" are not this compiler's business and are
/// skipped at parse; a "splat" entry that is malformed
/// is a compile error, never a skip.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct LayerList {
    #[serde(default)]
    pub layers: Vec<serde_json::Value>,
}

/// Compile the spec document's splat layer, if it has
/// one, into the boundary packet for `scene`'s camera
/// at `width` x `height`.
///
/// - No splat layer → `Ok(None)` (the caller leaves
///   `PackedScene::splats` as `None`; the boundary
///   property — a splat-less scene is bit-identical —
///   is preserved by construction).
/// - More than one splat layer → compile error.
/// - Asset hash mismatch, unreadable or malformed
///   asset, unknown axis, non-identity rotation →
///   compile error naming the cause.
pub fn compile_scene_splats(
    spec_json: &[u8],
    base_dir: &Path,
    scene: &ArcScene,
    width: u32,
    height: u32,
) -> Result<Option<PackedSplats>, RenderError> {
    let doc: LayerList = serde_json::from_slice(spec_json)
        .map_err(|e| spec_err(format!("spec does not parse: {e}")))?;
    let mut splat_entries: Vec<SplatLayerSpec> = Vec::new();
    for entry in &doc.layers {
        if entry.get("kind").and_then(|k| k.as_str()) == Some("splat") {
            let parsed: SplatLayerSpec = serde_json::from_value(entry.clone())
                .map_err(|e| spec_err(format!("splat layer does not parse: {e}")))?;
            splat_entries.push(parsed);
        }
    }
    match splat_entries.len() {
        0 => Ok(None),
        1 => Ok(Some(compile_splat_layer(
            &splat_entries[0],
            base_dir,
            scene,
            width,
            height,
        )?)),
        n => Err(spec_err(format!(
            "{n} splat layers named; the boundary packet carries one \
             (multi-layer composition is not built)"
        ))),
    }
}

/// Compile one splat layer: verify, load, place, pack.
pub fn compile_splat_layer(
    layer: &SplatLayerSpec,
    base_dir: &Path,
    scene: &ArcScene,
    width: u32,
    height: u32,
) -> Result<PackedSplats, RenderError> {
    // Rotation (programme Rung 3): the spec field is
    // the glTF quaternion (x, y, z, w); the storage
    // order is (w, x, y, z) and the pack normalises.
    // A non-finite or zero quaternion is not a
    // rotation and does not compile.
    let rotation = match layer.transform.rotation {
        Some(q) => {
            if !q.iter().all(|v| v.is_finite()) {
                return Err(spec_err(format!(
                    "layer rotation {q:?} has a non-finite component"
                )));
            }
            if q.iter().all(|v| *v == 0.0) {
                return Err(spec_err(
                    "layer rotation is the zero quaternion (not a rotation)",
                ));
            }
            [q[3], q[0], q[1], q[2]]
        }
        None => crate::splat_rotate::IDENTITY_QUAT,
    };
    if !(layer.transform.scale.is_finite() && layer.transform.scale > 0.0) {
        return Err(spec_err(format!(
            "layer scale {} is not a positive finite number",
            layer.transform.scale
        )));
    }
    let axis = match &layer.axis {
        Some(name) => AxisMap::parse(name)?,
        None => AxisMap::Gltf,
    };

    let asset_path: PathBuf = {
        let p = Path::new(&layer.asset.path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            base_dir.join(p)
        }
    };
    let bytes = std::fs::read(&asset_path).map_err(|e| {
        spec_err(format!(
            "asset {} cannot be read: {e}",
            asset_path.display()
        ))
    })?;
    let actual = sha256_hex(&bytes);
    if !actual.eq_ignore_ascii_case(layer.asset.sha256.trim()) {
        return Err(spec_err(format!(
            "asset {} hash mismatch: spec names {}, asset hashes {actual} \
             (a spec whose asset hash does not match does not compile)",
            asset_path.display(),
            layer.asset.sha256
        )));
    }
    let cloud = SplatCloud::load_ply(&bytes)?;
    let cloud = axis.apply(&cloud);
    let transform = SplatTransform {
        translation: layer.transform.translation,
        rotation,
        scale: layer.transform.scale,
    };
    Ok(pack_splats(&cloud, &transform, scene, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setscene::plate_rig_scene;
    use crate::splat::room_corner_cloud;

    fn test_scene() -> ArcScene {
        // The proving tests' wide framing of the room
        // corner, rebuilt from the same numbers.
        let eye = [2.6, 1.9, -3.4];
        let target = [0.0, 0.9, 0.0];
        let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
        let cross = |a: [f64; 3], b: [f64; 3]| {
            [
                a[1] * b[2] - a[2] * b[1],
                a[2] * b[0] - a[0] * b[2],
                a[0] * b[1] - a[1] * b[0],
            ]
        };
        let norm = |a: [f64; 3]| {
            let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
            [a[0] / l, a[1] / l, a[2] / l]
        };
        let forward = norm(sub(target, eye));
        let right = norm(cross(forward, [0.0, 1.0, 0.0]));
        let up = cross(right, forward);
        let back = [-forward[0], -forward[1], -forward[2]];
        let camera = Mat4::from_rows([
            [right[0], up[0], back[0], eye[0]],
            [right[1], up[1], back[1], eye[1]],
            [right[2], up[2], back[2], eye[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]);
        plate_rig_scene(camera)
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("torogao-splat-spec-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// Write the room cloud as a spec asset and return
    /// (base dir, spec json naming it).
    fn spec_for_cloud(tag: &str, extra_layer_fields: &str) -> (PathBuf, String) {
        let dir = temp_dir(tag);
        let cloud = room_corner_cloud();
        let bytes = cloud.write_ply();
        std::fs::write(dir.join("room.ply"), &bytes).expect("write asset");
        let hash = sha256_hex(&bytes);
        let json = format!(
            r#"{{ "layers": [ {{ "kind": "splat",
                "asset": {{ "path": "room.ply", "sha256": "{hash}" }}
                {extra_layer_fields} }} ] }}"#
        );
        (dir, json)
    }

    #[test]
    fn compiler_packet_equals_direct_packet() {
        // The wiring adds nothing and loses nothing:
        // through the spec (hash-checked, parsed,
        // placed) the packet is the direct packet.
        let (dir, json) = spec_for_cloud(
            "equals",
            r#", "transform": { "translation": [0.25, 0.0, -0.5], "scale": 1.5 },
               "axis": "gltf""#,
        );
        let scene = test_scene();
        let compiled = compile_scene_splats(json.as_bytes(), &dir, &scene, 512, 640)
            .expect("compile")
            .expect("one layer");
        let direct = pack_splats(
            &room_corner_cloud(),
            &SplatTransform {
                translation: [0.25, 0.0, -0.5],
                rotation: crate::splat_rotate::IDENTITY_QUAT,
                scale: 1.5,
            },
            &scene,
            512,
            640,
        );
        assert!(
            compiled.splat_count > 500,
            "the room must actually pack ({} splats) — an empty \
             packet would make the equality vacuous",
            compiled.splat_count
        );
        assert_eq!(compiled, direct);
    }

    #[test]
    fn hash_mismatch_does_not_compile() {
        let dir = temp_dir("hash");
        let bytes = room_corner_cloud().write_ply();
        std::fs::write(dir.join("room.ply"), &bytes).expect("write asset");
        let json = r#"{ "layers": [ { "kind": "splat",
            "asset": { "path": "room.ply",
                       "sha256": "0000000000000000000000000000000000000000000000000000000000000000" } } ] }"#;
        let scene = test_scene();
        let err = compile_scene_splats(json.as_bytes(), &dir, &scene, 512, 640)
            .expect_err("hash mismatch must fail");
        assert!(
            format!("{err}").contains("hash mismatch"),
            "error names the cause: {err}"
        );
    }

    #[test]
    fn no_splat_layer_compiles_to_none() {
        let scene = test_scene();
        let dir = temp_dir("none");
        for json in [
            r#"{ "layers": [] }"#,
            r#"{ "name": "no layers at all" }"#,
            r#"{ "layers": [ { "kind": "character" } ] }"#,
        ] {
            let out =
                compile_scene_splats(json.as_bytes(), &dir, &scene, 512, 640).expect("compile");
            assert!(out.is_none(), "no splat layer → None: {json}");
        }
    }

    #[test]
    fn two_splat_layers_do_not_compile() {
        let (dir, _json) = spec_for_cloud("two", "");
        let bytes = room_corner_cloud().write_ply();
        let hash = sha256_hex(&bytes);
        let json = format!(
            r#"{{ "layers": [
                {{ "kind": "splat", "asset": {{ "path": "room.ply", "sha256": "{hash}" }} }},
                {{ "kind": "splat", "asset": {{ "path": "room.ply", "sha256": "{hash}" }} }}
            ] }}"#
        );
        let scene = test_scene();
        let err = compile_scene_splats(json.as_bytes(), &dir, &scene, 512, 640)
            .expect_err("two layers must fail");
        assert!(format!("{err}").contains("2 splat layers"), "{err}");
    }

    #[test]
    fn rotation_compiles_and_matches_direct_pack() {
        // Rung 3 supersedes the Rung 1 refusal this test
        // used to pin (`rotation_names_rung_3`): a
        // non-identity rotation now compiles, and the
        // compiled packet is the direct packet of the
        // same rotated layer — the wiring adds nothing.
        // 90 deg about Y, spec order (x, y, z, w).
        let (dir, json) = spec_for_cloud(
            "rotation",
            r#", "transform": { "rotation": [0.0, 0.7071067811865476, 0.0, 0.7071067811865476] }"#,
        );
        let scene = test_scene();
        let compiled = compile_scene_splats(json.as_bytes(), &dir, &scene, 512, 640)
            .expect("rotation compiles at Rung 3")
            .expect("one layer");
        let direct = pack_splats(
            &room_corner_cloud(),
            &SplatTransform {
                translation: [0.0; 3],
                // Storage order (w, x, y, z).
                rotation: [
                    std::f64::consts::FRAC_1_SQRT_2,
                    0.0,
                    std::f64::consts::FRAC_1_SQRT_2,
                    0.0,
                ],
                scale: 1.0,
            },
            &scene,
            512,
            640,
        );
        assert!(
            compiled.splat_count > 500,
            "the rotated room must actually pack ({} splats)",
            compiled.splat_count
        );
        assert_eq!(compiled, direct);
        // And an identity rotation compiles.
        let (dir2, json2) = spec_for_cloud(
            "rotation-id",
            r#", "transform": { "rotation": [0.0, 0.0, 0.0, 1.0] }"#,
        );
        assert!(
            compile_scene_splats(json2.as_bytes(), &dir2, &scene, 512, 640)
                .expect("identity rotation compiles")
                .is_some()
        );
    }

    #[test]
    fn rotation_zero_quaternion_does_not_compile() {
        let (dir, json) = spec_for_cloud(
            "rotation-zero",
            r#", "transform": { "rotation": [0.0, 0.0, 0.0, 0.0] }"#,
        );
        let scene = test_scene();
        let err = compile_scene_splats(json.as_bytes(), &dir, &scene, 512, 640)
            .expect_err("zero quaternion must fail");
        assert!(format!("{err}").contains("zero quaternion"), "{err}");
    }

    #[test]
    fn axis_maps_move_known_vectors() {
        let one = |pos: [f32; 3], rot: [f32; 4]| SplatCloud {
            splats: vec![Splat {
                pos,
                dc: [0.0; 3],
                rest: [0.0; crate::splat::SH_REST_TOTAL],
                opacity_logit: 0.0,
                log_scale: [0.0; 3],
                rot,
            }],
        };
        // Colmap: (1, 2, 3) -> (1, -2, -3); a 180°
        // rotation about Y becomes 180° about Z.
        let mapped = AxisMap::Colmap.apply(&one([1.0, 2.0, 3.0], [0.0, 0.0, 1.0, 0.0]));
        let s = &mapped.splats[0];
        assert_eq!(s.pos, [1.0, -2.0, -3.0]);
        assert_eq!(s.rot, [0.0, 0.0, 0.0, 1.0]);
        // Colmap twice is the identity: the centre
        // returns exactly, and the rotation returns up
        // to the quaternion double cover (q and -q are
        // the same rotation; the pack's covariance is
        // identical for both representatives).
        let twice = AxisMap::Colmap
            .apply(&AxisMap::Colmap.apply(&one([1.0, 2.0, 3.0], [0.0, 0.0, 1.0, 0.0])));
        assert_eq!(twice.splats[0].pos, [1.0, 2.0, 3.0]);
        let r = twice.splats[0].rot;
        let dot = r[2];
        assert!((dot.abs() - 1.0).abs() < 1e-6, "same rotation: {r:?}");
        // Blender: (x, y, z) -> (x, z, -y).
        let mapped = AxisMap::Blender.apply(&one([1.0, 2.0, 3.0], [1.0, 0.0, 0.0, 0.0]));
        assert_eq!(mapped.splats[0].pos, [1.0, 3.0, -2.0]);
        // Gltf is the identity map.
        let same = AxisMap::Gltf.apply(&one([1.0, 2.0, 3.0], [0.0, 0.0, 1.0, 0.0]));
        assert_eq!(same.splats[0].pos, [1.0, 2.0, 3.0]);
        // Unknown axis names fail loudly.
        assert!(AxisMap::parse("opencv-ish").is_err());
    }

    #[test]
    fn malformed_splat_layer_does_not_compile() {
        let scene = test_scene();
        let dir = temp_dir("malformed");
        let json = r#"{ "layers": [ { "kind": "splat", "asset": { "path": "room.ply" } } ] }"#;
        assert!(compile_scene_splats(json.as_bytes(), &dir, &scene, 512, 640).is_err());
    }
}
