//! PM-1 scope item 5: the Playtime starter pack
//! (AMENDMENT-PLAYMODE §7, D-226 naming) — the Blossom
//! seat fixture, the mounted device, and the §7.3 sample
//! apparel set validate through T4 §2.7's pipeline, pack
//! deterministically, and resolve to the engine's device
//! seam values.

use torogao_gear::anchors::{ContentStamps, RigTemplate};
use torogao_gear::document::GearClass;
use torogao_gear::emit::document_text;
use torogao_gear::pack::{build_pack, read_pack};
use torogao_gear::payloads::MemoryPayloads;
use torogao_gear::playtime_pack::{
    STARTER_PACK_IDS, blossom_mount_document, blossom_seat_document, starter_pack_documents,
    starter_pack_materials, starter_pack_payloads,
};
use torogao_gear::validate::{ValidationContext, validate_document, validate_pack};

fn ctx<'a>(
    payloads: &'a MemoryPayloads,
    materials: &'a [String],
    fixture_ids: &'a [String],
) -> ValidationContext<'a> {
    ValidationContext {
        document_path: "playtime-starter.gear.ron".to_string(),
        rig: RigTemplate::prima(),
        payloads,
        materials,
        pack_fixture_ids: fixture_ids,
        stamps: ContentStamps::GOVERNED,
    }
}

#[test]
fn starter_pack_validates_and_packs() {
    let payloads = starter_pack_payloads();
    let materials = starter_pack_materials();
    let fixture_ids = vec!["device-mount".to_string()];
    let docs = starter_pack_documents(800);
    assert_eq!(docs.len(), STARTER_PACK_IDS.len());
    // Every document validates through the §2.7 pipeline
    // (the device resolves its fixture anchor against
    // the pack companion's declaration).
    for doc in &docs {
        let text = document_text(doc);
        let validated = validate_document(&text, &ctx(&payloads, &materials, &fixture_ids))
            .unwrap_or_else(|e| panic!("{} refused: {e}", doc.id));
        assert_eq!(validated.id, doc.id);
    }
    // Pack-level validation + deterministic packing.
    validate_pack(&docs, &payloads).expect("pack validates");
    let a = build_pack(&docs, &payloads).expect("pack builds");
    let b = build_pack(&docs, &payloads).expect("pack builds");
    assert_eq!(a, b, "the same inputs pack byte-identically");
    let pack = read_pack(&a).expect("pack reads back");
    assert_eq!(pack.document_texts.len(), STARTER_PACK_IDS.len());
    for id in STARTER_PACK_IDS {
        assert!(
            pack.document_texts.iter().any(|t| t.contains(id)),
            "pack carries {id}"
        );
    }
}

#[test]
fn seat_and_mount_carry_the_mode_contract() {
    let seat = blossom_seat_document();
    assert_eq!(seat.class, GearClass::RestraintFixture);
    let fixture = seat.fixture.as_ref().expect("fixture block");
    assert_eq!(fixture.declares_anchors, vec!["device-mount".to_string()]);
    // §7.1's engaged character anchors.
    let anchors: Vec<&str> = seat.anchors.iter().map(|a| a.anchor.as_str()).collect();
    for want in [
        "body.pelvis",
        "body.ankle.left",
        "body.ankle.right",
        "body.waist",
    ] {
        assert!(anchors.contains(&want), "seat engages {want}");
    }
    assert!(!fixture.pose_constraints.is_empty());

    let mount = blossom_mount_document(800);
    assert_eq!(mount.class, GearClass::VibratoryDevice);
    let device = mount.device.as_ref().expect("device block");
    // §7.2: perineum contact, §2.5 waveform, capability
    // declared and nothing more.
    assert_eq!(device.contact_site, "perineum");
    assert!(device.mirror_capable);
    assert_eq!(mount.anchors[0].anchor, "fixture.device-mount");
    assert_eq!(
        mount.sensation.as_ref().map(|s| s.site.as_str()),
        Some("perineum")
    );
}

#[test]
fn sample_apparel_is_labelled_placeholder_data() {
    for doc in starter_pack_documents(0) {
        assert_eq!(
            doc.provenance.honesty,
            torogao_gear::document::Honesty::Placeholder,
            "{} is placeholder-class",
            doc.id
        );
        assert_eq!(doc.provenance.source_pack, "playtime-starter");
    }
    // §7.3: the underwear covers the perineum opaque —
    // the contact model's q = 0 case is authored data,
    // not a mode override.
    let docs = starter_pack_documents(0);
    let underwear = docs
        .iter()
        .find(|d| d.id == "apparel.sample-underwear")
        .expect("underwear in the set");
    let apparel = underwear.apparel.as_ref().expect("apparel block");
    assert_eq!(apparel.covers, vec!["body.perineum".to_string()]);
    assert_eq!(apparel.transmission_permille, 0);
}
