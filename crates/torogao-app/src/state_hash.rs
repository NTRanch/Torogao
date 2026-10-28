//! The evaluated state hash: a canonical FNV-1a (64-bit)
//! digest over the evaluated state of a puppet, defined
//! once here and used by every mode of this crate.
//!
//! Coverage, in order: for each parameter in payload order,
//! the bit patterns of its last internal value (x, then y);
//! then for each node in pre-order from the root, the bit
//! patterns of its world transform — translation (x, y, z),
//! rotation (x, y, z), scale (x, y) — then the sixteen bit
//! patterns of its world matrix (column-major, as stored),
//! then the bit patterns of every deformation entry (x,
//! then y per vertex). These are the same values the
//! evaluation-parity probe dumps, so equal hashes mean the
//! app evaluated exactly what a headless engine evaluation
//! of the same parameter set evaluates.

use torogao_engine::Puppet;

/// FNV-1a offset basis (64-bit).
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a prime (64-bit).
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// An incremental FNV-1a hasher over little-endian words.
struct StateHasher {
    state: u64,
}

impl StateHasher {
    fn new() -> StateHasher {
        StateHasher {
            state: FNV_OFFSET_BASIS,
        }
    }

    fn write_u32(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.state ^= u64::from(byte);
            self.state = self.state.wrapping_mul(FNV_PRIME);
        }
    }

    fn write_f32(&mut self, value: f32) {
        self.write_u32(value.to_bits());
    }

    fn finish(&self) -> u64 {
        self.state
    }
}

/// Computes the evaluated state hash of a puppet in its
/// current evaluated state. Reading world transforms may
/// refresh the engine's lazy transform cache; it never
/// changes evaluated values.
pub fn evaluated_state_hash(puppet: &mut Puppet) -> u64 {
    let mut hasher = StateHasher::new();
    for param in &puppet.params {
        hasher.write_f32(param.last_internal.x);
        hasher.write_f32(param.last_internal.y);
    }
    let order = puppet.nodes.preorder(puppet.nodes.root);
    for index in order {
        let global = puppet.nodes.transform(index);
        hasher.write_f32(global.translation.x);
        hasher.write_f32(global.translation.y);
        hasher.write_f32(global.translation.z);
        hasher.write_f32(global.rotation.x);
        hasher.write_f32(global.rotation.y);
        hasher.write_f32(global.rotation.z);
        hasher.write_f32(global.scale.x);
        hasher.write_f32(global.scale.y);
        for element in global.trs.m {
            hasher.write_f32(element);
        }
        let node = &puppet.nodes.nodes[index];
        for entry in &node.deformation {
            hasher.write_f32(entry.x);
            hasher.write_f32(entry.y);
        }
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::{FNV_OFFSET_BASIS, StateHasher};

    #[test]
    fn empty_stream_hashes_to_the_offset_basis() {
        assert_eq!(StateHasher::new().finish(), FNV_OFFSET_BASIS);
    }

    #[test]
    fn hash_is_sensitive_to_value_bits() {
        let mut a = StateHasher::new();
        a.write_f32(1.0);
        let mut b = StateHasher::new();
        b.write_f32(1.0 + f32::EPSILON);
        assert_ne!(a.finish(), b.finish());
    }
}
