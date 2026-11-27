//! The conformance suite: the shared contract test every
//! hosted core must pass, whatever its shape.
//!
//! [`check_core`] runs one core factory through the checks
//! the plan's R1 gate names, over a caller-supplied corpus:
//!
//! 1. **Repeat determinism x3** — the corpus run in three
//!    fresh sessions produces identical hash chains (and
//!    identical genesis hashes).
//! 2. **Save/load continuity** — the corpus run to the
//!    checkpoint, saved, loaded into a fresh session, and
//!    continued, produces a chain that joins the uninterrupted
//!    chain exactly at the checkpoint (the loaded session's
//!    seeded tip equals the reference hash at that tick, and
//!    every subsequent hash matches).
//! 3. **Replay** — the recorded op log, replayed against a
//!    fresh core, reproduces the chain and the verdict stream
//!    exactly.
//! 4. **Presentation discipline** — `present()` at genesis
//!    and at the end reports the session's tick, and the
//!    act of presenting does not move the state hash (Tier 2
//!    is a view, never an input).
//!
//! The suite is a library function, not a test file, so the
//! same checks run in the runtime crate's tests (interpreter
//! witness), in a consumer tree's tests (the Loop Service
//! witness), and in any future title's gate.

use crate::command::Command;
use crate::core::GameCore;
use crate::hash::Hash;
use crate::session::Session;

/// One named check's outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckOutcome {
    /// The check's stable name.
    pub name: String,
    /// Whether it passed.
    pub passed: bool,
    /// What the check observed (hashes, ticks, failure detail).
    pub detail: String,
}

/// The suite's full report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractReport {
    /// Every check, in run order.
    pub checks: Vec<CheckOutcome>,
    /// The reference run's full hash chain.
    pub reference_chain: Vec<Hash>,
    /// The reference run's genesis hash.
    pub genesis_hash: Hash,
}

impl ContractReport {
    /// True when every check passed.
    #[must_use]
    pub fn all_passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }

    /// A one-line-per-check human summary, for gate records.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut out = String::new();
        for check in &self.checks {
            out.push_str(&format!(
                "{} {} — {}\n",
                if check.passed { "PASS" } else { "FAIL" },
                check.name,
                check.detail
            ));
        }
        out
    }
}

fn outcome(name: &str, passed: bool, detail: String) -> CheckOutcome {
    CheckOutcome {
        name: name.to_string(),
        passed,
        detail,
    }
}

fn run_corpus<C: GameCore>(core: C, corpus: &[Vec<Command>]) -> Session<C> {
    let mut session = Session::new(core);
    for commands in corpus {
        session.step(commands);
    }
    session
}

/// Runs the conformance suite against cores produced by
/// `make` (a fresh core per run — genesis must be a pure
/// function of the factory's captured context), over
/// `corpus`, checkpointing the save/load check after
/// `save_at` steps.
#[must_use]
pub fn check_core<C: GameCore>(
    make: &dyn Fn() -> C,
    corpus: &[Vec<Command>],
    save_at: usize,
) -> ContractReport {
    let mut checks = Vec::new();

    // The reference run.
    let reference = run_corpus(make(), corpus);
    let reference_chain = reference.chain().to_vec();
    let genesis_hash = reference.genesis_hash().clone();

    // 1. Repeat determinism x3 (the reference plus two more).
    let mut repeat_ok = true;
    let mut repeat_detail = String::new();
    for run in 1..=2 {
        let again = run_corpus(make(), corpus);
        if again.chain() != reference_chain.as_slice() || again.genesis_hash() != &genesis_hash {
            repeat_ok = false;
            repeat_detail = format!("run {run} diverged from the reference chain");
            break;
        }
    }
    if repeat_ok {
        repeat_detail = format!(
            "3 runs, {} ticks, identical chains; tip {}",
            reference_chain.len(),
            reference_chain
                .last()
                .map_or_else(|| "none".to_string(), |hash| hash.as_str().to_string())
        );
    }
    checks.push(outcome("repeat_determinism_x3", repeat_ok, repeat_detail));

    // 2. Save/load continuity.
    let save_check = if save_at == 0 || save_at >= corpus.len() {
        outcome(
            "save_load_continuity",
            false,
            format!(
                "checkpoint {save_at} is outside the corpus of {}",
                corpus.len()
            ),
        )
    } else {
        let mut head = Session::new(make());
        for commands in &corpus[..save_at] {
            head.step(commands);
        }
        match head.save() {
            Err(error) => outcome(
                "save_load_continuity",
                false,
                format!("save at step {save_at} refused: {error}"),
            ),
            Ok(text) => {
                let mut tail = Session::new(make());
                match tail.load(&text) {
                    Err(error) => outcome(
                        "save_load_continuity",
                        false,
                        format!("load refused: {error}"),
                    ),
                    Ok(()) => {
                        for commands in &corpus[save_at..] {
                            tail.step(commands);
                        }
                        // The loaded chain is seeded with the
                        // checkpoint tip: joined = reference
                        // chain up to the checkpoint, then the
                        // loaded session's post-restore hashes.
                        let mut joined: Vec<Hash> = reference_chain[..save_at].to_vec();
                        joined.extend_from_slice(&tail.chain()[1..]);
                        let joined_ok = joined == reference_chain
                            && tail.chain().first() == reference_chain.get(save_at - 1);
                        outcome(
                            "save_load_continuity",
                            joined_ok,
                            if joined_ok {
                                format!(
                                    "saved after step {save_at}, loaded fresh, continued {} steps; chain joins exactly",
                                    corpus.len() - save_at
                                )
                            } else {
                                "the continued chain does not join the reference chain".to_string()
                            },
                        )
                    }
                }
            }
        }
    };
    checks.push(save_check);

    // 3. Replay from the op log.
    let replay = Session::replay(make(), reference.log());
    checks.push(outcome(
        "replay_reproduces_chain",
        replay.matches && replay.chain == reference_chain,
        match replay.first_divergence {
            Some(tick) => format!("replay diverged at tick {tick}"),
            None => format!(
                "op log of {} ticks replays to the identical chain and verdicts",
                reference_chain.len()
            ),
        },
    ));

    // 4. Presentation discipline.
    let core = make();
    let genesis_present = core.present();
    let genesis_hash_before = core.state_hash();
    let _ = core.present();
    let present_ok = genesis_present.tick == 0 && core.state_hash() == genesis_hash_before;
    let mut session = Session::new(make());
    for commands in corpus {
        session.step(commands);
    }
    let end_present = session.core().present();
    let present_ok = present_ok && end_present.tick == session.tick();
    checks.push(outcome(
        "presentation_is_a_view",
        present_ok,
        format!(
            "present() at genesis tick {} and at end tick {}; presenting does not move the hash",
            genesis_present.tick, end_present.tick
        ),
    ));

    ContractReport {
        checks,
        reference_chain,
        genesis_hash,
    }
}
