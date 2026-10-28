//! Extraction: the unknown-field census (§2.7 step 2) and
//! the typed extraction of a [`GearDocument`] from a RON
//! value tree (step 3's shape half).
//!
//! The census is context-aware: every struct kind in the
//! format has a fixed field set, and any other name — at any
//! nesting level — is `TG-GEAR-FIELD-UNKNOWN`. Authority field
//! names (§2.1) are recognized here and deliberately passed
//! over: they belong to step 9's census, so an
//! authority-bearing file meets the authority refusal, not a
//! generic unknown-field one.

use torogao_author::ron::Ron;

use crate::authority::is_authority_field;
use crate::document::{
    ApparelBlock, Attachment, Device, DrawOrderEntry, DrawRelation, EffectBlock, Facing,
    FixtureBlock, GearClass, GearDocument, Honesty, Hook, HookKind, PartRef, Physics,
    PoseConstraint, Provenance, Sensation, StateVariant, WaveKind, WaveSpec, field, malformed,
    missing,
};
use crate::refusal::{GearRefusal, GearResult, TG_GEAR_FIELD_UNKNOWN};

/// Root field set of a gear document, alphabetical.
pub const ROOT_FIELDS: &[&str] = &[
    "anchors",
    "apparel",
    "class",
    "depends_on",
    "device",
    "display_name",
    "draw_order",
    "effect",
    "fixture",
    "format",
    "hooks",
    "id",
    "materials",
    "parts",
    "physics",
    "provenance",
    "sensation",
    "state_variants",
    "version",
];

const ANCHOR_FIELDS: &[&str] = &[
    "anchor",
    "contact_radius_px",
    "facing",
    "max_pressure_permille",
    "offset",
    "stiffness_permille",
];
const APPAREL_FIELDS: &[&str] = &["covers", "transmission_permille"];
const DEVICE_FIELDS: &[&str] = &["contact_site", "mirror_capable", "waveform"];
const DRAW_FIELDS: &[&str] = &["layer", "relation", "target"];
const EFFECT_FIELDS: &[&str] = &["inputs", "outputs"];
const FIXTURE_FIELDS: &[&str] = &["declares_anchors", "pose_constraints"];
const HOOK_FIELDS: &[&str] = &["kind", "name"];
const PART_FIELDS: &[&str] = &["hash", "name", "texture"];
const PHYSICS_FIELDS: &[&str] = &["displaceable", "displacement_threshold_px"];
const POSE_FIELDS: &[&str] = &["max_permille", "min_permille", "param"];
const PROVENANCE_FIELDS: &[&str] = &["author", "honesty", "source_pack"];
const SENSATION_FIELDS: &[&str] = &["arousal_floor_permille", "pressure_permille", "site"];
const WAVE_FIELDS: &[&str] = &[
    "amplitude_permille",
    "duty_permille",
    "frequency_millihz",
    "kind",
    "pattern",
    "pattern_step_ticks",
    "phase_ticks",
];

#[derive(Clone, Copy, PartialEq)]
enum StructKind {
    Apparel,
    Attachment,
    Device,
    Draw,
    Effect,
    Fixture,
    Hook,
    Part,
    Physics,
    Pose,
    Provenance,
    Root,
    Sensation,
    Wave,
}

impl StructKind {
    fn fields(&self) -> &'static [&'static str] {
        match self {
            Self::Apparel => APPAREL_FIELDS,
            Self::Attachment => ANCHOR_FIELDS,
            Self::Device => DEVICE_FIELDS,
            Self::Draw => DRAW_FIELDS,
            Self::Effect => EFFECT_FIELDS,
            Self::Fixture => FIXTURE_FIELDS,
            Self::Hook => HOOK_FIELDS,
            Self::Part => PART_FIELDS,
            Self::Physics => PHYSICS_FIELDS,
            Self::Pose => POSE_FIELDS,
            Self::Provenance => PROVENANCE_FIELDS,
            Self::Root => ROOT_FIELDS,
            Self::Sensation => SENSATION_FIELDS,
            Self::Wave => WAVE_FIELDS,
        }
    }

    /// The struct kind a field's value is censused as, when it
    /// is a struct or a list of structs.
    fn child_kind(&self, field_name: &str) -> Option<StructKind> {
        match (self, field_name) {
            (Self::Root, "anchors") => Some(StructKind::Attachment),
            (Self::Root, "apparel") => Some(StructKind::Apparel),
            (Self::Root, "device") => Some(StructKind::Device),
            (Self::Root, "draw_order") => Some(StructKind::Draw),
            (Self::Root, "effect") => Some(StructKind::Effect),
            (Self::Root, "fixture") => Some(StructKind::Fixture),
            (Self::Root, "hooks") => Some(StructKind::Hook),
            (Self::Root, "parts") => Some(StructKind::Part),
            (Self::Root, "physics") => Some(StructKind::Physics),
            (Self::Root, "provenance") => Some(StructKind::Provenance),
            (Self::Root, "sensation") => Some(StructKind::Sensation),
            (Self::Device, "waveform") => Some(StructKind::Wave),
            (Self::Fixture, "pose_constraints") => Some(StructKind::Pose),
            _ => None,
        }
    }
}

/// Validation step 2: the unknown-field census over the whole
/// document tree. Returns the first unknown field's path.
pub fn census_unknown_fields(value: &Ron, document_path: &str) -> GearResult<()> {
    census(value, StructKind::Root, "", document_path)
}

fn census(value: &Ron, kind: StructKind, path: &str, document_path: &str) -> GearResult<()> {
    match value {
        Ron::Struct(_, fields) => {
            for (name, inner) in fields {
                let here = join(path, name);
                if is_authority_field(name) {
                    continue; // step 9's census owns these names
                }
                if !kind.fields().contains(&name.as_str()) {
                    return Err(GearRefusal::new(
                        TG_GEAR_FIELD_UNKNOWN,
                        document_path,
                        &here,
                        format!("unknown field `{name}`"),
                    ));
                }
                if let Some(child) = kind.child_kind(name) {
                    census(inner, child, &here, document_path)?;
                }
            }
            Ok(())
        }
        Ron::List(items) => {
            for (index, item) in items.iter().enumerate() {
                census(item, kind, &format!("{path}[{index}]"), document_path)?;
            }
            Ok(())
        }
        Ron::Opt(Some(inner)) => census(inner, kind, path, document_path),
        _ => Ok(()),
    }
}

fn join(path: &str, field_name: &str) -> String {
    if path.is_empty() {
        field_name.to_string()
    } else {
        format!("{path}.{field_name}")
    }
}

// ---- typed extraction ----------------------------------------

fn get<'a>(fields: &'a [(String, Ron)], name: &str, doc: &str, path: &str) -> GearResult<&'a Ron> {
    field(fields, name).ok_or_else(|| missing(doc, path, &format!("field `{name}`")))
}

fn as_fields<'a>(value: &'a Ron, doc: &str, path: &str) -> GearResult<&'a [(String, Ron)]> {
    match value {
        Ron::Struct(_, fields) => Ok(fields),
        _ => Err(malformed(doc, path, "a struct")),
    }
}

fn as_int(value: &Ron, doc: &str, path: &str) -> GearResult<i64> {
    match value {
        Ron::Int(n) => Ok(*n),
        _ => Err(malformed(doc, path, "an integer")),
    }
}

fn as_num(value: &Ron, doc: &str, path: &str) -> GearResult<f64> {
    match value {
        Ron::Int(n) => Ok(*n as f64),
        Ron::Float(x) => Ok(*x),
        _ => Err(malformed(doc, path, "a number")),
    }
}

fn as_str(value: &Ron, doc: &str, path: &str) -> GearResult<String> {
    match value {
        Ron::Str(s) => Ok(s.clone()),
        _ => Err(malformed(doc, path, "a string")),
    }
}

fn as_ident(value: &Ron, doc: &str, path: &str) -> GearResult<String> {
    match value {
        Ron::Ident(s) | Ron::Str(s) => Ok(s.clone()),
        _ => Err(malformed(doc, path, "an identifier")),
    }
}

fn as_bool(value: &Ron, doc: &str, path: &str) -> GearResult<bool> {
    match value {
        Ron::Bool(b) => Ok(*b),
        _ => Err(malformed(doc, path, "a boolean")),
    }
}

fn as_list<'a>(value: &'a Ron, doc: &str, path: &str) -> GearResult<&'a [Ron]> {
    match value {
        Ron::List(items) => Ok(items),
        _ => Err(malformed(doc, path, "a list")),
    }
}

fn as_opt(value: &Ron) -> Option<Option<&Ron>> {
    match value {
        Ron::Opt(None) => Some(None),
        Ron::Opt(Some(inner)) => Some(Some(inner)),
        _ => None,
    }
}

fn opt_int(value: &Ron, doc: &str, path: &str) -> GearResult<Option<i64>> {
    match as_opt(value) {
        Some(None) => Ok(None),
        Some(Some(inner)) => Ok(Some(as_int(inner, doc, path)?)),
        None => Err(malformed(doc, path, "an optional integer")),
    }
}

fn str_list(value: &Ron, doc: &str, path: &str) -> GearResult<Vec<String>> {
    as_list(value, doc, path)?
        .iter()
        .map(|item| as_str(item, doc, path))
        .collect()
}

fn ident_list(value: &Ron, doc: &str, path: &str) -> GearResult<Vec<String>> {
    as_list(value, doc, path)?
        .iter()
        .map(|item| as_ident(item, doc, path))
        .collect()
}

/// Extracts the typed document from a root struct's fields.
/// Presence failures are completeness refusals; shape failures
/// are schema refusals. Semantic checks (anchors, bounds,
/// hooks, waveform, authority) are the validator's later
/// steps, not this function's.
pub fn extract_document(fields: &[(String, Ron)], doc: &str) -> GearResult<GearDocument> {
    for name in ROOT_FIELDS {
        if field(fields, name).is_none() {
            return Err(missing(doc, name, &format!("field `{name}`")));
        }
    }
    let class_name = as_ident(get(fields, "class", doc, "class")?, doc, "class")?;
    let class = GearClass::parse(&class_name).ok_or_else(|| {
        GearRefusal::new(
            crate::refusal::TG_GEAR_CLASS_INCOMPLETE,
            doc,
            "class",
            format!("unknown gear class `{class_name}`"),
        )
    })?;
    Ok(GearDocument {
        format: as_str(get(fields, "format", doc, "format")?, doc, "format")?,
        id: as_str(get(fields, "id", doc, "id")?, doc, "id")?,
        version: as_int(get(fields, "version", doc, "version")?, doc, "version")?,
        display_name: as_str(
            get(fields, "display_name", doc, "display_name")?,
            doc,
            "display_name",
        )?,
        class,
        anchors: extract_attachments(get(fields, "anchors", doc, "anchors")?, doc)?,
        parts: extract_parts(get(fields, "parts", doc, "parts")?, doc)?,
        materials: str_list(
            get(fields, "materials", doc, "materials")?,
            doc,
            "materials",
        )?,
        draw_order: extract_draw_order(get(fields, "draw_order", doc, "draw_order")?, doc)?,
        state_variants: extract_variants(
            get(fields, "state_variants", doc, "state_variants")?,
            doc,
        )?,
        hooks: extract_hooks(get(fields, "hooks", doc, "hooks")?, doc)?,
        physics: extract_physics(get(fields, "physics", doc, "physics")?, doc)?,
        sensation: extract_opt(
            get(fields, "sensation", doc, "sensation")?,
            doc,
            "sensation",
            extract_sensation,
        )?,
        device: extract_opt(
            get(fields, "device", doc, "device")?,
            doc,
            "device",
            extract_device,
        )?,
        apparel: extract_opt(
            get(fields, "apparel", doc, "apparel")?,
            doc,
            "apparel",
            extract_apparel,
        )?,
        fixture: extract_opt(
            get(fields, "fixture", doc, "fixture")?,
            doc,
            "fixture",
            extract_fixture,
        )?,
        effect: extract_opt(
            get(fields, "effect", doc, "effect")?,
            doc,
            "effect",
            extract_effect,
        )?,
        depends_on: str_list(
            get(fields, "depends_on", doc, "depends_on")?,
            doc,
            "depends_on",
        )?,
        provenance: extract_provenance(get(fields, "provenance", doc, "provenance")?, doc)?,
    })
}

fn extract_opt<T>(
    value: &Ron,
    doc: &str,
    path: &str,
    inner: fn(&Ron, &str, &str) -> GearResult<T>,
) -> GearResult<Option<T>> {
    match as_opt(value) {
        Some(None) => Ok(None),
        Some(Some(v)) => Ok(Some(inner(v, doc, path)?)),
        None => Err(malformed(doc, path, "an optional block")),
    }
}

fn extract_attachments(value: &Ron, doc: &str) -> GearResult<Vec<Attachment>> {
    let mut out = Vec::new();
    for (i, item) in as_list(value, doc, "anchors")?.iter().enumerate() {
        let path = format!("anchors[{i}]");
        let f = as_fields(item, doc, &path)?;
        let offset = get(f, "offset", doc, &path)?;
        let (ox, oy) = match offset {
            Ron::Tuple(pair) if pair.len() == 2 => {
                (as_num(&pair[0], doc, &path)?, as_num(&pair[1], doc, &path)?)
            }
            _ => return Err(malformed(doc, &path, "an (x, y) offset tuple")),
        };
        let facing_name = as_ident(get(f, "facing", doc, &path)?, doc, &path)?;
        out.push(Attachment {
            anchor: as_str(get(f, "anchor", doc, &path)?, doc, &path)?,
            offset_x_px: ox,
            offset_y_px: oy,
            contact_radius_px: as_int(get(f, "contact_radius_px", doc, &path)?, doc, &path)?,
            stiffness_permille: as_int(get(f, "stiffness_permille", doc, &path)?, doc, &path)?,
            max_pressure_permille: as_int(
                get(f, "max_pressure_permille", doc, &path)?,
                doc,
                &path,
            )?,
            facing: Facing::parse(&facing_name)
                .ok_or_else(|| malformed(doc, &path, "a facing ident"))?,
        });
    }
    Ok(out)
}

fn extract_parts(value: &Ron, doc: &str) -> GearResult<Vec<PartRef>> {
    let mut out = Vec::new();
    for (i, item) in as_list(value, doc, "parts")?.iter().enumerate() {
        let path = format!("parts[{i}]");
        let f = as_fields(item, doc, &path)?;
        let texture = match as_opt(get(f, "texture", doc, &path)?) {
            Some(None) => None,
            Some(Some(v)) => match v {
                Ron::Tuple(pair) if pair.len() == 2 => {
                    Some((as_int(&pair[0], doc, &path)?, as_int(&pair[1], doc, &path)?))
                }
                _ => return Err(malformed(doc, &path, "a (width, height) tuple")),
            },
            None => return Err(malformed(doc, &path, "an optional texture")),
        };
        out.push(PartRef {
            name: as_str(get(f, "name", doc, &path)?, doc, &path)?,
            hash: as_str(get(f, "hash", doc, &path)?, doc, &path)?,
            texture,
        });
    }
    Ok(out)
}

fn extract_draw_order(value: &Ron, doc: &str) -> GearResult<Vec<DrawOrderEntry>> {
    let mut out = Vec::new();
    for (i, item) in as_list(value, doc, "draw_order")?.iter().enumerate() {
        let path = format!("draw_order[{i}]");
        let f = as_fields(item, doc, &path)?;
        let relation_name = as_ident(get(f, "relation", doc, &path)?, doc, &path)?;
        out.push(DrawOrderEntry {
            layer: as_str(get(f, "layer", doc, &path)?, doc, &path)?,
            relation: DrawRelation::parse(&relation_name)
                .ok_or_else(|| malformed(doc, &path, "a draw relation ident"))?,
            target: as_str(get(f, "target", doc, &path)?, doc, &path)?,
        });
    }
    Ok(out)
}

fn extract_variants(value: &Ron, doc: &str) -> GearResult<Vec<StateVariant>> {
    let mut out = Vec::new();
    for name in ident_list(value, doc, "state_variants")? {
        out.push(StateVariant::parse(&name).ok_or_else(|| {
            GearRefusal::new(
                crate::refusal::TG_GEAR_CLASS_INCOMPLETE,
                doc,
                "state_variants",
                format!("unknown state variant `{name}`"),
            )
        })?);
    }
    Ok(out)
}

fn extract_hooks(value: &Ron, doc: &str) -> GearResult<Vec<Hook>> {
    let mut out = Vec::new();
    for (i, item) in as_list(value, doc, "hooks")?.iter().enumerate() {
        let path = format!("hooks[{i}]");
        let f = as_fields(item, doc, &path)?;
        let kind_name = as_ident(get(f, "kind", doc, &path)?, doc, &path)?;
        out.push(Hook {
            kind: HookKind::parse(&kind_name)
                .ok_or_else(|| malformed(doc, &path, "a hook kind ident"))?,
            name: as_str(get(f, "name", doc, &path)?, doc, &path)?,
        });
    }
    Ok(out)
}

fn extract_physics(value: &Ron, doc: &str) -> GearResult<Physics> {
    let f = as_fields(value, doc, "physics")?;
    Ok(Physics {
        displacement_threshold_px: as_int(
            get(f, "displacement_threshold_px", doc, "physics")?,
            doc,
            "physics",
        )?,
        displaceable: as_bool(get(f, "displaceable", doc, "physics")?, doc, "physics")?,
    })
}

fn extract_sensation(value: &Ron, doc: &str, path: &str) -> GearResult<Sensation> {
    let f = as_fields(value, doc, path)?;
    Ok(Sensation {
        site: as_str(get(f, "site", doc, path)?, doc, path)?,
        pressure_permille: opt_int(get(f, "pressure_permille", doc, path)?, doc, path)?,
        arousal_floor_permille: opt_int(get(f, "arousal_floor_permille", doc, path)?, doc, path)?,
    })
}

fn extract_wave(value: &Ron, doc: &str, path: &str) -> GearResult<WaveSpec> {
    let f = as_fields(value, doc, path)?;
    let kind_name = as_ident(get(f, "kind", doc, path)?, doc, path)?;
    let pattern = match as_opt(get(f, "pattern", doc, path)?) {
        Some(None) => None,
        Some(Some(v)) => Some(
            as_list(v, doc, path)?
                .iter()
                .map(|item| as_int(item, doc, path))
                .collect::<GearResult<Vec<i64>>>()?,
        ),
        None => return Err(malformed(doc, path, "an optional pattern")),
    };
    Ok(WaveSpec {
        kind: WaveKind::parse(&kind_name)
            .ok_or_else(|| malformed(doc, path, "a wave kind ident"))?,
        amplitude_permille: as_int(get(f, "amplitude_permille", doc, path)?, doc, path)?,
        frequency_millihz: as_int(get(f, "frequency_millihz", doc, path)?, doc, path)?,
        duty_permille: opt_int(get(f, "duty_permille", doc, path)?, doc, path)?,
        pattern,
        pattern_step_ticks: opt_int(get(f, "pattern_step_ticks", doc, path)?, doc, path)?,
        phase_ticks: as_int(get(f, "phase_ticks", doc, path)?, doc, path)?,
    })
}

fn extract_device(value: &Ron, doc: &str, path: &str) -> GearResult<Device> {
    let f = as_fields(value, doc, path)?;
    Ok(Device {
        waveform: extract_wave(get(f, "waveform", doc, path)?, doc, path)?,
        mirror_capable: as_bool(get(f, "mirror_capable", doc, path)?, doc, path)?,
        contact_site: as_str(get(f, "contact_site", doc, path)?, doc, path)?,
    })
}

fn extract_apparel(value: &Ron, doc: &str, path: &str) -> GearResult<ApparelBlock> {
    let f = as_fields(value, doc, path)?;
    Ok(ApparelBlock {
        covers: str_list(get(f, "covers", doc, path)?, doc, path)?,
        transmission_permille: as_int(get(f, "transmission_permille", doc, path)?, doc, path)?,
    })
}

fn extract_fixture(value: &Ron, doc: &str, path: &str) -> GearResult<FixtureBlock> {
    let f = as_fields(value, doc, path)?;
    let mut pose_constraints = Vec::new();
    for (i, item) in as_list(get(f, "pose_constraints", doc, path)?, doc, path)?
        .iter()
        .enumerate()
    {
        let ppath = format!("{path}.pose_constraints[{i}]");
        let pf = as_fields(item, doc, &ppath)?;
        pose_constraints.push(PoseConstraint {
            param: as_str(get(pf, "param", doc, &ppath)?, doc, &ppath)?,
            min_permille: as_int(get(pf, "min_permille", doc, &ppath)?, doc, &ppath)?,
            max_permille: as_int(get(pf, "max_permille", doc, &ppath)?, doc, &ppath)?,
        });
    }
    Ok(FixtureBlock {
        declares_anchors: str_list(get(f, "declares_anchors", doc, path)?, doc, path)?,
        pose_constraints,
    })
}

fn extract_effect(value: &Ron, doc: &str, path: &str) -> GearResult<EffectBlock> {
    let f = as_fields(value, doc, path)?;
    Ok(EffectBlock {
        inputs: str_list(get(f, "inputs", doc, path)?, doc, path)?,
        outputs: str_list(get(f, "outputs", doc, path)?, doc, path)?,
    })
}

fn extract_provenance(value: &Ron, doc: &str) -> GearResult<Provenance> {
    let f = as_fields(value, doc, "provenance")?;
    let honesty_name = as_ident(get(f, "honesty", doc, "provenance")?, doc, "provenance")?;
    Ok(Provenance {
        author: as_str(get(f, "author", doc, "provenance")?, doc, "provenance")?,
        source_pack: as_str(get(f, "source_pack", doc, "provenance")?, doc, "provenance")?,
        honesty: Honesty::parse(&honesty_name)
            .ok_or_else(|| malformed(doc, "provenance", "an honesty ident"))?,
    })
}
