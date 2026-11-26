//! The canonical cast table.
//!
//! Each record is data taken from the character's governing lock or
//! style sheet (cited in `source`) — never invented at a prompt box.
//! Locked traits stay with their owner: freckles are Lana's identity
//! lock (D-1482) and appear in no other record; Sirena's record
//! carries the matching negative guard (STYLE-SHEET v0.3: "No
//! freckles — ever"). The table grows only by admitted records.

/// One canonical cast member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Character {
    /// Stable id used in requests, gallery entries, and file names.
    pub id: &'static str,
    /// Display name for the shells' cast pickers.
    pub name: &'static str,
    /// Positive identity tags (Illustrious tag style), appended after
    /// the shared style prefix when a prompt is composed.
    pub identity: &'static str,
    /// Signature items / canonical outfit line, appended after the
    /// identity tags.
    pub signature: &'static str,
    /// Per-character additions to the shared negative prompt.
    pub negative_guards: &'static str,
    /// The lock / style sheet this record was taken from.
    pub source: &'static str,
}

/// Prima — the canonical-base pilot girl.
pub const PRIMA: Character = Character {
    id: "prima",
    name: "Prima",
    identity: "1girl, solo, very long wavy silver-white hair, lavender-grey hair shading, \
               centre part, face-framing locks, deep red eyes, fair skin",
    signature: "pink dress, frilled dress",
    negative_guards: "",
    source: "Canonical-base RUNG0-LOCK (identity lock held 2026-11-24): very long loose \
             wavy silver-white hair with lavender-grey shadow drift, centre part; iris \
             palette (0.656, 0.006, 0.012) — deep crimson; DressPink palette; ~7.6-head \
             proportions.",
};

/// Lana — cast candidate, identity locked D-1482.
pub const LANA: Character = Character {
    id: "lana",
    name: "Lana",
    identity: "1girl, solo, long flowing wavy red hair, amber-brown eyes, freckles, \
               fair skin, wry smile, lean athletic body",
    signature: "crop top, short shorts, thong above waistband, nipple piercings",
    negative_guards: "",
    source: "Identity lock D-1482 (Owner, 2026-11-13): vivid-red wavy hair, amber-brown \
             eyes, fair skin with light freckles (hers alone), wry smile, E-cup firm/perky \
             bust, lean athletic build; outfit as ruled.",
};

/// Sirena — mesmer's Sirena, style sheet v0.3.
pub const SIRENA: Character = Character {
    id: "sirena",
    name: "Sirena",
    identity: "1girl, solo, sleek glossy black hair, hair to mid-back, braided side hair \
               with gold chain, round-almond plum-violet eyes, gentle confident gaze, \
               curvaceous, wide hips, trained lower body",
    signature: "black ribbed top, open black leather jacket, gold chain details, \
                gold pendulum necklace, spiral nipple piercings",
    negative_guards: "freckles",
    source: "STYLE-SHEET.md v0.3 (2026-11-26) + admitted full-body keeper set v4/v7/v8/v9: \
             mid-back black hair, plum-violet eyes, G-cup perky, wider hips and trained \
             lower body, gold pendulum motif, spiral piercings never barbells, no freckles \
             ever.",
};

/// The full cast, in picker order.
pub const CAST: &[Character] = &[PRIMA, LANA, SIRENA];

/// Look a cast member up by id. Unknown ids are refused (`None`) —
/// the shells can only offer what the table carries.
pub fn character(id: &str) -> Option<&'static Character> {
    CAST.iter().find(|c| c.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cast_table_is_exactly_the_admitted_records() {
        let ids: Vec<&str> = CAST.iter().map(|c| c.id).collect();
        assert_eq!(ids, ["prima", "lana", "sirena"]);
    }

    #[test]
    fn lookup_round_trips_and_refuses_unknown() {
        for c in CAST {
            assert_eq!(character(c.id), Some(c));
        }
        assert_eq!(character("nobody"), None);
        assert_eq!(character(""), None);
    }

    #[test]
    fn freckles_belong_to_lana_alone() {
        // D-1482 / STYLE-SHEET v0.3: the trait is Lana's lock. No
        // other identity may carry it, and Sirena guards against it.
        assert!(LANA.identity.contains("freckles"));
        assert!(!PRIMA.identity.contains("freckles"));
        assert!(!SIRENA.identity.contains("freckles"));
        assert!(SIRENA.negative_guards.contains("freckles"));
    }

    #[test]
    fn every_record_cites_its_source() {
        for c in CAST {
            assert!(!c.source.is_empty(), "{} cites no source", c.id);
            assert!(!c.identity.is_empty(), "{} has no identity", c.id);
        }
    }
}
