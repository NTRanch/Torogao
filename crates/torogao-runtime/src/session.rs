//! The session: the runtime's unit of play.
//!
//! A session wraps one hosted core and owns its discipline:
//! genesis (the core as constructed, hash recorded), the
//! append-only op log, the per-tick hash chain, the save
//! service, and replay. Hosts drive a session; they may
//! translate OS events into commands and persist save text,
//! and may do nothing else to game state (the host-quarantine
//! rule, plan SS1.3).
//!
//! The session is also the headless replay harness — the
//! D-925 TUI-first validation surface. [`Session::replay`]
//! re-executes a recorded op log against a fresh core and
//! compares the full hash chain and verdict stream; no window
//! needs to exist for the runtime to be validated.

use crate::command::Command;
use crate::core::{GameCore, SaveBlob, StepReport};
use crate::hash::Hash;
use crate::oplog::{OpLog, OpLogEntry};
use crate::save::{SAVE_FORMAT, SaveEnvelope};
use torogao_author::{Refusal, RefusalClass};

/// The outcome of a replay: whether the recorded chain was
/// reproduced, the chain the replay produced, and where it
/// first diverged (when it did).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayReport {
    /// True when every tick's hash (and verdict stream)
    /// matched the recorded log.
    pub matches: bool,
    /// The hash chain the replay produced.
    pub chain: Vec<Hash>,
    /// The first tick whose hash or verdicts diverged, when
    /// the replay did not match.
    pub first_divergence: Option<u64>,
}

/// One running session over a hosted core.
pub struct Session<C: GameCore> {
    core: C,
    genesis_hash: Hash,
    log: OpLog,
    chain: Vec<Hash>,
}

impl<C: GameCore> Session<C> {
    /// Opens a session at genesis: the core as constructed
    /// (tick 0), its hash recorded as the chain's root.
    #[must_use]
    pub fn new(core: C) -> Self {
        let genesis_hash = core.state_hash();
        Self {
            core,
            genesis_hash,
            log: OpLog::new(),
            chain: Vec::new(),
        }
    }

    /// The genesis hash (the core's state hash at tick 0).
    #[must_use]
    pub fn genesis_hash(&self) -> &Hash {
        &self.genesis_hash
    }

    /// The wrapped core (read-only: hosts inspect, they do
    /// not mutate outside `step`).
    #[must_use]
    pub fn core(&self) -> &C {
        &self.core
    }

    /// The current tick.
    #[must_use]
    pub fn tick(&self) -> u64 {
        self.core.tick()
    }

    /// Executes one tick: forwards the commands to the core,
    /// appends the tick to the op log, and extends the hash
    /// chain with the resulting state hash.
    pub fn step(&mut self, commands: &[Command]) -> StepReport {
        let report = self.core.step(commands);
        let hash = self.core.state_hash();
        self.log.append(OpLogEntry {
            tick: report.tick,
            commands: commands.to_vec(),
            verdicts: report.verdicts.clone(),
            hash: hash.clone(),
        });
        self.chain.push(hash);
        report
    }

    /// The per-tick hash chain, in tick order. After a
    /// [`Session::load`], the chain restarts seeded with the
    /// restored envelope's tip (the envelope is a checkpoint;
    /// see `load`).
    #[must_use]
    pub fn chain(&self) -> &[Hash] {
        &self.chain
    }

    /// The op log as recorded so far.
    #[must_use]
    pub fn log(&self) -> &OpLog {
        &self.log
    }

    /// Takes a save at the current tick boundary: the core's
    /// canonical blob wrapped in the runtime envelope.
    ///
    /// # Errors
    ///
    /// Returns the core's typed refusal when the core refuses
    /// to save (e.g. mid-tick transient state).
    pub fn save(&self) -> Result<String, Refusal> {
        let blob = self.core.save()?;
        let chain_tip = self
            .chain
            .last()
            .cloned()
            .unwrap_or_else(|| self.genesis_hash.clone());
        let envelope = SaveEnvelope {
            format: SAVE_FORMAT.to_string(),
            core_id: self.core.core_id().to_string(),
            tick: self.core.tick(),
            chain_tip,
            log_len: u64::try_from(self.log.len()).unwrap_or(u64::MAX),
            core_blob: blob.0,
        };
        Ok(envelope.to_text())
    }

    /// Loads a save envelope into this session. The envelope
    /// is a checkpoint: the core restores from the blob (its
    /// own revision binding verified inside the core), then
    /// the session verifies the restored tick and state hash
    /// against the envelope's binding, and restarts its op
    /// log and chain from the checkpoint — the chain is seeded
    /// with the envelope's tip, so a continued run's chain
    /// reads `..tip, next, ..` with no gap.
    ///
    /// A failed load leaves the session's core untouched:
    /// the restore is staged on a scratch hash check before
    /// the session adopts it — concretely, the core's own
    /// load is transactional (a failed load mutates nothing,
    /// per the [`GameCore`] contract), and the session adopts
    /// the new log/chain only after every check passes.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal when the envelope does not
    /// parse, names a different core, or the restored state
    /// does not match the envelope's tick/hash binding.
    pub fn load(&mut self, text: &str) -> Result<(), Refusal> {
        let envelope = SaveEnvelope::from_text(text)?;
        if envelope.core_id != self.core.core_id() {
            return Err(Refusal::new(
                RefusalClass::Conflict,
                format!(
                    "save belongs to core '{}', this session hosts '{}'",
                    envelope.core_id,
                    self.core.core_id()
                ),
            ));
        }
        self.core.load(&SaveBlob(envelope.core_blob.clone()))?;
        if self.core.tick() != envelope.tick {
            return Err(Refusal::new(
                RefusalClass::ReplayDivergence,
                format!(
                    "restored tick {} does not match the envelope's tick {}",
                    self.core.tick(),
                    envelope.tick
                ),
            ));
        }
        let restored = self.core.state_hash();
        if restored != envelope.chain_tip {
            return Err(Refusal::new(
                RefusalClass::ReplayDivergence,
                format!(
                    "restored state hash {restored} does not match the envelope's chain tip {}",
                    envelope.chain_tip
                ),
            ));
        }
        self.log = OpLog::new();
        self.chain = vec![envelope.chain_tip.clone()];
        self.genesis_hash = envelope.chain_tip;
        Ok(())
    }

    /// Replays a recorded op log against a fresh core and
    /// compares every tick's hash and verdicts. The harness
    /// is pure: no window, no clock, no filesystem.
    #[must_use]
    pub fn replay(core: C, log: &OpLog) -> ReplayReport {
        let mut session = Session::new(core);
        let mut first_divergence = None;
        for entry in log.entries() {
            let report = session.step(&entry.commands);
            if first_divergence.is_none()
                && (report.tick != entry.tick
                    || report.verdicts != entry.verdicts
                    || session.chain.last() != Some(&entry.hash))
            {
                first_divergence = Some(entry.tick);
            }
        }
        ReplayReport {
            matches: first_divergence.is_none(),
            chain: session.chain.clone(),
            first_divergence,
        }
    }
}
