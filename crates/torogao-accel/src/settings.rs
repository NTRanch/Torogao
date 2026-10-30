//! The acceleration settings model (design section 1).
//!
//! The user's preferences are a typed value ([`AccelSettings`])
//! with a persisted record shape ([`AccelSettingsRecord`], the
//! `accel` entry of the save schema's globals partition).
//! Loading a record parses its closed token sets strictly and
//! validates the combination: an unknown token, an unsupported
//! schema, or a contradictory combination is a typed refusal
//! ([`SettingsError`]), never a silent coercion — the app falls
//! back to the estate defaults and shows the refusal.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::bounded_text;

/// Schema id of the persisted settings record.
pub const SETTINGS_SCHEMA_ID: &str = "torogao-accel-settings/1";

/// A settings load or validation refusal. Each variant carries a
/// stable `TG-ACCEL-SET-*` code so refusals can be asserted in
/// tests and recognized in support reports without matching on
/// prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    /// GPU compute was requested while the master switch is off.
    ComputeWithoutMaster,
    /// GPU compute was requested with the Software render
    /// backend: "software only" and "GPU compute" contradict.
    ComputeWithSoftwareBackend,
    /// A record field held a token outside its closed set.
    UnknownToken { field: &'static str, value: String },
    /// The record's schema id is not one this build reads.
    SchemaMismatch { found: String },
}

impl SettingsError {
    /// The stable refusal code for this error.
    pub fn code(&self) -> &'static str {
        match self {
            Self::ComputeWithoutMaster => "TG-ACCEL-SET-COMPUTE-WITHOUT-MASTER",
            Self::ComputeWithSoftwareBackend => "TG-ACCEL-SET-COMPUTE-SOFTWARE",
            Self::UnknownToken { .. } => "TG-ACCEL-SET-UNKNOWN-TOKEN",
            Self::SchemaMismatch { .. } => "TG-ACCEL-SET-SCHEMA",
        }
    }
}

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComputeWithoutMaster => {
                write!(f, "GPU compute requires the master acceleration switch")
            }
            Self::ComputeWithSoftwareBackend => {
                write!(
                    f,
                    "GPU compute is unavailable with the Software render backend"
                )
            }
            Self::UnknownToken { field, value } => {
                write!(
                    f,
                    "unknown value {value:?} for acceleration setting {field}"
                )
            }
            Self::SchemaMismatch { found } => {
                write!(f, "unsupported acceleration settings schema {found:?}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

/// Which rendering backend the user prefers. Resolution against
/// the machine happens later, in `capability::evaluate`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderBackendPref {
    /// The evaluator chooses (Vulkan, then OpenGL, then
    /// software).
    #[default]
    Auto,
    /// wgpu's Vulkan backend.
    Vulkan,
    /// wgpu's GL backend (the compatibility path, design
    /// section 2.2).
    OpenGl,
    /// CPU rendering. A fully supported configuration.
    Software,
}

impl RenderBackendPref {
    /// The persisted token for this value.
    pub fn token(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Vulkan => "vulkan",
            Self::OpenGl => "opengl",
            Self::Software => "software",
        }
    }

    /// Parses a persisted token. Anything outside the closed set
    /// is a typed refusal.
    pub fn from_token(token: &str) -> Result<Self, SettingsError> {
        match token {
            "auto" => Ok(Self::Auto),
            "vulkan" => Ok(Self::Vulkan),
            "opengl" => Ok(Self::OpenGl),
            "software" => Ok(Self::Software),
            other => Err(SettingsError::UnknownToken {
                field: "render_backend",
                value: bounded_text(other),
            }),
        }
    }
}

/// Adapter power preference: breaks ties among adapters of the
/// chosen backend (design section 1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PowerPref {
    /// Prefer discrete, then integrated.
    #[default]
    Auto,
    /// Prefer the integrated adapter.
    LowPower,
    /// Prefer the discrete adapter.
    HighPerformance,
}

impl PowerPref {
    /// The persisted token for this value.
    pub fn token(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::LowPower => "low-power",
            Self::HighPerformance => "high-performance",
        }
    }

    /// Parses a persisted token.
    pub fn from_token(token: &str) -> Result<Self, SettingsError> {
        match token {
            "auto" => Ok(Self::Auto),
            "low-power" => Ok(Self::LowPower),
            "high-performance" => Ok(Self::HighPerformance),
            other => Err(SettingsError::UnknownToken {
                field: "power_preference",
                value: bounded_text(other),
            }),
        }
    }
}

/// How the device for Vulkan *compute* launches is selected
/// (design section 7). This never applies to rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ComputeIcdPin {
    /// The evaluator decides; on a hybrid machine this resolves
    /// to the NVIDIA pin.
    #[default]
    Auto,
    /// Pin the NVIDIA ICD for every Vulkan compute launch.
    NvidiaOnly,
    /// Leave ICD selection to the system loader. On a hybrid
    /// machine the evaluator overrides this for compute (the
    /// hybrid-safety pin) and records the override.
    SystemDefault,
}

impl ComputeIcdPin {
    /// The persisted token for this value.
    pub fn token(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::NvidiaOnly => "nvidia-only",
            Self::SystemDefault => "system-default",
        }
    }

    /// Parses a persisted token.
    pub fn from_token(token: &str) -> Result<Self, SettingsError> {
        match token {
            "auto" => Ok(Self::Auto),
            "nvidia-only" => Ok(Self::NvidiaOnly),
            "system-default" => Ok(Self::SystemDefault),
            other => Err(SettingsError::UnknownToken {
                field: "compute_icd_pin",
                value: bounded_text(other),
            }),
        }
    }
}

/// The user's acceleration preferences, validated.
///
/// Construct through [`AccelSettings::estate_defaults`] or
/// [`AccelSettings::from_record`]; a hand-built value must pass
/// [`AccelSettings::validate`] before it is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccelSettings {
    /// Master switch. Off means software rendering and no GPU
    /// compute, whatever the other controls say.
    pub master_enabled: bool,
    /// Render backend preference.
    pub render_backend: RenderBackendPref,
    /// Adapter power preference.
    pub power_preference: PowerPref,
    /// GPU compute (gated offload workloads) on/off.
    pub gpu_compute_enabled: bool,
    /// Vulkan compute device pin policy.
    pub compute_icd_pin: ComputeIcdPin,
}

impl Default for AccelSettings {
    fn default() -> Self {
        Self::estate_defaults()
    }
}

impl AccelSettings {
    /// The estate defaults: acceleration on, backend Auto, GPU
    /// compute off (no offload has passed the determinism gate
    /// yet, and compute is where the hybrid hazard lives).
    pub fn estate_defaults() -> Self {
        Self {
            master_enabled: true,
            render_backend: RenderBackendPref::Auto,
            power_preference: PowerPref::Auto,
            gpu_compute_enabled: false,
            compute_icd_pin: ComputeIcdPin::Auto,
        }
    }

    /// Validates the combination. Contradictions refuse; inert
    /// combinations (a pin policy chosen while compute is off)
    /// are allowed: the policy is a preference, recorded
    /// verbatim, and takes effect when compute is enabled.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.gpu_compute_enabled && !self.master_enabled {
            return Err(SettingsError::ComputeWithoutMaster);
        }
        if self.gpu_compute_enabled && self.render_backend == RenderBackendPref::Software {
            return Err(SettingsError::ComputeWithSoftwareBackend);
        }
        Ok(())
    }

    /// The persisted record shape for this value.
    pub fn to_record(&self) -> AccelSettingsRecord {
        AccelSettingsRecord {
            schema: SETTINGS_SCHEMA_ID.to_owned(),
            master_enabled: self.master_enabled,
            render_backend: self.render_backend.token().to_owned(),
            power_preference: self.power_preference.token().to_owned(),
            gpu_compute_enabled: self.gpu_compute_enabled,
            compute_icd_pin: self.compute_icd_pin.token().to_owned(),
        }
    }

    /// Loads settings from a persisted record: strict schema
    /// check, strict token parsing, then combination
    /// validation. A record can never smuggle in a state the
    /// typed model refuses.
    pub fn from_record(record: &AccelSettingsRecord) -> Result<Self, SettingsError> {
        if record.schema != SETTINGS_SCHEMA_ID {
            return Err(SettingsError::SchemaMismatch {
                found: bounded_text(&record.schema),
            });
        }
        let settings = Self {
            master_enabled: record.master_enabled,
            render_backend: RenderBackendPref::from_token(&record.render_backend)?,
            power_preference: PowerPref::from_token(&record.power_preference)?,
            gpu_compute_enabled: record.gpu_compute_enabled,
            compute_icd_pin: ComputeIcdPin::from_token(&record.compute_icd_pin)?,
        };
        settings.validate()?;
        Ok(settings)
    }
}

/// The persisted shape of [`AccelSettings`]: plain data, stored
/// as the `accel` entry of the authored save schema's globals
/// partition (design section 1.2). Enum values are the closed
/// token sets of the typed model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccelSettingsRecord {
    /// Schema id; must equal [`SETTINGS_SCHEMA_ID`].
    pub schema: String,
    /// Master switch.
    pub master_enabled: bool,
    /// [`RenderBackendPref`] token.
    pub render_backend: String,
    /// [`PowerPref`] token.
    pub power_preference: String,
    /// GPU compute on/off.
    pub gpu_compute_enabled: bool,
    /// [`ComputeIcdPin`] token.
    pub compute_icd_pin: String,
}

/// How a settings change takes effect (design section 1.2).
/// The wgpu instance and adapter are chosen once at startup,
/// so backend-class changes honestly require a restart;
/// compute-class changes apply at the next compute launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeClass {
    /// Nothing changed.
    NoChange,
    /// Applies without a restart.
    Live,
    /// Applies at the next launch; the UI must say so.
    RestartRequired,
}

/// Classifies the change from `old` to `new`.
pub fn classify_change(old: &AccelSettings, new: &AccelSettings) -> ChangeClass {
    if old == new {
        return ChangeClass::NoChange;
    }
    if old.master_enabled != new.master_enabled
        || old.render_backend != new.render_backend
        || old.power_preference != new.power_preference
    {
        return ChangeClass::RestartRequired;
    }
    ChangeClass::Live
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid() {
        assert_eq!(AccelSettings::estate_defaults().validate(), Ok(()));
    }

    #[test]
    fn tokens_round_trip() {
        for value in [
            RenderBackendPref::Auto,
            RenderBackendPref::Vulkan,
            RenderBackendPref::OpenGl,
            RenderBackendPref::Software,
        ] {
            assert_eq!(RenderBackendPref::from_token(value.token()), Ok(value));
        }
        for value in [
            PowerPref::Auto,
            PowerPref::LowPower,
            PowerPref::HighPerformance,
        ] {
            assert_eq!(PowerPref::from_token(value.token()), Ok(value));
        }
        for value in [
            ComputeIcdPin::Auto,
            ComputeIcdPin::NvidiaOnly,
            ComputeIcdPin::SystemDefault,
        ] {
            assert_eq!(ComputeIcdPin::from_token(value.token()), Ok(value));
        }
    }
}
