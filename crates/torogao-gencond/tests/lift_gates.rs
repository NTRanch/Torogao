//! The LIFT layer's crate gates (D-838, Stage 1): the scene
//! record's shape law, the freeze operations' contracts, and
//! the custody gate — half validation, half adversarial.

use torogao_gencond::custody::CustodyState;
use torogao_gencond::error::{
    CODE_CONFLICT, CODE_CUSTODY_FROZEN, CODE_FREEZE_DEVIATION, CODE_MALFORMED,
    CODE_PALETTE_UNMATCHED, CODE_UNKNOWN_KIND, CODE_UNSUPPORTED_SCHEMA,
};
use torogao_gencond::freeze::canonical_sha256;
use torogao_gencond::ron_tree::Ron;
use torogao_gencond::{
    CustodyEntry, CustodyMap, EpsilonOrigin, ExtensionState, FreezeOp, FreezeProvenance,
    FrozenTrack, MediumKind, PaletteSnap, ParameterTrack, Puppet2dRef, RenderRecipe,
    RerenderGuarantee, SceneRecord, TrackSample, conditioning_sha256, freeze_palette, freeze_track,
    track_noise_epsilon,
};

fn hash(byte: u8) -> String {
    format!("{:02x}", byte).repeat(32)
}

fn recipe() -> RenderRecipe {
    RenderRecipe {
        conditioning_sha256: hash(0x11),
        graph_sha256: hash(0x22),
        seed: 20261301,
        backend: "seam-c-worker".to_string(),
        execution_context: "primo/a6000; seam build 1; pinned".to_string(),
        rerender: RerenderGuarantee::PinnedContextByteIdentical,
    }
}

fn full_record() -> SceneRecord {
    let mut record = SceneRecord::new("vivienne-keeper-scene".to_string(), recipe());
    record.extensions[0].state = ExtensionState::Puppet2d(Puppet2dRef {
        format: "lumen-rig-model".to_string(),
        format_version: 1,
        model_sha256: hash(0x33),
    });
    record.custody = CustodyMap {
        entries: vec![
            CustodyEntry {
                node: "face".to_string(),
                state: CustodyState::Frozen {
                    by: "owner".to_string(),
                    reason: "hand-tuned expression".to_string(),
                },
            },
            CustodyEntry {
                node: "hair".to_string(),
                state: CustodyState::Open,
            },
        ],
    };
    record.provenance = vec![
        FreezeProvenance {
            op: FreezeOp::FreezeTrack,
            source_sha256: hash(0x44),
            frozen_sha256: hash(0x55),
            epsilon: Some(0.01),
            epsilon_origin: Some(EpsilonOrigin::Caller),
            tolerance: None,
        },
        FreezeProvenance {
            op: FreezeOp::FreezePalette,
            source_sha256: hash(0x66),
            frozen_sha256: hash(0x77),
            epsilon: None,
            epsilon_origin: None,
            tolerance: Some(0),
        },
    ];
    record.frozen_tracks = vec![FrozenTrack {
        parameter: "angle-x".to_string(),
        keyforms: vec![
            torogao_gencond::Keyform {
                time: 0.0,
                value: 0.0,
                tangent_in: 1.0,
                tangent_out: 1.0,
            },
            torogao_gencond::Keyform {
                time: 1.0,
                value: 1.0,
                tangent_in: 1.0,
                tangent_out: 1.0,
            },
        ],
        max_deviation: 0.0,
    }];
    record.palette_snaps = vec![PaletteSnap {
        palette_sha256: hash(0x88),
        tolerance: 0,
        pairs: vec![torogao_gencond::PalettePair {
            source: "#a03c50".to_string(),
            snapped: "#a03c50".to_string(),
        }],
    }];
    record
}

fn sine_track(samples: usize) -> ParameterTrack {
    ParameterTrack {
        parameter: "angle-x".to_string(),
        samples: (0..samples)
            .map(|i| {
                let time = i as f64 * 0.1;
                TrackSample {
                    time,
                    value: time.sin(),
                }
            })
            .collect(),
    }
}

// ---- validation -------------------------------------------------

#[test]
fn scene_record_round_trips_tree_and_json() {
    let record = full_record();
    record.validate().expect("the full record validates");
    let tree = record.to_ron_tree();
    let back = SceneRecord::from_ron_tree(tree.clone()).expect("tree parses");
    assert_eq!(back, record);
    assert_eq!(back.to_ron_tree(), tree, "tree form is a fixed point");
    let json = serde_json::to_string(&record).expect("json");
    let reparsed: SceneRecord = serde_json::from_str(&json).expect("json parses");
    assert_eq!(reparsed, record);
}

#[test]
fn new_record_states_every_block_explicitly() {
    let record = SceneRecord::new("s".to_string(), recipe());
    assert_eq!(record.extensions.len(), 4);
    for kind in MediumKind::ALL {
        let block = record
            .extensions
            .iter()
            .find(|ext| ext.kind == kind)
            .expect("every kind present");
        assert_eq!(block.state, ExtensionState::NotYetPopulated);
    }
    record.validate().expect("the empty record validates");
}

#[test]
fn freeze_track_meets_its_epsilon_and_is_deterministic() {
    let track = sine_track(101);
    let frozen = freeze_track(&track, 0.01).expect("freezes");
    assert!(
        frozen.max_deviation <= 0.01,
        "deviation {}",
        frozen.max_deviation
    );
    assert!(
        frozen.keyforms.len() < track.samples.len(),
        "a smooth track freezes to fewer keys than samples"
    );
    assert!(frozen.keyforms.len() >= 2);
    let again = freeze_track(&track, 0.01).expect("freezes again");
    assert_eq!(frozen, again);
    assert_eq!(
        canonical_sha256(&frozen).expect("hash"),
        canonical_sha256(&again).expect("hash"),
        "double-bake: byte-identical frozen output"
    );
}

#[test]
fn freeze_track_with_zero_epsilon_keys_every_sample() {
    let track = sine_track(21);
    let frozen = freeze_track(&track, 0.0).expect("exact freeze");
    assert_eq!(frozen.keyforms.len(), track.samples.len());
    assert_eq!(frozen.max_deviation, 0.0);
}

#[test]
fn noise_epsilon_is_derived_from_the_track() {
    let straight = ParameterTrack {
        parameter: "p".to_string(),
        samples: (0..11)
            .map(|i| TrackSample {
                time: i as f64,
                value: i as f64 * 2.0,
            })
            .collect(),
    };
    assert_eq!(track_noise_epsilon(&straight), 0.0);
    let frozen = freeze_track(&straight, track_noise_epsilon(&straight)).expect("freezes");
    assert_eq!(frozen.max_deviation, 0.0);
    assert!(
        frozen.keyforms.len() < straight.samples.len(),
        "a straight track freezes far below its sample count"
    );
    let noisy = ParameterTrack {
        parameter: "p".to_string(),
        samples: (0..51)
            .map(|i| TrackSample {
                time: i as f64 * 0.1,
                value: if i % 2 == 0 { 0.05 } else { -0.05 },
            })
            .collect(),
    };
    let epsilon = track_noise_epsilon(&noisy);
    assert!(epsilon > 0.0, "the zigzag's own roughness is measured");
    let frozen = freeze_track(&noisy, epsilon).expect("freezes at its own noise scale");
    assert!(frozen.max_deviation <= epsilon);
}

#[test]
fn freeze_palette_snaps_and_is_deterministic() {
    let palette = vec![
        "#000000".to_string(),
        "#A03C50".to_string(),
        "#ffffff".to_string(),
    ];
    let colors = vec![
        "#a03c51".to_string(),
        "#010101".to_string(),
        "#FFFFFF".to_string(),
    ];
    let snap = freeze_palette(&colors, &palette, 2).expect("snaps");
    assert_eq!(snap.pairs.len(), 3);
    assert_eq!(snap.pairs[0].source, "#010101");
    assert_eq!(snap.pairs[0].snapped, "#000000");
    assert_eq!(snap.pairs[1].snapped, "#a03c50");
    assert_eq!(snap.tolerance, 2);
    let again = freeze_palette(&colors, &palette, 2).expect("snaps again");
    assert_eq!(snap, again, "double-bake: byte-identical snap");
}

#[test]
fn custody_check_passes_open_absent_and_released_targets() {
    let record = full_record();
    record
        .custody
        .check_targets(&["hair".to_string(), "wardrobe/hem".to_string()])
        .expect("open and absent targets pass");
    record
        .custody
        .check_targets(&[])
        .expect("an empty target list passes");
}

#[test]
fn conditioning_hash_is_stable_and_well_formed() {
    let json = serde_json::json!({
        "schema": 1,
        "text": {"positive": [], "negative": "x"},
        "lora_stack": [],
        "sources": [],
    });
    let conditioning: torogao_gencond::Conditioning =
        serde_json::from_value(json).expect("conditioning parses");
    let first = conditioning_sha256(&conditioning).expect("hash");
    let second = conditioning_sha256(&conditioning).expect("hash");
    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
}

// ---- adversarial ------------------------------------------------

#[test]
fn custody_check_refuses_frozen_node_subtree_and_names_them() {
    let record = full_record();
    let error = record
        .custody
        .check_targets(&[
            "face".to_string(),
            "face/eyes".to_string(),
            "hair".to_string(),
        ])
        .expect_err("frozen targets refuse");
    assert_eq!(error.code, CODE_CUSTODY_FROZEN);
    assert!(
        error.message.contains("face"),
        "names the node: {}",
        error.message
    );
    assert!(
        error.message.contains("face/eyes"),
        "names the subtree target: {}",
        error.message
    );
    assert!(
        !error.message.contains("hair"),
        "the open target is not named: {}",
        error.message
    );
}

#[test]
fn unknown_medium_kind_is_refused_in_tree_and_json() {
    let record = full_record();
    let mut tree = record.to_ron_tree();
    fn poison(tree: &mut Ron) {
        if let Ron::Struct(name, fields) = tree {
            if name == "MediumExtension" {
                for (key, value) in fields.iter_mut() {
                    if key == "kind" {
                        *value = Ron::Ident("Hologram".to_string());
                    }
                }
            }
            for (_, value) in fields.iter_mut() {
                poison(value);
            }
        } else if let Ron::List(items) = tree {
            for item in items.iter_mut() {
                poison(item);
            }
        }
    }
    poison(&mut tree);
    let error = SceneRecord::from_ron_tree(tree).expect_err("unknown kind refuses");
    assert_eq!(error.code, CODE_UNKNOWN_KIND);
    assert!(error.message.contains("Hologram"));
    // The JSON wire form refuses the same kind at parse.
    let mut json = serde_json::to_value(&record).expect("json");
    json["extensions"][1]["kind"] = serde_json::json!("hologram");
    assert!(
        serde_json::from_value::<SceneRecord>(json).is_err(),
        "an unknown medium kind does not parse from JSON either"
    );
}

#[test]
fn missing_or_misplaced_medium_blocks_are_refused() {
    let mut record = full_record();
    record.extensions.remove(3);
    let error = record.validate().expect_err("three blocks refuse");
    assert_eq!(error.code, CODE_MALFORMED);
    let mut record = full_record();
    record.extensions[1].state = ExtensionState::Puppet2d(Puppet2dRef {
        format: "lumen-rig-model".to_string(),
        format_version: 1,
        model_sha256: hash(0x33),
    });
    let error = record
        .validate()
        .expect_err("a puppet ref in the armature block refuses");
    assert_eq!(error.code, CODE_CONFLICT);
}

#[test]
fn recipe_honesty_is_typed() {
    let mut record = full_record();
    record.recipe.execution_context = String::new();
    let error = record
        .validate()
        .expect_err("byte identity without a pinned context refuses");
    assert_eq!(error.code, CODE_MALFORMED);
    let mut record = full_record();
    record.recipe.rerender = RerenderGuarantee::BestEffort {
        reason: String::new(),
    };
    let error = record
        .validate()
        .expect_err("best effort without its reason refuses");
    assert_eq!(error.code, CODE_MALFORMED);
    let mut record = full_record();
    record.recipe.rerender = RerenderGuarantee::BestEffort {
        reason: "remote backend; build unknowable".to_string(),
    };
    record.recipe.execution_context = String::new();
    record.validate().expect("honest best effort validates");
}

#[test]
fn puppet_reference_must_name_lumens_format_at_the_referenced_version() {
    let mut record = full_record();
    if let ExtensionState::Puppet2d(reference) = &mut record.extensions[0].state {
        reference.format = "some-other-format".to_string();
    }
    let error = record.validate().expect_err("a foreign format refuses");
    assert_eq!(error.code, CODE_UNKNOWN_KIND);
    let mut record = full_record();
    if let ExtensionState::Puppet2d(reference) = &mut record.extensions[0].state {
        reference.format_version = 99;
    }
    let error = record
        .validate()
        .expect_err("an unreferenced version refuses");
    assert_eq!(error.code, CODE_UNSUPPORTED_SCHEMA);
}

#[test]
fn palette_beyond_tolerance_refuses_whole_and_names_the_colour() {
    let palette = vec!["#000000".to_string(), "#ffffff".to_string()];
    let colors = vec!["#010101".to_string(), "#a03c50".to_string()];
    let error = freeze_palette(&colors, &palette, 2).expect_err("the far colour refuses");
    assert_eq!(error.code, CODE_PALETTE_UNMATCHED);
    assert!(
        error.message.contains("#a03c50"),
        "names it: {}",
        error.message
    );
}

#[test]
fn track_that_cannot_meet_epsilon_within_the_key_bound_refuses() {
    let zigzag = ParameterTrack {
        parameter: "angle-x".to_string(),
        samples: (0..300)
            .map(|i| TrackSample {
                time: i as f64 * 0.01,
                value: if i % 2 == 0 { 1.0 } else { -1.0 },
            })
            .collect(),
    };
    let error = freeze_track(&zigzag, 0.0).expect_err("an unfittable contract refuses");
    assert_eq!(error.code, CODE_FREEZE_DEVIATION);
}

#[test]
fn provenance_contract_discipline_is_enforced() {
    let mut record = full_record();
    record.provenance[0].tolerance = Some(3);
    let error = record
        .validate()
        .expect_err("a track entry carrying a tolerance refuses");
    assert_eq!(error.code, CODE_CONFLICT);
    let mut record = full_record();
    record.provenance[1].epsilon = Some(0.5);
    let error = record
        .validate()
        .expect_err("a palette entry carrying an epsilon refuses");
    assert_eq!(error.code, CODE_CONFLICT);
}
