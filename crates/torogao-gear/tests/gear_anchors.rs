//! Gate T4-G1.2 (brief B2): every anchor in the canonical
//! body map resolves on the published cast rigs (Prima, Yui,
//! Alma); a fixture anchor with no fixture in scope refuses
//! TG-GEAR-ANCHOR-UNKNOWN; a fixture anchor resolves when a
//! pack-companion fixture declares it.

use torogao_gear::anchors::{BODY_MAP_V1, ContentStamps, RigTemplate};
use torogao_gear::document::{ApparelBlock, Attachment, Facing, FixtureBlock, GearClass};
use torogao_gear::emit::document_text;
use torogao_gear::examples::worked_example_payloads;
use torogao_gear::examples::{clamp_pair_document, worked_example_materials};
use torogao_gear::refusal::TG_GEAR_ANCHOR_UNKNOWN;
use torogao_gear::validate::{ValidationContext, pack_fixture_ids, validate_document};

fn apparel_on(anchor: &str) -> torogao_gear::GearDocument {
    let mut doc = clamp_pair_document();
    doc.id = "census.probe".to_string();
    doc.class = GearClass::Apparel;
    doc.anchors = vec![Attachment {
        anchor: anchor.to_string(),
        offset_x_px: 0.0,
        offset_y_px: 0.0,
        contact_radius_px: 40,
        stiffness_permille: 800,
        max_pressure_permille: 800,
        facing: Facing::Front,
    }];
    doc.sensation = None;
    doc.device = None;
    doc.apparel = Some(ApparelBlock {
        covers: Vec::new(),
        transmission_permille: 500,
    });
    doc.state_variants = GearClass::Apparel.required_variants().to_vec();
    doc.hooks = Vec::new();
    doc
}

fn validate_on(rig: &RigTemplate, doc: &torogao_gear::GearDocument, fixtures: &[String]) {
    let payloads = worked_example_payloads();
    let materials = worked_example_materials();
    let context = ValidationContext {
        document_path: "census.gear.ron".to_string(),
        rig: rig.clone(),
        payloads: &payloads,
        materials: &materials,
        pack_fixture_ids: fixtures,
        stamps: ContentStamps::GOVERNED,
    };
    validate_document(&document_text(doc), &context).unwrap_or_else(|r| {
        panic!(
            "{} on {} should validate: {}",
            doc.anchors[0].anchor,
            rig.character,
            r.verbatim()
        )
    });
}

#[test]
fn every_v1_anchor_resolves_on_every_cast_rig() {
    let rigs = RigTemplate::cast();
    assert_eq!(rigs.len(), 3);
    let mut checked = 0;
    for rig in &rigs {
        for def in BODY_MAP_V1 {
            assert!(
                rig.resolve(def.name).is_some(),
                "{} missing on {}",
                def.name,
                rig.character
            );
            validate_on(rig, &apparel_on(def.name), &[]);
            checked += 1;
        }
    }
    assert_eq!(checked, 3 * BODY_MAP_V1.len());
}

#[test]
fn nipple_anchors_derive_from_breast_anchor_and_parts() {
    // §2.4: the nipple anchors are derived, not placed — the
    // derivation must land inside the breast part's box.
    for rig in RigTemplate::cast() {
        let breast = rig.resolve("body.breast.left").expect("breast anchor");
        let nipple = rig.resolve("body.nipple.left").expect("nipple anchor");
        assert!(
            (nipple.x_px - breast.x_px).abs() <= 46.0 && (nipple.y_px - breast.y_px).abs() <= 46.0,
            "nipple outside breast box on {}",
            rig.character
        );
    }
}

#[test]
fn fixture_anchor_needs_a_fixture_in_scope() {
    let doc = apparel_on("fixture.mount-a");
    let payloads = worked_example_payloads();
    let materials = worked_example_materials();
    let context = ValidationContext {
        document_path: "census.gear.ron".to_string(),
        rig: RigTemplate::prima(),
        payloads: &payloads,
        materials: &materials,
        pack_fixture_ids: &[],
        stamps: ContentStamps::GOVERNED,
    };
    let refusal = validate_document(&document_text(&doc), &context)
        .expect_err("fixture anchor without fixture must refuse");
    assert_eq!(refusal.code, TG_GEAR_ANCHOR_UNKNOWN);
    assert_eq!(refusal.field_path, "anchors[0].anchor");
    // With a pack-companion fixture declaring it, it resolves.
    let context = ValidationContext {
        pack_fixture_ids: &["mount-a".to_string()],
        ..context
    };
    validate_document(&document_text(&doc), &context)
        .expect("fixture anchor with fixture in scope should validate");
}

#[test]
fn self_declared_fixture_anchor_resolves() {
    let mut doc = clamp_pair_document();
    doc.class = GearClass::RestraintFixture;
    doc.fixture = Some(FixtureBlock {
        declares_anchors: vec!["mount-a".to_string()],
        pose_constraints: Vec::new(),
    });
    doc.anchors[0].anchor = "fixture.mount-a".to_string();
    doc.sensation = None;
    doc.device = None;
    doc.state_variants = GearClass::RestraintFixture.required_variants().to_vec();
    doc.hooks = Vec::new();
    validate_on(&RigTemplate::prima(), &doc, &[]);
    // And pack_fixture_ids collects declarations for companions.
    assert_eq!(pack_fixture_ids(&[doc]), vec!["mount-a".to_string()]);
}
