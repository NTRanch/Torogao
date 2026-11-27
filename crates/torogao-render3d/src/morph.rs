//! Sparse morph packing (gap analysis R2): morph deltas are
//! stored as compressed sparse rows so the GPU work is
//! proportional to the deltas that exist, not to
//! `vertices * targets`. Prima's body stores 5,628 sparse
//! position entries across 37 targets — against
//! 13,920 * 37 = 515,040 dense slots.
//!
//! Layout (all arrays uploaded verbatim as SSBOs):
//! - `ranges[v] = [start, end)`: vertex `v`'s slice of the
//!   entry arrays.
//! - entries sorted by `(vertex, target)`: `entry_target`
//!   indexes the per-frame weight array; `entry_dpos` /
//!   `entry_dnrm` carry the deltas as vec4s (w unused, kept
//!   for std430 alignment).
//!
//! The same packed form feeds the CPU reference, so the
//! cross-check compares arithmetic, not data paths.

use crate::model::Mesh;

/// CSR-packed morph deltas for one mesh.
#[derive(Debug, Clone)]
pub struct PackedMorphs {
    /// Per-vertex `[start, end)` into the entry arrays;
    /// length `vertex_count + 1` (last entry = total), the
    /// classic CSR sentinel layout.
    pub ranges: Vec<[u32; 2]>,
    /// Target index per entry.
    pub entry_target: Vec<u32>,
    /// Position delta per entry (xyz; w = 0).
    pub entry_dpos: Vec<[f32; 4]>,
    /// Normal delta per entry (xyz; w = 0).
    pub entry_dnrm: Vec<[f32; 4]>,
    /// Number of morph targets on the mesh.
    pub target_count: u32,
}

impl PackedMorphs {
    /// Pack a mesh's morph targets. A mesh with no targets
    /// packs to empty arrays.
    pub fn pack(mesh: &Mesh) -> PackedMorphs {
        let vertex_count = mesh.vertex_count();
        let mut flat: Vec<(u32, u32, [f32; 3], [f32; 3])> = Vec::new();
        for (target, data) in mesh.targets.iter().enumerate() {
            for (vertex, dpos, dnrm) in &data.entries {
                flat.push((*vertex, target as u32, *dpos, *dnrm));
            }
        }
        flat.sort_by_key(|(vertex, target, _, _)| (*vertex, *target));
        let total = flat.len();
        let mut ranges = vec![[0_u32, 0]; vertex_count + 1];
        let mut entry_target = Vec::with_capacity(total);
        let mut entry_dpos = Vec::with_capacity(total);
        let mut entry_dnrm = Vec::with_capacity(total);
        let mut cursor = 0_usize;
        for (vertex, range) in ranges.iter_mut().enumerate().take(vertex_count) {
            let start = cursor;
            while cursor < total && flat[cursor].0 == vertex as u32 {
                let (_, target, dpos, dnrm) = flat[cursor];
                entry_target.push(target);
                entry_dpos.push([dpos[0], dpos[1], dpos[2], 0.0]);
                entry_dnrm.push([dnrm[0], dnrm[1], dnrm[2], 0.0]);
                cursor += 1;
            }
            *range = [start as u32, cursor as u32];
        }
        ranges[vertex_count] = [total as u32, total as u32];
        PackedMorphs {
            ranges,
            entry_target,
            entry_dpos,
            entry_dnrm,
            target_count: mesh.targets.len() as u32,
        }
    }

    /// Total stored entries.
    pub fn entry_count(&self) -> usize {
        self.entry_target.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MorphTarget;

    fn test_mesh() -> Mesh {
        Mesh {
            name: "t".to_string(),
            node: 0,
            material: 0,
            positions: vec![[0.0; 3]; 4],
            normals: vec![[0.0, 0.0, 1.0]; 4],
            color0: vec![[1.0; 4]; 4],
            color1: vec![[0.0; 4]; 4],
            has_color1: false,
            texcoord0: vec![[0.0; 2]; 4],
            texcoord1: vec![[0.0; 2]; 4],
            has_texcoord1: false,
            outline_normal: vec![[0.0, 0.0, 1.0]; 4],
            joints: vec![[0; 4]; 4],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; 4],
            indices: vec![0, 1, 2],
            targets: vec![
                MorphTarget {
                    entries: vec![
                        (2, [0.0, 1.0, 0.0], [0.0; 3]),
                        (0, [1.0, 0.0, 0.0], [0.0; 3]),
                    ],
                    stored_position_entries: 2,
                },
                MorphTarget {
                    entries: vec![(2, [0.0, 0.0, 1.0], [0.0; 3])],
                    stored_position_entries: 1,
                },
            ],
            target_names: vec!["a".to_string(), "b".to_string()],
            default_weights: vec![0.0, 0.0],
        }
    }

    #[test]
    fn csr_ranges_and_order() {
        let packed = PackedMorphs::pack(&test_mesh());
        assert_eq!(packed.entry_count(), 3);
        // vertex 0: one entry (target 0); vertex 1: none;
        // vertex 2: two entries sorted by target.
        assert_eq!(packed.ranges[0], [0, 1]);
        assert_eq!(packed.ranges[1], [1, 1]);
        assert_eq!(packed.ranges[2], [1, 3]);
        assert_eq!(packed.ranges[3], [3, 3]);
        assert_eq!(packed.ranges[4], [3, 3]);
        assert_eq!(packed.entry_target, vec![0, 0, 1]);
        assert_eq!(packed.entry_dpos[0], [1.0, 0.0, 0.0, 0.0]);
        assert_eq!(packed.entry_dpos[2], [0.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn empty_mesh_packs_empty() {
        let mut mesh = test_mesh();
        mesh.targets.clear();
        let packed = PackedMorphs::pack(&mesh);
        assert_eq!(packed.entry_count(), 0);
        assert_eq!(packed.target_count, 0);
        assert!(packed.ranges.iter().all(|r| *r == [0, 0]));
    }
}
