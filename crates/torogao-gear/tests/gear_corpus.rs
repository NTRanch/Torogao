//! Gate T4-G1.1 (brief B1): the validation corpus — 24 valid
//! documents across all five classes accepted, 24 adversarial
//! documents refused with the exact §2.7 code and field path.
//! Plus T4-G1.4's pack-duplicate case (corpus item 24) and
//! the worked examples' round-trip.

use torogao_author::ron::Ron;
use torogao_gear::anchors::{ContentStamps, RigTemplate};
use torogao_gear::document::{
    ApparelBlock, Attachment, Device, DrawOrderEntry, DrawRelation, EffectBlock, Facing,
    FixtureBlock, GearClass, GearDocument, Honesty, Hook, HookKind, PartRef, Physics,
    PoseConstraint, Provenance, Sensation, WaveKind, WaveSpec,
};
use torogao_gear::emit::document_text;
use torogao_gear::payloads::{MemoryPayloads, sha256_hex};
use torogao_gear::refusal::{
    TG_GEAR_ANCHOR_UNKNOWN, TG_GEAR_BOUND, TG_GEAR_CLASS_INCOMPLETE, TG_GEAR_CONSENT_STAMP,
    TG_GEAR_FIELD_UNKNOWN, TG_GEAR_HOOK_DENIED, TG_GEAR_MIRROR_AUTHORITY, TG_GEAR_PACK_DUPLICATE,
    TG_GEAR_PART_HASH, TG_GEAR_PART_MISSING, TG_GEAR_SCHEMA, TG_GEAR_WAVEFORM,
};
use torogao_gear::validate::{ValidationContext, validate_document, validate_pack};

const PART_A: &[u8] = b"corpus part a bytes";
const PART_B: &[u8] = b"corpus part b bytes";

fn corpus_payloads() -> MemoryPayloads {
    let mut store = MemoryPayloads::new();
    store.insert("corpus-part-a", PART_A.to_vec());
    store.insert("corpus-part-b", PART_B.to_vec());
    store
}

fn corpus_materials() -> Vec<String> {
    ["cloth", "placeholder-grey", "silicone-pink", "steel"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn ctx<'a>(
    payloads: &'a MemoryPayloads,
    materials: &'a [String],
    fixture_ids: &'a [String],
) -> ValidationContext<'a> {
    ValidationContext {
        document_path: "corpus.gear.ron".to_string(),
        rig: RigTemplate::prima(),
        payloads,
        materials,
        pack_fixture_ids: fixture_ids,
        stamps: ContentStamps::GOVERNED,
    }
}

fn attachment(anchor: &str) -> Attachment {
    Attachment {
        anchor: anchor.to_string(),
        offset_x_px: 0.0,
        offset_y_px: 0.0,
        contact_radius_px: 40,
        stiffness_permille: 800,
        max_pressure_permille: 800,
        facing: Facing::Front,
    }
}

fn part(name: &str, bytes: &[u8]) -> PartRef {
    PartRef {
        name: name.to_string(),
        hash: sha256_hex(bytes),
        texture: Some((128, 128)),
    }
}

fn base(id: &str, class: GearClass) -> GearDocument {
    GearDocument {
        format: "torogao-gear/1".to_string(),
        id: id.to_string(),
        version: 1,
        display_name: format!("Corpus {id}"),
        class,
        anchors: vec![attachment("body.chest")],
        parts: vec![part("corpus-part-a", PART_A)],
        materials: vec!["cloth".to_string()],
        draw_order: vec![DrawOrderEntry {
            layer: "main".to_string(),
            relation: DrawRelation::Above,
            target: "body".to_string(),
        }],
        state_variants: class.required_variants().to_vec(),
        hooks: Vec::new(),
        physics: Physics {
            displacement_threshold_px: 24,
            displaceable: true,
        },
        sensation: None,
        device: None,
        apparel: None,
        fixture: None,
        effect: None,
        depends_on: Vec::new(),
        provenance: Provenance {
            author: "estate".to_string(),
            source_pack: "corpus".to_string(),
            honesty: Honesty::Authored,
        },
    }
}

fn wave(kind: WaveKind) -> WaveSpec {
    let mut spec = WaveSpec {
        kind,
        amplitude_permille: 250,
        frequency_millihz: 0,
        duty_permille: None,
        pattern: None,
        pattern_step_ticks: None,
        phase_ticks: 0,
    };
    match kind {
        WaveKind::Pulse => {
            spec.frequency_millihz = 2000;
            spec.duty_permille = Some(500);
        }
        WaveKind::Wave => spec.frequency_millihz = 1500,
        WaveKind::Ramp => spec.frequency_millihz = 1000,
        WaveKind::Pattern => {
            spec.pattern = Some(vec![200, 800]);
            spec.pattern_step_ticks = Some(30);
        }
        WaveKind::Constant => {}
    }
    spec
}

/// The 24 valid documents: 5 apparel, 5 attached, 5
/// vibratory (one per waveform kind), 5 restraint, 4 effect.
fn valid_documents() -> Vec<GearDocument> {
    let mut docs = Vec::new();
    for (i, anchor) in [
        "body.chest",
        "body.waist",
        "body.neck",
        "body.pelvis",
        "body.breast.right",
    ]
    .iter()
    .enumerate()
    {
        let mut doc = base(&format!("corpus.apparel-{i}"), GearClass::Apparel);
        doc.anchors = vec![attachment(anchor)];
        doc.apparel = Some(ApparelBlock {
            covers: vec![anchor.to_string()],
            transmission_permille: 100 * i as i64,
        });
        doc.hooks = vec![Hook {
            kind: HookKind::BodyParam,
            name: "ParamBreath".into(),
        }];
        docs.push(doc);
    }
    for (i, (anchor, site)) in [
        ("body.nipple.left", "nipple"),
        ("body.nipple.right", "nipple"),
        ("body.perineum", "perineum"),
        ("body.chest", "chest"),
        ("body.face.left", "face"),
    ]
    .iter()
    .enumerate()
    {
        let mut doc = base(&format!("corpus.attached-{i}"), GearClass::AttachedDevice);
        doc.anchors = vec![attachment(anchor)];
        doc.sensation = Some(Sensation {
            site: site.to_string(),
            pressure_permille: Some(200 + 100 * i as i64),
            arousal_floor_permille: if i == 0 { Some(320) } else { None },
        });
        doc.hooks = vec![Hook {
            kind: HookKind::ArousalInput,
            name: site.to_string(),
        }];
        docs.push(doc);
    }
    for (i, kind) in [
        WaveKind::Constant,
        WaveKind::Pulse,
        WaveKind::Wave,
        WaveKind::Pattern,
        WaveKind::Ramp,
    ]
    .iter()
    .enumerate()
    {
        let mut doc = base(&format!("corpus.vibratory-{i}"), GearClass::VibratoryDevice);
        doc.anchors = vec![attachment("body.perineum")];
        doc.sensation = Some(Sensation {
            site: "perineum".to_string(),
            pressure_permille: Some(300),
            arousal_floor_permille: None,
        });
        doc.device = Some(Device {
            waveform: wave(*kind),
            mirror_capable: i % 2 == 0,
            contact_site: "perineum".to_string(),
        });
        doc.hooks = vec![Hook {
            kind: HookKind::DeviceCycle,
            name: "self".into(),
        }];
        docs.push(doc);
    }
    for (i, anchor) in [
        "body.wrist.left",
        "body.wrist.right",
        "body.ankle.left",
        "body.ankle.right",
        "body.waist",
    ]
    .iter()
    .enumerate()
    {
        let mut doc = base(
            &format!("corpus.restraint-{i}"),
            GearClass::RestraintFixture,
        );
        doc.anchors = vec![attachment(anchor)];
        doc.fixture = Some(FixtureBlock {
            declares_anchors: Vec::new(),
            pose_constraints: vec![PoseConstraint {
                param: "ParamBodyAngleY".to_string(),
                min_permille: 100,
                max_permille: 900,
            }],
        });
        doc.hooks = vec![Hook {
            kind: HookKind::BodyParam,
            name: "ParamBodyAngleY".into(),
        }];
        docs.push(doc);
    }
    for (i, output) in ["blush", "sweat", "sheen", "tremble"].iter().enumerate() {
        let mut doc = base(&format!("corpus.effect-{i}"), GearClass::Effect);
        doc.anchors = vec![attachment("body.face.left")];
        doc.effect = Some(EffectBlock {
            inputs: vec!["arousal".to_string()],
            outputs: vec![output.to_string()],
        });
        doc.hooks = vec![Hook {
            kind: HookKind::EffectParam,
            name: output.to_string(),
        }];
        docs.push(doc);
    }
    docs
}

#[test]
fn valid_corpus_24_of_24_accepted() {
    let payloads = corpus_payloads();
    let materials = corpus_materials();
    let fixtures: Vec<String> = Vec::new();
    let docs = valid_documents();
    assert_eq!(docs.len(), 24);
    for doc in &docs {
        let text = document_text(doc);
        let parsed = validate_document(&text, &ctx(&payloads, &materials, &fixtures))
            .unwrap_or_else(|r| panic!("{} should validate: {}", doc.id, r.verbatim()));
        assert_eq!(&parsed, doc, "round-trip mismatch for {}", doc.id);
    }
}

// ---- adversarial corpus -------------------------------------

fn fields_of(value: &mut Ron) -> &mut Vec<(String, Ron)> {
    match value {
        Ron::Struct(_, fields) => fields,
        other => panic!("expected struct, got {other:?}"),
    }
}

fn set_field(fields: &mut Vec<(String, Ron)>, name: &str, value: Ron) {
    if let Some(slot) = fields.iter_mut().find(|(n, _)| n == name) {
        slot.1 = value;
    } else {
        fields.push((name.to_string(), value));
    }
}

fn nested_fields<'a>(fields: &'a mut Vec<(String, Ron)>, name: &str) -> &'a mut Vec<(String, Ron)> {
    for (n, value) in fields.iter_mut() {
        if n == name {
            return match value {
                Ron::Struct(_, inner) => inner,
                Ron::Opt(Some(inner)) => match inner.as_mut() {
                    Ron::Struct(_, inner_fields) => inner_fields,
                    other => panic!("expected struct inside Some, got {other:?}"),
                },
                other => panic!("expected struct, got {other:?}"),
            };
        }
    }
    panic!("field {name} not found");
}

fn mutate(doc: &GearDocument, f: impl Fn(&mut Vec<(String, Ron)>)) -> String {
    let mut value = torogao_gear::emit::to_ron(doc);
    f(fields_of(&mut value));
    torogao_author::ron::emit_document(&value)
}

struct Adversarial {
    name: &'static str,
    text: String,
    bad_stamps: bool,
    code: &'static str,
    field_path: &'static str,
}

fn adversarial_corpus(docs: &[GearDocument]) -> Vec<Adversarial> {
    let apparel = &docs[0];
    let attached = &docs[5];
    let vibratory = &docs[10];
    let mut out: Vec<Adversarial> = Vec::new();
    let mut push = |name, text, code, field_path| {
        out.push(Adversarial {
            name,
            text,
            bad_stamps: false,
            code,
            field_path,
        });
    };
    push(
        "schema-version",
        mutate(apparel, |f| {
            set_field(f, "format", Ron::Str("torogao-gear/2".into()));
        }),
        TG_GEAR_SCHEMA,
        "format",
    );
    let full = document_text(apparel);
    push(
        "schema-malformed",
        full[..full.len() - 10].to_string(),
        TG_GEAR_SCHEMA,
        "$",
    );
    push(
        "unknown-root-field",
        mutate(apparel, |f| {
            f.push(("bogus_field".into(), Ron::Int(1)));
        }),
        TG_GEAR_FIELD_UNKNOWN,
        "bogus_field",
    );
    push(
        "unknown-nested-field",
        mutate(apparel, |f| {
            nested_fields(f, "physics").push(("bogus".into(), Ron::Int(2)));
        }),
        TG_GEAR_FIELD_UNKNOWN,
        "physics.bogus",
    );
    push(
        "apparel-block-missing",
        mutate(apparel, |f| {
            set_field(f, "apparel", Ron::Opt(None));
        }),
        TG_GEAR_CLASS_INCOMPLETE,
        "apparel",
    );
    push(
        "device-block-missing",
        mutate(vibratory, |f| {
            set_field(f, "device", Ron::Opt(None));
        }),
        TG_GEAR_CLASS_INCOMPLETE,
        "device",
    );
    push(
        "variant-missing",
        mutate(apparel, |f| {
            set_field(
                f,
                "state_variants",
                Ron::List(vec![
                    Ron::Ident("Worn".into()),
                    Ron::Ident("Removed".into()),
                ]),
            );
        }),
        TG_GEAR_CLASS_INCOMPLETE,
        "state_variants",
    );
    let device_block = Ron::Opt(Some(Box::new(Ron::Struct(
        "Device".into(),
        vec![
            ("contact_site".into(), Ron::Str("perineum".into())),
            ("mirror_capable".into(), Ron::Bool(true)),
            (
                "waveform".into(),
                Ron::Struct(
                    "WaveSpec".into(),
                    vec![
                        ("amplitude_permille".into(), Ron::Int(250)),
                        ("duty_permille".into(), Ron::Opt(None)),
                        ("frequency_millihz".into(), Ron::Int(0)),
                        ("kind".into(), Ron::Ident("Constant".into())),
                        ("pattern".into(), Ron::Opt(None)),
                        ("pattern_step_ticks".into(), Ron::Opt(None)),
                        ("phase_ticks".into(), Ron::Int(0)),
                    ],
                ),
            ),
        ],
    ))));
    push(
        "foreign-device-block",
        mutate(attached, |f| {
            set_field(f, "device", device_block.clone());
        }),
        TG_GEAR_CLASS_INCOMPLETE,
        "device",
    );
    push(
        "anchor-unknown",
        mutate(apparel, |f| {
            let anchors = match f.iter_mut().find(|(n, _)| n == "anchors") {
                Some((_, Ron::List(items))) => items,
                _ => panic!("anchors list"),
            };
            set_field(
                fields_of(&mut anchors[0]),
                "anchor",
                Ron::Str("body.elbow.left".into()),
            );
        }),
        TG_GEAR_ANCHOR_UNKNOWN,
        "anchors[0].anchor",
    );
    push(
        "fixture-anchor-unknown",
        mutate(apparel, |f| {
            let anchors = match f.iter_mut().find(|(n, _)| n == "anchors") {
                Some((_, Ron::List(items))) => items,
                _ => panic!("anchors list"),
            };
            set_field(
                fields_of(&mut anchors[0]),
                "anchor",
                Ron::Str("fixture.ghost".into()),
            );
        }),
        TG_GEAR_ANCHOR_UNKNOWN,
        "anchors[0].anchor",
    );
    push(
        "pressure-bound",
        mutate(attached, |f| {
            set_field(
                nested_fields(f, "sensation"),
                "pressure_permille",
                Ron::Opt(Some(Box::new(Ron::Int(1001)))),
            );
        }),
        TG_GEAR_BOUND,
        "sensation.pressure_permille",
    );
    push(
        "floor-bound",
        mutate(attached, |f| {
            set_field(
                nested_fields(f, "sensation"),
                "arousal_floor_permille",
                Ron::Opt(Some(Box::new(Ron::Int(601)))),
            );
        }),
        TG_GEAR_BOUND,
        "sensation.arousal_floor_permille",
    );
    push(
        "texture-bound",
        mutate(apparel, |f| {
            let parts = match f.iter_mut().find(|(n, _)| n == "parts") {
                Some((_, Ron::List(items))) => items,
                _ => panic!("parts list"),
            };
            set_field(
                fields_of(&mut parts[0]),
                "texture",
                Ron::Opt(Some(Box::new(Ron::Tuple(vec![
                    Ron::Int(8193),
                    Ron::Int(128),
                ])))),
            );
        }),
        TG_GEAR_BOUND,
        "parts[0].texture",
    );
    push(
        "parts-count-bound",
        mutate(apparel, |f| {
            let slot = f.iter_mut().find(|(n, _)| n == "parts").expect("parts");
            if let Ron::List(items) = &mut slot.1 {
                let first = items[0].clone();
                for _ in 1..129 {
                    items.push(first.clone());
                }
            }
        }),
        TG_GEAR_BOUND,
        "parts",
    );
    push(
        "part-missing",
        mutate(apparel, |f| {
            let parts = match f.iter_mut().find(|(n, _)| n == "parts") {
                Some((_, Ron::List(items))) => items,
                _ => panic!("parts list"),
            };
            set_field(
                fields_of(&mut parts[0]),
                "name",
                Ron::Str("ghost.placeholder".into()),
            );
        }),
        TG_GEAR_PART_MISSING,
        "parts[0].name",
    );
    push(
        "part-hash",
        mutate(apparel, |f| {
            let parts = match f.iter_mut().find(|(n, _)| n == "parts") {
                Some((_, Ron::List(items))) => items,
                _ => panic!("parts list"),
            };
            set_field(fields_of(&mut parts[0]), "hash", Ron::Str("0".repeat(64)));
        }),
        TG_GEAR_PART_HASH,
        "parts[0].hash",
    );
    push(
        "hook-bodyparam-denied",
        mutate(apparel, |f| {
            set_field(
                f,
                "hooks",
                Ron::List(vec![Ron::Struct(
                    "Hook".into(),
                    vec![
                        ("kind".into(), Ron::Ident("BodyParam".into())),
                        ("name".into(), Ron::Str("ParamEyeRoll".into())),
                    ],
                )]),
            );
        }),
        TG_GEAR_HOOK_DENIED,
        "hooks[0]",
    );
    push(
        "hook-expression-denied",
        mutate(apparel, |f| {
            set_field(
                f,
                "hooks",
                Ron::List(vec![Ron::Struct(
                    "Hook".into(),
                    vec![
                        ("kind".into(), Ron::Ident("Expression".into())),
                        ("name".into(), Ron::Str("smug".into())),
                    ],
                )]),
            );
        }),
        TG_GEAR_HOOK_DENIED,
        "hooks[0]",
    );
    push(
        "waveform-pattern-missing",
        mutate(vibratory, |f| {
            set_field(
                nested_fields(f, "device"),
                "waveform",
                Ron::Struct(
                    "WaveSpec".into(),
                    vec![
                        ("amplitude_permille".into(), Ron::Int(250)),
                        ("duty_permille".into(), Ron::Opt(None)),
                        ("frequency_millihz".into(), Ron::Int(0)),
                        ("kind".into(), Ron::Ident("Pattern".into())),
                        ("pattern".into(), Ron::Opt(None)),
                        ("pattern_step_ticks".into(), Ron::Opt(None)),
                        ("phase_ticks".into(), Ron::Int(0)),
                    ],
                ),
            );
        }),
        TG_GEAR_WAVEFORM,
        "device.waveform.pattern",
    );
    push(
        "waveform-constant-duty",
        mutate(vibratory, |f| {
            let wave = nested_fields(f, "device");
            set_field(
                nested_fields(wave, "waveform"),
                "duty_permille",
                Ron::Opt(Some(Box::new(Ron::Int(500)))),
            );
        }),
        TG_GEAR_WAVEFORM,
        "device.waveform",
    );
    push(
        "waveform-amplitude-bound",
        mutate(vibratory, |f| {
            let wave = nested_fields(f, "device");
            set_field(
                nested_fields(wave, "waveform"),
                "amplitude_permille",
                Ron::Int(1001),
            );
        }),
        TG_GEAR_WAVEFORM,
        "device.waveform.amplitude_permille",
    );
    push(
        "mirror-authority",
        mutate(apparel, |f| {
            f.push(("mirror_enabled".into(), Ron::Bool(true)));
        }),
        TG_GEAR_MIRROR_AUTHORITY,
        "mirror_enabled",
    );
    out.push(Adversarial {
        name: "consent-stamp",
        text: document_text(apparel),
        bad_stamps: true,
        code: TG_GEAR_CONSENT_STAMP,
        field_path: "target",
    });
    out
}

#[test]
fn adversarial_corpus_refused_with_exact_codes() {
    let payloads = corpus_payloads();
    let materials = corpus_materials();
    let fixtures: Vec<String> = Vec::new();
    let docs = valid_documents();
    let cases = adversarial_corpus(&docs);
    assert_eq!(cases.len(), 23, "23 document-level adversarial cases");
    for case in &cases {
        let mut context = ctx(&payloads, &materials, &fixtures);
        if case.bad_stamps {
            context.stamps = ContentStamps {
                adult: false,
                consent_forward: true,
            };
        }
        let refusal = validate_document(&case.text, &context)
            .expect_err(&format!("{} should be refused", case.name));
        assert_eq!(
            (refusal.code, refusal.field_path.as_str()),
            (case.code, case.field_path),
            "case {}",
            case.name
        );
    }
    // Corpus item 24: the pack-level adversarial — a duplicate
    // id inside one pack (step 10).
    let dup = vec![docs[0].clone(), docs[0].clone()];
    let refusal = validate_pack(&dup, &payloads).expect_err("duplicate should refuse");
    assert_eq!(
        (refusal.code, refusal.field_path.as_str()),
        (TG_GEAR_PACK_DUPLICATE, "id")
    );
}

#[test]
fn worked_examples_validate_on_all_cast_rigs() {
    use torogao_gear::examples::{
        clamp_pair_document, vibrator_document, vibrator_pulse_document, vibrator_wave_document,
        worked_example_materials, worked_example_payloads,
    };
    let payloads = worked_example_payloads();
    let materials = worked_example_materials();
    let fixtures: Vec<String> = Vec::new();
    let docs = [
        clamp_pair_document(),
        vibrator_document(250),
        vibrator_pulse_document(500),
        vibrator_wave_document(750),
    ];
    for rig in RigTemplate::cast() {
        for doc in &docs {
            let context = ValidationContext {
                document_path: "worked.gear.ron".to_string(),
                rig: rig.clone(),
                payloads: &payloads,
                materials: &materials,
                pack_fixture_ids: &fixtures,
                stamps: ContentStamps::GOVERNED,
            };
            validate_document(&document_text(doc), &context).unwrap_or_else(|r| {
                panic!(
                    "{} on {} should validate: {}",
                    doc.id,
                    rig.character,
                    r.verbatim()
                )
            });
        }
    }
    // The clamp pair carries the F-CLAMP floor.
    assert_eq!(clamp_pair_document().arousal_floor_permille(), Some(320));
}
