//! Gate T4-G1.4 (brief B4): the mirror-authority refusal
//! set — file attempts to enable, bind, consent, or set a
//! ceiling for mirroring are refused TG-GEAR-MIRROR-AUTHORITY
//! (D-203 §7.1: files declare capability, never authority).
//! Six variants across gear, scene, and preference-set files;
//! the gear variants go through the full validator, the scene
//! and preference-set variants through the shared authority
//! census their validators consume.

use torogao_author::ron::Ron;
use torogao_gear::anchors::RigTemplate;
use torogao_gear::authority::scan_authority_text;
use torogao_gear::emit::document_text;
use torogao_gear::examples::{
    clamp_pair_document, worked_example_materials, worked_example_payloads,
};
use torogao_gear::refusal::TG_GEAR_MIRROR_AUTHORITY;
use torogao_gear::surfaces::context_for;
use torogao_gear::validate::validate_document;

fn with_root_field(field: &str, value: Ron) -> String {
    let mut value_root = torogao_gear::emit::to_ron(&clamp_pair_document());
    if let Ron::Struct(_, fields) = &mut value_root {
        fields.push((field.to_string(), value));
    }
    torogao_author::ron::emit_document(&value_root)
}

fn with_device_field(field: &str, value: Ron) -> String {
    let mut root = torogao_gear::emit::to_ron(&torogao_gear::examples::vibrator_document(250));
    if let Ron::Struct(_, fields) = &mut root
        && let Some((_, Ron::Opt(Some(device)))) = fields.iter_mut().find(|(n, _)| n == "device")
        && let Ron::Struct(_, device_fields) = device.as_mut()
    {
        device_fields.push((field.to_string(), value));
    }
    torogao_author::ron::emit_document(&root)
}

fn refuse(text: &str) -> torogao_gear::GearRefusal {
    let payloads = worked_example_payloads();
    let materials = worked_example_materials();
    let context = context_for(
        "authority.gear.ron",
        &RigTemplate::prima(),
        &payloads,
        &materials,
        &[],
    );
    validate_document(text, &context).expect_err("authority attempt must refuse")
}

#[test]
fn six_authority_variants_refused_zero_applied() {
    // Gear file, root: mirror_enabled.
    let r = refuse(&with_root_field("mirror_enabled", Ron::Bool(true)));
    assert_eq!(
        (r.code, r.field_path.as_str()),
        (TG_GEAR_MIRROR_AUTHORITY, "mirror_enabled")
    );
    // Gear file, root: consent granted by a file.
    let r = refuse(&with_root_field("consent", Ron::Bool(true)));
    assert_eq!(
        (r.code, r.field_path.as_str()),
        (TG_GEAR_MIRROR_AUTHORITY, "consent")
    );
    // Gear file, device block: bind_device / ceiling.
    let r = refuse(&with_device_field(
        "bind_device",
        Ron::Str("ache-01".into()),
    ));
    assert_eq!(
        (r.code, r.field_path.as_str()),
        (TG_GEAR_MIRROR_AUTHORITY, "device.bind_device")
    );
    let r = refuse(&with_device_field("ceiling", Ron::Int(900)));
    assert_eq!(
        (r.code, r.field_path.as_str()),
        (TG_GEAR_MIRROR_AUTHORITY, "device.ceiling")
    );
    // Scene file: the shared census finds the authority field
    // (the scene format's validator refuses on the same code).
    let scene = "Scene(format: \"torogao-scene/1\", id: \"s1\", mirror_enabled: true)";
    assert_eq!(
        scan_authority_text(scene).as_deref(),
        Some("mirror_enabled")
    );
    // Preference-set file, nested: ceiling override.
    let prefset = "PreferenceSet(id: \"p1\", device: DevicePrefs(ceiling_override: 500))";
    assert_eq!(
        scan_authority_text(prefset).as_deref(),
        Some("device.ceiling_override")
    );
    // Capability stays legal: mirror_capable validates fine.
    let text = document_text(&torogao_gear::examples::vibrator_document(250));
    let payloads = worked_example_payloads();
    let materials = worked_example_materials();
    let context = context_for(
        "ok.gear.ron",
        &RigTemplate::prima(),
        &payloads,
        &materials,
        &[],
    );
    validate_document(&text, &context).expect("mirror_capable must validate");
    assert_eq!(scan_authority_text(&text), None);
}
