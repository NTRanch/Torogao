//! The Playtime starter pack (AMENDMENT-PLAYMODE §7, as
//! amended by D-226: the setting is **Blossom**):
//! placeholder-class parts, labelled as such — the seat
//! fixture, the mounted device, and the §7.3 sample
//! apparel set — sufficient to stage the mode. Keeper
//! art is an Owner item (§11 item 3) and is NOT produced
//! here.
//!
//! The composition is ordinary T4 gear, applied by
//! ordinary session operations; there is no mode-private
//! equipment path (§7 of the amendment).

use crate::document::{
    ApparelBlock, Attachment, Device, DrawOrderEntry, DrawRelation, Facing, FixtureBlock,
    GearClass, GearDocument, Honesty, Hook, HookKind, PartRef, Physics, PoseConstraint, Provenance,
    Sensation, StateVariant, WaveKind, WaveSpec,
};
use crate::payloads::{MemoryPayloads, sha256_hex};

/// Payload bytes of the seat fixture's part (placeholder).
pub const SEAT_BYTES: &[u8] = b"PLACEHOLDER-ART blossom seat fixture v1";
/// Payload bytes of the mounted device's part (placeholder).
pub const MOUNT_BYTES: &[u8] = b"PLACEHOLDER-ART blossom mounted device v1";
/// Payload bytes of the sample cropped top (placeholder).
pub const TOP_BYTES: &[u8] = b"PLACEHOLDER-ART sample cropped top v1";
/// Payload bytes of the sample underwear (placeholder).
pub const UNDERWEAR_BYTES: &[u8] = b"PLACEHOLDER-ART sample glossy underwear v1";
/// Payload bytes of the sample stockings (placeholder).
pub const STOCKINGS_BYTES: &[u8] = b"PLACEHOLDER-ART sample thigh-high stockings v1";
/// Payload bytes of the sample visor (placeholder).
pub const VISOR_BYTES: &[u8] = b"PLACEHOLDER-ART sample glowing visor v1";

/// The starter pack's item ids, in pack order.
pub const STARTER_PACK_IDS: [&str; 6] = [
    "fixture.blossom-seat",
    "device.blossom-mount",
    "apparel.sample-cropped-top",
    "apparel.sample-underwear",
    "apparel.sample-stockings",
    "apparel.sample-visor",
];

/// The starter pack's material registry.
pub fn starter_pack_materials() -> Vec<String> {
    vec![
        "cloth".to_string(),
        "placeholder-grey".to_string(),
        "silicone-pink".to_string(),
    ]
}

/// The payload store carrying the starter pack's parts.
pub fn starter_pack_payloads() -> MemoryPayloads {
    let mut store = MemoryPayloads::new();
    store.insert("seat.placeholder", SEAT_BYTES.to_vec());
    store.insert("mount.placeholder", MOUNT_BYTES.to_vec());
    store.insert("top.placeholder", TOP_BYTES.to_vec());
    store.insert("underwear.placeholder", UNDERWEAR_BYTES.to_vec());
    store.insert("stockings.placeholder", STOCKINGS_BYTES.to_vec());
    store.insert("visor.placeholder", VISOR_BYTES.to_vec());
    store
}

fn provenance() -> Provenance {
    Provenance {
        author: "estate".to_string(),
        source_pack: "playtime-starter".to_string(),
        honesty: Honesty::Placeholder,
    }
}

fn attachment(anchor: &str, contact_radius_px: i64) -> Attachment {
    Attachment {
        anchor: anchor.to_string(),
        offset_x_px: 0.0,
        offset_y_px: 0.0,
        contact_radius_px,
        stiffness_permille: 900,
        max_pressure_permille: 800,
        facing: Facing::Front,
    }
}

fn part(name: &str, bytes: &[u8], texture: (i64, i64)) -> PartRef {
    PartRef {
        name: name.to_string(),
        hash: sha256_hex(bytes),
        texture: Some(texture),
    }
}

/// The seat (§7.1): a `RestraintFixture` declaring its
/// device mount, engaging the pelvis/ankle/waist
/// anchors, with bounded pose constraints on the rig
/// template's published pose parameters (seated,
/// legs-spread envelope — ranges on named parameters,
/// never per-frame writes). Its surfaces are the mode's
/// wetness hit areas (the mode data names them).
pub fn blossom_seat_document() -> GearDocument {
    GearDocument {
        format: crate::document::GEAR_FORMAT.to_string(),
        id: "fixture.blossom-seat".to_string(),
        version: 1,
        display_name: "Blossom Seat (placeholder parts)".to_string(),
        class: GearClass::RestraintFixture,
        anchors: vec![
            attachment("body.pelvis", 88),
            attachment("body.ankle.left", 44),
            attachment("body.ankle.right", 44),
            attachment("body.waist", 80),
        ],
        parts: vec![part("seat.placeholder", SEAT_BYTES, (1024, 1024))],
        materials: vec!["placeholder-grey".to_string()],
        draw_order: vec![DrawOrderEntry {
            layer: "seat".to_string(),
            relation: DrawRelation::Below,
            target: "pelvis".to_string(),
        }],
        state_variants: vec![
            StateVariant::Attached,
            StateVariant::Displaced,
            StateVariant::Removed,
        ],
        hooks: vec![
            Hook {
                kind: HookKind::AttachmentSolve,
                name: "self".to_string(),
            },
            Hook {
                kind: HookKind::BodyParam,
                name: "ParamBodyAngleY".to_string(),
            },
        ],
        physics: Physics {
            // The validator's bound is 1..=256; a fixture
            // does not displace (displaceable: false), so
            // the threshold sits at the bound's top.
            displacement_threshold_px: 256,
            displaceable: false,
        },
        sensation: None,
        device: None,
        apparel: None,
        fixture: Some(FixtureBlock {
            declares_anchors: vec!["device-mount".to_string()],
            pose_constraints: vec![
                PoseConstraint {
                    param: "ParamBodyAngleY".to_string(),
                    min_permille: 350,
                    max_permille: 650,
                },
                PoseConstraint {
                    param: "ParamBodyAngleX".to_string(),
                    min_permille: 400,
                    max_permille: 600,
                },
            ],
        }),
        effect: None,
        depends_on: Vec::new(),
        provenance: provenance(),
    }
}

/// The mounted device (§7.2): a `VibratoryDevice` seated
/// at the fixture's device mount, contact at
/// `body.perineum`, the §2.5 waveform contract,
/// mirror-capable (the capability declaration only —
/// enablement is never file data).
pub fn blossom_mount_document(amplitude_permille: i64) -> GearDocument {
    GearDocument {
        format: crate::document::GEAR_FORMAT.to_string(),
        id: "device.blossom-mount".to_string(),
        version: 1,
        display_name: "Blossom Mount (placeholder parts)".to_string(),
        class: GearClass::VibratoryDevice,
        anchors: vec![attachment("fixture.device-mount", 40)],
        parts: vec![part("mount.placeholder", MOUNT_BYTES, (512, 512))],
        materials: vec!["silicone-pink".to_string()],
        draw_order: vec![DrawOrderEntry {
            layer: "mount".to_string(),
            relation: DrawRelation::Above,
            target: "pelvis".to_string(),
        }],
        state_variants: vec![
            StateVariant::Attached,
            StateVariant::Displaced,
            StateVariant::Removed,
        ],
        hooks: vec![
            Hook {
                kind: HookKind::ArousalInput,
                name: "perineum".to_string(),
            },
            Hook {
                kind: HookKind::DeviceCycle,
                name: "self".to_string(),
            },
            Hook {
                kind: HookKind::Expression,
                name: "building".to_string(),
            },
        ],
        physics: Physics {
            displacement_threshold_px: 20,
            displaceable: true,
        },
        sensation: Some(Sensation {
            site: "perineum".to_string(),
            pressure_permille: Some(300),
            arousal_floor_permille: None,
        }),
        device: Some(Device {
            waveform: WaveSpec {
                kind: WaveKind::Constant,
                amplitude_permille,
                frequency_millihz: 0,
                duty_permille: None,
                pattern: None,
                pattern_step_ticks: None,
                phase_ticks: 0,
            },
            mirror_capable: true,
            contact_site: "perineum".to_string(),
        }),
        apparel: None,
        fixture: None,
        effect: None,
        depends_on: vec!["fixture.blossom-seat".to_string()],
        provenance: provenance(),
    }
}

/// One sample-apparel item's authoring parameters.
struct ApparelSpec {
    id: &'static str,
    display_name: &'static str,
    part_name: &'static str,
    bytes: &'static [u8],
    covers: &'static [&'static str],
    transmission_permille: i64,
    layer: &'static str,
    draw_target: &'static str,
}

fn apparel_document(spec: ApparelSpec) -> GearDocument {
    GearDocument {
        format: crate::document::GEAR_FORMAT.to_string(),
        id: spec.id.to_string(),
        version: 1,
        display_name: spec.display_name.to_string(),
        class: GearClass::Apparel,
        anchors: vec![attachment(spec.covers[0], 60)],
        parts: vec![part(spec.part_name, spec.bytes, (512, 512))],
        materials: vec!["cloth".to_string()],
        draw_order: vec![DrawOrderEntry {
            layer: spec.layer.to_string(),
            relation: DrawRelation::Above,
            target: spec.draw_target.to_string(),
        }],
        state_variants: vec![
            StateVariant::Worn,
            StateVariant::Displaced,
            StateVariant::Removed,
        ],
        hooks: vec![Hook {
            kind: HookKind::AttachmentSolve,
            name: "self".to_string(),
        }],
        physics: Physics {
            displacement_threshold_px: 30,
            displaceable: true,
        },
        sensation: None,
        device: None,
        apparel: Some(ApparelBlock {
            covers: spec.covers.iter().map(|s| s.to_string()).collect(),
            transmission_permille: spec.transmission_permille,
        }),
        fixture: None,
        effect: None,
        depends_on: Vec::new(),
        provenance: provenance(),
    }
}

/// §7.3's sample apparel set: the reference's outfit
/// treated as fixture data — a sample preference set,
/// never any girl's canonical wardrobe.
pub fn sample_apparel_documents() -> Vec<GearDocument> {
    vec![
        apparel_document(ApparelSpec {
            id: "apparel.sample-cropped-top",
            display_name: "Sample Cropped Top (placeholder parts)",
            part_name: "top.placeholder",
            bytes: TOP_BYTES,
            covers: &["body.chest"],
            transmission_permille: 0,
            layer: "top",
            draw_target: "chest",
        }),
        // The underwear authors its coverage over the
        // perineum opaque (transmission 0): §7.3's
        // mechanism — the mode stages it in the
        // displaced state at setup, and §4.4's computed
        // coverage of the displaced geometry is what the
        // contact model reads. Left worn and opaque over
        // the site, a set gets q = 0 by §4.3/§4.4 — the
        // mode does not override the contact model.
        apparel_document(ApparelSpec {
            id: "apparel.sample-underwear",
            display_name: "Sample Glossy Underwear (placeholder parts)",
            part_name: "underwear.placeholder",
            bytes: UNDERWEAR_BYTES,
            covers: &["body.perineum"],
            transmission_permille: 0,
            layer: "underwear",
            draw_target: "pelvis",
        }),
        apparel_document(ApparelSpec {
            id: "apparel.sample-stockings",
            display_name: "Sample Thigh-High Stockings (placeholder parts)",
            part_name: "stockings.placeholder",
            bytes: STOCKINGS_BYTES,
            covers: &["body.ankle.left", "body.ankle.right"],
            transmission_permille: 0,
            layer: "stockings",
            draw_target: "legs",
        }),
        // The visor is an apparel example only (§7.3):
        // the cast rigs' eye performance is never
        // occluded by mode data.
        apparel_document(ApparelSpec {
            id: "apparel.sample-visor",
            display_name: "Sample Glowing Visor (placeholder parts)",
            part_name: "visor.placeholder",
            bytes: VISOR_BYTES,
            covers: &["body.head"],
            transmission_permille: 0,
            layer: "visor",
            draw_target: "head",
        }),
    ]
}

/// The whole starter pack: fixture, device (at the
/// given §2.5 amplitude), and the sample apparel set, in
/// pack order ([`STARTER_PACK_IDS`]).
pub fn starter_pack_documents(amplitude_permille: i64) -> Vec<GearDocument> {
    let mut docs = vec![
        blossom_seat_document(),
        blossom_mount_document(amplitude_permille),
    ];
    docs.extend(sample_apparel_documents());
    docs
}
