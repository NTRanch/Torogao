//! The admitted cast, migrated into `torogao-character/1`
//! records.
//!
//! Facts are copied from the mobile core cast table
//! (`torogao_mobile_core::cast`), which took them from each
//! character's governing lock or style sheet — the citations
//! travel verbatim in `prompt_identity.source` and
//! `identity_lock.source`. The migration is honest about
//! lock state:
//!
//! - Prima and Lana carry Step 0 locks of record, but their
//!   lock artefacts are not yet hash-pinned into a record, so
//!   their refs are unhashed — and an unhashed ref cannot
//!   carry a derivation (see
//!   [`crate::gate::derive_eligibility`]). Hashing the lock
//!   artefacts primo-side is the recorded unblocking act.
//! - Sirena's governing document is a style sheet, not a
//!   Step 0 lock, so her record is `unlocked`: she can
//!   originate art and cannot derive until a lock is held.
//!
//! No record carries a derivation spec: a builder spec is
//! imported primo-side per character before her first
//! derivation. Parameters start neutral.

use crate::params::{BodyParams, FaceParams};
use crate::schema::{
    ArtSection, CharacterRecord, IdentityLock, LockRef, LockStatus, OutfitRef, PromptIdentity,
    RigSection, SCHEMA_ID,
};

fn record(
    id: &str,
    name: &str,
    now: &str,
    lock: IdentityLock,
    prompt_identity: PromptIdentity,
    outfit_ref: &str,
    builder_family: &str,
) -> CharacterRecord {
    CharacterRecord {
        schema: SCHEMA_ID.to_string(),
        id: id.to_string(),
        name: name.to_string(),
        identity_lock: lock,
        prompt_identity,
        body_params: BodyParams::default(),
        face_params: FaceParams::default(),
        expressions: Vec::new(),
        outfit: OutfitRef {
            spec_ref: outfit_ref.to_string(),
        },
        rig: RigSection {
            builder_family: builder_family.to_string(),
            ..RigSection::default()
        },
        art: ArtSection::default(),
        revision: 1,
        updated_at: now.to_string(),
    }
}

/// Prima — the canonical-base pilot girl. Lock of record:
/// canonical-base RUNG0-LOCK, held 2026-11-24.
pub fn prima_record(now: &str) -> CharacterRecord {
    record(
        "prima",
        "Prima",
        now,
        IdentityLock {
            status: LockStatus::Locked,
            refs: vec![LockRef {
                label: "canonical-base RUNG0-LOCK".to_string(),
                sha256: String::new(),
            }],
            proportions: None,
            locked_by: Some("Owner".to_string()),
            locked_at: Some("2026-11-24".to_string()),
            source: "Canonical-base RUNG0-LOCK (identity lock held 2026-11-24): very long \
                     loose wavy silver-white hair with lavender-grey shadow drift, centre \
                     part; iris palette (0.656, 0.006, 0.012) — deep crimson; DressPink \
                     palette; ~7.6-head proportions. Refs unhashed at migration: hashing \
                     the lock artefacts primo-side unblocks derivation."
                .to_string(),
        },
        PromptIdentity {
            identity: "1girl, solo, very long wavy silver-white hair, lavender-grey hair shading, \
                 centre part, face-framing locks, deep red eyes, fair skin"
                .to_string(),
            signature: "pink dress, frilled dress".to_string(),
            negative_guards: String::new(),
            source: "Canonical-base RUNG0-LOCK (identity lock held 2026-11-24)".to_string(),
        },
        "DressPink (design-party palette of record)",
        "torogao",
    )
}

/// Lana — cast candidate, identity locked D-1482.
pub fn lana_record(now: &str) -> CharacterRecord {
    record(
        "lana",
        "Lana",
        now,
        IdentityLock {
            status: LockStatus::Locked,
            refs: vec![LockRef {
                label: "identity lock D-1482".to_string(),
                sha256: String::new(),
            }],
            proportions: None,
            locked_by: Some("Owner".to_string()),
            locked_at: Some("2026-11-13".to_string()),
            source: "Identity lock D-1482 (Owner, 2026-11-13): vivid-red wavy hair, \
                     amber-brown eyes, fair skin with light freckles (hers alone), wry \
                     smile, E-cup firm/perky bust, lean athletic build; outfit as ruled. \
                     Refs unhashed at migration: hashing the lock artefacts primo-side \
                     unblocks derivation."
                .to_string(),
        },
        PromptIdentity {
            identity: "1girl, solo, long flowing wavy red hair, amber-brown eyes, freckles, \
                 fair skin, wry smile, lean athletic body"
                .to_string(),
            signature: "crop top, short shorts, thong above waistband, nipple piercings"
                .to_string(),
            negative_guards: String::new(),
            source: "Identity lock D-1482 (Owner, 2026-11-13)".to_string(),
        },
        "D-1482 outfit of record (crop top, short shorts, thong above waistband)",
        "lana",
    )
}

/// Sirena — style sheet v0.3, NOT a Step 0 lock: her record
/// is unlocked until a lock is held.
pub fn sirena_record(now: &str) -> CharacterRecord {
    record(
        "sirena",
        "Sirena",
        now,
        IdentityLock {
            status: LockStatus::Unlocked,
            refs: vec![LockRef {
                label: "STYLE-SHEET.md v0.3".to_string(),
                sha256: String::new(),
            }],
            proportions: None,
            locked_by: None,
            locked_at: None,
            source: "STYLE-SHEET.md v0.3 (2026-11-26) + admitted full-body keeper set \
                     v4/v7/v8/v9: mid-back black hair, plum-violet eyes, G-cup perky, \
                     wider hips and trained lower body, gold pendulum motif, spiral \
                     piercings never barbells, no freckles ever. A style sheet is not a \
                     Step 0 identity lock: derivation stays refused until a lock is held."
                .to_string(),
        },
        PromptIdentity {
            identity: "1girl, solo, sleek glossy black hair, hair to mid-back, braided side \
                 hair with gold chain, round-almond plum-violet eyes, gentle confident \
                 gaze, curvaceous, wide hips, trained lower body"
                .to_string(),
            signature: "black ribbed top, open black leather jacket, gold chain details, \
                 gold pendulum necklace, spiral nipple piercings"
                .to_string(),
            negative_guards: "freckles".to_string(),
            source: "STYLE-SHEET.md v0.3 (2026-11-26)".to_string(),
        },
        "STYLE-SHEET v0.3 outfit of record",
        "torogao",
    )
}

/// The migrated cast, in the core table's picker order.
pub fn migrated_cast(now: &str) -> Vec<CharacterRecord> {
    vec![prima_record(now), lana_record(now), sirena_record(now)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::derive_eligibility;

    #[test]
    fn migrated_records_validate() {
        for r in migrated_cast("2026-11-27T00:00:00Z") {
            assert!(r.validate().is_empty(), "{}: {:?}", r.id, r.validate());
        }
    }

    #[test]
    fn no_migrated_record_can_derive_yet_and_the_reasons_are_honest() {
        // Prima/Lana: locks held but refs unhashed and no spec
        // imported. Sirena: no lock at all. The gate must say
        // so, in each case, rather than deriving on faith.
        for r in migrated_cast("2026-11-27T00:00:00Z") {
            let reasons = derive_eligibility(&r).expect_err("must not derive yet");
            assert!(!reasons.is_empty(), "{} gave no reasons", r.id);
        }
        let sirena = sirena_record("2026-11-27T00:00:00Z");
        let reasons = derive_eligibility(&sirena).expect_err("must not derive");
        assert!(reasons.iter().any(|x| x.contains("identity lock")));
    }

    #[test]
    fn freckles_stay_lanas_alone() {
        let cast = migrated_cast("2026-11-27T00:00:00Z");
        assert!(cast[1].prompt_identity.identity.contains("freckles"));
        assert!(!cast[0].prompt_identity.identity.contains("freckles"));
        assert!(!cast[2].prompt_identity.identity.contains("freckles"));
        assert!(cast[2].prompt_identity.negative_guards.contains("freckles"));
    }
}
