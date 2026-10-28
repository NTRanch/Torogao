//! The ordered validation pipeline (AMENDMENT-T4 §2.7).
//!
//! Validation runs in the spec's order and returns the FIRST
//! refusal: (1) parse + schema, (2) unknown-field census,
//! (3) class completeness, (3.5) target consent stamps,
//! (4) anchor resolution, (5) numeric bounds, (6) part /
//! material existence + hash, (7) hook whitelist, (8) waveform
//! bounds, (9) authority census, (10) pack consistency
//! ([`validate_pack`], a separate entry point because it spans
//! documents).
//!
//! The validator is a pure library function: the Creator
//! surface and the test corpus consume this same code path,
//! headlessly.

use std::collections::{BTreeMap, BTreeSet};

use torogao_author::ron::Ron;

use crate::anchors::{
    ContentStamps, RigTemplate, body_anchor, fixture_anchor_id, is_fixture_anchor,
};
use crate::authority::find_authority_field;
use crate::document::{GEAR_FORMAT, GearClass, GearDocument, HookKind, WaveKind};
use crate::extract::{census_unknown_fields, extract_document};
use crate::payloads::{PayloadLookup, sha256_hex};
use crate::refusal::{
    GearRefusal, GearResult, TG_GEAR_ANCHOR_UNKNOWN, TG_GEAR_BOUND, TG_GEAR_CLASS_INCOMPLETE,
    TG_GEAR_CONSENT_STAMP, TG_GEAR_HOOK_DENIED, TG_GEAR_MIRROR_AUTHORITY, TG_GEAR_PACK_CYCLE,
    TG_GEAR_PACK_DUPLICATE, TG_GEAR_PART_HASH, TG_GEAR_PART_MISSING, TG_GEAR_SCHEMA,
    TG_GEAR_WAVEFORM,
};

/// Attachments per item, maximum (§2.7 bounds family).
pub const ATTACHMENTS_MAX: usize = 32;
/// Part payloads per item, maximum (§2.7).
pub const PARTS_MAX: usize = 128;
/// Texture axis maximum in pixels (§2.7).
pub const TEXTURE_AXIS_MAX_PX: i64 = 8192;
/// Total payload bytes per pack, maximum (§2.7 step 10).
pub const PACK_PAYLOAD_BYTES_MAX: usize = 64 * 1024 * 1024;
/// Pattern steps maximum (§2.5).
pub const PATTERN_STEPS_MAX: usize = 64;

/// Body-parameter channels the rig template publishes for
/// hooks (§2.8): breath, posture, and the breast/hair
/// secondary channels. Anything else — including raw rig
/// parameters like `ParamEyeRoll` — is denied.
pub const PUBLISHED_BODY_PARAMS: &[&str] = &[
    "ParamAngleX",
    "ParamAngleY",
    "ParamAngleZ",
    "ParamBodyAngleX",
    "ParamBodyAngleY",
    "ParamBodyAngleZ",
    "ParamBreastSecondary",
    "ParamBreath",
    "ParamHairSecondary",
];

/// Fluid emitters gear may hook (§6.4).
pub const FLUID_EMITTERS: &[&str] = &["milk", "saliva", "spray"];
/// Effect parameters gear may hook (§6).
pub const EFFECT_PARAMS: &[&str] = &["blush", "sheen", "sweat", "tremble"];
/// Arousal-engine state fields (§5.2): readable by effects,
/// writable by NOTHING in a gear file.
pub const STATE_FIELDS: &[&str] = &[
    "afterglow_level",
    "arousal",
    "dose",
    "floor",
    "peak_input",
    "phase",
    "pleasure_charge",
    "sensitivity",
];
/// Contact events an effect may read (§2.3 E).
pub const CONTACT_EVENTS: &[&str] = &["contact_enter", "contact_leave"];
/// Sensation site names (the §5.3 profile seam's keys).
pub const SITES: &[&str] = &[
    "breast", "chest", "face", "mouth", "neck", "nipple", "pelvis", "perineum", "waist",
];

/// Everything validation needs beyond the document text.
pub struct ValidationContext<'a> {
    /// Document path carried into every refusal.
    pub document_path: String,
    /// The rig template anchors resolve against.
    pub rig: RigTemplate,
    /// The payload store part references resolve against.
    pub payloads: &'a dyn PayloadLookup,
    /// The material registry (lumen/atelier lane names).
    pub materials: &'a [String],
    /// Fixture anchor ids declared by pack companions
    /// (§2.4 `fixture.<id>` resolution).
    pub pack_fixture_ids: &'a [String],
    /// Content stamps of the target character/scene.
    pub stamps: ContentStamps,
}

fn refuse(
    code: &'static str,
    ctx: &ValidationContext<'_>,
    field_path: &str,
    message: impl Into<String>,
) -> GearRefusal {
    GearRefusal::new(code, &ctx.document_path, field_path, message)
}

/// Validates one gear document's text against a rig template.
/// Returns the validated document or the first refusal in
/// §2.7 pipeline order (steps 1–9; step 10 is
/// [`validate_pack`]).
pub fn validate_document(text: &str, ctx: &ValidationContext<'_>) -> GearResult<GearDocument> {
    // Step 1: parse + schema version.
    let value = torogao_author::ron::parse_document(text).map_err(|e| {
        GearRefusal::new(TG_GEAR_SCHEMA, &ctx.document_path, "$", e.message.clone())
    })?;
    let fields = match &value {
        Ron::Struct(_, fields) => fields,
        _ => {
            return Err(refuse(
                TG_GEAR_SCHEMA,
                ctx,
                "$",
                "a gear document is a struct",
            ));
        }
    };
    match crate::document::field(fields, "format") {
        Some(Ron::Str(token)) if token == GEAR_FORMAT => {}
        _ => {
            return Err(refuse(
                TG_GEAR_SCHEMA,
                ctx,
                "format",
                format!("format must be exactly `{GEAR_FORMAT}`"),
            ));
        }
    }
    // Step 2: unknown-field census.
    census_unknown_fields(&value, &ctx.document_path)?;
    // Step 3: extraction (presence + shape) + class rules.
    let doc = extract_document(fields, &ctx.document_path)?;
    check_class_completeness(&doc, ctx)?;
    // Step 3.5: target consent stamps.
    if !ctx.stamps.is_complete() {
        return Err(refuse(
            TG_GEAR_CONSENT_STAMP,
            ctx,
            "target",
            "target character/scene lacks the adult + consent-forward stamps",
        ));
    }
    // Steps 4–9, in order.
    check_anchors(&doc, ctx)?;
    check_bounds(&doc, ctx)?;
    check_parts_and_materials(&doc, ctx)?;
    check_hooks(&doc, ctx)?;
    check_waveform(&doc, ctx)?;
    if let Some(path) = find_authority_field(&value) {
        return Err(refuse(
            TG_GEAR_MIRROR_AUTHORITY,
            ctx,
            &path,
            "a file may declare mirror capability only; it may never \
             enable mirroring, bind a device, carry consent, or set a ceiling",
        ));
    }
    Ok(doc)
}

fn class_incomplete(
    ctx: &ValidationContext<'_>,
    field_path: &str,
    message: impl Into<String>,
) -> GearRefusal {
    refuse(TG_GEAR_CLASS_INCOMPLETE, ctx, field_path, message)
}

fn check_class_completeness(doc: &GearDocument, ctx: &ValidationContext<'_>) -> GearResult<()> {
    for required in doc.class.required_variants() {
        if !doc.state_variants.contains(required) {
            return Err(class_incomplete(
                ctx,
                "state_variants",
                format!(
                    "class {} requires the `{}` state variant",
                    doc.class.name(),
                    required.name()
                ),
            ));
        }
    }
    if doc.class != GearClass::Effect && doc.anchors.is_empty() {
        return Err(class_incomplete(
            ctx,
            "anchors",
            "an item anchors somewhere",
        ));
    }
    let forbidden: &[(&str, bool)] = &[
        (
            "apparel",
            doc.apparel.is_some() && doc.class != GearClass::Apparel,
        ),
        (
            "device",
            doc.device.is_some() && doc.class != GearClass::VibratoryDevice,
        ),
        (
            "effect",
            doc.effect.is_some() && doc.class != GearClass::Effect,
        ),
        (
            "fixture",
            doc.fixture.is_some() && doc.class != GearClass::RestraintFixture,
        ),
    ];
    for (name, present) in forbidden {
        if *present {
            return Err(class_incomplete(
                ctx,
                name,
                format!(
                    "the `{name}` block does not belong to class {}",
                    doc.class.name()
                ),
            ));
        }
    }
    match doc.class {
        GearClass::Apparel => {
            if doc.apparel.is_none() {
                return Err(class_incomplete(
                    ctx,
                    "apparel",
                    "Apparel requires its block",
                ));
            }
        }
        GearClass::AttachedDevice => {
            let sensation = doc.sensation.as_ref().ok_or_else(|| {
                class_incomplete(
                    ctx,
                    "sensation",
                    "AttachedDevice requires a sensation profile",
                )
            })?;
            if sensation.pressure_permille.is_none() {
                return Err(class_incomplete(
                    ctx,
                    "sensation.pressure_permille",
                    "AttachedDevice requires pressure_permille",
                ));
            }
        }
        GearClass::Effect => {
            let effect = doc
                .effect
                .as_ref()
                .ok_or_else(|| class_incomplete(ctx, "effect", "Effect requires its block"))?;
            if effect.inputs.is_empty() || effect.outputs.is_empty() {
                return Err(class_incomplete(
                    ctx,
                    "effect",
                    "an effect reads at least one input and writes at least one output",
                ));
            }
            if doc.sensation.is_some() {
                return Err(class_incomplete(
                    ctx,
                    "sensation",
                    "an effect is an engine output; it carries no sensation profile",
                ));
            }
        }
        GearClass::RestraintFixture => {
            if doc.fixture.is_none() {
                return Err(class_incomplete(
                    ctx,
                    "fixture",
                    "RestraintFixture requires its block",
                ));
            }
        }
        GearClass::VibratoryDevice => {
            if doc.device.is_none() {
                return Err(class_incomplete(
                    ctx,
                    "device",
                    "VibratoryDevice requires a waveform contract",
                ));
            }
            if doc.sensation.is_none() {
                return Err(class_incomplete(
                    ctx,
                    "sensation",
                    "VibratoryDevice requires a sensation profile",
                ));
            }
        }
    }
    Ok(())
}

fn anchor_known(doc: &GearDocument, ctx: &ValidationContext<'_>, anchor: &str) -> bool {
    if body_anchor(anchor).is_some() {
        return ctx.rig.resolve(anchor).is_some();
    }
    if is_fixture_anchor(anchor) {
        let id = fixture_anchor_id(anchor);
        if ctx.pack_fixture_ids.iter().any(|f| f == id) {
            return true;
        }
        // A fixture's own declared anchors resolve against
        // itself.
        if let Some(fixture) = &doc.fixture {
            return fixture.declares_anchors.iter().any(|f| f == id);
        }
    }
    false
}

fn check_anchors(doc: &GearDocument, ctx: &ValidationContext<'_>) -> GearResult<()> {
    for (i, attachment) in doc.anchors.iter().enumerate() {
        if !anchor_known(doc, ctx, &attachment.anchor) {
            return Err(refuse(
                TG_GEAR_ANCHOR_UNKNOWN,
                ctx,
                &format!("anchors[{i}].anchor"),
                format!(
                    "anchor `{}` does not resolve on rig {}",
                    attachment.anchor, ctx.rig.character
                ),
            ));
        }
    }
    if let Some(apparel) = &doc.apparel {
        for (i, covered) in apparel.covers.iter().enumerate() {
            if !anchor_known(doc, ctx, covered) {
                return Err(refuse(
                    TG_GEAR_ANCHOR_UNKNOWN,
                    ctx,
                    &format!("apparel.covers[{i}]"),
                    format!("covered anchor `{covered}` does not resolve"),
                ));
            }
        }
    }
    Ok(())
}

fn bound(
    ctx: &ValidationContext<'_>,
    field_path: &str,
    what: &str,
    value: i64,
    range: std::ops::RangeInclusive<i64>,
) -> GearResult<()> {
    if range.contains(&value) {
        Ok(())
    } else {
        Err(refuse(
            TG_GEAR_BOUND,
            ctx,
            field_path,
            format!("{what} {value} is outside {range:?}"),
        ))
    }
}

fn check_bounds(doc: &GearDocument, ctx: &ValidationContext<'_>) -> GearResult<()> {
    bound(ctx, "version", "version", doc.version, 1..=9999)?;
    if doc.parts.len() > PARTS_MAX {
        return Err(refuse(
            TG_GEAR_BOUND,
            ctx,
            "parts",
            format!(
                "{} part payloads exceed the {PARTS_MAX} bound",
                doc.parts.len()
            ),
        ));
    }
    if doc.anchors.len() > ATTACHMENTS_MAX {
        return Err(refuse(
            TG_GEAR_BOUND,
            ctx,
            "anchors",
            format!(
                "{} attachments exceed the {ATTACHMENTS_MAX} bound",
                doc.anchors.len()
            ),
        ));
    }
    for (i, attachment) in doc.anchors.iter().enumerate() {
        let at = format!("anchors[{i}]");
        bound(
            ctx,
            &format!("{at}.contact_radius_px"),
            "contact radius",
            attachment.contact_radius_px,
            4..=512,
        )?;
        bound(
            ctx,
            &format!("{at}.stiffness_permille"),
            "stiffness",
            attachment.stiffness_permille,
            0..=1000,
        )?;
        bound(
            ctx,
            &format!("{at}.max_pressure_permille"),
            "max pressure",
            attachment.max_pressure_permille,
            1..=1000,
        )?;
        if let Some(site) = ctx.rig.resolve(&attachment.anchor) {
            let offset_len =
                (attachment.offset_x_px.powi(2) + attachment.offset_y_px.powi(2)).sqrt();
            if offset_len > site.contact_radius_px {
                return Err(refuse(
                    TG_GEAR_BOUND,
                    ctx,
                    &format!("{at}.offset"),
                    format!(
                        "offset length {offset_len:.1} px exceeds the anchor's \
                         declared contact radius {:.1} px",
                        site.contact_radius_px
                    ),
                ));
            }
        }
    }
    bound(
        ctx,
        "physics.displacement_threshold_px",
        "displacement threshold",
        doc.physics.displacement_threshold_px,
        1..=256,
    )?;
    if let Some(sensation) = &doc.sensation {
        if let Some(pressure) = sensation.pressure_permille {
            bound(
                ctx,
                "sensation.pressure_permille",
                "pressure",
                pressure,
                0..=1000,
            )?;
        }
        if let Some(floor) = sensation.arousal_floor_permille {
            bound(
                ctx,
                "sensation.arousal_floor_permille",
                "arousal floor",
                floor,
                0..=600,
            )?;
        }
    }
    if let Some(apparel) = &doc.apparel {
        bound(
            ctx,
            "apparel.transmission_permille",
            "transmission",
            apparel.transmission_permille,
            0..=1000,
        )?;
    }
    for (i, part) in doc.parts.iter().enumerate() {
        if let Some((w, h)) = part.texture {
            bound(
                ctx,
                &format!("parts[{i}].texture"),
                "texture width",
                w,
                1..=TEXTURE_AXIS_MAX_PX,
            )?;
            bound(
                ctx,
                &format!("parts[{i}].texture"),
                "texture height",
                h,
                1..=TEXTURE_AXIS_MAX_PX,
            )?;
        }
    }
    if let Some(fixture) = &doc.fixture {
        for (i, pose) in fixture.pose_constraints.iter().enumerate() {
            let at = format!("fixture.pose_constraints[{i}]");
            bound(ctx, &at, "pose minimum", pose.min_permille, 0..=1000)?;
            bound(ctx, &at, "pose maximum", pose.max_permille, 0..=1000)?;
            if pose.min_permille > pose.max_permille {
                return Err(refuse(
                    TG_GEAR_BOUND,
                    ctx,
                    &at,
                    "pose constraint minimum exceeds its maximum",
                ));
            }
        }
    }
    Ok(())
}

fn check_parts_and_materials(doc: &GearDocument, ctx: &ValidationContext<'_>) -> GearResult<()> {
    for (i, part) in doc.parts.iter().enumerate() {
        let bytes = ctx.payloads.payload(&part.name).ok_or_else(|| {
            refuse(
                TG_GEAR_PART_MISSING,
                ctx,
                &format!("parts[{i}].name"),
                format!("payload `{}` is absent", part.name),
            )
        })?;
        if sha256_hex(&bytes) != part.hash {
            return Err(refuse(
                TG_GEAR_PART_HASH,
                ctx,
                &format!("parts[{i}].hash"),
                format!(
                    "payload `{}` bytes do not match the declared hash",
                    part.name
                ),
            ));
        }
    }
    for (i, material) in doc.materials.iter().enumerate() {
        if !ctx.materials.iter().any(|m| m == material) {
            return Err(refuse(
                TG_GEAR_PART_MISSING,
                ctx,
                &format!("materials[{i}]"),
                format!("material `{material}` is not in the registry"),
            ));
        }
    }
    Ok(())
}

fn hook_denied(
    ctx: &ValidationContext<'_>,
    field_path: &str,
    message: impl Into<String>,
) -> GearRefusal {
    refuse(TG_GEAR_HOOK_DENIED, ctx, field_path, message)
}

fn check_hooks(doc: &GearDocument, ctx: &ValidationContext<'_>) -> GearResult<()> {
    for (i, hook) in doc.hooks.iter().enumerate() {
        let at = format!("hooks[{i}]");
        if STATE_FIELDS.contains(&hook.name.as_str()) {
            return Err(hook_denied(
                ctx,
                &at,
                format!(
                    "`{}` is arousal-engine state; gear may never write it",
                    hook.name
                ),
            ));
        }
        let legal = match hook.kind {
            HookKind::ArousalInput => SITES.contains(&hook.name.as_str()),
            HookKind::AttachmentSolve | HookKind::DeviceCycle => hook.name == "self",
            HookKind::BodyParam => PUBLISHED_BODY_PARAMS.contains(&hook.name.as_str()),
            HookKind::EffectParam => EFFECT_PARAMS.contains(&hook.name.as_str()),
            HookKind::Expression => ctx.rig.governed_expressions.iter().any(|g| g == &hook.name),
            HookKind::Fluid => FLUID_EMITTERS.contains(&hook.name.as_str()),
        };
        if !legal {
            return Err(hook_denied(
                ctx,
                &at,
                format!(
                    "hook {} `{}` is outside the §2.8 whitelist",
                    hook.kind.name(),
                    hook.name
                ),
            ));
        }
    }
    if let Some(effect) = &doc.effect {
        for (i, input) in effect.inputs.iter().enumerate() {
            if !STATE_FIELDS.contains(&input.as_str()) && !CONTACT_EVENTS.contains(&input.as_str())
            {
                return Err(hook_denied(
                    ctx,
                    &format!("effect.inputs[{i}]"),
                    format!("effect input `{input}` is not an engine state field or contact event"),
                ));
            }
        }
        for (i, output) in effect.outputs.iter().enumerate() {
            if !EFFECT_PARAMS.contains(&output.as_str()) {
                return Err(hook_denied(
                    ctx,
                    &format!("effect.outputs[{i}]"),
                    format!("effect output `{output}` is not a whitelisted effect parameter"),
                ));
            }
        }
    }
    if let Some(fixture) = &doc.fixture {
        for (i, pose) in fixture.pose_constraints.iter().enumerate() {
            if !PUBLISHED_BODY_PARAMS.contains(&pose.param.as_str()) {
                return Err(hook_denied(
                    ctx,
                    &format!("fixture.pose_constraints[{i}].param"),
                    format!(
                        "pose parameter `{}` is not published by the rig template",
                        pose.param
                    ),
                ));
            }
        }
    }
    Ok(())
}

fn check_waveform(doc: &GearDocument, ctx: &ValidationContext<'_>) -> GearResult<()> {
    let Some(device) = &doc.device else {
        return Ok(());
    };
    let wave = &device.waveform;
    let at = "device.waveform";
    let wave_refuse =
        |field_path: &str, message: String| refuse(TG_GEAR_WAVEFORM, ctx, field_path, message);
    if !(0..=1000).contains(&wave.amplitude_permille) {
        return Err(wave_refuse(
            &format!("{at}.amplitude_permille"),
            format!("amplitude {} is outside 0..=1000", wave.amplitude_permille),
        ));
    }
    if !(0..=20_000).contains(&wave.frequency_millihz) {
        return Err(wave_refuse(
            &format!("{at}.frequency_millihz"),
            format!(
                "frequency {} is outside 0..=20000 millihz",
                wave.frequency_millihz
            ),
        ));
    }
    if !(-1_000_000..=1_000_000).contains(&wave.phase_ticks) {
        return Err(wave_refuse(
            &format!("{at}.phase_ticks"),
            "phase_ticks is outside the bounded phase range".to_string(),
        ));
    }
    match wave.kind {
        WaveKind::Constant | WaveKind::Ramp | WaveKind::Wave => {
            if wave.duty_permille.is_some()
                || wave.pattern.is_some()
                || wave.pattern_step_ticks.is_some()
            {
                return Err(wave_refuse(
                    at,
                    format!("{} carries no duty/pattern parameters", wave.kind.name()),
                ));
            }
        }
        WaveKind::Pulse => {
            let duty = wave.duty_permille.ok_or_else(|| {
                wave_refuse(
                    &format!("{at}.duty_permille"),
                    "Pulse requires a duty cycle".to_string(),
                )
            })?;
            if !(0..=1000).contains(&duty) {
                return Err(wave_refuse(
                    &format!("{at}.duty_permille"),
                    format!("duty {duty} is outside 0..=1000"),
                ));
            }
            if wave.pattern.is_some() || wave.pattern_step_ticks.is_some() {
                return Err(wave_refuse(at, "Pulse carries no pattern".to_string()));
            }
        }
        WaveKind::Pattern => {
            let pattern = wave.pattern.as_ref().ok_or_else(|| {
                wave_refuse(
                    &format!("{at}.pattern"),
                    "Pattern requires its steps".to_string(),
                )
            })?;
            if pattern.is_empty() || pattern.len() > PATTERN_STEPS_MAX {
                return Err(wave_refuse(
                    &format!("{at}.pattern"),
                    format!(
                        "pattern length {} is outside 1..={PATTERN_STEPS_MAX}",
                        pattern.len()
                    ),
                ));
            }
            for step in pattern {
                if !(0..=1000).contains(step) {
                    return Err(wave_refuse(
                        &format!("{at}.pattern"),
                        format!("pattern step {step} is outside 0..=1000"),
                    ));
                }
            }
            let step_ticks = wave.pattern_step_ticks.ok_or_else(|| {
                wave_refuse(
                    &format!("{at}.pattern_step_ticks"),
                    "Pattern requires pattern_step_ticks".to_string(),
                )
            })?;
            if !(1..=3600).contains(&step_ticks) {
                return Err(wave_refuse(
                    &format!("{at}.pattern_step_ticks"),
                    format!("pattern_step_ticks {step_ticks} is outside 1..=3600"),
                ));
            }
            if let Some(duty) = wave.duty_permille
                && !(0..=1000).contains(&duty)
            {
                return Err(wave_refuse(
                    &format!("{at}.duty_permille"),
                    format!("duty {duty} is outside 0..=1000"),
                ));
            }
        }
    }
    Ok(())
}

/// Collects the fixture anchor ids a pack's RestraintFixture
/// documents declare, for other documents' context.
pub fn pack_fixture_ids(documents: &[GearDocument]) -> Vec<String> {
    let mut ids = Vec::new();
    for doc in documents {
        if let Some(fixture) = &doc.fixture {
            for id in &fixture.declares_anchors {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
        }
    }
    ids
}

/// Validation step 10: pack consistency across documents —
/// duplicate ids, dependency existence, dependency cycles,
/// and the total payload bound. Documents are assumed to have
/// passed [`validate_document`] individually.
pub fn validate_pack(documents: &[GearDocument], payloads: &dyn PayloadLookup) -> GearResult<()> {
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for doc in documents {
        if !seen.insert(doc.id.as_str()) {
            return Err(GearRefusal::new(
                TG_GEAR_PACK_DUPLICATE,
                &doc.id,
                "id",
                format!("duplicate item id `{}` inside the pack", doc.id),
            ));
        }
    }
    let by_id: BTreeMap<&str, &GearDocument> =
        documents.iter().map(|d| (d.id.as_str(), d)).collect();
    for doc in documents {
        for dep in &doc.depends_on {
            if !by_id.contains_key(dep.as_str()) {
                return Err(GearRefusal::new(
                    TG_GEAR_PART_MISSING,
                    &doc.id,
                    "depends_on",
                    format!("dependency `{dep}` is not in the pack"),
                ));
            }
        }
    }
    // Cycle check: iterative DFS with an explicit stack and a
    // colour map; depth is bounded by the document count.
    let mut colour: BTreeMap<&str, u8> = BTreeMap::new();
    for doc in documents {
        if colour.get(doc.id.as_str()).copied().unwrap_or(0) != 0 {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(doc.id.as_str(), 0)];
        colour.insert(doc.id.as_str(), 1);
        while let Some(&(id, next)) = stack.last() {
            let deps = &by_id[id].depends_on;
            if next >= deps.len() {
                colour.insert(id, 2);
                stack.pop();
                continue;
            }
            if let Some(top) = stack.last_mut() {
                top.1 += 1;
            }
            let dep = deps[next].as_str();
            match colour.get(dep).copied().unwrap_or(0) {
                1 => {
                    return Err(GearRefusal::new(
                        TG_GEAR_PACK_CYCLE,
                        id,
                        "depends_on",
                        format!("dependency cycle through `{dep}`"),
                    ));
                }
                0 => {
                    colour.insert(dep, 1);
                    stack.push((dep, 0));
                }
                _ => {}
            }
        }
    }
    let mut counted: BTreeSet<&str> = BTreeSet::new();
    let mut total_bytes: usize = 0;
    for doc in documents {
        for part in &doc.parts {
            if counted.insert(part.name.as_str()) {
                let bytes = payloads.payload(&part.name).ok_or_else(|| {
                    GearRefusal::new(
                        TG_GEAR_PART_MISSING,
                        &doc.id,
                        "parts",
                        format!("payload `{}` is absent", part.name),
                    )
                })?;
                total_bytes = total_bytes.saturating_add(bytes.len());
            }
        }
    }
    if total_bytes > PACK_PAYLOAD_BYTES_MAX {
        return Err(GearRefusal::new(
            TG_GEAR_BOUND,
            "pack",
            "parts",
            format!("total payload {total_bytes} bytes exceeds the pack bound"),
        ));
    }
    Ok(())
}
