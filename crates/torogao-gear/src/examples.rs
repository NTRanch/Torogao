//! The worked-example gear (BRIEF scope item 7):
//! placeholder-class parts, labelled as such — a nipple-clamp
//! pair and a wearable vibrator, sufficient to run the
//! fixtures. Keeper art is an Owner item (AMENDMENT-T4 §9.1)
//! and is NOT produced here.

use crate::document::{
    Attachment, Device, DrawOrderEntry, DrawRelation, Facing, GearClass, GearDocument, Honesty,
    Hook, HookKind, PartRef, Physics, Provenance, Sensation, StateVariant, WaveKind, WaveSpec,
};
use crate::payloads::{MemoryPayloads, sha256_hex};

/// Payload bytes of the clamp pair's left part (placeholder).
pub const CLAMP_LEFT_BYTES: &[u8] = b"PLACEHOLDER-ART clamp left v1";
/// Payload bytes of the clamp pair's right part (placeholder).
pub const CLAMP_RIGHT_BYTES: &[u8] = b"PLACEHOLDER-ART clamp right v1";
/// Payload bytes of the vibrator's body part (placeholder).
pub const VIBE_BODY_BYTES: &[u8] = b"PLACEHOLDER-ART vibrator body v1";

/// The worked examples' material registry.
pub fn worked_example_materials() -> Vec<String> {
    vec!["placeholder-grey".to_string(), "silicone-pink".to_string()]
}

/// The payload store carrying the worked examples' parts.
pub fn worked_example_payloads() -> MemoryPayloads {
    let mut store = MemoryPayloads::new();
    store.insert("clamp-left.placeholder", CLAMP_LEFT_BYTES.to_vec());
    store.insert("clamp-right.placeholder", CLAMP_RIGHT_BYTES.to_vec());
    store.insert("vibe-body.placeholder", VIBE_BODY_BYTES.to_vec());
    store
}

fn provenance() -> Provenance {
    Provenance {
        author: "estate".to_string(),
        source_pack: "worked-examples".to_string(),
        honesty: Honesty::Placeholder,
    }
}

/// The nipple-clamp pair: one `AttachedDevice` item, two
/// attachments (one per nipple anchor), floor 320 (§8.2's
/// fixture item). Attaches and removes as one user act; the
/// state record keeps both sides separately.
pub fn clamp_pair_document() -> GearDocument {
    let nip = |anchor: &str| Attachment {
        anchor: anchor.to_string(),
        offset_x_px: 0.0,
        offset_y_px: 0.0,
        contact_radius_px: 30,
        stiffness_permille: 900,
        max_pressure_permille: 800,
        facing: Facing::Front,
    };
    GearDocument {
        format: crate::document::GEAR_FORMAT.to_string(),
        id: "estate.clamp-pair".to_string(),
        version: 1,
        display_name: "Clamp Pair (placeholder parts)".to_string(),
        class: GearClass::AttachedDevice,
        anchors: vec![nip("body.nipple.left"), nip("body.nipple.right")],
        parts: vec![
            PartRef {
                name: "clamp-left.placeholder".to_string(),
                hash: sha256_hex(CLAMP_LEFT_BYTES),
                texture: Some((256, 256)),
            },
            PartRef {
                name: "clamp-right.placeholder".to_string(),
                hash: sha256_hex(CLAMP_RIGHT_BYTES),
                texture: Some((256, 256)),
            },
        ],
        materials: vec!["placeholder-grey".to_string()],
        draw_order: vec![DrawOrderEntry {
            layer: "clamp-left".to_string(),
            relation: DrawRelation::Above,
            target: "breast".to_string(),
        }],
        state_variants: vec![
            StateVariant::Attached,
            StateVariant::Displaced,
            StateVariant::Removed,
        ],
        hooks: vec![
            Hook {
                kind: HookKind::ArousalInput,
                name: "nipple".to_string(),
            },
            Hook {
                kind: HookKind::AttachmentSolve,
                name: "self".to_string(),
            },
            Hook {
                kind: HookKind::Expression,
                name: "aroused".to_string(),
            },
        ],
        physics: Physics {
            displacement_threshold_px: 24,
            displaceable: true,
        },
        sensation: Some(Sensation {
            site: "nipple".to_string(),
            pressure_permille: Some(800),
            arousal_floor_permille: Some(320),
        }),
        device: None,
        apparel: None,
        fixture: None,
        effect: None,
        depends_on: Vec::new(),
        provenance: provenance(),
    }
}

/// The wearable vibrator: a `VibratoryDevice` with a constant
/// waveform at the given amplitude, mirror-capable (the
/// capability declaration only — §2.1).
pub fn vibrator_document(amplitude_permille: i64) -> GearDocument {
    GearDocument {
        format: crate::document::GEAR_FORMAT.to_string(),
        id: "estate.wearable-vibrator".to_string(),
        version: 1,
        display_name: "Wearable Vibrator (placeholder parts)".to_string(),
        class: GearClass::VibratoryDevice,
        anchors: vec![Attachment {
            anchor: "body.perineum".to_string(),
            offset_x_px: 0.0,
            offset_y_px: 0.0,
            contact_radius_px: 40,
            stiffness_permille: 850,
            max_pressure_permille: 700,
            facing: Facing::Front,
        }],
        parts: vec![PartRef {
            name: "vibe-body.placeholder".to_string(),
            hash: sha256_hex(VIBE_BODY_BYTES),
            texture: Some((512, 512)),
        }],
        materials: vec!["silicone-pink".to_string()],
        draw_order: vec![DrawOrderEntry {
            layer: "vibe-body".to_string(),
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
        depends_on: Vec::new(),
        provenance: provenance(),
    }
}

/// The vibrator with a pulse waveform (the second worked
/// waveform family of the brief's item 7).
pub fn vibrator_pulse_document(amplitude_permille: i64) -> GearDocument {
    let mut doc = vibrator_document(amplitude_permille);
    doc.id = "estate.wearable-vibrator-pulse".to_string();
    if let Some(device) = &mut doc.device {
        device.waveform = WaveSpec {
            kind: WaveKind::Pulse,
            amplitude_permille,
            frequency_millihz: 2000,
            duty_permille: Some(500),
            pattern: None,
            pattern_step_ticks: None,
            phase_ticks: 0,
        };
    }
    doc
}

/// The vibrator with a triangle-wave waveform (the third
/// worked waveform family).
pub fn vibrator_wave_document(amplitude_permille: i64) -> GearDocument {
    let mut doc = vibrator_document(amplitude_permille);
    doc.id = "estate.wearable-vibrator-wave".to_string();
    if let Some(device) = &mut doc.device {
        device.waveform = WaveSpec {
            kind: WaveKind::Wave,
            amplitude_permille,
            frequency_millihz: 1500,
            duty_permille: None,
            pattern: None,
            pattern_step_ticks: None,
            phase_ticks: 0,
        };
    }
    doc
}
