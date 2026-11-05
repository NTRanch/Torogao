//! Validation and adversarial tests for the shared workload.

use serde_json::json;
use torogao_tui_bakeoff_core::a2a::{signed_agent_card, verify_card_document};
use torogao_tui_bakeoff_core::policy::{
    Audience, FlowEvent, FlowLabel, GateDecision, Trust, allowed_event, decide, violating_event,
};
use torogao_tui_bakeoff_core::pqc::{establish_session, validate_confirmation_bytes};
use torogao_tui_bakeoff_core::scenario::unicode_probe_line;

#[test]
fn ml_kem_handshake_establishes_and_rejects_tamper() {
    let outcome = establish_session().expect("ML-KEM handshake runs");
    assert!(outcome.valid_established);
    assert!(outcome.tamper_rejected);
    assert_eq!(outcome.ciphertext_bytes, 1088);
    assert_eq!(outcome.shared_secret_bytes, 32);
}

#[test]
fn gate_allows_equal_and_more_restrictive_contexts() {
    assert_eq!(decide(allowed_event()), GateDecision::Allowed);
    let raised = FlowEvent {
        index: 3,
        tool: "rig_census",
        data_label: FlowLabel {
            audience: Audience::Agent,
            trust: Trust::Medium,
        },
        context_label: FlowLabel {
            audience: Audience::Operator,
            trust: Trust::High,
        },
    };
    assert_eq!(decide(raised), GateDecision::Allowed);
}

#[test]
fn gate_refuses_a_label_downgrade_with_a_machine_readable_remedy() {
    let GateDecision::Refused { code, remedy } = decide(violating_event()) else {
        panic!("the violating event must be refused");
    };
    assert_eq!(code, "label_violation");
    assert_eq!(remedy["action"], json!("raise_context"));
    assert_eq!(remedy["required_context"]["audience"], json!("operator"));
    assert_eq!(remedy["required_context"]["trust"], json!("high"));
}

#[test]
fn gate_refuses_when_only_trust_is_downgraded() {
    let event = FlowEvent {
        index: 4,
        tool: "rig_census",
        data_label: FlowLabel {
            audience: Audience::Operator,
            trust: Trust::High,
        },
        context_label: FlowLabel {
            audience: Audience::Operator,
            trust: Trust::Medium,
        },
    };
    assert!(matches!(decide(event), GateDecision::Refused { .. }));
}

#[test]
fn signed_agent_card_verifies_and_tampering_fails() {
    let signed = signed_agent_card("http://127.0.0.1:1");
    let (verified, card) = verify_card_document(&signed).expect("card document parses");
    assert!(verified);
    assert_eq!(card["name"], json!("Torogao Bake-off Agent"));

    let mut tampered = signed;
    tampered["name"] = json!("Mallory");
    let (verified, _) = verify_card_document(&tampered).expect("tampered document parses");
    assert!(!verified);
}

#[test]
fn malformed_card_fields_are_typed_errors_not_panics() {
    let malformed = json!({"signature": "zz", "signerPublicKey": "00"});
    assert!(verify_card_document(&malformed).is_err());
    assert!(validate_confirmation_bytes(&[0_u8; 31]).is_err());
    assert!(validate_confirmation_bytes(&[0_u8; 32]).is_ok());
}

#[test]
fn unicode_probe_contains_wide_characters() {
    assert!(
        unicode_probe_line()
            .chars()
            .any(|character| character == '界')
    );
    assert!(
        unicode_probe_line()
            .chars()
            .any(|character| character == '✓')
    );
}
