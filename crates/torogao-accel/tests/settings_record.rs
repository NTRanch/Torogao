//! Settings model tests: validation refusals, the persisted
//! record shape, and change classification (design section 1).

use torogao_accel::{
    AccelSettings, AccelSettingsRecord, ChangeClass, ComputeIcdPin, PowerPref, RenderBackendPref,
    SettingsError, classify_change,
};

#[test]
fn defaults_record_json_shape_is_pinned() {
    let record = AccelSettings::estate_defaults().to_record();
    let json = serde_json::to_string(&record).expect("record serializes");
    assert_eq!(
        json,
        concat!(
            "{\"schema\":\"torogao-accel-settings/1\",",
            "\"master_enabled\":true,",
            "\"render_backend\":\"auto\",",
            "\"power_preference\":\"auto\",",
            "\"gpu_compute_enabled\":false,",
            "\"compute_icd_pin\":\"auto\"}"
        )
    );
}

#[test]
fn record_round_trips_through_json() {
    let settings = AccelSettings {
        master_enabled: true,
        render_backend: RenderBackendPref::Vulkan,
        power_preference: PowerPref::LowPower,
        gpu_compute_enabled: true,
        compute_icd_pin: ComputeIcdPin::NvidiaOnly,
    };
    let json = serde_json::to_string(&settings.to_record()).expect("serializes");
    let record: AccelSettingsRecord = serde_json::from_str(&json).expect("record parses");
    assert_eq!(AccelSettings::from_record(&record), Ok(settings));
}

#[test]
fn compute_without_master_is_refused() {
    let settings = AccelSettings {
        master_enabled: false,
        gpu_compute_enabled: true,
        ..AccelSettings::estate_defaults()
    };
    let err = settings.validate().expect_err("must refuse");
    assert_eq!(err.code(), "TG-ACCEL-SET-COMPUTE-WITHOUT-MASTER");
}

#[test]
fn compute_with_software_backend_is_refused() {
    let settings = AccelSettings {
        render_backend: RenderBackendPref::Software,
        gpu_compute_enabled: true,
        ..AccelSettings::estate_defaults()
    };
    let err = settings.validate().expect_err("must refuse");
    assert_eq!(err.code(), "TG-ACCEL-SET-COMPUTE-SOFTWARE");
}

#[test]
fn persisted_contradiction_is_refused_on_load() {
    // A hand-edited record cannot smuggle in a state the typed
    // model refuses.
    let mut record = AccelSettings::estate_defaults().to_record();
    record.master_enabled = false;
    record.gpu_compute_enabled = true;
    let err = AccelSettings::from_record(&record).expect_err("must refuse");
    assert_eq!(err, SettingsError::ComputeWithoutMaster);
}

#[test]
fn unknown_tokens_are_refused_with_the_field_named() {
    let mut record = AccelSettings::estate_defaults().to_record();
    record.render_backend = "metal".to_owned();
    let err = AccelSettings::from_record(&record).expect_err("must refuse");
    assert_eq!(err.code(), "TG-ACCEL-SET-UNKNOWN-TOKEN");
    match err {
        SettingsError::UnknownToken { field, value } => {
            assert_eq!(field, "render_backend");
            assert_eq!(value, "metal");
        }
        other => panic!("wrong refusal: {other:?}"),
    }

    let mut record = AccelSettings::estate_defaults().to_record();
    record.compute_icd_pin = "intel".to_owned();
    let err = AccelSettings::from_record(&record).expect_err("must refuse");
    assert!(matches!(
        err,
        SettingsError::UnknownToken {
            field: "compute_icd_pin",
            ..
        }
    ));
}

#[test]
fn unknown_schema_is_refused() {
    let mut record = AccelSettings::estate_defaults().to_record();
    record.schema = "torogao-accel-settings/99".to_owned();
    let err = AccelSettings::from_record(&record).expect_err("must refuse");
    assert_eq!(err.code(), "TG-ACCEL-SET-SCHEMA");
}

#[test]
fn record_json_missing_a_field_does_not_parse() {
    let json = "{\"schema\":\"torogao-accel-settings/1\",\"master_enabled\":true}";
    assert!(serde_json::from_str::<AccelSettingsRecord>(json).is_err());
}

#[test]
fn inert_pin_with_compute_off_is_allowed() {
    let settings = AccelSettings {
        gpu_compute_enabled: false,
        compute_icd_pin: ComputeIcdPin::NvidiaOnly,
        ..AccelSettings::estate_defaults()
    };
    assert_eq!(settings.validate(), Ok(()));
}

#[test]
fn change_classification_matrix() {
    let base = AccelSettings::estate_defaults();
    assert_eq!(classify_change(&base, &base), ChangeClass::NoChange);

    let backend = AccelSettings {
        render_backend: RenderBackendPref::OpenGl,
        ..base
    };
    assert_eq!(
        classify_change(&base, &backend),
        ChangeClass::RestartRequired
    );

    let master = AccelSettings {
        master_enabled: false,
        ..base
    };
    assert_eq!(
        classify_change(&base, &master),
        ChangeClass::RestartRequired
    );

    let power = AccelSettings {
        power_preference: PowerPref::HighPerformance,
        ..base
    };
    assert_eq!(classify_change(&base, &power), ChangeClass::RestartRequired);

    let compute = AccelSettings {
        gpu_compute_enabled: true,
        ..base
    };
    assert_eq!(classify_change(&base, &compute), ChangeClass::Live);

    let pin = AccelSettings {
        compute_icd_pin: ComputeIcdPin::SystemDefault,
        ..base
    };
    assert_eq!(classify_change(&base, &pin), ChangeClass::Live);
}
