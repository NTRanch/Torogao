//! A deterministic OpenAPPA-style flow gate for the bake-off.
//!
//! Labels are compared component-by-component. A handoff may keep
//! or increase restrictiveness, but may never lower either the
//! audience clearance or the trust clearance. The decision is a
//! pure function of one bounded event record: no model, clock, or
//! ambient state participates.

use serde_json::{Value, json};

/// The audience named by a flow label, ordered by clearance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Audience {
    /// External recipients have the lowest clearance.
    External,
    /// Another agent has intermediate clearance.
    Agent,
    /// The local operator has the highest clearance.
    Operator,
}

impl Audience {
    /// Stable wire name.
    pub fn name(self) -> &'static str {
        match self {
            Self::External => "external",
            Self::Agent => "agent",
            Self::Operator => "operator",
        }
    }

    fn clearance(self) -> u8 {
        match self {
            Self::External => 1,
            Self::Agent => 2,
            Self::Operator => 3,
        }
    }
}

/// The trust named by a flow label, ordered by clearance.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Trust {
    /// Low trust has the lowest clearance.
    Low,
    /// Medium trust has intermediate clearance.
    Medium,
    /// High trust has the highest clearance.
    High,
}

impl Trust {
    /// Stable wire name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn clearance(self) -> u8 {
        match self {
            Self::Low => 1,
            Self::Medium => 2,
            Self::High => 3,
        }
    }
}

/// One audience-by-trust flow label.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlowLabel {
    /// The audience clearance.
    pub audience: Audience,
    /// The trust clearance.
    pub trust: Trust,
}

impl FlowLabel {
    /// Whether this context can receive data carrying `data`
    /// without lowering either component of the data label.
    pub fn can_receive(self, data: FlowLabel) -> bool {
        self.audience.clearance() >= data.audience.clearance()
            && self.trust.clearance() >= data.trust.clearance()
    }

    /// The label as a JSON object.
    pub fn to_json(self) -> Value {
        json!({"audience": self.audience.name(), "trust": self.trust.name()})
    }
}

/// One bounded gate event, in the order it was presented.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlowEvent {
    /// Monotonic index within the scenario's event record.
    pub index: usize,
    /// The requested tool name.
    pub tool: &'static str,
    /// The label carried by the data.
    pub data_label: FlowLabel,
    /// The label of the proposed receiving context.
    pub context_label: FlowLabel,
}

/// The gate's decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateDecision {
    /// The call may proceed to the MCP client.
    Allowed,
    /// The call is refused before dispatch, with a remedy record.
    Refused {
        /// Stable machine-readable refusal code.
        code: &'static str,
        /// Machine-readable remedy plan.
        remedy: Value,
    },
}

/// Decides one flow event deterministically from the event alone.
pub fn decide(event: FlowEvent) -> GateDecision {
    if event.context_label.can_receive(event.data_label) {
        return GateDecision::Allowed;
    }
    GateDecision::Refused {
        code: "label_violation",
        remedy: json!({
            "action": "raise_context",
            "code": "label_violation",
            "event_index": event.index,
            "required_context": event.data_label.to_json(),
            "result": "refused",
            "tool": event.tool,
        }),
    }
}

/// The scenario's allowed rig-census flow.
pub fn allowed_event() -> FlowEvent {
    let label = FlowLabel {
        audience: Audience::Operator,
        trust: Trust::High,
    };
    FlowEvent {
        index: 1,
        tool: "rig_census",
        data_label: label,
        context_label: label,
    }
}

/// The scenario's label-violating rig-census flow.
pub fn violating_event() -> FlowEvent {
    FlowEvent {
        index: 2,
        tool: "rig_census",
        data_label: FlowLabel {
            audience: Audience::Operator,
            trust: Trust::High,
        },
        context_label: FlowLabel {
            audience: Audience::External,
            trust: Trust::Low,
        },
    }
}
