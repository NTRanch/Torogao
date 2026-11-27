//! The animated helpers: a deterministic state machine in core.
//!
//! The Torogao girls appear in the app as helpers who guide
//! navigation and help run its processes. Everything about what
//! a helper is doing, saying, and proposing lives here, in core,
//! exactly once; the shells render the state (an animation strip
//! on Android, a still on desktop) and forward user intent as
//! events. The machine is deterministic: same state + same event
//! = same next state, on every shell.
//!
//! Duty assignment (against the cast records in [`crate::cast`]):
//! - **Prima — seat setup.** The pilot and headmistress figure
//!   of the cast; the flagship first-run flow (seat setup) is
//!   hers, start to finish.
//! - **Lana — generation.** The wry, energetic one runs the
//!   queue: composing a still, watching the seat, reporting back.
//! - **Sirena — gallery.** The hostess keeps the collection:
//!   viewing, exporting, tidying.
//!
//! Standing rules the machine itself enforces:
//! - Helpers PROPOSE; the user CONFIRMS anything consequential.
//!   A proposal is data ([`ProposedAction`]); the shell executes
//!   it only on the user's confirm, through the session.
//! - The age gate and the content screen are never bypassable
//!   through a helper flow: the generation flow refuses to begin
//!   (or continue) unattested, and actually generating still
//!   goes through the queue's own gate and screen.

use crate::cast::{Character, LANA, PRIMA, SIRENA};

/// A major area of the app a helper is responsible for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperArea {
    /// Seat configuration and first-run setup (Prima).
    SeatSetup,
    /// Composing and running generations (Lana).
    Generation,
    /// Browsing and managing the gallery (Sirena).
    Gallery,
}

/// The cast member responsible for an area.
pub fn helper_for(area: HelperArea) -> &'static Character {
    match area {
        HelperArea::SeatSetup => &PRIMA,
        HelperArea::Generation => &LANA,
        HelperArea::Gallery => &SIRENA,
    }
}

/// A whole-body pose key the shell renders (a strip frame set on
/// Android, a still on desktop). Expression is whole-body: each
/// pose is a posture, not a face swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    /// Standing welcome, open posture.
    Greet,
    /// Leaning in, explaining — weight forward, hands open.
    Explain,
    /// Pointing at the control the user needs next.
    Point,
    /// Head tilted, thinking / waiting on the seat.
    Think,
    /// Busy posture while a job runs.
    Work,
    /// Arms up, celebrating a finished job.
    Cheer,
    /// Softer, closer posture for errors and refusals.
    Comfort,
    /// Neutral resting idle.
    Idle,
}

/// A guided flow a helper can walk the user through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowId {
    /// First-run seat setup: propose the default seat, test it,
    /// save it. Prima's flagship flow.
    SeatSetup,
    /// Compose and run a first still, end to end. Lana's flow;
    /// the same path as the phone-cues-primo trial milestone.
    FirstGeneration,
}

/// A consequential action the helper proposes. The shell shows
/// it; nothing runs until the user confirms, and confirming
/// calls the session — never a helper-internal shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposedAction {
    /// Fill in the default seat address (the Owner's primo over
    /// the tailnet).
    UseDefaultSeat,
    /// Run the seat reachability test.
    TestSeatConnection,
    /// Save the tested seat and finish setup.
    SaveSeatAndFinish,
    /// Queue the composed still on the seat.
    RunGeneration,
    /// Open the gallery to see the finished still.
    OpenGallery,
}

/// The helper's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperState {
    /// Resting in her area.
    Idle,
    /// Walking a flow; `step` indexes the flow's steps.
    Guiding {
        /// The flow being walked.
        flow: FlowId,
        /// Current step within the flow.
        step: u32,
    },
    /// A job (or the seat test) is running; waiting on the seat.
    /// The flow is remembered so the reply lands in the right
    /// flow's next step.
    WaitingOnSeat {
        /// The flow that is waiting.
        flow: FlowId,
    },
    /// The flow or job just succeeded.
    Succeeded,
    /// Something failed or was refused; the helper is in her
    /// comfort posture with an explanation.
    Comforting,
}

/// Events the shells feed the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelperEvent {
    /// The user entered an area; its helper takes over.
    EnterArea(HelperArea),
    /// The user asked to start a guided flow.
    BeginFlow(FlowId),
    /// The user confirmed the proposed action for this step.
    ConfirmStep,
    /// The user went back a step.
    BackStep,
    /// The user cancelled the flow or the running job.
    Cancel,
    /// The seat test came back reachable.
    SeatTestPassed,
    /// The seat test failed, or the seat was lost mid-flow.
    SeatLost,
    /// A queued job started running.
    JobStarted,
    /// A queued job finished with a filed still.
    JobSucceeded,
    /// A queued job failed.
    JobFailed,
    /// The age gate refused mid-flow (attestation missing).
    AttestationMissing,
    /// The user dismissed the helper's success/comfort beat.
    Dismiss,
}

/// The gate facts a flow's transitions depend on, supplied by
/// the shell from the session at event time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateContext {
    /// Whether the 18+ attestation stands.
    pub attested: bool,
}

/// One helper: the state machine for whichever girl is on duty.
#[derive(Debug, Clone)]
pub struct Helper {
    area: HelperArea,
    state: HelperState,
}

impl Default for Helper {
    fn default() -> Self {
        Self::new()
    }
}

impl Helper {
    /// A helper at rest in the seat-setup area (Prima greets
    /// first runs).
    pub fn new() -> Self {
        Self {
            area: HelperArea::SeatSetup,
            state: HelperState::Idle,
        }
    }

    /// The area the helper is currently responsible for.
    pub fn area(&self) -> HelperArea {
        self.area
    }

    /// The cast member on duty.
    pub fn character(&self) -> &'static Character {
        helper_for(self.area)
    }

    /// The current state.
    pub fn state(&self) -> &HelperState {
        &self.state
    }

    /// Feed one event. Deterministic; invalid events for the
    /// current state are no-ops, except the safety ones: seat
    /// loss and attestation loss always land in `Comforting`,
    /// from any state.
    pub fn on_event(&mut self, event: HelperEvent, gate: &GateContext) {
        use HelperEvent as E;
        use HelperState as S;
        match event {
            E::EnterArea(area) => {
                self.area = area;
                self.state = S::Idle;
            }
            E::SeatLost | E::JobFailed => {
                self.state = S::Comforting;
            }
            E::AttestationMissing => {
                self.state = S::Comforting;
            }
            E::Cancel => {
                self.state = S::Idle;
            }
            E::Dismiss => {
                if matches!(self.state, S::Succeeded | S::Comforting) {
                    self.state = S::Idle;
                }
            }
            E::BeginFlow(flow) => {
                if flow == FlowId::FirstGeneration && !gate.attested {
                    // The generation flow never begins unattested;
                    // the helper says why instead.
                    self.area = HelperArea::Generation;
                    self.state = S::Comforting;
                    return;
                }
                self.area = area_of(flow);
                self.state = S::Guiding { flow, step: 0 };
            }
            E::ConfirmStep => {
                if let S::Guiding { flow, step } = self.state {
                    self.state = advance(flow, step);
                }
            }
            E::BackStep => {
                if let S::Guiding { flow, step } = self.state {
                    self.state = S::Guiding {
                        flow,
                        step: step.saturating_sub(1),
                    };
                }
            }
            E::SeatTestPassed => {
                if self.state
                    == (S::WaitingOnSeat {
                        flow: FlowId::SeatSetup,
                    })
                {
                    self.state = S::Guiding {
                        flow: FlowId::SeatSetup,
                        step: 2,
                    };
                }
            }
            E::JobStarted => {
                if let S::Guiding { flow, .. } = self.state {
                    self.state = S::WaitingOnSeat { flow };
                }
            }
            E::JobSucceeded => {
                if self.state
                    == (S::WaitingOnSeat {
                        flow: FlowId::FirstGeneration,
                    })
                {
                    self.state = S::Succeeded;
                }
            }
        }
    }

    /// The action the helper proposes in the current state, if
    /// any. Confirming it is the user's act.
    pub fn proposed_action(&self) -> Option<ProposedAction> {
        match self.state {
            HelperState::Guiding {
                flow: FlowId::SeatSetup,
                step: 0,
            } => Some(ProposedAction::UseDefaultSeat),
            HelperState::Guiding {
                flow: FlowId::SeatSetup,
                step: 1,
            } => Some(ProposedAction::TestSeatConnection),
            HelperState::Guiding {
                flow: FlowId::SeatSetup,
                step: 2,
            } => Some(ProposedAction::SaveSeatAndFinish),
            HelperState::Guiding {
                flow: FlowId::FirstGeneration,
                step: 0,
            } => Some(ProposedAction::RunGeneration),
            HelperState::Succeeded => Some(ProposedAction::OpenGallery),
            _ => None,
        }
    }

    /// The pose the shell should render for the current state.
    pub fn pose(&self) -> Pose {
        match self.state {
            HelperState::Idle => Pose::Idle,
            HelperState::Guiding { step: 0, .. } => Pose::Greet,
            HelperState::Guiding { .. } => Pose::Point,
            HelperState::WaitingOnSeat { .. } => Pose::Work,
            HelperState::Succeeded => Pose::Cheer,
            HelperState::Comforting => Pose::Comfort,
        }
    }

    /// The helper's spoken line for the current state.
    pub fn line(&self) -> &'static str {
        match (self.area, &self.state) {
            (_, HelperState::Comforting) if self.area == HelperArea::Generation => {
                "That one's on hold — generation needs the 18+ check done first, or the seat stopped answering. We'll pick it up right where we left off."
            }
            (_, HelperState::Comforting) => {
                "Something didn't answer the way it should. Nothing's lost — let's check the seat and try that step again."
            }
            (_, HelperState::WaitingOnSeat { .. }) => {
                "The seat has it now. These take a little while — I'll tell you the moment it's ready."
            }
            (_, HelperState::Succeeded) => "Done! It's filed in your gallery. Want to look?",
            (HelperArea::SeatSetup, HelperState::Guiding { step: 0, .. }) => {
                "Welcome! I'm Prima. First, let's connect you to a generation seat — I can fill in the usual one if you like."
            }
            (HelperArea::SeatSetup, HelperState::Guiding { step: 1, .. }) => {
                "Now let's knock on the seat's door and see if it answers."
            }
            (HelperArea::SeatSetup, HelperState::Guiding { step: 2, .. }) => {
                "It answered. Shall I save this seat so it's ready next time?"
            }
            (HelperArea::Generation, HelperState::Guiding { step: 0, .. }) => {
                "I'm Lana — I run the queue. Describe the scene and I'll cue it on the seat when you say go."
            }
            (HelperArea::Gallery, HelperState::Idle) => {
                "Sirena, at your service. Everything the seat has made for you lives here."
            }
            (HelperArea::SeatSetup, HelperState::Idle) => {
                "I'm Prima. I can walk you through seat setup whenever you're ready."
            }
            (HelperArea::Generation, HelperState::Idle) => {
                "I'm Lana. Say the word and I'll cue a still on the seat."
            }
            _ => "I'm here if you need a hand.",
        }
    }
}

/// The area a flow belongs to.
fn area_of(flow: FlowId) -> HelperArea {
    match flow {
        FlowId::SeatSetup => HelperArea::SeatSetup,
        FlowId::FirstGeneration => HelperArea::Generation,
    }
}

/// Advance a guided flow on a confirmed step. Confirming the
/// seat-test step moves to `WaitingOnSeat` (the shell runs the
/// test and reports back); confirming the generation step does
/// the same for the job. Final confirms land in `Succeeded`.
fn advance(flow: FlowId, step: u32) -> HelperState {
    match (flow, step) {
        (FlowId::SeatSetup, 0) => HelperState::Guiding { flow, step: 1 },
        (FlowId::SeatSetup, 1) => HelperState::WaitingOnSeat { flow },
        (FlowId::SeatSetup, _) => HelperState::Succeeded,
        (FlowId::FirstGeneration, 0) => HelperState::WaitingOnSeat { flow },
        (FlowId::FirstGeneration, _) => HelperState::Succeeded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ATTESTED: GateContext = GateContext { attested: true };
    const UNATTESTED: GateContext = GateContext { attested: false };

    #[test]
    fn duties_assign_one_face_per_area() {
        assert_eq!(helper_for(HelperArea::SeatSetup).id, "prima");
        assert_eq!(helper_for(HelperArea::Generation).id, "lana");
        assert_eq!(helper_for(HelperArea::Gallery).id, "sirena");
    }

    #[test]
    fn seat_setup_flow_walks_its_steps_in_order() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::BeginFlow(FlowId::SeatSetup), &ATTESTED);
        assert_eq!(h.proposed_action(), Some(ProposedAction::UseDefaultSeat));
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        assert_eq!(
            h.proposed_action(),
            Some(ProposedAction::TestSeatConnection)
        );
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        assert_eq!(
            h.state(),
            &HelperState::WaitingOnSeat {
                flow: FlowId::SeatSetup
            }
        );
        h.on_event(HelperEvent::SeatTestPassed, &ATTESTED);
        assert_eq!(h.proposed_action(), Some(ProposedAction::SaveSeatAndFinish));
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        assert_eq!(h.state(), &HelperState::Succeeded);
    }

    #[test]
    fn generation_flow_runs_to_success() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::BeginFlow(FlowId::FirstGeneration), &ATTESTED);
        assert_eq!(h.character().id, "lana");
        assert_eq!(h.proposed_action(), Some(ProposedAction::RunGeneration));
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        assert_eq!(
            h.state(),
            &HelperState::WaitingOnSeat {
                flow: FlowId::FirstGeneration
            }
        );
        assert_eq!(h.pose(), Pose::Work);
        h.on_event(HelperEvent::JobSucceeded, &ATTESTED);
        assert_eq!(h.state(), &HelperState::Succeeded);
        assert_eq!(h.pose(), Pose::Cheer);
        assert_eq!(h.proposed_action(), Some(ProposedAction::OpenGallery));
        h.on_event(HelperEvent::Dismiss, &ATTESTED);
        assert_eq!(h.state(), &HelperState::Idle);
    }

    // ---- adversarial transitions ----

    #[test]
    fn generation_flow_refuses_to_begin_unattested() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::BeginFlow(FlowId::FirstGeneration), &UNATTESTED);
        assert_eq!(h.state(), &HelperState::Comforting);
        assert_eq!(h.proposed_action(), None);
        assert!(h.line().contains("18+"));
    }

    #[test]
    fn seat_loss_mid_flow_lands_in_comfort_from_any_step() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::BeginFlow(FlowId::SeatSetup), &ATTESTED);
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        h.on_event(HelperEvent::SeatLost, &ATTESTED);
        assert_eq!(h.state(), &HelperState::Comforting);
        assert_eq!(h.pose(), Pose::Comfort);
    }

    #[test]
    fn attestation_loss_mid_flow_lands_in_comfort() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::BeginFlow(FlowId::FirstGeneration), &ATTESTED);
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        h.on_event(HelperEvent::AttestationMissing, &ATTESTED);
        assert_eq!(h.state(), &HelperState::Comforting);
    }

    #[test]
    fn cancel_during_a_guided_run_returns_to_idle() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::BeginFlow(FlowId::FirstGeneration), &ATTESTED);
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        assert_eq!(
            h.state(),
            &HelperState::WaitingOnSeat {
                flow: FlowId::FirstGeneration
            }
        );
        h.on_event(HelperEvent::Cancel, &ATTESTED);
        assert_eq!(h.state(), &HelperState::Idle);
        assert_eq!(h.proposed_action(), None);
    }

    #[test]
    fn stray_events_do_not_corrupt_idle() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        h.on_event(HelperEvent::JobSucceeded, &ATTESTED);
        h.on_event(HelperEvent::BackStep, &ATTESTED);
        assert_eq!(h.state(), &HelperState::Idle);
    }

    #[test]
    fn back_step_retreats_one_step_and_stops_at_zero() {
        let mut h = Helper::new();
        h.on_event(HelperEvent::BeginFlow(FlowId::SeatSetup), &ATTESTED);
        h.on_event(HelperEvent::BackStep, &ATTESTED);
        assert_eq!(
            h.state(),
            &HelperState::Guiding {
                flow: FlowId::SeatSetup,
                step: 0
            }
        );
        h.on_event(HelperEvent::ConfirmStep, &ATTESTED);
        h.on_event(HelperEvent::BackStep, &ATTESTED);
        assert_eq!(
            h.state(),
            &HelperState::Guiding {
                flow: FlowId::SeatSetup,
                step: 0
            }
        );
    }
}
