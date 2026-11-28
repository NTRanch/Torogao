//! The expression morph table — Rule 10 made data.
//!
//! Every morph target the fleet builder emits, with the
//! mechanism that moves it, stated from the builder's own
//! `compute_morphs` (the gated builder copies of record):
//! which vertex zones the delta lands on and what the delta
//! does. A morph's NAME is never the evidence — this table
//! is, and a pose-test verdict cites the entry plus the
//! measured displacement from the artefact.
//!
//! Zone vocabulary (the builder's zone names): `LID_U` /
//! `LID_L` / `LASH_U` / `LASH_D` are the eyelid and lash
//! rings at the eye line; `EYE_S` / `EYE_I` / `PUPIL` /
//! `HEART` are the eyeball assembly (sclera, iris, pupil,
//! heart inlay); `BROW_` is the brow band, joined by the
//! forehead skin band at reduced amplitude; `LIP` /
//! `MOUTH_IN` / `TEETH` are the mouth assembly, at zone
//! scales 1.0 / 0.55 / 0.25 in every mouth morph. Distances
//! are metres in the builder's raw frame. `cf` is the mouth
//! corner factor (smoothstep of |x| from 6 mm to 16 mm);
//! `inner` is the inner-corner gaussian weight.

/// One morph and its moving mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MorphMechanism {
    /// Morph target name, as emitted in the GLB.
    pub name: &'static str,
    /// The vertex zones the delta lands on.
    pub zone: &'static str,
    /// What the delta does, from `compute_morphs`.
    pub mechanism: &'static str,
}

const LID: &str = "eyelid/lash rings (LID_U, LID_L, LASH_U, LASH_D), both eyes";
const LID_L: &str = "eyelid/lash rings, character-left eye only";
const LID_R: &str = "eyelid/lash rings, character-right eye only";
const EYE: &str = "eyeball assembly (EYE_S, EYE_I, PUPIL, HEART)";
const BROW: &str = "brow band (BROW_) + forehead skin band at reduced amplitude";
const MOUTH: &str = "mouth assembly (LIP ×1.0, MOUTH_IN ×0.55, TEETH ×0.25)";

/// The full fleet morph set with mechanisms, in the builder
/// spec's canonical order.
pub const MORPH_MECHANISMS: &[MorphMechanism] = &[
    MorphMechanism {
        name: "Fac_Eye_Close",
        zone: LID,
        mechanism: "lid verts fold toward the lid-margin arc and forward to a dome \
                    depth derived from the eyeball (sphere front minus 1.8 mm proud); \
                    the closed-lid contour is sculpted into the delta",
    },
    MorphMechanism {
        name: "Fac_Eye_L_Open",
        zone: LID_L,
        mechanism: "upper lid +5.2 mm z / −1.2 mm y, lower lid −1.6 mm z, under a \
                    gaussian falloff centred on the eye (wide-open eye)",
    },
    MorphMechanism {
        name: "Fac_Eye_R_Open",
        zone: LID_R,
        mechanism: "upper lid +5.2 mm z / −1.2 mm y, lower lid −1.6 mm z, under a \
                    gaussian falloff centred on the eye (wide-open eye)",
    },
    MorphMechanism {
        name: "Fac_Eye_L_Wink",
        zone: LID_L,
        mechanism: "the full Fac_Eye_Close fold delta, left eye only",
    },
    MorphMechanism {
        name: "Fac_Eye_R_Wink",
        zone: LID_R,
        mechanism: "the full Fac_Eye_Close fold delta, right eye only",
    },
    MorphMechanism {
        name: "Fac_Eye_HalfClose",
        zone: LID,
        mechanism: "the Fac_Eye_Close fold delta at 0.5 amplitude",
    },
    MorphMechanism {
        name: "Fac_Eye_Sad",
        zone: LID,
        mechanism: "upper lids only: z shift graded by the inner-corner gaussian, \
                    −4.5 mm at the inner corner rising to +1.5 mm at the outer",
    },
    MorphMechanism {
        name: "Fac_Eye_Angry",
        zone: LID,
        mechanism: "upper lids only: z drop of −6 mm × (0.4 + 0.6 × inner-corner weight)",
    },
    MorphMechanism {
        name: "Fac_Eye_MidUp",
        zone: EYE,
        mechanism: "the whole eyeball assembly translates +3.4 mm z (gaze raised)",
    },
    MorphMechanism {
        name: "Fac_Eye_MidDown",
        zone: EYE,
        mechanism: "the whole eyeball assembly translates −3.4 mm z (gaze lowered)",
    },
    MorphMechanism {
        name: "Fac_Eye_LowlidUp",
        zone: LID,
        mechanism: "lower lids only: +5.2 mm z under the eye-centre gaussian falloff",
    },
    MorphMechanism {
        name: "Fac_Ebr_Up",
        zone: BROW,
        mechanism: "brow band +6.0 mm z; forehead skin band +2.1 mm z",
    },
    MorphMechanism {
        name: "Fac_Ebr_Down",
        zone: BROW,
        mechanism: "brow band −5.2 mm z and −0.8 mm y; forehead skin band −1.8 mm z",
    },
    MorphMechanism {
        name: "Fac_Ebr_Angry",
        zone: BROW,
        mechanism: "brow z = −7.5 mm × inner + 2.2 mm × (1 − inner): the inner brow \
                    drops while the outer rises; forehead band −2.6 mm × inner",
    },
    MorphMechanism {
        name: "Fac_Ebr_Sad",
        zone: BROW,
        mechanism: "brow z = +6.2 mm × inner − 2.0 mm × (1 − inner): the inner brow \
                    rises; forehead band +2.2 mm × inner",
    },
    MorphMechanism {
        name: "Fac_Ebr_Relax",
        zone: BROW,
        mechanism: "brow band +1.8 mm z (forehead band does not join this channel)",
    },
    MorphMechanism {
        name: "Fac_Mth_Aa",
        zone: MOUTH,
        mechanism: "lower side drops by the jaw delta −9.2 mm × (1 − 0.45 × cf), \
                    upper side +2.6 mm (open-jaw 'aa')",
    },
    MorphMechanism {
        name: "Fac_Mth_Ii",
        zone: MOUTH,
        mechanism: "corners pull outward ±5.8 mm × cf and +1.2 mm forward, with a \
                    ±1.2 mm upper/lower split (wide 'ii')",
    },
    MorphMechanism {
        name: "Fac_Mth_Uu",
        zone: MOUTH,
        mechanism: "corners pull inward ∓3.8 mm × cf and −4.2 mm back, with a \
                    ±1.8 mm upper/lower split (pursed 'uu')",
    },
    MorphMechanism {
        name: "Fac_Mth_Ee",
        zone: MOUTH,
        mechanism: "corners pull outward ±3.8 mm × cf with a ±2.0 mm upper/lower split",
    },
    MorphMechanism {
        name: "Fac_Mth_Oo",
        zone: MOUTH,
        mechanism: "corners pull inward ∓2.0 mm × cf and −5.0 mm back, with a \
                    ±3.4 mm upper/lower split (rounded 'oo')",
    },
    MorphMechanism {
        name: "Fac_Mth_Talk",
        zone: MOUTH,
        mechanism: "corners ±2.6 mm × cf plus 0.55 × the Fac_Mth_Aa jaw delta \
                    (speech-cycle base shape)",
    },
    MorphMechanism {
        name: "Fac_Mth_Laugh",
        zone: MOUTH,
        mechanism: "corners ±4.8 mm × cf; upper side +3.0 mm, lower side −12 mm × \
                    (1 − 0.4 × cf) with a +3.8 mm × cf corner lift",
    },
    MorphMechanism {
        name: "Fac_Mth_Shout",
        zone: MOUTH,
        mechanism: "corners ±3.0 mm × cf; upper side +4.2 mm, lower side −13.5 mm",
    },
    MorphMechanism {
        name: "Fac_Mth_Triangle",
        zone: MOUTH,
        mechanism: "centre verts −2.2 mm × (1 − cf) forward and −4.2 mm × cf in z \
                    (triangular mouth)",
    },
    MorphMechanism {
        name: "Fac_Mth_Smile",
        zone: MOUTH,
        mechanism: "corners ±2.8 mm × cf outward and +4.6 mm × cf in z (smile lift)",
    },
    MorphMechanism {
        name: "Fac_Mth_Open",
        zone: MOUTH,
        mechanism: "upper side +2.0 mm, lower side −6.8 mm × (1 − 0.4 × cf); \
                    no corner pull",
    },
    MorphMechanism {
        name: "Fac_Mth_L_Up",
        zone: MOUTH,
        mechanism: "character-left mouth corner only (|x| > 6 mm, +x side): \
                    +4.2 mm × cf in z",
    },
    MorphMechanism {
        name: "Fac_Mth_L_Down",
        zone: MOUTH,
        mechanism: "character-left mouth corner only: −4.2 mm × cf in z",
    },
    MorphMechanism {
        name: "Fac_Mth_L_In",
        zone: MOUTH,
        mechanism: "character-left mouth corner only: −3.4 mm × cf in x (toward midline)",
    },
    MorphMechanism {
        name: "Fac_Mth_L_Out",
        zone: MOUTH,
        mechanism: "character-left mouth corner only: +3.4 mm × cf in x (away from midline)",
    },
    MorphMechanism {
        name: "Fac_Mth_R_Up",
        zone: MOUTH,
        mechanism: "character-right mouth corner only (|x| > 6 mm, −x side): \
                    +4.2 mm × cf in z",
    },
    MorphMechanism {
        name: "Fac_Mth_R_Down",
        zone: MOUTH,
        mechanism: "character-right mouth corner only: −4.2 mm × cf in z",
    },
    MorphMechanism {
        name: "Fac_Mth_R_In",
        zone: MOUTH,
        mechanism: "character-right mouth corner only: +3.4 mm × cf in x (toward midline)",
    },
    MorphMechanism {
        name: "Fac_Mth_R_Out",
        zone: MOUTH,
        mechanism: "character-right mouth corner only: −3.4 mm × cf in x (away from midline)",
    },
    MorphMechanism {
        name: "Fac_Pupil_Heart",
        zone: EYE,
        mechanism: "pupil verts pulled 88% toward a target point below the eye \
                    centre; HEART-zone verts set exactly onto the authored heart \
                    outline (heart-pupil presentation; Prima/Alma's, never Lana's)",
    },
];

/// Look a morph's mechanism up by name. `None` for a morph
/// the fleet builder does not emit — such a morph cannot be
/// set in a record, posed, or verdicted (Rule 10).
pub fn mechanism(name: &str) -> Option<&'static MorphMechanism> {
    MORPH_MECHANISMS.iter().find(|m| m.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fleet set of record (the MoonDeep party spec's
    /// `morphs` list, identical across the fleet builders).
    const FLEET_SET: &[&str] = &[
        "Fac_Eye_Close",
        "Fac_Eye_L_Open",
        "Fac_Eye_R_Open",
        "Fac_Eye_L_Wink",
        "Fac_Eye_R_Wink",
        "Fac_Eye_HalfClose",
        "Fac_Eye_Sad",
        "Fac_Eye_Angry",
        "Fac_Eye_MidUp",
        "Fac_Eye_MidDown",
        "Fac_Eye_LowlidUp",
        "Fac_Ebr_Up",
        "Fac_Ebr_Down",
        "Fac_Ebr_Angry",
        "Fac_Ebr_Sad",
        "Fac_Ebr_Relax",
        "Fac_Mth_Aa",
        "Fac_Mth_Ii",
        "Fac_Mth_Uu",
        "Fac_Mth_Ee",
        "Fac_Mth_Oo",
        "Fac_Mth_Talk",
        "Fac_Mth_Laugh",
        "Fac_Mth_Shout",
        "Fac_Mth_Triangle",
        "Fac_Mth_Smile",
        "Fac_Mth_Open",
        "Fac_Mth_L_Up",
        "Fac_Mth_L_Down",
        "Fac_Mth_L_In",
        "Fac_Mth_L_Out",
        "Fac_Mth_R_Up",
        "Fac_Mth_R_Down",
        "Fac_Mth_R_In",
        "Fac_Mth_R_Out",
        "Fac_Pupil_Heart",
    ];

    #[test]
    fn the_table_is_exactly_the_fleet_set() {
        assert_eq!(MORPH_MECHANISMS.len(), FLEET_SET.len());
        for name in FLEET_SET {
            assert!(mechanism(name).is_some(), "missing mechanism for {name}");
        }
        for m in MORPH_MECHANISMS {
            assert!(
                FLEET_SET.contains(&m.name),
                "table carries foreign morph {}",
                m.name
            );
            assert!(!m.mechanism.is_empty(), "{} has no mechanism", m.name);
            assert!(!m.zone.is_empty(), "{} has no zone", m.name);
        }
    }

    #[test]
    fn unknown_morphs_have_no_mechanism() {
        assert!(mechanism("Fac_Nope").is_none());
        assert!(mechanism("").is_none());
        assert!(mechanism("fac_mth_smile").is_none());
    }
}
