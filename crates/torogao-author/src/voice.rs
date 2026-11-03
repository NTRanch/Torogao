//! The torori voice seam's validation half: registry
//! snapshots and the shippable-build sweep (voice plan §3).
//!
//! A voice binding (see [`crate::model::AudioBinding`]) names
//! a torori VoiceIdentity by reference. Whether that identity
//! may ship is torori's fact, not this crate's: the identity's
//! lock state lives in torori's registry. The seam's contract
//! is a **snapshot**: the caller supplies a registry snapshot
//! document (below) vouching for the lock states at check
//! time, and [`validate_shippable`] reads states from it —
//! this crate never opens torori's state, calls torori, or
//! caches a state past the check.
//!
//! The rule: a voice binding is legal while authoring at any
//! lock state; a shippable build refuses every binding whose
//! identity is not ear-admitted in the snapshot — including
//! when the identity is absent from the snapshot, and when no
//! snapshot is supplied at all (the check refuses closed,
//! `TG-AUTH-VOICE-UNADMITTED`).

use crate::model::{AudioBindingKind, Project};
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::ron::{Ron, parse_document};

/// Maximum identities in one registry snapshot document.
pub const SNAPSHOT_IDENTITIES_MAX: usize = 4096;

/// A torori VoiceIdentity lock state, mirrored by name from
/// torori's `LockState` (ordering included):
/// `Designed < Anchored < EarAdmitted`. The last transition is
/// the Owner's ear verdict in torori; this crate only reads
/// the state, it never grants one.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum VoiceLockState {
    /// Designed: a construction record exists, no locked
    /// reference performance yet.
    Designed,
    /// Anchored: a locked reference performance exists.
    Anchored,
    /// Ear-admitted: the Owner's ear verdict admitted the
    /// anchor. The only state a shippable build accepts.
    EarAdmitted,
}

impl VoiceLockState {
    /// The ident naming this state in a snapshot document:
    /// torori's `LockState` variant name.
    pub fn ident(self) -> &'static str {
        match self {
            VoiceLockState::Designed => "Designed",
            VoiceLockState::Anchored => "Anchored",
            VoiceLockState::EarAdmitted => "EarAdmitted",
        }
    }

    /// The label torori's own JSON surface uses for this state
    /// (its `LockState::as_str`); used in refusal messages.
    pub fn label(self) -> &'static str {
        match self {
            VoiceLockState::Designed => "designed",
            VoiceLockState::Anchored => "anchored",
            VoiceLockState::EarAdmitted => "ear-admitted",
        }
    }

    fn from_ident(name: &str) -> Option<VoiceLockState> {
        match name {
            "Designed" => Some(VoiceLockState::Designed),
            "Anchored" => Some(VoiceLockState::Anchored),
            "EarAdmitted" => Some(VoiceLockState::EarAdmitted),
            _ => None,
        }
    }

    /// True exactly at `EarAdmitted`.
    pub fn is_ear_admitted(self) -> bool {
        self == VoiceLockState::EarAdmitted
    }
}

/// One identity's lock state in a registry snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoiceIdentityState {
    /// The torori VoiceIdentity id (`identity_ref`).
    pub identity: String,
    /// Its lock state when the snapshot was taken.
    pub lock_state: VoiceLockState,
}

/// A snapshot of torori's VoiceIdentity registry: the
/// lock-state facts a caller vouches for at check time.
///
/// The document form is a RON record in this crate's dialect:
///
/// ```text
/// VoiceRegistrySnapshot(
///     identities: [
///         VoiceIdentityState(
///             identity: "voice.trial.prima-1",
///             lock_state: EarAdmitted,
///         ),
///     ],
/// )
/// ```
///
/// Only the facts the sweep needs cross the seam: identity id
/// and lock state. Construction records, locked references,
/// and conditioning fields stay in torori.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct VoiceRegistrySnapshot {
    identities: Vec<VoiceIdentityState>,
}

impl VoiceRegistrySnapshot {
    /// Parses a snapshot document. Unknown records, fields, or
    /// lock-state names are refused typed, as is a duplicate
    /// identity (a snapshot must be a function from identity
    /// to state).
    pub fn parse(text: &str) -> Result<VoiceRegistrySnapshot, Refusal> {
        let value = parse_document(text)?;
        let mut fields = open_snapshot(value, "VoiceRegistrySnapshot")?;
        let items = take_list(&mut fields, "identities", "VoiceRegistrySnapshot")?;
        if items.len() > SNAPSHOT_IDENTITIES_MAX {
            return refuse(
                RefusalClass::Range,
                format!(
                    "registry snapshot carries {} identities, over the {SNAPSHOT_IDENTITIES_MAX} bound",
                    items.len()
                ),
            );
        }
        let mut identities: Vec<VoiceIdentityState> = Vec::new();
        for item in items {
            let state = identity_state_from_ron(item)?;
            if identities
                .iter()
                .any(|existing| existing.identity == state.identity)
            {
                return refuse(
                    RefusalClass::Malformed,
                    format!(
                        "registry snapshot names identity '{}' twice",
                        state.identity
                    ),
                );
            }
            identities.push(state);
        }
        done_snapshot(fields, "VoiceRegistrySnapshot")?;
        Ok(VoiceRegistrySnapshot { identities })
    }

    /// The lock state this snapshot vouches for an identity,
    /// or `None` when the identity is absent from it.
    pub fn lock_state(&self, identity: &str) -> Option<VoiceLockState> {
        self.identities
            .iter()
            .find(|state| state.identity == identity)
            .map(|state| state.lock_state)
    }
}

/// The shippable-build sweep: one `TG-AUTH-VOICE-UNADMITTED`
/// finding per voice binding whose torori identity is not
/// ear-admitted in `snapshot`. A project with no voice
/// bindings has no findings under any snapshot, including
/// none. Authoring validation ([`crate::validate::validate`])
/// never runs this sweep: unadmitted bindings are legal while
/// authoring.
pub fn validate_shippable(
    project: &Project,
    snapshot: Option<&VoiceRegistrySnapshot>,
) -> Vec<Refusal> {
    let mut findings = Vec::new();
    for binding in &project.audio_bindings {
        // The sweep judges voice bindings only; a hypnosis
        // track row carries no torori identity to admit.
        let AudioBindingKind::Voice(voice) = &binding.kind else {
            continue;
        };
        let identity = &voice.delivery.identity;
        let message = match snapshot {
            None => Some(format!(
                "voice binding '{}' cannot ship: no torori registry snapshot was supplied, \
                 so identity '{identity}' cannot be shown ear-admitted (the check refuses closed)",
                binding.id
            )),
            Some(snapshot) => match snapshot.lock_state(identity) {
                None => Some(format!(
                    "voice binding '{}' cannot ship: identity '{identity}' is absent \
                     from the supplied registry snapshot",
                    binding.id
                )),
                Some(state) if !state.is_ear_admitted() => Some(format!(
                    "voice binding '{}' cannot ship: identity '{identity}' is {} \
                     in the supplied registry snapshot, below ear-admitted",
                    binding.id,
                    state.label()
                )),
                Some(_) => None,
            },
        };
        if let Some(message) = message {
            findings.push(Refusal::new(RefusalClass::VoiceUnadmitted, message));
        }
    }
    findings
}

// ---- snapshot document parsing --------------------------------

fn open_snapshot(value: Ron, expected: &str) -> Result<Vec<(String, Ron)>, Refusal> {
    match value {
        Ron::Struct(name, fields) if name == expected => Ok(fields),
        Ron::Struct(name, _) => refuse(
            RefusalClass::UnknownKind,
            format!("record kind '{name}' is not known where '{expected}' is expected"),
        ),
        _ => refuse(
            RefusalClass::Malformed,
            format!("expected a {expected} record"),
        ),
    }
}

fn take_field(fields: &mut Vec<(String, Ron)>, key: &str, record: &str) -> Result<Ron, Refusal> {
    match fields.iter().position(|(name, _)| name == key) {
        Some(index) => Ok(fields.remove(index).1),
        None => refuse(
            RefusalClass::Malformed,
            format!("{record} record lacks field '{key}'"),
        ),
    }
}

fn take_list(
    fields: &mut Vec<(String, Ron)>,
    key: &str,
    record: &str,
) -> Result<Vec<Ron>, Refusal> {
    match take_field(fields, key, record)? {
        Ron::List(items) => Ok(items),
        _ => refuse(
            RefusalClass::Malformed,
            format!("field '{key}' must be a list"),
        ),
    }
}

fn done_snapshot(fields: Vec<(String, Ron)>, record: &str) -> Result<(), Refusal> {
    match fields.first() {
        Some((name, _)) => refuse(
            RefusalClass::Malformed,
            format!("{record} record has unknown field '{name}'"),
        ),
        None => Ok(()),
    }
}

fn identity_state_from_ron(value: Ron) -> Result<VoiceIdentityState, Refusal> {
    let mut fields = open_snapshot(value, "VoiceIdentityState")?;
    let identity = match take_field(&mut fields, "identity", "VoiceIdentityState")? {
        Ron::Str(text) => text,
        _ => {
            return refuse(RefusalClass::Malformed, "field 'identity' must be a string");
        }
    };
    if identity.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "snapshot identity must not be empty",
        );
    }
    let lock_state = match take_field(&mut fields, "lock_state", "VoiceIdentityState")? {
        Ron::Ident(name) => VoiceLockState::from_ident(&name).ok_or_else(|| {
            Refusal::new(
                RefusalClass::UnknownKind,
                format!("lock_state names unknown state '{name}'"),
            )
        })?,
        _ => {
            return refuse(
                RefusalClass::Malformed,
                "field 'lock_state' must be a state name",
            );
        }
    };
    done_snapshot(fields, "VoiceIdentityState")?;
    Ok(VoiceIdentityState {
        identity,
        lock_state,
    })
}
