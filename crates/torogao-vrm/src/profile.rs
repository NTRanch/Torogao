//! `vrm-profile/1`: the per-character permission profile.
//!
//! A profile is the ONLY source of VRM meta in an export. Every
//! permission field the VRM 1.0 meta defines is a required field
//! of the record: there is no default profile, no default field
//! value, and no code path that invents one. The raw record keeps
//! every field optional so the checker can NAME the unset fields
//! in its refusal instead of failing on the first one.
//!
//! Spelling law: RON documents write the Rust variant names
//! (`OnlyAuthor`, `PersonalProfit`, `Prohibited`). The VRM JSON
//! spellings (`onlyAuthor`, ...) exist only at emission, via
//! each enum's `as_str()` — serde renames are deliberately
//! absent, because one Deserialize impl serves the RON surface
//! and a rename would silently change the authoring format.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::error::{VrmError, VrmResult};

/// The profile format token, carried in the record's `id` field.
pub const PROFILE_ID: &str = "vrm-profile/1";

/// Largest profile document this crate will parse, in bytes.
pub const PROFILE_BYTES_MAX: usize = 65_536;

/// The RON reader options for every record format in this
/// crate. `IMPLICIT_SOME` is the authoring contract: profile
/// and spring documents are hand-written per character, so a
/// present field is written bare (`name: "Vivienne"`) and an
/// absent-optional field is written `None`; requiring a
/// `Some(...)` wrapper on every line would bury the permission
/// decisions the documents exist to state. `Some(...)` remains
/// accepted for fields an author chooses to wrap.
pub fn ron_options() -> ron::Options {
    ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
}

/// VRM `avatarPermission` values.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum AvatarPermission {
    /// Only the author may perform as this avatar.
    OnlyAuthor,
    /// Separately licensed persons may perform as this avatar.
    OnlySeparatelyLicensedPerson,
    /// Anyone may perform as this avatar.
    Everyone,
}

impl AvatarPermission {
    /// The exact VRM meta string.
    pub fn as_str(&self) -> &'static str {
        match self {
            AvatarPermission::OnlyAuthor => "onlyAuthor",
            AvatarPermission::OnlySeparatelyLicensedPerson => "onlySeparatelyLicensedPerson",
            AvatarPermission::Everyone => "everyone",
        }
    }
}

/// VRM `commercialUsage` values.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum CommercialUsage {
    /// Personal, non-profit use only.
    PersonalNonProfit,
    /// Personal profit use permitted.
    PersonalProfit,
    /// Corporate use permitted.
    Corporation,
}

impl CommercialUsage {
    /// The exact VRM meta string.
    pub fn as_str(&self) -> &'static str {
        match self {
            CommercialUsage::PersonalNonProfit => "personalNonProfit",
            CommercialUsage::PersonalProfit => "personalProfit",
            CommercialUsage::Corporation => "corporation",
        }
    }
}

/// VRM `creditNotation` values.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum CreditNotation {
    /// Credit display is required.
    Required,
    /// Credit display is not required.
    Unnecessary,
}

impl CreditNotation {
    /// The exact VRM meta string.
    pub fn as_str(&self) -> &'static str {
        match self {
            CreditNotation::Required => "required",
            CreditNotation::Unnecessary => "unnecessary",
        }
    }
}

/// VRM `modification` values.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
pub enum Modification {
    /// Modification is prohibited.
    Prohibited,
    /// Modification is permitted.
    AllowModification,
    /// Modification and redistribution of derivatives are permitted.
    AllowModificationRedistribution,
}

impl Modification {
    /// The exact VRM meta string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Modification::Prohibited => "prohibited",
            Modification::AllowModification => "allowModification",
            Modification::AllowModificationRedistribution => "allowModificationRedistribution",
        }
    }
}

/// The raw profile record: every field optional, so a partially
/// written profile parses and the checker can name what is unset.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct RawProfile {
    /// Antisocial/hate usage flag (required).
    pub allow_antisocial_or_hate_usage: Option<bool>,
    /// Excessively sexual usage flag (required).
    pub allow_excessively_sexual_usage: Option<bool>,
    /// Excessively violent usage flag (required).
    pub allow_excessively_violent_usage: Option<bool>,
    /// Political/religious usage flag (required).
    pub allow_political_or_religious_usage: Option<bool>,
    /// Redistribution flag (required).
    pub allow_redistribution: Option<bool>,
    /// Author names (required).
    pub authors: Option<Vec<String>>,
    /// Avatar permission enum (required).
    pub avatar_permission: Option<AvatarPermission>,
    /// Commercial usage enum (required).
    pub commercial_usage: Option<CommercialUsage>,
    /// Contact information (optional meta text).
    pub contact_information: Option<String>,
    /// Copyright information (optional meta text).
    pub copyright_information: Option<String>,
    /// Credit notation enum (required).
    pub credit_notation: Option<CreditNotation>,
    /// Format token; must equal [`PROFILE_ID`].
    pub id: Option<String>,
    /// License URL (required: the VRM spec requires it in meta).
    pub license_url: Option<String>,
    /// Modification enum (required).
    pub modification: Option<Modification>,
    /// Model name (required).
    pub name: Option<String>,
    /// Other license URL (optional meta text).
    pub other_license_url: Option<String>,
    /// References / original works (optional meta list).
    pub references: Option<Vec<String>>,
    /// Third-party licenses (optional meta text).
    pub third_party_licenses: Option<String>,
    /// Model version (required).
    pub version: Option<String>,
}

/// A checked profile: every required field present and non-empty.
/// The only profile shape the exporter accepts.
#[derive(Clone, Debug, PartialEq)]
pub struct VrmProfile {
    /// Antisocial/hate usage flag.
    pub allow_antisocial_or_hate_usage: bool,
    /// Excessively sexual usage flag.
    pub allow_excessively_sexual_usage: bool,
    /// Excessively violent usage flag.
    pub allow_excessively_violent_usage: bool,
    /// Political/religious usage flag.
    pub allow_political_or_religious_usage: bool,
    /// Redistribution flag.
    pub allow_redistribution: bool,
    /// Author names, at least one, none empty.
    pub authors: Vec<String>,
    /// Avatar permission.
    pub avatar_permission: AvatarPermission,
    /// Commercial usage.
    pub commercial_usage: CommercialUsage,
    /// Contact information, when the profile carries it.
    pub contact_information: Option<String>,
    /// Copyright information, when the profile carries it.
    pub copyright_information: Option<String>,
    /// Credit notation.
    pub credit_notation: CreditNotation,
    /// License URL.
    pub license_url: String,
    /// Modification rule.
    pub modification: Modification,
    /// Model name.
    pub name: String,
    /// Other license URL, when the profile carries it.
    pub other_license_url: Option<String>,
    /// References, when the profile carries them.
    pub references: Option<Vec<String>>,
    /// Third-party licenses, when the profile carries them.
    pub third_party_licenses: Option<String>,
    /// Model version.
    pub version: String,
}

impl RawProfile {
    /// Parse a profile document (RON). Bounded by
    /// [`PROFILE_BYTES_MAX`].
    pub fn parse(text: &str) -> VrmResult<RawProfile> {
        if text.len() > PROFILE_BYTES_MAX {
            return Err(VrmError::ProfileParse {
                detail: format!(
                    "document is {} bytes, over the {PROFILE_BYTES_MAX}-byte bound",
                    text.len()
                ),
            });
        }
        ron_options()
            .from_str(text)
            .map_err(|err| VrmError::ProfileParse {
                detail: err.to_string(),
            })
    }

    /// Check the record: wrong id first, then the full unset list,
    /// then emptiness. On success the profile is export-ready.
    pub fn check(self) -> VrmResult<VrmProfile> {
        if let Some(id) = &self.id {
            if id != PROFILE_ID {
                return Err(VrmError::ProfileWrongId { found: id.clone() });
            }
        }
        let mut unset: Vec<&'static str> = Vec::new();
        if self.allow_antisocial_or_hate_usage.is_none() {
            unset.push("allow_antisocial_or_hate_usage");
        }
        if self.allow_excessively_sexual_usage.is_none() {
            unset.push("allow_excessively_sexual_usage");
        }
        if self.allow_excessively_violent_usage.is_none() {
            unset.push("allow_excessively_violent_usage");
        }
        if self.allow_political_or_religious_usage.is_none() {
            unset.push("allow_political_or_religious_usage");
        }
        if self.allow_redistribution.is_none() {
            unset.push("allow_redistribution");
        }
        if self.authors.is_none() {
            unset.push("authors");
        }
        if self.avatar_permission.is_none() {
            unset.push("avatar_permission");
        }
        if self.commercial_usage.is_none() {
            unset.push("commercial_usage");
        }
        if self.credit_notation.is_none() {
            unset.push("credit_notation");
        }
        if self.id.is_none() {
            unset.push("id");
        }
        if self.license_url.is_none() {
            unset.push("license_url");
        }
        if self.modification.is_none() {
            unset.push("modification");
        }
        if self.name.is_none() {
            unset.push("name");
        }
        if self.version.is_none() {
            unset.push("version");
        }
        if !unset.is_empty() {
            unset.sort_unstable();
            return Err(VrmError::ProfileUnset {
                fields: unset.iter().map(ToString::to_string).collect(),
            });
        }
        let authors = self.authors.expect("authors checked present");
        if authors.is_empty() || authors.iter().any(|author| author.trim().is_empty()) {
            return Err(VrmError::ProfileEmpty {
                field: "authors".to_string(),
            });
        }
        let name = self.name.expect("name checked present");
        if name.trim().is_empty() {
            return Err(VrmError::ProfileEmpty {
                field: "name".to_string(),
            });
        }
        let license_url = self.license_url.expect("license_url checked present");
        if license_url.trim().is_empty() {
            return Err(VrmError::ProfileEmpty {
                field: "license_url".to_string(),
            });
        }
        Ok(VrmProfile {
            allow_antisocial_or_hate_usage: self
                .allow_antisocial_or_hate_usage
                .expect("flag checked present"),
            allow_excessively_sexual_usage: self
                .allow_excessively_sexual_usage
                .expect("flag checked present"),
            allow_excessively_violent_usage: self
                .allow_excessively_violent_usage
                .expect("flag checked present"),
            allow_political_or_religious_usage: self
                .allow_political_or_religious_usage
                .expect("flag checked present"),
            allow_redistribution: self.allow_redistribution.expect("flag checked present"),
            authors,
            avatar_permission: self.avatar_permission.expect("enum checked present"),
            commercial_usage: self.commercial_usage.expect("enum checked present"),
            contact_information: self.contact_information,
            copyright_information: self.copyright_information,
            credit_notation: self.credit_notation.expect("enum checked present"),
            license_url,
            modification: self.modification.expect("enum checked present"),
            name,
            other_license_url: self.other_license_url,
            references: self.references,
            third_party_licenses: self.third_party_licenses,
            version: self.version.expect("version checked present"),
        })
    }
}

impl VrmProfile {
    /// Parse and check a profile document in one step.
    pub fn load(text: &str) -> VrmResult<VrmProfile> {
        RawProfile::parse(text)?.check()
    }

    /// The VRM 1.0 meta block this profile writes, in schema
    /// order. Optional text fields appear only when the profile
    /// carries them; every permission field always appears.
    /// `thumbnailImage` is not expressible in vrm-profile/1 and is
    /// never emitted.
    pub fn meta_json(&self) -> Value {
        let mut meta = Map::new();
        meta.insert("name".to_string(), json!(self.name));
        meta.insert("version".to_string(), json!(self.version));
        meta.insert("authors".to_string(), json!(self.authors));
        if let Some(text) = &self.copyright_information {
            meta.insert("copyrightInformation".to_string(), json!(text));
        }
        if let Some(text) = &self.contact_information {
            meta.insert("contactInformation".to_string(), json!(text));
        }
        if let Some(references) = &self.references {
            meta.insert("references".to_string(), json!(references));
        }
        if let Some(text) = &self.third_party_licenses {
            meta.insert("thirdPartyLicenses".to_string(), json!(text));
        }
        meta.insert("licenseUrl".to_string(), json!(self.license_url));
        meta.insert(
            "avatarPermission".to_string(),
            json!(self.avatar_permission.as_str()),
        );
        meta.insert(
            "allowExcessivelyViolentUsage".to_string(),
            json!(self.allow_excessively_violent_usage),
        );
        meta.insert(
            "allowExcessivelySexualUsage".to_string(),
            json!(self.allow_excessively_sexual_usage),
        );
        meta.insert(
            "commercialUsage".to_string(),
            json!(self.commercial_usage.as_str()),
        );
        meta.insert(
            "allowPoliticalOrReligiousUsage".to_string(),
            json!(self.allow_political_or_religious_usage),
        );
        meta.insert(
            "allowAntisocialOrHateUsage".to_string(),
            json!(self.allow_antisocial_or_hate_usage),
        );
        meta.insert(
            "creditNotation".to_string(),
            json!(self.credit_notation.as_str()),
        );
        meta.insert(
            "allowRedistribution".to_string(),
            json!(self.allow_redistribution),
        );
        meta.insert(
            "modification".to_string(),
            json!(self.modification.as_str()),
        );
        if let Some(text) = &self.other_license_url {
            meta.insert("otherLicenseUrl".to_string(), json!(text));
        }
        Value::Object(meta)
    }
}

/// Compare a file's meta block against a profile, field by field.
/// The first disagreement is a [`VrmError::MetaMismatch`] naming
/// the field; meta fields the profile does not carry (beyond
/// `extensions`/`extras`) are also mismatches.
pub fn compare_meta(meta: &Value, profile: &VrmProfile) -> VrmResult<()> {
    let expected = profile.meta_json();
    let expected_map = expected.as_object().expect("meta_json builds an object");
    let found_map = meta.as_object().ok_or_else(|| VrmError::MetaMismatch {
        detail: "meta is not an object".to_string(),
        expected: "an object".to_string(),
        field: "meta".to_string(),
    })?;
    for (field, expected_value) in expected_map {
        match found_map.get(field) {
            Some(found_value) if found_value == expected_value => {}
            Some(found_value) => {
                return Err(VrmError::MetaMismatch {
                    detail: format!("file carries {found_value}"),
                    expected: expected_value.to_string(),
                    field: field.clone(),
                });
            }
            None => {
                return Err(VrmError::MetaMismatch {
                    detail: "field absent from file".to_string(),
                    expected: expected_value.to_string(),
                    field: field.clone(),
                });
            }
        }
    }
    for field in found_map.keys() {
        if !expected_map.contains_key(field) && field != "extensions" && field != "extras" {
            return Err(VrmError::MetaMismatch {
                detail: "field present in file but not in profile".to_string(),
                expected: "absent".to_string(),
                field: field.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{RawProfile, VrmProfile};
    use crate::error::VrmError;

    /// A complete profile document, as the data file carries it.
    const COMPLETE: &str = r#"(
        id: "vrm-profile/1",
        name: "Fixture",
        version: "1.0.0",
        authors: ["Fixture Studio"],
        license_url: "https://vrm.dev/licenses/1.0/",
        avatar_permission: OnlyAuthor,
        allow_excessively_violent_usage: false,
        allow_excessively_sexual_usage: false,
        commercial_usage: PersonalNonProfit,
        allow_political_or_religious_usage: false,
        allow_antisocial_or_hate_usage: false,
        credit_notation: Required,
        allow_redistribution: false,
        modification: Prohibited,
    )"#;

    #[test]
    fn complete_profile_checks_and_writes_meta() {
        let profile = VrmProfile::load(COMPLETE).expect("complete profile must check");
        let meta = profile.meta_json();
        assert_eq!(meta["avatarPermission"], "onlyAuthor");
        assert_eq!(meta["modification"], "prohibited");
        assert_eq!(meta["authors"], serde_json::json!(["Fixture Studio"]));
    }

    #[test]
    fn unset_fields_are_all_named() {
        let raw = RawProfile::parse("(name: \"Half\")").expect("parses");
        let err = raw.check().expect_err("must refuse");
        match err {
            VrmError::ProfileUnset { fields } => {
                assert!(fields.contains(&"modification".to_string()));
                assert!(fields.contains(&"avatar_permission".to_string()));
                assert!(fields.contains(&"id".to_string()));
                assert!(!fields.contains(&"name".to_string()));
            }
            other => panic!("wrong error: {other}"),
        }
    }

    #[test]
    fn wrong_id_is_refused_by_name() {
        let text = COMPLETE.replace("vrm-profile/1", "vrm-profile/9");
        let err = VrmProfile::load(&text).expect_err("must refuse");
        assert!(matches!(err, VrmError::ProfileWrongId { .. }));
    }

    #[test]
    fn empty_authors_are_refused() {
        let text = COMPLETE.replace("[\"Fixture Studio\"]", "[]");
        let err = VrmProfile::load(&text).expect_err("must refuse");
        assert!(matches!(err, VrmError::ProfileEmpty { .. }));
    }

    #[test]
    fn malformed_document_is_a_parse_error() {
        let err = VrmProfile::load("(name: ").expect_err("must fail");
        assert!(matches!(err, VrmError::ProfileParse { .. }));
    }
}
