//! Document validation, in the order SPEC.md section 9
//! prescribes: the first failure is the reported failure,
//! deterministically.
//!
//! Steps 1–3 run here (byte bound, header probe, full
//! parse); steps 4–10 run over the unchecked form
//! ([`crate::uclip::UClip`]) in `check_*` functions, one per
//! spec step; binding validation (steps 11–13) lives in
//! [`crate::bind`]. Within steps 8 and 9 the checks run
//! class-major — every track's presence before any track's
//! key order, and so on — matching the order section 9
//! lists the checks. A numeric value's finiteness is always
//! checked before its range: a non-finite value makes the
//! range comparison meaningless, so `TMC-301` precedes the
//! range code for the same value.

use crate::error::{ClipError, bounded_quote};
use crate::limits::*;
use crate::model::*;
use crate::raw::RawClip;
use crate::uclip::{UClip, UGroup, ULayer, ULayerTrack, UTrack, emit_canonical, sha256_hex};

/// Parse and document-validate a clip document.
///
/// Accepts the raw bytes of a `clip.json` file and returns
/// the validated [`Clip`], or the first [`ClipError`] in
/// SPEC.md section 9 order. The clip is not evaluable until
/// it is also bound ([`crate::bind::BoundClip::bind`]).
pub fn parse_document(bytes: &[u8]) -> Result<Clip, ClipError> {
    // Step 1: input byte length (TMC-101).
    if bytes.len() > CLIP_BYTES_MAX {
        return Err(ClipError::InputTooLarge {
            bytes: bytes.len(),
            max: CLIP_BYTES_MAX,
        });
    }
    // A byte-order mark is invalid (TMC-102).
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(ClipError::Syntax {
            line: 1,
            column: 1,
            message: "byte-order mark is not permitted".to_string(),
        });
    }
    // Step 2: header probe (TMC-105 / TMC-106), before full
    // schema validation, so a future version's file fails as
    // a version error, not a schema error.
    probe_format(bytes)?;
    // Step 3: full JSON parse — syntax (TMC-102), duplicate
    // keys and type errors (TMC-103), unknown fields
    // (TMC-104).
    let raw: RawClip = serde_json::from_slice(bytes).map_err(map_serde_error)?;
    let clip = UClip::from_raw(raw)?;
    // Steps 4–10.
    check_counts(&clip)?;
    check_ids(&clip)?;
    check_header(&clip)?;
    check_groups(&clip)?;
    check_base_tracks(&clip)?;
    check_layers(&clip)?;
    check_provenance(&clip)?;
    build_clip(clip)
}

/// Step 2: read only the `format` token and classify it.
fn probe_format(bytes: &[u8]) -> Result<(), ClipError> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(map_serde_error)?;
    let found = value
        .get("format")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    match found.as_deref() {
        Some(FORMAT_TOKEN) => Ok(()),
        Some(other) if other.starts_with("torogao-motion-clip/") => {
            Err(ClipError::UnsupportedVersion {
                found: bounded_quote(other),
            })
        }
        other => Err(ClipError::UnsupportedFormat {
            found: bounded_quote(other.unwrap_or("")),
        }),
    }
}

/// Map a serde_json failure onto the loud-error table.
/// Derived-struct messages carry the classification: an
/// unknown field is `TMC-104`; a float that overflows f64
/// reports "number out of range" and is `TMC-301` (every
/// integer field in this format is u32, whose overflows
/// serde reports as invalid values, i.e. schema errors);
/// syntax failures are `TMC-102`; everything else serde
/// raises here is a schema error (`TMC-103`).
fn map_serde_error(error: serde_json::Error) -> ClipError {
    let message = error.to_string();
    if message.contains("unknown field") {
        return ClipError::UnknownField { message };
    }
    if message.contains("number out of range") {
        return ClipError::NonFinite {
            context: format!("JSON number at {}:{}", error.line(), error.column()),
        };
    }
    if error.is_syntax() {
        return ClipError::Syntax {
            line: error.line(),
            column: error.column(),
            message,
        };
    }
    ClipError::Schema { message }
}

/// Whether a string is a valid id (SPEC.md section 1.3):
/// 1..=64 bytes of `[A-Za-z0-9_-]`.
pub(crate) fn valid_id(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= ID_BYTES_MAX
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Whether a string is a well-formed hash field: exactly
/// 64 lowercase hex characters (SPEC.md section 1.4).
pub(crate) fn valid_hash(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A SkeletonDef token: `<name>/<version>`, the name drawn
/// from the id charset, the version all digits — the take
/// family's token style (SPEC.md section 3.2).
fn valid_def_token(text: &str) -> bool {
    if text.matches('/').count() != 1 {
        return false;
    }
    let Some((name, version)) = text.split_once('/') else {
        return false;
    };
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-' || byte == b'.'
        })
        && !version.is_empty()
        && version.bytes().all(|byte| byte.is_ascii_digit())
}

/// Step 4: collection count limits (TMC-107; the empty
/// group list is TMC-103 per the section 10.1 table).
fn check_counts(clip: &UClip) -> Result<(), ClipError> {
    if clip.groups.is_empty() {
        return Err(ClipError::Schema {
            message: "empty group list".to_string(),
        });
    }
    if clip.groups.len() > GROUPS_MAX {
        return Err(ClipError::CountExceeded {
            collection: "groups",
            count: clip.groups.len(),
            max: GROUPS_MAX,
        });
    }
    let mut total_tracks = 0usize;
    for group in &clip.groups {
        if let Some(tracks) = &group.tracks {
            if tracks.len() > TRACKS_PER_GROUP_MAX {
                return Err(ClipError::CountExceeded {
                    collection: "tracks in one group",
                    count: tracks.len(),
                    max: TRACKS_PER_GROUP_MAX,
                });
            }
            total_tracks += tracks.len();
            for track in tracks {
                check_key_list_counts_base(track)?;
            }
        }
    }
    if total_tracks > TRACKS_PER_CLIP_MAX {
        return Err(ClipError::CountExceeded {
            collection: "base tracks in one clip",
            count: total_tracks,
            max: TRACKS_PER_CLIP_MAX,
        });
    }
    if clip.layers.len() > LAYERS_MAX {
        return Err(ClipError::CountExceeded {
            collection: "layers",
            count: clip.layers.len(),
            max: LAYERS_MAX,
        });
    }
    for layer in &clip.layers {
        if layer.tracks.len() > TRACKS_PER_LAYER_MAX {
            return Err(ClipError::CountExceeded {
                collection: "tracks in one layer",
                count: layer.tracks.len(),
                max: TRACKS_PER_LAYER_MAX,
            });
        }
        for (collection, list) in [
            ("scope bone list", &layer.scope.bones),
            ("scope channel list", &layer.scope.channels),
        ] {
            if list.len() > SCOPE_ENTRIES_MAX {
                return Err(ClipError::CountExceeded {
                    collection,
                    count: list.len(),
                    max: SCOPE_ENTRIES_MAX,
                });
            }
        }
        if let Some(name) = &layer.name
            && name.len() > NAME_BYTES_MAX
        {
            return Err(ClipError::CountExceeded {
                collection: "layer name bytes",
                count: name.len(),
                max: NAME_BYTES_MAX,
            });
        }
        for track in &layer.tracks {
            check_key_list_counts_layer(track)?;
        }
    }
    Ok(())
}

fn check_key_list_counts_base(track: &UTrack) -> Result<(), ClipError> {
    for list_len in [
        track.rotation.as_ref().map(Vec::len),
        track.scale.as_ref().map(Vec::len),
        track.translation.as_ref().map(Vec::len),
        track.keys.as_ref().map(Vec::len),
    ]
    .into_iter()
    .flatten()
    {
        if list_len > KEYS_PER_TRACK_MAX {
            return Err(ClipError::CountExceeded {
                collection: "keys in one component list",
                count: list_len,
                max: KEYS_PER_TRACK_MAX,
            });
        }
    }
    Ok(())
}

fn check_key_list_counts_layer(track: &ULayerTrack) -> Result<(), ClipError> {
    for list_len in [
        track.rotation.as_ref().map(Vec::len),
        track.scale.as_ref().map(Vec::len),
        track.translation.as_ref().map(Vec::len),
        track.keys.as_ref().map(Vec::len),
    ]
    .into_iter()
    .flatten()
    {
        if list_len > KEYS_PER_TRACK_MAX {
            return Err(ClipError::CountExceeded {
                collection: "keys in one component list",
                count: list_len,
                max: KEYS_PER_TRACK_MAX,
            });
        }
    }
    Ok(())
}

/// Step 5: ids — charset/length (TMC-201), then duplicates
/// (TMC-202), per collection in section 9's order: clip id,
/// group kinds, tracks within each group, layers, tracks
/// within each layer. Scope-entry duplicates are step 9's;
/// layer-track target duplicates are TMC-603 (step 9).
fn check_ids(clip: &UClip) -> Result<(), ClipError> {
    if !valid_id(&clip.id) {
        return Err(ClipError::InvalidId {
            id: bounded_quote(&clip.id),
        });
    }
    let mut kinds: Vec<&str> = Vec::new();
    for group in &clip.groups {
        if GroupKind::from_token(&group.kind).is_none() {
            return Err(ClipError::Schema {
                message: format!("unknown group kind `{}`", bounded_quote(&group.kind)),
            });
        }
        if kinds.contains(&group.kind.as_str()) {
            return Err(ClipError::Duplicate {
                collection: "group kinds",
                id: bounded_quote(&group.kind),
            });
        }
        kinds.push(&group.kind);
    }
    for group in &clip.groups {
        if let Some(tracks) = &group.tracks {
            let mut targets: Vec<&str> = Vec::new();
            for track in tracks {
                check_track_target_id(track.bone.as_deref(), track.channel.as_deref())?;
                if let Some(target) = track.target() {
                    if targets.contains(&target) {
                        return Err(ClipError::Duplicate {
                            collection: "tracks within a group",
                            id: bounded_quote(target),
                        });
                    }
                    targets.push(target);
                }
            }
        }
    }
    let mut layer_ids: Vec<&str> = Vec::new();
    for layer in &clip.layers {
        if !valid_id(&layer.id) {
            return Err(ClipError::InvalidId {
                id: bounded_quote(&layer.id),
            });
        }
        if layer_ids.contains(&layer.id.as_str()) {
            return Err(ClipError::Duplicate {
                collection: "layers",
                id: bounded_quote(&layer.id),
            });
        }
        layer_ids.push(&layer.id);
        for name in layer.scope.bones.iter().chain(layer.scope.channels.iter()) {
            if !valid_id(name) {
                return Err(ClipError::InvalidId {
                    id: bounded_quote(name),
                });
            }
        }
    }
    for layer in &clip.layers {
        for track in &layer.tracks {
            check_track_target_id(track.bone.as_deref(), track.channel.as_deref())?;
        }
    }
    Ok(())
}

fn check_track_target_id(bone: Option<&str>, channel: Option<&str>) -> Result<(), ClipError> {
    for name in [bone, channel].into_iter().flatten() {
        if !valid_id(name) {
            return Err(ClipError::InvalidId {
                id: bounded_quote(name),
            });
        }
    }
    Ok(())
}

/// Step 6: header fields — `frames` range (TMC-306),
/// skeleton block shape, self-hash verification (TMC-705).
fn check_header(clip: &UClip) -> Result<(), ClipError> {
    if clip.frames == 0 || clip.frames > FRAMES_MAX {
        return Err(ClipError::FramesOutOfRange {
            frames: clip.frames,
        });
    }
    if !valid_def_token(&clip.skeleton.def) {
        return Err(ClipError::Schema {
            message: format!(
                "skeleton def `{}` is not a versioned token",
                bounded_quote(&clip.skeleton.def)
            ),
        });
    }
    if !valid_hash(&clip.skeleton.sha256) {
        return Err(ClipError::HashMalformed {
            field: "skeleton.sha256",
        });
    }
    if let Some(sha) = &clip.sha256 {
        if !valid_hash(sha) {
            return Err(ClipError::HashMalformed { field: "sha256" });
        }
        // Section 3.4: the hash of the canonical
        // serialization with the `sha256` field emptied.
        let canonical = emit_canonical(clip, Some(""));
        if sha256_hex(canonical.as_bytes()) != *sha {
            return Err(ClipError::SelfHashMismatch);
        }
    }
    Ok(())
}

/// Step 7: groups — declaration-only groups carry no
/// tracks (TMC-108); canonical group/track order (TMC-205).
fn check_groups(clip: &UClip) -> Result<(), ClipError> {
    for group in &clip.groups {
        check_group_shape(group)?;
    }
    for pair in clip.groups.windows(2) {
        if pair[0].kind > pair[1].kind {
            return Err(ClipError::CanonicalOrder { what: "groups" });
        }
    }
    for group in &clip.groups {
        if let Some(tracks) = &group.tracks {
            for pair in tracks.windows(2) {
                if let (Some(a), Some(b)) = (pair[0].target(), pair[1].target())
                    && a > b
                {
                    return Err(ClipError::CanonicalOrder { what: "tracks" });
                }
            }
        }
    }
    Ok(())
}

fn check_group_shape(group: &UGroup) -> Result<(), ClipError> {
    match group.kind.as_str() {
        "camera" => {
            if group.tracks.is_some() {
                return Err(ClipError::DeclarationOnlyTracks {
                    kind: group.kind.clone(),
                });
            }
            match &group.mode {
                None => Err(ClipError::Schema {
                    message: "camera group requires a `mode` field".to_string(),
                }),
                Some(mode) if CameraMode::from_token(mode).is_none() => Err(ClipError::Schema {
                    message: format!("unknown camera mode `{}`", bounded_quote(mode)),
                }),
                _ => Ok(()),
            }
        }
        "hands" => {
            if group.tracks.is_some() {
                return Err(ClipError::DeclarationOnlyTracks {
                    kind: group.kind.clone(),
                });
            }
            if group.mode.is_some() {
                return Err(ClipError::UnknownField {
                    message: "`mode` is a camera-group field".to_string(),
                });
            }
            Ok(())
        }
        "body" | "face" => {
            if group.mode.is_some() {
                return Err(ClipError::UnknownField {
                    message: "`mode` is a camera-group field".to_string(),
                });
            }
            if group.tracks.is_none() {
                return Err(ClipError::Schema {
                    message: format!("{} group requires a `tracks` field", group.kind),
                });
            }
            Ok(())
        }
        // Kinds were classified in step 5; this arm is
        // defence in depth, not a reachable path.
        _ => Err(ClipError::Schema {
            message: "unclassified group kind".to_string(),
        }),
    }
}

/// One component key list of a base or delta track, in a
/// shape the step-8/9 passes can check uniformly. The list
/// keeps its own key type; only frames and tokens are
/// shared at this level.
struct KeyListView<'a> {
    /// Context for error messages, e.g. "body track `Hips`".
    context: String,
    /// The list's role: rotation, scale, translation, keys.
    role: &'static str,
    /// Key frames, in authored order.
    frames: Vec<u32>,
    /// Interpolation tokens, in authored order.
    interpolations: Vec<&'a str>,
}

/// Step 8: base tracks and keys. Class-major in section
/// 9's order: per-track component presence (TMC-103); key
/// frame order and range (TMC-501); interpolation tokens
/// (TMC-502); numeric ranges (TMC-303/304/305, with
/// finiteness, TMC-301, per value first).
fn check_base_tracks(clip: &UClip) -> Result<(), ClipError> {
    for group in &clip.groups {
        if let Some(tracks) = &group.tracks {
            for track in tracks {
                check_base_track_shape(&group.kind, track)?;
            }
        }
    }
    let lists = collect_base_lists(clip);
    check_key_order(&lists, clip.frames)?;
    check_interpolation_tokens(&lists)?;
    for group in &clip.groups {
        if let Some(tracks) = &group.tracks {
            for track in tracks {
                check_base_track_numbers(&group.kind, track)?;
            }
        }
    }
    Ok(())
}

fn check_base_track_shape(kind: &str, track: &UTrack) -> Result<(), ClipError> {
    match (track.bone.is_some(), track.channel.is_some()) {
        (true, false) => {
            if kind != "body" {
                return Err(ClipError::Schema {
                    message: "bone track outside the body group".to_string(),
                });
            }
            if track.keys.is_some() {
                return Err(ClipError::Schema {
                    message: "bone track carries scalar `keys`".to_string(),
                });
            }
            check_component_presence(
                track.rotation.as_deref(),
                track.scale.as_deref(),
                track.translation.as_deref(),
            )
        }
        (false, true) => {
            if kind != "face" {
                return Err(ClipError::Schema {
                    message: "channel track outside the face group".to_string(),
                });
            }
            if track.rotation.is_some() || track.scale.is_some() || track.translation.is_some() {
                return Err(ClipError::Schema {
                    message: "channel track carries component key lists".to_string(),
                });
            }
            match &track.keys {
                None => Err(ClipError::Schema {
                    message: "channel track carries no `keys`".to_string(),
                }),
                Some(keys) if keys.is_empty() => Err(ClipError::Schema {
                    message: "empty component key list".to_string(),
                }),
                _ => Ok(()),
            }
        }
        _ => Err(ClipError::Schema {
            message: "track must name exactly one of `bone` or `channel`".to_string(),
        }),
    }
}

/// A bone track must carry at least one component list, and
/// every present list must be non-empty (TMC-103).
fn check_component_presence<K1, K2, K3>(
    rotation: Option<&[K1]>,
    scale: Option<&[K2]>,
    translation: Option<&[K3]>,
) -> Result<(), ClipError> {
    if rotation.is_none() && scale.is_none() && translation.is_none() {
        return Err(ClipError::Schema {
            message: "track with no component lists".to_string(),
        });
    }
    for list in [
        rotation.map(<[K1]>::len),
        scale.map(<[K2]>::len),
        translation.map(<[K3]>::len),
    ]
    .into_iter()
    .flatten()
    {
        if list == 0 {
            return Err(ClipError::Schema {
                message: "empty component key list".to_string(),
            });
        }
    }
    Ok(())
}

fn collect_base_lists(clip: &UClip) -> Vec<KeyListView<'_>> {
    let mut lists = Vec::new();
    for group in &clip.groups {
        if let Some(tracks) = &group.tracks {
            for track in tracks {
                let context = format!(
                    "{} track `{}`",
                    group.kind,
                    track.target().unwrap_or("<none>")
                );
                if let Some(keys) = &track.rotation {
                    lists.push(KeyListView {
                        context: context.clone(),
                        role: "rotation",
                        frames: keys.iter().map(|key| key.frame).collect(),
                        interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                    });
                }
                if let Some(keys) = &track.scale {
                    lists.push(KeyListView {
                        context: context.clone(),
                        role: "scale",
                        frames: keys.iter().map(|key| key.frame).collect(),
                        interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                    });
                }
                if let Some(keys) = &track.translation {
                    lists.push(KeyListView {
                        context: context.clone(),
                        role: "translation",
                        frames: keys.iter().map(|key| key.frame).collect(),
                        interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                    });
                }
                if let Some(keys) = &track.keys {
                    lists.push(KeyListView {
                        context,
                        role: "keys",
                        frames: keys.iter().map(|key| key.frame).collect(),
                        interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                    });
                }
            }
        }
    }
    lists
}

fn check_key_order(lists: &[KeyListView<'_>], frames: u32) -> Result<(), ClipError> {
    for list in lists {
        for pair in list.frames.windows(2) {
            if pair[0] >= pair[1] {
                return Err(ClipError::KeyOrder {
                    context: format!("{} {}", list.context, list.role),
                });
            }
        }
        if let Some(last) = list.frames.last()
            && *last >= frames
        {
            return Err(ClipError::KeyOrder {
                context: format!("{} {}", list.context, list.role),
            });
        }
    }
    Ok(())
}

fn check_interpolation_tokens(lists: &[KeyListView<'_>]) -> Result<(), ClipError> {
    for list in lists {
        for token in &list.interpolations {
            if Interpolation::from_token(token).is_none() {
                return Err(ClipError::InterpolationToken {
                    found: bounded_quote(token),
                });
            }
        }
    }
    Ok(())
}

fn check_base_track_numbers(kind: &str, track: &UTrack) -> Result<(), ClipError> {
    let context = format!("{} track `{}`", kind, track.target().unwrap_or("<none>"));
    if let Some(keys) = &track.rotation {
        for key in keys {
            check_quat(&key.value, &context)?;
        }
    }
    if let Some(keys) = &track.scale {
        for key in keys {
            for component in key.value {
                check_finite(component, &context)?;
                if component <= 0.0 {
                    return Err(ClipError::ScaleViolation { context });
                }
            }
        }
    }
    if let Some(keys) = &track.translation {
        for key in keys {
            for component in key.value {
                check_magnitude(component, COORD_MAGNITUDE_MAX, &context)?;
            }
        }
    }
    if let Some(keys) = &track.keys {
        for key in keys {
            check_magnitude(key.value, SCALAR_MAGNITUDE_MAX, &context)?;
        }
    }
    Ok(())
}

/// Finiteness first (TMC-301), then the unit tolerance
/// (TMC-303): a non-finite component makes the norm
/// meaningless.
fn check_quat(value: &[f64; 4], context: &str) -> Result<(), ClipError> {
    for component in value {
        check_finite(*component, context)?;
    }
    let norm =
        (value[0] * value[0] + value[1] * value[1] + value[2] * value[2] + value[3] * value[3])
            .sqrt();
    if (norm - 1.0).abs() > QUAT_UNIT_TOLERANCE {
        return Err(ClipError::NonUnitQuaternion {
            context: context.to_string(),
        });
    }
    Ok(())
}

fn check_finite(value: f64, context: &str) -> Result<(), ClipError> {
    if !value.is_finite() {
        return Err(ClipError::NonFinite {
            context: context.to_string(),
        });
    }
    Ok(())
}

fn check_magnitude(value: f64, max: f64, context: &str) -> Result<(), ClipError> {
    check_finite(value, context)?;
    if value.abs() > max {
        return Err(ClipError::MagnitudeExceeded {
            context: context.to_string(),
        });
    }
    Ok(())
}

/// Step 9: layers, in section 9's order — weight range
/// (TMC-302); scope list shape (TMC-202, order TMC-205);
/// track shape (TMC-103); every track's target inside the
/// scope (TMC-601); per-target uniqueness (TMC-603); key
/// order (TMC-501) and interpolation tokens (TMC-502);
/// delta-key ranges as in step 8.
fn check_layers(clip: &UClip) -> Result<(), ClipError> {
    for layer in &clip.layers {
        if !layer.weight.is_finite() {
            return Err(ClipError::NonFinite {
                context: format!("layer `{}` weight", layer.id),
            });
        }
        if !(0.0..=1.0).contains(&layer.weight) {
            return Err(ClipError::WeightOutOfRange {
                layer: bounded_quote(&layer.id),
                weight: layer.weight,
            });
        }
    }
    for layer in &clip.layers {
        check_scope_shape(layer)?;
    }
    for layer in &clip.layers {
        for track in &layer.tracks {
            check_layer_track_shape(track)?;
        }
    }
    for layer in &clip.layers {
        for track in &layer.tracks {
            let in_scope = match (track.bone.as_deref(), track.channel.as_deref()) {
                (Some(bone), None) => layer.scope.bones.iter().any(|name| name == bone),
                (None, Some(channel)) => layer.scope.channels.iter().any(|name| name == channel),
                // Shape was checked above; both/neither was
                // already refused there.
                _ => false,
            };
            if !in_scope {
                return Err(ClipError::ScopeViolation {
                    layer: bounded_quote(&layer.id),
                    target: bounded_quote(track.target().unwrap_or("<none>")),
                });
            }
        }
    }
    for layer in &clip.layers {
        let mut bones: Vec<&str> = Vec::new();
        let mut channels: Vec<&str> = Vec::new();
        for track in &layer.tracks {
            if let Some(bone) = track.bone.as_deref() {
                if bones.contains(&bone) {
                    return Err(ClipError::DuplicateLayerTarget {
                        layer: bounded_quote(&layer.id),
                        target: bounded_quote(bone),
                    });
                }
                bones.push(bone);
            }
            if let Some(channel) = track.channel.as_deref() {
                if channels.contains(&channel) {
                    return Err(ClipError::DuplicateLayerTarget {
                        layer: bounded_quote(&layer.id),
                        target: bounded_quote(channel),
                    });
                }
                channels.push(channel);
            }
        }
    }
    let lists = collect_layer_lists(clip);
    check_key_order(&lists, clip.frames)?;
    check_interpolation_tokens(&lists)?;
    for layer in &clip.layers {
        for track in &layer.tracks {
            check_layer_track_numbers(layer, track)?;
        }
    }
    Ok(())
}

fn check_scope_shape(layer: &ULayer) -> Result<(), ClipError> {
    for (collection, list) in [
        ("scope bone list", &layer.scope.bones),
        ("scope channel list", &layer.scope.channels),
    ] {
        let mut seen: Vec<&str> = Vec::new();
        for name in list {
            if seen.contains(&name.as_str()) {
                return Err(ClipError::Duplicate {
                    collection,
                    id: bounded_quote(name),
                });
            }
            seen.push(name);
        }
        for pair in list.windows(2) {
            if pair[0] > pair[1] {
                return Err(ClipError::CanonicalOrder {
                    what: "scope lists",
                });
            }
        }
    }
    Ok(())
}

fn check_layer_track_shape(track: &ULayerTrack) -> Result<(), ClipError> {
    match (track.bone.is_some(), track.channel.is_some()) {
        (true, false) => {
            if track.keys.is_some() {
                return Err(ClipError::Schema {
                    message: "bone layer track carries scalar `keys`".to_string(),
                });
            }
            check_component_presence(
                track.rotation.as_deref(),
                track.scale.as_deref(),
                track.translation.as_deref(),
            )
        }
        (false, true) => {
            if track.rotation.is_some() || track.scale.is_some() || track.translation.is_some() {
                return Err(ClipError::Schema {
                    message: "channel layer track carries component key lists".to_string(),
                });
            }
            match &track.keys {
                None => Err(ClipError::Schema {
                    message: "channel layer track carries no `keys`".to_string(),
                }),
                Some(keys) if keys.is_empty() => Err(ClipError::Schema {
                    message: "empty component key list".to_string(),
                }),
                _ => Ok(()),
            }
        }
        _ => Err(ClipError::Schema {
            message: "layer track must name exactly one of `bone` or `channel`".to_string(),
        }),
    }
}

fn collect_layer_lists(clip: &UClip) -> Vec<KeyListView<'_>> {
    let mut lists = Vec::new();
    for layer in &clip.layers {
        for track in &layer.tracks {
            let context = format!(
                "layer `{}` track `{}`",
                layer.id,
                track.target().unwrap_or("<none>")
            );
            if let Some(keys) = &track.rotation {
                lists.push(KeyListView {
                    context: context.clone(),
                    role: "rotation",
                    frames: keys.iter().map(|key| key.frame).collect(),
                    interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                });
            }
            if let Some(keys) = &track.scale {
                lists.push(KeyListView {
                    context: context.clone(),
                    role: "scale",
                    frames: keys.iter().map(|key| key.frame).collect(),
                    interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                });
            }
            if let Some(keys) = &track.translation {
                lists.push(KeyListView {
                    context: context.clone(),
                    role: "translation",
                    frames: keys.iter().map(|key| key.frame).collect(),
                    interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                });
            }
            if let Some(keys) = &track.keys {
                lists.push(KeyListView {
                    context,
                    role: "keys",
                    frames: keys.iter().map(|key| key.frame).collect(),
                    interpolations: keys.iter().map(|key| key.interpolation.as_str()).collect(),
                });
            }
        }
    }
    lists
}

fn check_layer_track_numbers(layer: &ULayer, track: &ULayerTrack) -> Result<(), ClipError> {
    let context = format!(
        "layer `{}` track `{}`",
        layer.id,
        track.target().unwrap_or("<none>")
    );
    if let Some(keys) = &track.rotation {
        for key in keys {
            check_quat(&key.value, &context)?;
        }
    }
    if let Some(keys) = &track.scale {
        for key in keys {
            for component in key.value {
                check_finite(component, &context)?;
                if component <= 0.0 || component > SCALE_FACTOR_MAX {
                    return Err(ClipError::ScaleViolation { context });
                }
            }
        }
    }
    if let Some(keys) = &track.translation {
        for key in keys {
            for component in key.value {
                check_magnitude(component, COORD_MAGNITUDE_MAX, &context)?;
            }
        }
    }
    if let Some(keys) = &track.keys {
        for key in keys {
            check_magnitude(key.value, SCALAR_MAGNITUDE_MAX, &context)?;
        }
    }
    Ok(())
}

/// Step 10: provenance — block shape; exactly one entry
/// per present group (TMC-703); source-class tokens
/// (TMC-701); hash field shapes (TMC-704); frame maps
/// (TMC-702).
fn check_provenance(clip: &UClip) -> Result<(), ClipError> {
    if clip.provenance.format != PROVENANCE_TOKEN {
        return Err(ClipError::Schema {
            message: format!(
                "provenance format `{}` is not {PROVENANCE_TOKEN}",
                bounded_quote(&clip.provenance.format)
            ),
        });
    }
    let present: Vec<&str> = clip
        .groups
        .iter()
        .map(|group| group.kind.as_str())
        .collect();
    let mut covered: Vec<&str> = Vec::new();
    for entry in &clip.provenance.groups {
        if GroupKind::from_token(&entry.group).is_none() || !present.contains(&entry.group.as_str())
        {
            return Err(ClipError::ProvenanceCoverage {
                message: format!(
                    "entry for group `{}` the clip does not carry",
                    bounded_quote(&entry.group)
                ),
            });
        }
        if covered.contains(&entry.group.as_str()) {
            return Err(ClipError::ProvenanceCoverage {
                message: format!("two entries for group `{}`", bounded_quote(&entry.group)),
            });
        }
        covered.push(&entry.group);
    }
    if covered.len() != present.len() {
        return Err(ClipError::ProvenanceCoverage {
            message: "provenance entries do not cover every group".to_string(),
        });
    }
    for entry in &clip.provenance.groups {
        let Some(source_class) = SourceClass::from_token(&entry.source_class) else {
            return Err(ClipError::SourceClassToken {
                found: bounded_quote(&entry.source_class),
            });
        };
        if source_class == SourceClass::Authored && entry.converter != "authored" {
            return Err(ClipError::Schema {
                message: "an authored group must name converter \"authored\"".to_string(),
            });
        }
        if let Some(reference) = &entry.measurement_ref
            && !valid_id(reference)
        {
            return Err(ClipError::InvalidId {
                id: bounded_quote(reference),
            });
        }
    }
    for entry in &clip.provenance.groups {
        for (field, hash) in [
            ("model_sha256", &entry.model_sha256),
            ("source_sha256", &entry.source_sha256),
        ] {
            if let Some(hash) = hash
                && !valid_hash(hash)
            {
                return Err(ClipError::HashMalformed { field });
            }
        }
    }
    for entry in &clip.provenance.groups {
        if let Some(map) = &entry.frame_map {
            check_frame_map(map, clip.frames)?;
        }
    }
    Ok(())
}

fn check_frame_map(map: &crate::uclip::UFrameMap, frames: u32) -> Result<(), ClipError> {
    if map.source_frame_count > SOURCE_FRAMES_MAX {
        return Err(ClipError::CountExceeded {
            collection: "frame map source_frame_count",
            count: map.source_frame_count as usize,
            max: SOURCE_FRAMES_MAX as usize,
        });
    }
    if let crate::uclip::UFrameRate::Rational { den, .. } = map.source_rate
        && den == 0
    {
        return Err(ClipError::FrameMapInvalid {
            message: "source_rate denominator is zero".to_string(),
        });
    }
    if map.entries.len() != frames as usize {
        return Err(ClipError::FrameMapInvalid {
            message: format!(
                "frame map has {} entries for {} clip frames",
                map.entries.len(),
                frames
            ),
        });
    }
    for pair in map.entries.windows(2) {
        if pair[0] > pair[1] {
            return Err(ClipError::FrameMapInvalid {
                message: "frame map entries decrease".to_string(),
            });
        }
    }
    if let Some(entry) = map
        .entries
        .iter()
        .find(|entry| **entry >= map.source_frame_count)
    {
        return Err(ClipError::FrameMapInvalid {
            message: format!("frame map entry {entry} is beyond the source frame count"),
        });
    }
    Ok(())
}

/// Convert the validated unchecked form into the public
/// model. Every token mapping here was already proven by
/// the check steps; a failure is returned as a schema error
/// rather than panicking, as defence in depth.
fn build_clip(clip: UClip) -> Result<Clip, ClipError> {
    let mut groups = Vec::with_capacity(clip.groups.len());
    for group in clip.groups {
        groups.push(build_group(group)?);
    }
    let mut layers = Vec::with_capacity(clip.layers.len());
    for layer in clip.layers {
        layers.push(build_layer(layer)?);
    }
    let mut provenance_groups = Vec::with_capacity(clip.provenance.groups.len());
    for entry in clip.provenance.groups {
        provenance_groups.push(GroupProvenance {
            group: GroupKind::from_token(&entry.group).ok_or_else(|| ClipError::Schema {
                message: "unclassified group kind survived validation".to_string(),
            })?,
            source_class: SourceClass::from_token(&entry.source_class).ok_or_else(|| {
                ClipError::Schema {
                    message: "unclassified source class survived validation".to_string(),
                }
            })?,
            converter: entry.converter,
            converter_version: entry.converter_version,
            model_sha256: entry.model_sha256,
            source_sha256: entry.source_sha256,
            region: entry.region.map(|region| Region {
                x: region.x,
                y: region.y,
                w: region.w,
                h: region.h,
            }),
            measurement_ref: entry.measurement_ref,
            frame_map: entry.frame_map.map(|map| FrameMap {
                source_frame_count: map.source_frame_count,
                source_rate: match map.source_rate {
                    crate::uclip::UFrameRate::Rational { num, den } => {
                        FrameRate::Rational { num, den }
                    }
                    crate::uclip::UFrameRate::Variable => FrameRate::Variable,
                },
                entries: map.entries,
            }),
        });
    }
    Ok(Clip {
        id: clip.id,
        skeleton: SkeletonBinding {
            def: clip.skeleton.def,
            sha256: clip.skeleton.sha256,
        },
        frames: clip.frames,
        loop_: clip.loop_,
        groups,
        layers,
        provenance: Provenance {
            groups: provenance_groups,
        },
        sha256: clip.sha256,
    })
}

fn build_group(group: UGroup) -> Result<Group, ClipError> {
    match group.kind.as_str() {
        "body" => {
            let tracks = group.tracks.unwrap_or_default();
            let mut built = Vec::with_capacity(tracks.len());
            for track in tracks {
                built.push(BoneTrack {
                    bone: track.bone.ok_or_else(|| ClipError::Schema {
                        message: "body track without a bone survived validation".to_string(),
                    })?,
                    rotation: map_keys(track.rotation, |key| {
                        Ok(QuatKey {
                            frame: key.frame,
                            value: key.value,
                            interpolation: build_interpolation(&key.interpolation)?,
                        })
                    })?,
                    scale: map_keys(track.scale, |key| {
                        Ok(VecKey {
                            frame: key.frame,
                            value: key.value,
                            interpolation: build_interpolation(&key.interpolation)?,
                        })
                    })?,
                    translation: map_keys(track.translation, |key| {
                        Ok(VecKey {
                            frame: key.frame,
                            value: key.value,
                            interpolation: build_interpolation(&key.interpolation)?,
                        })
                    })?,
                });
            }
            Ok(Group::Body { tracks: built })
        }
        "face" => {
            let tracks = group.tracks.unwrap_or_default();
            let mut built = Vec::with_capacity(tracks.len());
            for track in tracks {
                built.push(ChannelTrack {
                    channel: track.channel.ok_or_else(|| ClipError::Schema {
                        message: "face track without a channel survived validation".to_string(),
                    })?,
                    keys: map_keys(track.keys, |key| {
                        Ok(ScalarKey {
                            frame: key.frame,
                            value: key.value,
                            interpolation: build_interpolation(&key.interpolation)?,
                        })
                    })?
                    .unwrap_or_default(),
                });
            }
            Ok(Group::Face { tracks: built })
        }
        "camera" => Ok(Group::Camera {
            mode: CameraMode::from_token(group.mode.as_deref().unwrap_or("")).ok_or_else(|| {
                ClipError::Schema {
                    message: "camera mode survived validation unclassified".to_string(),
                }
            })?,
        }),
        "hands" => Ok(Group::Hands),
        _ => Err(ClipError::Schema {
            message: "unclassified group kind survived validation".to_string(),
        }),
    }
}

fn build_layer(layer: ULayer) -> Result<Layer, ClipError> {
    let mut tracks = Vec::with_capacity(layer.tracks.len());
    for track in layer.tracks {
        if let Some(bone) = track.bone {
            tracks.push(LayerTrack::Bone(LayerBoneTrack {
                bone,
                rotation: map_keys(track.rotation, |key| {
                    Ok(DeltaQuatKey {
                        frame: key.frame,
                        value: key.value,
                        interpolation: build_interpolation(&key.interpolation)?,
                    })
                })?,
                scale: map_keys(track.scale, |key| {
                    Ok(DeltaVecKey {
                        frame: key.frame,
                        value: key.value,
                        interpolation: build_interpolation(&key.interpolation)?,
                    })
                })?,
                translation: map_keys(track.translation, |key| {
                    Ok(DeltaVecKey {
                        frame: key.frame,
                        value: key.value,
                        interpolation: build_interpolation(&key.interpolation)?,
                    })
                })?,
            }));
        } else if let Some(channel) = track.channel {
            tracks.push(LayerTrack::Channel(LayerChannelTrack {
                channel,
                keys: map_keys(track.keys, |key| {
                    Ok(DeltaScalarKey {
                        frame: key.frame,
                        value: key.value,
                        interpolation: build_interpolation(&key.interpolation)?,
                    })
                })?
                .unwrap_or_default(),
            }));
        } else {
            return Err(ClipError::Schema {
                message: "layer track without a target survived validation".to_string(),
            });
        }
    }
    Ok(Layer {
        id: layer.id,
        name: layer.name,
        weight: layer.weight,
        scope: Scope {
            bones: layer.scope.bones,
            channels: layer.scope.channels,
        },
        tracks,
    })
}

fn build_interpolation(token: &str) -> Result<Interpolation, ClipError> {
    Interpolation::from_token(token).ok_or_else(|| ClipError::Schema {
        message: "unclassified interpolation survived validation".to_string(),
    })
}

/// Map an optional key list through a fallible key builder.
fn map_keys<K, T>(
    keys: Option<Vec<K>>,
    build: impl Fn(K) -> Result<T, ClipError>,
) -> Result<Option<Vec<T>>, ClipError> {
    match keys {
        None => Ok(None),
        Some(keys) => Ok(Some(
            keys.into_iter().map(build).collect::<Result<Vec<_>, _>>()?,
        )),
    }
}
