//! Runtime game state: the interpreter's whole world, and its
//! canonical serialization.
//!
//! State is a pure function of (project, run seed, choice log)
//! (§G3.1). Every map is a `BTreeMap`, so serialization order is
//! key order — the state hash never depends on hash-map
//! iteration. Two fields are transient within a tick
//! (`pending_scene`, `emitted`) and are never serialized; both
//! are always empty at tick boundaries, which is the only place
//! a state is hashed or saved.

use crate::codec::{open, opt, record, text, wrong_type};
use crate::intake::sha256_hex;
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::ron::{Ron, emit_document};
use std::collections::{BTreeMap, BTreeSet};

/// One stat instance's key: the stat id, plus the character for
/// character-scoped stats.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct StatKey {
    /// The character instance (absent for game/party scope).
    pub character: Option<String>,
    /// The stat id.
    pub stat: String,
}

/// One trigger's runtime state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TriggerState {
    /// A `once` trigger that has fired (or whose offer was
    /// accepted/expired): it never fires again.
    pub fired: bool,
    /// The earliest tick at which the trigger may fire again
    /// (cooldown bookkeeping).
    pub next_tick: u64,
    /// For threshold triggers: the side of the threshold the
    /// stat was on at the last evaluation (edge tracking).
    pub above: bool,
    /// For flag triggers: the flag's value at the last
    /// evaluation (edge tracking).
    pub flag_was: bool,
    /// The day an offer from this trigger was declined on; the
    /// trigger is suppressed for the rest of that day (D-464).
    pub deferred_day: Option<i64>,
    /// The pending offer, when the trigger is offered.
    pub offer: Option<OfferState>,
}

/// A pending offer's runtime state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OfferState {
    /// The day the offer was made.
    pub offered_day: i64,
    /// Whole days the offer has aged (it ages on day rollover,
    /// the same clock that drives stat lifetimes).
    pub age_days: u32,
}

/// The whole runtime state of one run. The slot partition
/// (per-run state) is everything except `flags_globals`, which
/// is the globals partition of §G3.3.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GameState {
    /// Ticks elapsed.
    pub tick: u64,
    /// Current day (1-based).
    pub day: i64,
    /// Current slot index into the clock's slots.
    pub slot: u32,
    /// Stat instances by key.
    pub stats: BTreeMap<StatKey, i64>,
    /// Armed lifetime counters (days remaining), by stat key.
    /// A counter exists only while its stat's value differs
    /// from the stat's default.
    pub lifetimes: BTreeMap<StatKey, u32>,
    /// Per-stream RNG state (the SplitMix64 state word), by
    /// stream id. Seeded at genesis from the run seed; a draw
    /// advances exactly one stream by exactly one step.
    pub rng: BTreeMap<String, u64>,
    /// Run-scope flag values.
    pub flags_run: BTreeMap<String, bool>,
    /// Globals-scope flag values (the globals partition).
    pub flags_globals: BTreeMap<String, bool>,
    /// The scene the run is in.
    pub current_scene: Option<String>,
    /// The ending the run reached, when one fired (the ending
    /// evaluation step sets it and the run stops). Emitted
    /// only when reached (absent means absent), so a state
    /// document from before endings existed keeps its bytes
    /// and its hash.
    pub ending: Option<String>,
    /// The current dialogue line, when the run sits on one.
    pub line: Option<String>,
    /// Trigger runtime states by trigger id.
    pub triggers: BTreeMap<String, TriggerState>,
    /// Threads already delivered by a scene interlude in this
    /// run (P2-7/D-461): delivery is fire-once, so re-entering
    /// the scene does not re-deliver the batch. Emitted only
    /// when non-empty (absent means absent), so a state
    /// document from before threads existed keeps its bytes
    /// and its hash.
    pub threads_delivered: BTreeSet<String>,
    /// The thread reply delay still to elapse, in milliseconds
    /// (P2-7/D-461): set at interlude delivery, consumed on
    /// the step clock at `THREAD_STEP_MS` per tick, and while
    /// it is positive no choice in the interlude's scene may
    /// fire. Emitted only when positive, like the ending.
    pub thread_delay_ms: u64,
    /// Transient: a scene transition awaiting the tick's
    /// transition step. Never serialized.
    pub pending_scene: Option<String>,
    /// Transient: events emitted by effect sets this tick,
    /// awaiting the re-evaluation pass. Never serialized.
    pub emitted: Vec<String>,
}

/// Emits the canonical state document (the save partition
/// image and the hash-chain input).
pub fn state_to_ron(state: &GameState) -> Ron {
    let mut fields: Vec<(&str, Ron)> = vec![
        (
            "current_scene",
            opt(state.current_scene.as_ref(), |id| text(id)),
        ),
        ("day", Ron::Int(state.day)),
        ("flags_globals", Ron::List(flag_list(&state.flags_globals))),
        ("flags_run", Ron::List(flag_list(&state.flags_run))),
        (
            "lifetimes",
            Ron::List(
                state
                    .lifetimes
                    .iter()
                    .map(|(key, days)| {
                        record(
                            "LifetimeState",
                            vec![
                                ("character", opt(key.character.as_ref(), |id| text(id))),
                                ("days", Ron::Int(i64::from(*days))),
                                ("stat", text(&key.stat)),
                            ],
                        )
                    })
                    .collect(),
            ),
        ),
        ("line", opt(state.line.as_ref(), |id| text(id))),
        ("slot", Ron::Int(i64::from(state.slot))),
        (
            "stats",
            Ron::List(
                state
                    .stats
                    .iter()
                    .map(|(key, value)| {
                        record(
                            "StatState",
                            vec![
                                ("character", opt(key.character.as_ref(), |id| text(id))),
                                ("stat", text(&key.stat)),
                                ("value", Ron::Int(*value)),
                            ],
                        )
                    })
                    .collect(),
            ),
        ),
        (
            "tick",
            Ron::Int(i64::try_from(state.tick).unwrap_or(i64::MAX)),
        ),
        (
            "triggers",
            Ron::List(
                state
                    .triggers
                    .iter()
                    .map(|(id, trigger)| {
                        record(
                            "TriggerState",
                            vec![
                                ("above", Ron::Bool(trigger.above)),
                                (
                                    "deferred_day",
                                    opt(trigger.deferred_day.as_ref(), |day| Ron::Int(*day)),
                                ),
                                ("flag_was", Ron::Bool(trigger.flag_was)),
                                ("fired", Ron::Bool(trigger.fired)),
                                ("id", text(id)),
                                (
                                    "next_tick",
                                    Ron::Int(i64::try_from(trigger.next_tick).unwrap_or(i64::MAX)),
                                ),
                                (
                                    "offer",
                                    opt(trigger.offer.as_ref(), |offer| {
                                        record(
                                            "OfferState",
                                            vec![
                                                ("age_days", Ron::Int(i64::from(offer.age_days))),
                                                ("offered_day", Ron::Int(offer.offered_day)),
                                            ],
                                        )
                                    }),
                                ),
                            ],
                        )
                    })
                    .collect(),
            ),
        ),
    ];
    // The rng partition is emitted only when a stream has
    // state (absent means absent): a state document written
    // before streams existed — every P2-2/P2-3 save — keeps
    // its bytes and its hash. The state word rides as an i64
    // bit-cast (RON integers are i64; the cast is exact).
    if !state.rng.is_empty() {
        let pos = fields
            .iter()
            .position(|(name, _)| *name == "line")
            .unwrap_or(fields.len());
        fields.insert(
            pos,
            (
                "rng",
                Ron::List(
                    state
                        .rng
                        .iter()
                        .map(|(stream, word)| {
                            record(
                                "RngState",
                                vec![("state", Ron::Int(*word as i64)), ("stream", text(stream))],
                            )
                        })
                        .collect(),
                ),
            ),
        );
    }
    // The thread partition rides only once a thread has been
    // delivered or a delay is pending, under the same
    // absent-means-absent law as the rng partition.
    if !state.threads_delivered.is_empty() || state.thread_delay_ms > 0 {
        let pos = fields
            .iter()
            .position(|(name, _)| *name == "triggers")
            .unwrap_or(fields.len());
        fields.insert(
            pos,
            (
                "threads_delivered",
                Ron::List(state.threads_delivered.iter().map(|id| text(id)).collect()),
            ),
        );
        fields.insert(
            pos,
            (
                "thread_delay_ms",
                Ron::Int(i64::try_from(state.thread_delay_ms).unwrap_or(i64::MAX)),
            ),
        );
    }
    // The ending rides only when a run reached one, under
    // the same absent-means-absent law as the rng partition.
    if let Some(ending) = &state.ending {
        let pos = fields
            .iter()
            .position(|(name, _)| *name == "flags_globals")
            .unwrap_or(fields.len());
        fields.insert(pos, ("ending", text(ending)));
    }
    record("State", fields)
}

fn flag_list(flags: &BTreeMap<String, bool>) -> Vec<Ron> {
    flags
        .iter()
        .map(|(id, value)| {
            record(
                "FlagState",
                vec![("flag", text(id)), ("value", Ron::Bool(*value))],
            )
        })
        .collect()
}

/// The canonical state hash: sha256 of the state document's
/// canonical bytes.
pub fn state_hash(state: &GameState) -> String {
    sha256_hex(emit_document(&state_to_ron(state)).as_bytes())
}

fn as_u64(value: Ron, key: &str) -> Result<u64, Refusal> {
    match value {
        Ron::Int(number) => {
            u64::try_from(number).or_else(|_| wrong_type(key, "a non-negative integer"))
        }
        _ => wrong_type(key, "an integer"),
    }
}

fn flag_map(items: Vec<Ron>, key: &str) -> Result<BTreeMap<String, bool>, Refusal> {
    let mut map = BTreeMap::new();
    for item in items {
        let mut fields = open(item, "FlagState")?;
        let flag = fields.string("flag")?;
        let value = fields.boolean("value")?;
        fields.done()?;
        if map.insert(flag.clone(), value).is_some() {
            return refuse(
                RefusalClass::Malformed,
                format!("state {key} carries flag '{flag}' twice"),
            );
        }
    }
    Ok(map)
}

/// Parses a state document. `format_version` is the game
/// format the document was written under: fields introduced by
/// v2 (`lifetimes`, and the trigger states' `offer` /
/// `deferred_day`) may be absent in a v1 document and read as
/// their defaults — the record-level half of the v1 → v2
/// migration (see `save::migrate_state_v1_to_v2`).
pub fn state_from_ron(value: Ron, format_version: i64) -> Result<GameState, Refusal> {
    let mut fields = open(value, "State")?;
    let mut stats = BTreeMap::new();
    for item in fields.list("stats")? {
        let mut row = open(item, "StatState")?;
        let key = StatKey {
            character: row.opt_string("character")?,
            stat: row.string("stat")?,
        };
        let value = row.int("value")?;
        row.done()?;
        if stats.insert(key.clone(), value).is_some() {
            return refuse(
                RefusalClass::Malformed,
                format!("state carries stat '{}' twice", key.stat),
            );
        }
    }
    let lifetime_items = if format_version >= 2 {
        fields.list("lifetimes")?
    } else {
        match fields.try_take("lifetimes") {
            None => Vec::new(),
            Some(Ron::List(items)) => items,
            Some(_) => return wrong_type("lifetimes", "a list"),
        }
    };
    let mut lifetimes = BTreeMap::new();
    for item in lifetime_items {
        let mut row = open(item, "LifetimeState")?;
        let key = StatKey {
            character: row.opt_string("character")?,
            stat: row.string("stat")?,
        };
        let days = row.uint("days")?;
        row.done()?;
        if lifetimes.insert(key.clone(), days).is_some() {
            return refuse(
                RefusalClass::Malformed,
                format!("state carries a lifetime for '{}' twice", key.stat),
            );
        }
    }
    let mut triggers = BTreeMap::new();
    for item in fields.list("triggers")? {
        let mut row = open(item, "TriggerState")?;
        // `offer` and `deferred_day` are v2 fields: a v1 state
        // may lack them entirely (they read as absent).
        let offer = match row.try_take("offer") {
            None => None,
            Some(Ron::Opt(None)) => None,
            Some(Ron::Opt(Some(inner))) => {
                let mut offer_fields = open(*inner, "OfferState")?;
                let offer = OfferState {
                    offered_day: offer_fields.int("offered_day")?,
                    age_days: offer_fields.uint("age_days")?,
                };
                offer_fields.done()?;
                Some(offer)
            }
            Some(_) => return wrong_type("offer", "None or Some(OfferState(...))"),
        };
        let deferred_day = match row.try_take("deferred_day") {
            None => None,
            Some(Ron::Opt(None)) => None,
            Some(Ron::Opt(Some(inner))) => Some(crate::codec::as_i64(*inner, "deferred_day")?),
            Some(_) => return wrong_type("deferred_day", "None or Some(day)"),
        };
        let trigger = TriggerState {
            fired: row.boolean("fired")?,
            next_tick: as_u64(row.take("next_tick")?, "next_tick")?,
            above: row.boolean("above")?,
            flag_was: row.boolean("flag_was")?,
            deferred_day,
            offer,
        };
        let id = row.string("id")?;
        row.done()?;
        if triggers.insert(id.clone(), trigger).is_some() {
            return refuse(
                RefusalClass::Malformed,
                format!("state carries trigger '{id}' twice"),
            );
        }
    }
    // The rng partition is absent in state documents written
    // before streams existed; it reads as empty.
    let mut rng = BTreeMap::new();
    match fields.try_take("rng") {
        None => {}
        Some(Ron::List(items)) => {
            for item in items {
                let mut row = open(item, "RngState")?;
                let word = row.int("state")?;
                let stream = row.string("stream")?;
                row.done()?;
                if rng.insert(stream.clone(), word as u64).is_some() {
                    return refuse(
                        RefusalClass::Malformed,
                        format!("state carries stream '{stream}' twice"),
                    );
                }
            }
        }
        Some(_) => return wrong_type("rng", "a list"),
    }
    let ending = match fields.try_take("ending") {
        None => None,
        Some(value) => Some(crate::codec::as_string(value, "ending")?),
    };
    // The thread partition is absent in state documents
    // written before threads existed; it reads as empty.
    let mut threads_delivered = BTreeSet::new();
    match fields.try_take("threads_delivered") {
        None => {}
        Some(Ron::List(items)) => {
            for item in items {
                let id = crate::codec::as_string(item, "threads_delivered")?;
                if !threads_delivered.insert(id.clone()) {
                    return refuse(
                        RefusalClass::Malformed,
                        format!("state carries delivered thread '{id}' twice"),
                    );
                }
            }
        }
        Some(_) => return wrong_type("threads_delivered", "a list"),
    }
    let thread_delay_ms = match fields.try_take("thread_delay_ms") {
        None => 0,
        Some(Ron::Int(value)) => u64::try_from(value)
            .map_err(|_| Refusal::new(RefusalClass::Range, "thread_delay_ms is below 0"))?,
        Some(_) => return wrong_type("thread_delay_ms", "an integer"),
    };
    let state = GameState {
        tick: as_u64(fields.take("tick")?, "tick")?,
        day: fields.int("day")?,
        slot: fields.uint("slot")?,
        stats,
        lifetimes,
        rng,
        flags_run: flag_map(fields.list("flags_run")?, "flags_run")?,
        flags_globals: flag_map(fields.list("flags_globals")?, "flags_globals")?,
        current_scene: fields.opt_string("current_scene")?,
        ending,
        line: fields.opt_string("line")?,
        triggers,
        threads_delivered,
        thread_delay_ms,
        pending_scene: None,
        emitted: Vec::new(),
    };
    fields.done()?;
    Ok(state)
}
