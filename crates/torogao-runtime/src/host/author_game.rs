//! The AUTH-G interpreter as a hosted core: an AUTH-G-authored
//! game implements [`GameCore`] BY BEING INTERPRETED.
//!
//! The adapter is thin by design. The interpreter
//! (torogao-author's game module) already owns the semantics:
//! genesis from (project, seed), one input per tick from the
//! choice log, the per-tick state hash, and the revision-bound
//! save document (D-467: the save binds the project hash it
//! belongs to, and load verifies it). This core translates the
//! runtime's [`Command`] vocabulary into the interpreter's
//! input set, one tick per `step`:
//!
//! - `Advance` / `Event` / `Choice` / `Interact` / `Offer`
//!   map onto the interpreter's same-named inputs. The
//!   interpreter admits ONE input per tick: the first game
//!   input in a step's command list takes the tick's slot;
//!   further game inputs are refused, in order.
//! - Pointer / key / gamepad commands are acknowledged as
//!   ignored: they are host vocabulary, reduced by the host
//!   before the core in a real shell; an interpreter core has
//!   nothing to do with them.
//! - Lifecycle commands are acknowledged as ignored: the
//!   session survives them by construction.
//! - [`Command::Invoke`] is refused: the interpreter's
//!   vocabulary is the AUTH-G input set, closed by design.
//!
//! A step that the interpreter refuses changes nothing: the
//! adapter stages the tick on a cloned state and adopts it
//! only on success.

use crate::command::Command;
use crate::core::{GameCore, SaveBlob, StepReport, Verdict};
use crate::hash::Hash;
use crate::present::{GaugeView, MeterView, PresentationState};
use torogao_author::game::model::Game;
use torogao_author::game::op::LogEntryInput;
use torogao_author::game::save::{load_game, save_game};
use torogao_author::game::simulate::{genesis, parse_log, run_ticks};
use torogao_author::game::state::{GameState, state_hash};
use torogao_author::model::Project;
use torogao_author::{Refusal, RefusalClass};

/// An AUTH-G game hosted through its interpreter.
pub struct InterpreterCore {
    project: Project,
    project_sha256: String,
    run_seed: i64,
    state: GameState,
    core_id: String,
    clock_slots: i64,
}

impl InterpreterCore {
    /// Constructs the core at genesis for `project` (already
    /// loaded, with its content hash) under `run_seed`.
    ///
    /// # Errors
    ///
    /// Returns the interpreter's typed refusal when the
    /// project carries no game section or genesis fails
    /// (e.g. no clock defined).
    pub fn new(project: Project, project_sha256: String, run_seed: i64) -> Result<Self, Refusal> {
        let game: &Game = project.game.as_ref().ok_or_else(|| {
            Refusal::new(
                RefusalClass::Malformed,
                "the project carries no game section to interpret",
            )
        })?;
        let core_id = format!("auth-g:{}", game.id);
        let clock_slots = game
            .clock
            .as_ref()
            .map_or(0, |clock| i64::try_from(clock.slots.len()).unwrap_or(0));
        let state = genesis(&project, run_seed)?;
        Ok(Self {
            project,
            project_sha256,
            run_seed,
            state,
            core_id,
            clock_slots,
        })
    }

    /// The interpreted state (read-only inspection for gates
    /// and shells that speak AUTH-G directly).
    #[must_use]
    pub fn state(&self) -> &GameState {
        &self.state
    }
}

/// Translates one command into the interpreter's input kinds,
/// or classifies why it cannot be one.
enum Translated {
    /// The command takes the tick's input slot.
    Input {
        /// Interpreter input kind.
        kind: &'static str,
        /// Event/trigger/choice/grid id.
        id: String,
        /// Option payload, by kind.
        option: Option<String>,
        /// Index into the step's verdict list to fill.
        verdict_index: usize,
    },
    /// The command is already verdicted.
    Settled,
}

fn ignored(reason: &str) -> Verdict {
    Verdict::Ignored {
        reason: reason.to_string(),
    }
}

fn refused(reason: &str) -> Verdict {
    Verdict::Refused {
        reason: reason.to_string(),
    }
}

impl GameCore for InterpreterCore {
    fn core_id(&self) -> &str {
        &self.core_id
    }

    fn tick(&self) -> u64 {
        self.state.tick
    }

    fn step(&mut self, commands: &[Command]) -> StepReport {
        let next_tick = self.state.tick + 1;
        let mut verdicts: Vec<Verdict> = Vec::with_capacity(commands.len());
        let mut chosen: Option<Translated> = None;
        for command in commands {
            let verdict_index = verdicts.len();
            match command {
                Command::Advance
                | Command::Event { .. }
                | Command::Choice { .. }
                | Command::Interact { .. }
                | Command::Offer { .. } => {
                    if chosen.is_some() {
                        verdicts.push(refused(
                            "the interpreter admits one input per tick; this tick's slot is taken",
                        ));
                        continue;
                    }
                    let (kind, id, option) = match command {
                        Command::Advance => ("advance", String::new(), None),
                        Command::Event { id } => ("event", id.clone(), None),
                        Command::Choice { id, option } => ("choice", id.clone(), option.clone()),
                        Command::Interact { id, option } => {
                            ("interact", id.clone(), option.clone())
                        }
                        Command::Offer { id, accept } => (
                            "offer",
                            id.clone(),
                            Some(if *accept { "accept" } else { "decline" }.to_string()),
                        ),
                        _ => unreachable!("the outer match already narrowed the variants"),
                    };
                    chosen = Some(Translated::Input {
                        kind,
                        id,
                        option,
                        verdict_index,
                    });
                    // Filled in after the tick runs: Applied, or
                    // Refused when the interpreter refuses.
                    verdicts.push(Verdict::Applied);
                }
                Command::PointerTap { .. }
                | Command::PointerHold { .. }
                | Command::PointerDrag { .. }
                | Command::KeyPress { .. }
                | Command::PadPress { .. }
                | Command::PadAxis { .. } => {
                    verdicts.push(ignored(
                        "host vocabulary: the host reduces pointer/key/pad input before the core",
                    ));
                }
                Command::Pause | Command::Resume | Command::Suspend => {
                    verdicts.push(ignored(
                        "lifecycle notice: the session survives by construction; nothing to do in the core",
                    ));
                }
                Command::Invoke { verb, .. } => {
                    verdicts.push(refused(&format!(
                        "the interpreter's vocabulary is the AUTH-G input set; invoke verb '{verb}' is not in it"
                    )));
                }
            }
        }
        let _ = Translated::Settled;

        let inputs: Vec<LogEntryInput> = match &chosen {
            Some(Translated::Input {
                kind, id, option, ..
            }) => vec![LogEntryInput {
                tick: next_tick,
                kind: (*kind).to_string(),
                id: id.clone(),
                option: option.clone(),
            }],
            _ => Vec::new(),
        };
        let fail = |verdicts: &mut Vec<Verdict>, chosen: &Option<Translated>, error: &Refusal| {
            if let Some(Translated::Input { verdict_index, .. }) = chosen {
                verdicts[*verdict_index] =
                    refused(&format!("interpreter refused the tick: {error}"));
            }
        };
        let log = match parse_log(&inputs, next_tick) {
            Ok(log) => log,
            Err(error) => {
                fail(&mut verdicts, &chosen, &error);
                return StepReport {
                    tick: self.state.tick,
                    verdicts,
                    events: Vec::new(),
                };
            }
        };
        // Stage on a clone: a refused tick changes nothing.
        let mut staged = self.state.clone();
        let mut events = Vec::new();
        let mut chain = Vec::new();
        match run_ticks(
            &self.project,
            &mut staged,
            &log,
            next_tick,
            &mut events,
            &mut chain,
        ) {
            Ok(()) => {
                self.state = staged;
                StepReport {
                    tick: self.state.tick,
                    verdicts,
                    events,
                }
            }
            Err(error) => {
                fail(&mut verdicts, &chosen, &error);
                StepReport {
                    tick: self.state.tick,
                    verdicts,
                    events: Vec::new(),
                }
            }
        }
    }

    fn state_hash(&self) -> Hash {
        Hash(state_hash(&self.state))
    }

    fn save(&self) -> Result<SaveBlob, Refusal> {
        let text = save_game(
            &self.project,
            &self.project_sha256,
            &self.state,
            self.run_seed,
        )?;
        Ok(SaveBlob(text))
    }

    fn load(&mut self, blob: &SaveBlob) -> Result<(), Refusal> {
        let saved = load_game(&self.project, &self.project_sha256, blob.as_str())?;
        if saved.run_seed != self.run_seed {
            return Err(Refusal::new(
                RefusalClass::Conflict,
                format!(
                    "the save binds run seed {}, this core runs seed {}",
                    saved.run_seed, self.run_seed
                ),
            ));
        }
        self.state = saved.state;
        Ok(())
    }

    fn present(&self) -> PresentationState {
        let mut meters = vec![
            MeterView {
                id: "tick".to_string(),
                value: i64::try_from(self.state.tick).unwrap_or(i64::MAX),
            },
            MeterView {
                id: "day".to_string(),
                value: self.state.day,
            },
            MeterView {
                id: "slot".to_string(),
                value: i64::from(self.state.slot),
            },
        ];
        for (key, value) in &self.state.stats {
            let id = match &key.character {
                Some(character) => format!("{character}/{}", key.stat),
                None => key.stat.clone(),
            };
            meters.push(MeterView { id, value: *value });
        }
        PresentationState {
            tick: self.state.tick,
            headline: self.state.current_scene.clone().unwrap_or_default(),
            detail: self.state.line.clone(),
            meters,
            gauges: vec![GaugeView::ratio(
                "day_progress",
                i64::from(self.state.slot),
                self.clock_slots,
            )],
        }
    }
}

/// Authors the Changr-parity fixture (the AUTH-G test suites'
/// measured fixture: an 8-slot clock, a character-scoped
/// `control_level` stat with a 6-day lifetime, and an
/// `apply_control` event that raises it to 1) into a scratch
/// directory through the ordinary verb pipeline, loads it,
/// and returns the hosted core. The scratch directory is
/// removed once the project is in memory.
///
/// This is fixture machinery for the runtime's own gates and
/// replay harness — the same role the fixture plays in
/// torogao-author's suites.
///
/// # Errors
///
/// Returns a typed refusal when authoring or genesis fails.
pub fn changr_fixture_core(run_seed: i64) -> Result<InterpreterCore, Refusal> {
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = std::env::temp_dir().join(format!(
        "torogao-runtime-fixture-{}-{n}",
        std::process::id()
    ));
    let _removed = std::fs::remove_dir_all(&root);
    let requests: Vec<torogao_author::store::Request> = vec![
        ("project_init", json!({
            "name": "runtime_fixture",
            "stamp": {"basis": "runtime R1 fixture", "lane": "all-ages", "pack": "test", "rating": "E"},
            "consent": {"adult": true, "schema": "torogao-consent/1", "wants_selected_scenes": true},
        })),
        ("game_define", json!({"id": "changr_fixture", "start_scene": "room"})),
        ("clock_define", json!({
            "slots": ["morning_1", "morning_2", "afternoon_1", "afternoon_2",
                      "evening_1", "evening_2", "night_1", "night_2"],
            "days_per_week": 7,
        })),
        ("stat_define", json!({
            "id": "control_level", "scope": "character", "kind": {"kind": "int"},
            "min": 0, "max": 5, "default": 0, "overflow": "clamp",
            "lifetime_days": 6,
        })),
        ("character_define", json!({"id": "amy", "name_ref": "name_amy", "name_text": "Amy"})),
        ("scene_define", json!({"id": "room", "presentation": "room"})),
        ("effect_set_define", json!({
            "id": "apply_control_set",
            "atoms": [{
                "kind": "stat_set", "stat": "control_level", "character": "amy",
                "value": {"kind": "lit", "value": 1},
            }],
        })),
        ("trigger_define", json!({
            "id": "apply_control",
            "kind": {"kind": "on_event", "event": "apply_control"},
            "effect_set": "apply_control_set",
        })),
    ]
    .into_iter()
    .map(|(verb, inputs)| torogao_author::store::Request {
        verb: verb.to_string(),
        inputs,
    })
    .collect();
    let answer = torogao_author::run_requests(&root, &root, &requests);
    if answer.status != torogao_author::Status::Ok {
        let _removed = std::fs::remove_dir_all(&root);
        return Err(Refusal::new(
            RefusalClass::Malformed,
            format!("the runtime fixture failed to author: {}", answer.doc),
        ));
    }
    let loaded = torogao_author::store::load(&root)?;
    let _removed = std::fs::remove_dir_all(&root);
    let loaded = loaded.ok_or_else(|| {
        Refusal::new(
            RefusalClass::Malformed,
            "the runtime fixture authored no project",
        )
    })?;
    InterpreterCore::new(loaded.project, loaded.sha256, run_seed)
}
