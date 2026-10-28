//! Gate T4-G1.3 (brief B3): pack determinism — the same
//! inputs produce a byte-identical `.tgearpack` across two
//! builds; plus round-trip, tamper detection, install, and
//! the step-10 pack refusals (cycle, missing dependency).

use torogao_gear::examples::{clamp_pair_document, vibrator_document, worked_example_payloads};
use torogao_gear::pack::{build_pack, install_pack, read_pack};
use torogao_gear::payloads::{PayloadLookup, sha256_hex};
use torogao_gear::refusal::{TG_GEAR_PACK_CYCLE, TG_GEAR_PART_HASH, TG_GEAR_PART_MISSING};
use torogao_gear::validate::validate_pack;

fn fixture_docs() -> Vec<torogao_gear::GearDocument> {
    vec![clamp_pair_document(), vibrator_document(250)]
}

#[test]
fn repack_is_byte_identical() {
    let docs = fixture_docs();
    let payloads = worked_example_payloads();
    let first = build_pack(&docs, &payloads).expect("pack builds");
    let second = build_pack(&docs, &payloads).expect("pack builds");
    assert_eq!(first, second, "re-pack must be byte-identical");
    assert_eq!(sha256_hex(&first), sha256_hex(&second));
    // Round-trip: documents and payloads come back intact.
    let read = read_pack(&first).expect("pack reads");
    assert_eq!(read.document_texts.len(), 2);
    assert_eq!(read.payloads.len(), 3);
    assert_eq!(
        read.payloads["clamp-left.placeholder"],
        payloads.payload("clamp-left.placeholder").unwrap()
    );
    // And the read-back documents still validate as a pack.
    let materials = torogao_gear::examples::worked_example_materials();
    let parsed: Vec<_> = read
        .document_texts
        .iter()
        .map(|t| {
            let context = torogao_gear::surfaces::context_for(
                "pack.gear.ron",
                &torogao_gear::RigTemplate::prima(),
                &payloads,
                &materials,
                &[],
            );
            torogao_gear::validate::validate_document(t, &context).expect("validates")
        })
        .collect();
    validate_pack(&parsed, &payloads).expect("pack validates");
}

#[test]
fn tampered_pack_refuses_part_hash() {
    let docs = fixture_docs();
    let payloads = worked_example_payloads();
    let bytes = build_pack(&docs, &payloads).expect("pack builds");
    let mut tampered = bytes.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;
    let refusal = read_pack(&tampered).expect_err("tamper must refuse");
    assert_eq!(refusal.code, TG_GEAR_PART_HASH);
}

#[test]
fn pack_cycle_and_missing_dependency_refuse() {
    let mut a = clamp_pair_document();
    a.id = "cycle.a".to_string();
    a.depends_on = vec!["cycle.b".to_string()];
    let mut b = vibrator_document(250);
    b.id = "cycle.b".to_string();
    b.depends_on = vec!["cycle.a".to_string()];
    let payloads = worked_example_payloads();
    let refusal = validate_pack(&[a.clone(), b], &payloads).expect_err("cycle must refuse");
    assert_eq!(refusal.code, TG_GEAR_PACK_CYCLE);

    let mut orphan = a.clone();
    orphan.depends_on = vec!["ghost.pack-mate".to_string()];
    let refusal = validate_pack(&[orphan], &payloads).expect_err("missing dep must refuse");
    assert_eq!(refusal.code, TG_GEAR_PART_MISSING);
    assert_eq!(refusal.field_path, "depends_on");
}

#[test]
fn install_places_verified_bytes_under_data_root() {
    let docs = fixture_docs();
    let payloads = worked_example_payloads();
    let bytes = build_pack(&docs, &payloads).expect("pack builds");
    let root = std::env::temp_dir().join(format!("tgear-install-{}", std::process::id()));
    let path = install_pack(&bytes, &root, "fixture-pack").expect("install works");
    assert!(
        path.ends_with("gear/fixture-pack.tgearpack"),
        "path: {}",
        path.display()
    );
    let on_disk = std::fs::read(&path).expect("file exists");
    assert_eq!(on_disk, bytes);
    std::fs::remove_dir_all(&root).ok();
}
