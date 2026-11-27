//! The interpreter: `game_simulate` and the §G3.1 tick contract.
//!
//! A run is a pure function of (project, run seed, choice log).
//! The tick order is part of the format contract (game format
//! v2) and changes only by a format-version ruling:
//!
//! 1. Advance the clock one slot. On a day rollover, lifetimes
//!    decrement and hard-reset at zero, offers age and expire,
//!    and yesterday's deferrals clear — all before any trigger
//!    is evaluated (Changr's `controlLevelDays` runs on day
//!    advance, before event checks).
//! 2. Fire due `on_tick` triggers in (priority desc, id asc)
//!    order; each applies its effect set as one transaction (or
//!    becomes an offer, for offer-bearing triggers).
//! 3. Consume one choice-log input: an advance, an event (its
//!    `on_event` triggers fire as the transaction), an offer
//!    decision, a choice, or an interact (P2-8: a grid cell
//!    resolves, or the pair is a typed no-op). A choice resolves its authored
//!    record: the offered options (visible and enabled, in
//!    (sort value, authored ordinal) order) are recorded in the
//!    trace, the picked option must be among them, its effect
//!    set applies as one transaction, the game's alter-ego
//!    veto set applies as its own transaction when an
//!    alter-ego option was offered and a non-alter option was
//!    picked (Amendment 2 §G5), and the option's target
//!    resolves — a line target joins the scene's dialogue
//!    chain, an exit or scene target sets the pending
//!    transition for step 5.
//! 4. Re-evaluate `on_threshold` / `on_flag` triggers and the
//!    `on_event` triggers of events emitted in steps 2–3, in the
//!    same order, looping until none fire; each trigger fires at
//!    most once per tick. Then the fail-ceiling channels
//!    (P2-9) evaluate: gains from the tick's events, the
//!    ceiling check, decay — a crossing applies its adverse
//!    set as one transaction, still inside this step's order,
//!    so step 5 honours its transition.
//! 5. Apply the scene transition (the last `enter_scene` of the
//!    tick wins): the old scene's `on_exit` effect set
//!    transacts, then the new scene's `on_enter` effect set
//!    transacts, then `on_enter_scene` triggers fire. A scene
//!    naming a thread interlude (P2-7/D-461) delivers its
//!    thread's messages as one instant batch at entry — once
//!    per run — and arms the authored reply delay; the delay
//!    is consumed at step 1 of the following ticks on the step
//!    clock (`THREAD_STEP_MS` per tick, never wall time), and
//!    while it remains, a choice input in that scene refuses:
//!    the pending branch/choice fires only after it elapses.
//! 6. Ending rules evaluate in ascending priority order
//!    (priorities are unique across endings, enforced at
//!    define time); the first whose condition holds sets the
//!    run's ending, and the run stops after this tick's hash.
//! 7. Hash the state into the chain.
//!
//! Effect sets apply as transactions on a working copy: a
//! refused atom (an out-of-range write under the `refuse`
//! overflow policy) discards the whole set and is recorded as a
//! runtime event in the trace, never a partial state.

use crate::game::codec::emit_content;
use crate::game::expr::{Value as ExprValue, eval, eval_condition};
use crate::game::model::{
    Atom, ChoiceOrigin, ChoiceTarget, Game, OfferDef, Repetition, THREAD_STEP_MS, TriggerDef,
    TriggerKind,
};
use crate::game::op::{LogEntryInput, SimulateInput};
use crate::game::state::{GameState, StatKey, TriggerState, state_hash};
use crate::intake::sha256_hex;
use crate::limits::{GAME_SIM_LOG_MAX, GAME_SIM_TICKS_MAX};
use crate::model::Project;
use crate::refusal::{Refusal, RefusalClass, refuse};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// One parsed choice-log entry.
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// The tick the input lands on (1-based).
    pub tick: u64,
    /// `advance`, `event`, `offer`, `choice`, or `interact`.
    pub kind: String,
    /// The event name, trigger id, choice id, or interaction
    /// grid id, by kind.
    pub id: String,
    /// The offer decision (`accept` / `decline`), for offers;
    /// the chosen option id, for choices; the
    /// `<tool>:<node>` pair, for interacts.
    pub option: Option<String>,
}

/// The outcome of a run: the final state, the per-tick hash
/// chain, and the event trace.
#[derive(Clone, Debug)]
pub struct Run {
    /// The final state.
    pub state: GameState,
    /// One state hash per tick, in tick order.
    pub chain: Vec<String>,
    /// Human-readable runtime events, in order.
    pub events: Vec<String>,
}

/// Derives a stream's genesis state word from the run seed:
/// the first eight bytes (little-endian) of
/// `sha256("estate-rng/1:<run_seed>:<stream_id>")`. The
/// derivation is part of the interpreter contract — fixed by
/// the algorithm name, never by the project.
fn stream_seed(run_seed: i64, stream_id: &str) -> u64 {
    let digest = sha256_hex(format!("estate-rng/1:{run_seed}:{stream_id}").as_bytes());
    u64::from_str_radix(&digest[..16], 16).unwrap_or(0)
}

/// Genesis: the state before tick 1.
pub fn genesis(project: &Project, run_seed: i64) -> Result<GameState, Refusal> {
    let game = crate::game::decide::section(project)?;
    let Some(clock) = &game.clock else {
        return refuse(
            RefusalClass::Malformed,
            "the game defines no clock; run clock_define before simulating",
        );
    };
    if clock.slots.is_empty() {
        return refuse(RefusalClass::Malformed, "the game's clock has no slots");
    }
    let mut state = GameState {
        tick: 0,
        day: 1,
        slot: 0,
        ..GameState::default()
    };
    for stat in &game.stats {
        match stat.scope {
            crate::game::model::StatScope::Character => {
                for character in &game.characters {
                    let value = character
                        .initials
                        .iter()
                        .find(|row| row.stat == stat.id)
                        .map_or(stat.default, |row| row.value);
                    let key = StatKey {
                        character: Some(character.id.clone()),
                        stat: stat.id.clone(),
                    };
                    state.stats.insert(key.clone(), value);
                    arm_lifetime(game, &mut state, &key);
                }
            }
            _ => {
                let key = StatKey {
                    character: None,
                    stat: stat.id.clone(),
                };
                state.stats.insert(key.clone(), stat.default);
                arm_lifetime(game, &mut state, &key);
            }
        }
    }
    for flag in &game.flags {
        match flag.scope {
            crate::game::model::FlagScope::Globals => {
                state.flags_globals.insert(flag.id.clone(), flag.default);
            }
            crate::game::model::FlagScope::Run => {
                state.flags_run.insert(flag.id.clone(), flag.default);
            }
        }
    }
    for stream in &game.streams {
        state
            .rng
            .insert(stream.id.clone(), stream_seed(run_seed, &stream.id));
    }
    for channel in &game.fail_ceilings {
        state.fail_ceilings.insert(
            channel.id.clone(),
            crate::game::state::FailCeilingState::default(),
        );
    }
    state.current_scene = game.start_scene.clone();
    state.line = game.start_scene.as_deref().and_then(|scene| {
        game.lines_in_scene(scene)
            .first()
            .map(|line| line.id.clone())
    });
    for trigger in &game.triggers {
        let mut trigger_state = TriggerState {
            next_tick: 1,
            ..TriggerState::default()
        };
        match &trigger.kind {
            TriggerKind::OnThreshold {
                character,
                stat,
                threshold,
                ..
            } => {
                trigger_state.above =
                    stat_value(game, &state, character.as_deref(), stat) >= *threshold;
            }
            TriggerKind::OnFlag { flag, .. } => {
                trigger_state.flag_was = flag_value(game, &state, flag);
            }
            _ => {}
        }
        state.triggers.insert(trigger.id.clone(), trigger_state);
    }
    Ok(state)
}

/// Arms (or disarms) a stat instance's lifetime counter from
/// its current value: a value different from the default arms
/// the full window; the default disarms.
fn arm_lifetime(game: &Game, state: &mut GameState, key: &StatKey) {
    let Some(stat) = game.stat(&key.stat) else {
        return;
    };
    let Some(lifetime) = &stat.lifetime else {
        return;
    };
    let value = state.stats.get(key).copied().unwrap_or(stat.default);
    if value == stat.default {
        state.lifetimes.remove(key);
    } else {
        state.lifetimes.insert(key.clone(), lifetime.days);
    }
}

fn stat_value(game: &Game, state: &GameState, character: Option<&str>, stat: &str) -> i64 {
    let key = StatKey {
        character: character.map(str::to_string),
        stat: stat.to_string(),
    };
    state
        .stats
        .get(&key)
        .copied()
        .unwrap_or_else(|| game.stat(stat).map_or(0, |def| def.default))
}

fn flag_value(game: &Game, state: &GameState, flag: &str) -> bool {
    let Some(def) = game.flag(flag) else {
        return false;
    };
    match def.scope {
        crate::game::model::FlagScope::Globals => state
            .flags_globals
            .get(flag)
            .copied()
            .unwrap_or(def.default),
        crate::game::model::FlagScope::Run => {
            state.flags_run.get(flag).copied().unwrap_or(def.default)
        }
    }
}

/// Validates and indexes a choice log by tick.
pub fn parse_log(inputs: &[LogEntryInput], ticks: u64) -> Result<Vec<Option<LogEntry>>, Refusal> {
    if inputs.len() > GAME_SIM_LOG_MAX {
        return refuse(
            RefusalClass::Range,
            format!("a choice log carries at most {GAME_SIM_LOG_MAX} entries"),
        );
    }
    let mut by_tick: Vec<Option<LogEntry>> = vec![None; usize::try_from(ticks).unwrap_or(0)];
    for input in inputs {
        if input.tick == 0 || input.tick > ticks {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "choice-log entry tick {} is outside the run's 1..={ticks}",
                    input.tick
                ),
            );
        }
        match input.kind.as_str() {
            "advance" | "choice" | "event" | "interact" | "offer" => {}
            other => {
                return refuse(
                    RefusalClass::UnknownKind,
                    format!("choice-log entry kind '{other}' is not known"),
                );
            }
        }
        let slot = &mut by_tick[usize::try_from(input.tick - 1).unwrap_or(0)];
        if slot.is_some() {
            return refuse(
                RefusalClass::Malformed,
                format!("the choice log carries two inputs for tick {}", input.tick),
            );
        }
        *slot = Some(LogEntry {
            tick: input.tick,
            kind: input.kind.clone(),
            id: input.id.clone(),
            option: input.option.clone(),
        });
    }
    Ok(by_tick)
}

/// Runs ticks `state.tick + 1 ..= target`, consuming log
/// entries by absolute tick, appending hashes and events.
pub fn run_ticks(
    project: &Project,
    state: &mut GameState,
    log: &[Option<LogEntry>],
    target: u64,
    events: &mut Vec<String>,
    chain: &mut Vec<String>,
) -> Result<(), Refusal> {
    let game = crate::game::decide::section(project)?;
    while state.tick < target {
        let entry = log
            .get(usize::try_from(state.tick).unwrap_or(usize::MAX))
            .cloned()
            .flatten();
        tick(game, state, entry.as_ref(), events)?;
        chain.push(state_hash(state));
        if state.ending.is_some() {
            break;
        }
    }
    Ok(())
}

fn tick(
    game: &Game,
    state: &mut GameState,
    entry: Option<&LogEntry>,
    events: &mut Vec<String>,
) -> Result<(), Refusal> {
    let clock = game
        .clock
        .as_ref()
        .ok_or_else(|| Refusal::new(RefusalClass::Malformed, "the game defines no clock"))?;
    state.tick += 1;
    state.emitted.clear();
    // Step 1: advance the clock; day rollover drives lifetimes,
    // offer expiry, and deferral clearing (one clock).
    let slot_count = u32::try_from(clock.slots.len()).unwrap_or(u32::MAX);
    state.slot += 1;
    if state.slot >= slot_count {
        state.slot = 0;
        state.day += 1;
        decay_lifetimes(game, state, events);
        age_offers(game, state, events);
        clear_deferrals(state);
    }
    // Step 1d: the thread reply delay (P2-7/D-461) is consumed
    // on the same step clock, every tick, rollover or not.
    age_thread_delay(state, events);
    let mut fired: BTreeSet<String> = BTreeSet::new();
    // Step 2: due on_tick triggers.
    fire_tick_triggers(game, state, events, &mut fired)?;
    // Step 3: consume one input.
    if let Some(entry) = entry {
        apply_input(game, state, entry, events, &mut fired)?;
    }
    // Step 4: re-evaluate threshold/flag/emitted-event triggers.
    reevaluate(game, state, events, &mut fired)?;
    // Step 4b: fail-ceiling channels (P2-9) — after the
    // trigger re-evaluation settles, before the transition
    // step, so a crossing's adverse transaction (an
    // `enter_scene` atom included) is honoured by step 5 in
    // the same tick.
    evaluate_fail_ceilings(game, state, entry, events);
    // Step 5: scene transition — the old scene's on_exit set,
    // then the new scene's on_enter set, then on_enter_scene
    // triggers.
    if let Some(scene) = state.pending_scene.take() {
        if let Some(old) = state.current_scene.clone() {
            if let Some(set) = game.scene(&old).and_then(|def| def.on_exit.clone()) {
                apply_named_set(game, state, &set, events, &format!("scene '{old}' exit"));
            }
            // P2-12: finish-clear markers bound to the scene
            // being left clear as it is left.
            clear_markers(
                game,
                state,
                &old,
                crate::game::model::MarkerClear::Finish,
                events,
            );
        }
        state.current_scene = Some(scene.clone());
        state.line = game
            .lines_in_scene(&scene)
            .first()
            .map(|line| line.id.clone());
        events.push(format!("tick {}: entered scene '{scene}'", state.tick));
        deliver_thread_interlude(game, state, &scene, events);
        if let Some(set) = game.scene(&scene).and_then(|def| def.on_enter.clone()) {
            apply_named_set(game, state, &set, events, &format!("scene '{scene}' entry"));
        }
        // P2-12: start-clear markers bound to the scene being
        // entered clear as it is entered.
        clear_markers(
            game,
            state,
            &scene,
            crate::game::model::MarkerClear::Start,
            events,
        );
        fire_enter_scene(game, state, &scene, events, &mut fired)?;
    }
    // Step 6: ending rules, in ascending priority order; the
    // first satisfied ending ends the run (run_ticks stops
    // after this tick's hash).
    if state.ending.is_none() && !game.endings.is_empty() {
        let mut endings: Vec<&crate::game::model::EndingDef> = game.endings.iter().collect();
        endings.sort_by(|a, b| a.priority.cmp(&b.priority).then_with(|| a.id.cmp(&b.id)));
        for ending in endings {
            if eval_condition(game, state, &ending.condition)? {
                state.ending = Some(ending.id.clone());
                events.push(format!(
                    "tick {}: ending '{}' reached",
                    state.tick, ending.id
                ));
                break;
            }
        }
    }
    Ok(())
}

/// Step 1a: lifetime decay at day rollover. Each armed counter
/// decrements; at zero the stat hard-resets to its default.
fn decay_lifetimes(game: &Game, state: &mut GameState, events: &mut Vec<String>) {
    let keys: Vec<StatKey> = state.lifetimes.keys().cloned().collect();
    for key in keys {
        let Some(days) = state.lifetimes.get(&key).copied() else {
            continue;
        };
        let Some(stat) = game.stat(&key.stat) else {
            state.lifetimes.remove(&key);
            continue;
        };
        if days <= 1 {
            state.stats.insert(key.clone(), stat.default);
            state.lifetimes.remove(&key);
            events.push(format!(
                "tick {}: stat '{}' lifetime expired; reset to {}",
                state.tick, key.stat, stat.default
            ));
        } else {
            state.lifetimes.insert(key, days - 1);
        }
    }
}

/// Step 1b: offer ageing at day rollover (D-464). Each pending
/// offer ages one day on the same rollover that decrements
/// lifetimes (one clock); an offer whose age reaches its
/// authored `expires_days` is withdrawn unfired and its
/// trigger settles (a `once` trigger is spent; a repeating one
/// cools down) — so a lapsed offer can be re-offered on a
/// later matching day, never the same tick it died.
fn age_offers(game: &Game, state: &mut GameState, events: &mut Vec<String>) {
    let pending: Vec<String> = state
        .triggers
        .iter()
        .filter(|(_, trigger)| trigger.offer.is_some())
        .map(|(id, _)| id.clone())
        .collect();
    for id in pending {
        let Some(def) = game.trigger(&id) else {
            continue;
        };
        let Some(offer_def) = def.offer else {
            continue;
        };
        let expired = {
            let Some(trigger) = state.triggers.get_mut(&id) else {
                continue;
            };
            let Some(offer) = trigger.offer.as_mut() else {
                continue;
            };
            offer.age_days += 1;
            offer_def
                .expires_days
                .is_some_and(|limit| offer.age_days >= limit)
        };
        if expired {
            events.push(format!(
                "tick {}: offer from trigger '{}' expired unfired",
                state.tick, id
            ));
            settle_trigger(state, def);
        }
    }
}

/// Step 1d: consume one step of the pending thread reply
/// delay (P2-7/D-461). The delay is authored data in
/// milliseconds; the step clock consumes `THREAD_STEP_MS`
/// per tick, so the wait is a pure function of the run —
/// never of wall time.
fn age_thread_delay(state: &mut GameState, events: &mut Vec<String>) {
    if state.thread_delay_ms == 0 {
        return;
    }
    state.thread_delay_ms = state.thread_delay_ms.saturating_sub(THREAD_STEP_MS);
    if state.thread_delay_ms == 0 {
        events.push(format!("tick {}: thread reply delay elapsed", state.tick));
    }
}

/// Step 5 (thread interlude, P2-7/D-461): a scene naming an
/// interlude delivers its thread's messages as ONE instant
/// batch, in authored order, and arms the authored reply
/// delay. Delivery is fire-once per run: re-entering the
/// scene after the thread was delivered delivers nothing and
/// re-arms nothing.
fn deliver_thread_interlude(
    game: &Game,
    state: &mut GameState,
    scene: &str,
    events: &mut Vec<String>,
) {
    let Some(interlude) = game
        .scene(scene)
        .and_then(|def| def.thread_interlude.as_ref())
    else {
        return;
    };
    if state.threads_delivered.contains(&interlude.thread) {
        return;
    }
    let Some(thread) = game.thread(&interlude.thread) else {
        events.push(format!(
            "tick {}: scene '{scene}' names missing thread '{}'",
            state.tick, interlude.thread
        ));
        return;
    };
    state.threads_delivered.insert(thread.id.clone());
    let ids: Vec<&str> = thread.messages.iter().map(|row| row.id.as_str()).collect();
    events.push(format!(
        "tick {}: thread '{}' delivered [{}]",
        state.tick,
        thread.id,
        ids.join(", ")
    ));
    if interlude.reply_delay_ms > 0 {
        state.thread_delay_ms = interlude.reply_delay_ms;
        events.push(format!(
            "tick {}: thread '{}' reply delay {} ms pending",
            state.tick, thread.id, interlude.reply_delay_ms
        ));
    }
}

/// The `thread_read` verb body (P2-7/D-461, read-only): the
/// thread's messages in authored order, each with its speaker
/// and content key — the prose itself stays in `content.ron`.
pub fn thread_messages(project: &Project, thread_id: &str) -> Result<Value, Refusal> {
    let game = crate::game::decide::section(project)?;
    let Some(thread) = game.thread(thread_id) else {
        return refuse(
            RefusalClass::UnknownTarget,
            format!("thread '{thread_id}' does not exist"),
        );
    };
    Ok(json!({
        "messages": thread.messages.iter().map(|row| json!({
            "at": row.at,
            "id": row.id,
            "line": row.line,
            "speaker": row.speaker,
        })).collect::<Vec<_>>(),
        "participants": thread.participants,
        "result": "ok",
        "thread": thread.id,
    }))
}

/// Step 1c: deferrals are per-day; a new day clears them.
fn clear_deferrals(state: &mut GameState) {
    for trigger in state.triggers.values_mut() {
        if trigger.deferred_day.is_some_and(|day| day < state.day) {
            trigger.deferred_day = None;
        }
    }
}

/// Trigger firing order: priority descending, id ascending.
fn ordered(game: &Game) -> Vec<&TriggerDef> {
    let mut triggers: Vec<&TriggerDef> = game.triggers.iter().collect();
    triggers.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
    triggers
}

fn eligible(state: &GameState, def: &TriggerDef) -> bool {
    let Some(trigger) = state.triggers.get(&def.id) else {
        return false;
    };
    !trigger.fired
        && trigger.offer.is_none()
        && state.tick >= trigger.next_tick
        && trigger.deferred_day != Some(state.day)
}

fn gate_passes(game: &Game, state: &GameState, def: &TriggerDef) -> Result<bool, Refusal> {
    match &def.gate {
        None => Ok(true),
        Some(condition) => eval_condition(game, state, condition),
    }
}

/// Fires one trigger: applies its effect set as one transaction
/// and does the repetition/cooldown bookkeeping. Offer-bearing
/// triggers become offers instead (D-464): the effect set runs
/// only if the run accepts the offer.
fn fire(
    game: &Game,
    state: &mut GameState,
    def: &TriggerDef,
    events: &mut Vec<String>,
    fired: &mut BTreeSet<String>,
) -> Result<(), Refusal> {
    fired.insert(def.id.clone());
    if let Some(offer) = &def.offer {
        return make_offer(state, def, *offer, events);
    }
    apply_effect_set(game, state, def, events);
    Ok(())
}

fn make_offer(
    state: &mut GameState,
    def: &TriggerDef,
    _offer: OfferDef,
    events: &mut Vec<String>,
) -> Result<(), Refusal> {
    let trigger = state
        .triggers
        .get_mut(&def.id)
        .ok_or_else(|| Refusal::new(RefusalClass::UnknownTarget, "trigger state missing"))?;
    trigger.offer = Some(crate::game::state::OfferState {
        offered_day: state.day,
        age_days: 0,
    });
    events.push(format!("tick {}: trigger '{}' offered", state.tick, def.id));
    Ok(())
}

/// The bookkeeping after a trigger's effect set has run (or its
/// offer was accepted/expired): `once` triggers are spent;
/// cooldowns push the next eligible tick out.
fn settle_trigger(state: &mut GameState, def: &TriggerDef) {
    if let Some(trigger) = state.triggers.get_mut(&def.id) {
        trigger.offer = None;
        trigger.deferred_day = None;
        match def.repetition {
            Repetition::Once => trigger.fired = true,
            Repetition::Repeat => {
                trigger.next_tick = state.tick + u64::from(def.cooldown_ticks) + 1;
            }
        }
    }
}

/// Applies an effect set as one transaction on a working copy.
/// A refused atom discards the whole set (a runtime event in
/// the trace, never a partial state).
fn apply_effect_set(
    game: &Game,
    state: &mut GameState,
    def: &TriggerDef,
    events: &mut Vec<String>,
) {
    let Some(set) = game.effect_set(&def.effect_set) else {
        events.push(format!(
            "tick {}: trigger '{}' names missing effect set '{}'",
            state.tick, def.id, def.effect_set
        ));
        return;
    };
    let mut working = state.clone();
    let mut notes = Vec::new();
    match apply_atoms(game, &mut working, &set.atoms, &mut notes, 0) {
        Ok(()) => {
            *state = working;
            // Draw notes commit with the transaction; a refused
            // set's notes die with its working copy, like its
            // state changes.
            events.extend(notes);
            events.push(format!(
                "tick {}: trigger '{}' fired effect set '{}'",
                state.tick, def.id, set.id
            ));
        }
        Err(reason) => {
            events.push(format!(
                "tick {}: effect set '{}' refused ({reason}); no state changed",
                state.tick, set.id
            ));
        }
    }
    settle_trigger(state, def);
}

/// Applies atoms in order on behalf of a named source (used
/// for input events whose triggers are the transaction).
/// `notes` collects trace lines (table draws) that commit only
/// if the whole transaction does.
fn apply_atoms(
    game: &Game,
    state: &mut GameState,
    atoms: &[Atom],
    notes: &mut Vec<String>,
    depth: u32,
) -> Result<(), String> {
    for atom in atoms {
        apply_atom(game, state, atom, notes, depth)?;
    }
    Ok(())
}

fn apply_atom(
    game: &Game,
    state: &mut GameState,
    atom: &Atom,
    notes: &mut Vec<String>,
    depth: u32,
) -> Result<(), String> {
    match atom {
        Atom::EmitEvent { event } => {
            state.emitted.push(event.clone());
            Ok(())
        }
        Atom::EnterScene { scene } => {
            state.pending_scene = Some(scene.clone());
            Ok(())
        }
        Atom::FlagClear { flag } => {
            set_flag(game, state, flag, false);
            Ok(())
        }
        Atom::FlagSet { flag } => {
            set_flag(game, state, flag, true);
            Ok(())
        }
        Atom::MarkNew { marker } => {
            let Some(def) = game.marker(marker) else {
                return Err(format!("unknown marker '{marker}'"));
            };
            let flagged = match def.scope {
                crate::game::model::FlagScope::Globals => {
                    state.markers_globals.insert(marker.clone())
                }
                crate::game::model::FlagScope::Run => state.markers_run.insert(marker.clone()),
            };
            if flagged {
                notes.push(format!(
                    "tick {}: marker '{marker}' flagged new",
                    state.tick
                ));
            }
            Ok(())
        }
        Atom::RollTable { table } => draw_table(game, state, table, notes, depth),
        Atom::StatAdd {
            character,
            stat,
            amount,
        } => {
            let amount = eval_int(game, state, amount)?;
            let current = stat_value(game, state, character.as_deref(), stat);
            write_stat(
                game,
                state,
                character.as_deref(),
                stat,
                current.saturating_add(amount),
            )
        }
        Atom::StatSet {
            character,
            stat,
            value,
        } => {
            let value = eval_int(game, state, value)?;
            write_stat(game, state, character.as_deref(), stat, value)
        }
    }
}

/// One step of a stream's SplitMix64 (`estate-rng/1`): the
/// state word advances by the golden gamma and the output is
/// the new word, mixed. A draw consumes exactly one step,
/// regardless of the weights it is drawn against — that is
/// what makes stream consumption weight-independent.
fn rng_next(state: &mut GameState, stream: &str) -> u64 {
    let word = state.rng.entry(stream.to_string()).or_insert(0);
    *word = word.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *word;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Draws on a weighted table: evaluates outcome weights
/// against the drawing state (D-469 — static weights are
/// already values; expression weights evaluate now), excludes
/// gated-out and non-positive outcomes, consumes exactly one
/// step of the table's stream, and applies the winning
/// outcome's effect set inside the same transaction. When no
/// outcome is eligible the draw is a trace event, never a
/// state change (and no stream step is consumed — there was
/// no draw).
fn draw_table(
    game: &Game,
    state: &mut GameState,
    table_id: &str,
    notes: &mut Vec<String>,
    depth: u32,
) -> Result<(), String> {
    if depth >= crate::limits::GAME_TABLE_NESTING_MAX {
        return Err(format!(
            "table draw nesting exceeds {}",
            crate::limits::GAME_TABLE_NESTING_MAX
        ));
    }
    let Some(table) = game.table(table_id) else {
        return Err(format!("unknown table '{table_id}'"));
    };
    let mut eligible: Vec<(&crate::game::model::TableOutcome, u128)> = Vec::new();
    let mut total: u128 = 0;
    for outcome in &table.outcomes {
        if let Some(gate) = &outcome.gate {
            match eval_condition(game, state, gate) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(refusal) => return Err(refusal.message),
            }
        }
        let weight = match &outcome.weight {
            crate::game::model::TableWeight::Expr(expr) => eval_int(game, state, expr)?,
            crate::game::model::TableWeight::Static(weight) => *weight,
        };
        if weight > 0 {
            total += weight as u128;
            eligible.push((outcome, weight as u128));
        }
    }
    if eligible.is_empty() {
        notes.push(format!(
            "tick {}: table '{}' drew nothing (no eligible outcomes)",
            state.tick, table.id
        ));
        return Ok(());
    }
    let roll = u128::from(rng_next(state, &table.stream)) % total;
    let mut cumulative: u128 = 0;
    for (outcome, weight) in &eligible {
        cumulative += weight;
        if roll < cumulative {
            notes.push(format!(
                "tick {}: table '{}' drew outcome '{}'",
                state.tick, table.id, outcome.id
            ));
            let Some(set) = game.effect_set(&outcome.effect_set) else {
                return Err(format!(
                    "table '{}' outcome '{}' names missing effect set '{}'",
                    table.id, outcome.id, outcome.effect_set
                ));
            };
            return apply_atoms(game, state, &set.atoms, notes, depth + 1);
        }
    }
    Ok(())
}

fn eval_int(
    game: &Game,
    state: &GameState,
    expr: &crate::game::model::Expr,
) -> Result<i64, String> {
    match eval(game, state, expr) {
        Ok(ExprValue::Int(value)) => Ok(value),
        Ok(ExprValue::Bool(_)) => Err("an effect amount evaluated to a boolean".to_string()),
        Err(refusal) => Err(refusal.message.clone()),
    }
}

fn set_flag(game: &Game, state: &mut GameState, flag: &str, value: bool) {
    let Some(def) = game.flag(flag) else {
        return;
    };
    match def.scope {
        crate::game::model::FlagScope::Globals => {
            state.flags_globals.insert(flag.to_string(), value);
        }
        crate::game::model::FlagScope::Run => {
            state.flags_run.insert(flag.to_string(), value);
        }
    }
}

/// Writes a stat under its overflow policy and (re)arms its
/// lifetime: any authored write that leaves the value different
/// from the default arms the full window; writing the default
/// disarms it.
fn write_stat(
    game: &Game,
    state: &mut GameState,
    character: Option<&str>,
    stat_id: &str,
    value: i64,
) -> Result<(), String> {
    let Some(def) = game.stat(stat_id) else {
        return Err(format!("unknown stat '{stat_id}'"));
    };
    let value = match def.overflow {
        crate::game::model::Overflow::Clamp => value.clamp(def.min, def.max),
        crate::game::model::Overflow::Refuse => {
            if value < def.min || value > def.max {
                return Err(format!(
                    "write of {value} to stat '{stat_id}' is outside [{}, {}] under the refuse policy",
                    def.min, def.max
                ));
            }
            value
        }
    };
    let key = StatKey {
        character: character.map(str::to_string),
        stat: stat_id.to_string(),
    };
    state.stats.insert(key.clone(), value);
    arm_lifetime(game, state, &key);
    Ok(())
}

/// Step 2: `on_tick` triggers whose pattern matches the new
/// clock position.
fn fire_tick_triggers(
    game: &Game,
    state: &mut GameState,
    events: &mut Vec<String>,
    fired: &mut BTreeSet<String>,
) -> Result<(), Refusal> {
    let clock = game
        .clock
        .as_ref()
        .ok_or_else(|| Refusal::new(RefusalClass::Malformed, "the game defines no clock"))?;
    let slot_id = clock
        .slots
        .get(usize::try_from(state.slot).unwrap_or(usize::MAX))
        .cloned()
        .unwrap_or_default();
    for def in ordered(game) {
        let TriggerKind::OnTick {
            slots,
            weekdays,
            day_modulo,
            exact_day,
        } = &def.kind
        else {
            continue;
        };
        if fired.contains(&def.id) || !eligible(state, def) {
            continue;
        }
        let matches = slots.as_ref().is_none_or(|set| set.contains(&slot_id))
            && weekdays.as_ref().is_none_or(|set| {
                clock.days_per_week.is_some_and(|cycle| {
                    let weekday = u32::try_from((state.day - 1).rem_euclid(i64::from(cycle)) + 1)
                        .unwrap_or(0);
                    set.contains(&weekday)
                })
            })
            && day_modulo.is_none_or(|(period, remainder)| {
                period > 0 && state.day.rem_euclid(i64::from(period)) == i64::from(remainder)
            })
            && exact_day.is_none_or(|day| day == state.day);
        if matches && gate_passes(game, state, def)? {
            fire(game, state, def, events, fired)?;
        }
    }
    Ok(())
}

/// Step 3: one choice-log input.
fn apply_input(
    game: &Game,
    state: &mut GameState,
    entry: &LogEntry,
    events: &mut Vec<String>,
    fired: &mut BTreeSet<String>,
) -> Result<(), Refusal> {
    match entry.kind.as_str() {
        "advance" => {
            let next = state
                .line
                .as_deref()
                .and_then(|id| game.dialogue_line(id))
                .and_then(|line| line.next.clone());
            state.line = next;
            Ok(())
        }
        "event" => {
            for def in ordered(game) {
                let TriggerKind::OnEvent { event } = &def.kind else {
                    continue;
                };
                if *event != entry.id || fired.contains(&def.id) || !eligible(state, def) {
                    continue;
                }
                if gate_passes(game, state, def)? {
                    fire(game, state, def, events, fired)?;
                }
            }
            Ok(())
        }
        "offer" => apply_offer_input(game, state, entry, events),
        "choice" => apply_choice_input(game, state, entry, events),
        "interact" => apply_interact_input(game, state, entry, events),
        _ => refuse(
            RefusalClass::UnknownKind,
            format!("choice-log entry kind '{}' is not known", entry.kind),
        ),
    }
}

/// Applies a named effect set as one transaction on a
/// working copy (the `apply_effect_set` discipline, without
/// trigger bookkeeping): a refused atom discards the whole
/// set and is recorded as a runtime event. Used by choice
/// options, the veto rule, scene exits, and scene entry/exit
/// sets.
fn apply_named_set(
    game: &Game,
    state: &mut GameState,
    set_id: &str,
    events: &mut Vec<String>,
    why: &str,
) {
    let Some(set) = game.effect_set(set_id) else {
        events.push(format!(
            "tick {}: {why} names missing effect set '{set_id}'",
            state.tick
        ));
        return;
    };
    let mut working = state.clone();
    let mut notes = Vec::new();
    match apply_atoms(game, &mut working, &set.atoms, &mut notes, 0) {
        Ok(()) => {
            *state = working;
            events.extend(notes);
            events.push(format!(
                "tick {}: {why} applied effect set '{}'",
                state.tick, set.id
            ));
        }
        Err(reason) => {
            events.push(format!(
                "tick {}: effect set '{}' refused ({reason}); no state changed",
                state.tick, set.id
            ));
        }
    }
}

/// Step 3 (choices): the input names an authored choice and
/// the option the player picked. The choice must belong to
/// the scene the run is in; the presented options are the
/// visible ones, in (sort value, authored ordinal) order, and
/// the picked option must be offered — visible AND enabled —
/// or the run refuses (`TG-GAME-GRAPH`): the log and the game
/// have diverged. The picked option's effect set applies as
/// one transaction; when an alter-ego option was offered and
/// the pick is not one, the game's veto set applies as its
/// own transaction (Amendment 2 §G5). The option's target
/// then resolves: a line target moves the run's line, an exit
/// target applies the exit's own set (when its condition
/// holds) and sets the pending transition, a scene target
/// sets the pending transition directly.
fn apply_choice_input(
    game: &Game,
    state: &mut GameState,
    entry: &LogEntry,
    events: &mut Vec<String>,
) -> Result<(), Refusal> {
    let Some(choice) = game.choice(&entry.id) else {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "input picks choice '{}', but the game defines no such choice",
                entry.id
            ),
        );
    };
    let Some(option_id) = &entry.option else {
        return refuse(
            RefusalClass::Malformed,
            "a choice input must carry an option (the chosen option id)",
        );
    };
    let Some(option) = choice.options.iter().find(|row| &row.id == option_id) else {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "input picks option '{option_id}', but choice '{}' defines no such option",
                entry.id
            ),
        );
    };
    if state.current_scene.as_deref() != Some(choice.scene.as_str()) {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "input picks choice '{}' of scene '{}', but the run is in scene '{}'",
                entry.id,
                choice.scene,
                state.current_scene.as_deref().unwrap_or("<none>")
            ),
        );
    }
    // A thread interlude's reply delay gates the scene's
    // choices (P2-7/D-461): the pending branch/choice fires
    // only after the authored delay has elapsed on the step
    // clock. A thread choice is otherwise an ordinary choice:
    // it routes through this same machinery (conditions,
    // presented order, effect transaction, veto).
    if state.thread_delay_ms > 0
        && game
            .scene(&choice.scene)
            .is_some_and(|def| def.thread_interlude.is_some())
    {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "input picks choice '{}', but the thread reply delay has {} ms left on the step clock",
                entry.id, state.thread_delay_ms
            ),
        );
    }
    // The presented order: visible options by (sort value,
    // authored ordinal); sort defaults to 0.
    let mut presented: Vec<(&crate::game::model::ChoiceOption, i64, usize, bool)> = Vec::new();
    for (ordinal, row) in choice.options.iter().enumerate() {
        let visible = match &row.visible_if {
            None => true,
            Some(condition) => eval_condition(game, state, condition)?,
        };
        if !visible {
            continue;
        }
        let enabled = match &row.enabled_if {
            None => true,
            Some(condition) => eval_condition(game, state, condition)?,
        };
        let sort = match &row.sort {
            None => 0,
            Some(expr) => eval_int(game, state, expr)
                .map_err(|message| Refusal::new(RefusalClass::GameExpr, message))?,
        };
        presented.push((row, sort, ordinal, enabled));
    }
    presented.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.2.cmp(&b.2)));
    let offered_ids: Vec<&str> = presented.iter().map(|row| row.0.id.as_str()).collect();
    events.push(format!(
        "tick {}: choice '{}' presented [{}]",
        state.tick,
        choice.id,
        offered_ids.join(", ")
    ));
    let picked = presented.iter().find(|row| row.0.id == *option_id).copied();
    match picked {
        Some((_, _, _, true)) => {}
        Some((_, _, _, false)) => {
            return refuse(
                RefusalClass::GameGraph,
                format!(
                    "input picks option '{option_id}' of choice '{}', but the option is disabled in this state",
                    entry.id
                ),
            );
        }
        None => {
            return refuse(
                RefusalClass::GameGraph,
                format!(
                    "input picks option '{option_id}' of choice '{}', but the option is not offered in this state",
                    entry.id
                ),
            );
        }
    }
    let alter_offered = presented
        .iter()
        .any(|row| row.3 && row.0.origin == ChoiceOrigin::AlterEgo);
    if let Some(set) = &option.effect_set {
        apply_named_set(
            game,
            state,
            set,
            events,
            &format!("choice '{}' option '{}'", choice.id, option.id),
        );
    }
    if alter_offered && option.origin != ChoiceOrigin::AlterEgo {
        if let Some(veto) = &game.alter_ego_veto {
            apply_named_set(game, state, veto, events, "the alter-ego veto");
        }
    }
    match &option.target {
        ChoiceTarget::EndingCheck => {}
        ChoiceTarget::Line(line) => {
            state.line = Some(line.clone());
        }
        ChoiceTarget::Scene(scene) => {
            state.pending_scene = Some(scene.clone());
        }
        ChoiceTarget::Exit(exit_id) => {
            let Some(exit) = game.scene_exit(&choice.scene, exit_id) else {
                return refuse(
                    RefusalClass::GameGraph,
                    format!(
                        "choice '{}' option '{}' targets exit '{exit_id}', which scene '{}' does not define",
                        choice.id, option.id, choice.scene
                    ),
                );
            };
            let gated = match &exit.condition {
                None => false,
                Some(condition) => !eval_condition(game, state, condition)?,
            };
            if gated {
                events.push(format!(
                    "tick {}: exit '{exit_id}' is gated by its condition; no transition",
                    state.tick
                ));
            } else {
                if let Some(set) = &exit.effect_set {
                    apply_named_set(game, state, set, events, &format!("exit '{exit_id}'"));
                }
                state.pending_scene = Some(exit.target.clone());
            }
        }
    }
    Ok(())
}

/// Step 3 (interactions, P2-8): the input names an
/// interaction grid and a `<tool>:<node>` pair. A grid the
/// game does not define refuses the run (`TG-GAME-GRAPH`):
/// the log and the game have diverged. A pair the grid does
/// not record — or a grid played outside its own scene — is
/// a typed no-op: a trace event, never a state change, so an
/// exploratory touch costs the run nothing. A recorded cell
/// resolves in the trace (with its camera preset when it
/// carries one) and its response set applies as one
/// transaction, like a choice option's set.
fn apply_interact_input(
    game: &Game,
    state: &mut GameState,
    entry: &LogEntry,
    events: &mut Vec<String>,
) -> Result<(), Refusal> {
    let Some(grid) = game.interaction(&entry.id) else {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "input interacts through grid '{}', but the game defines no such grid",
                entry.id
            ),
        );
    };
    let Some(pair) = &entry.option else {
        return refuse(
            RefusalClass::Malformed,
            "an interact input must carry an option (the '<tool>:<node>' pair)",
        );
    };
    let Some((tool, node)) = pair.split_once(':') else {
        return refuse(
            RefusalClass::Malformed,
            format!("interact pair '{pair}' is not of the form '<tool>:<node>'"),
        );
    };
    if state.current_scene.as_deref() != Some(grid.scene.as_str()) {
        events.push(format!(
            "tick {}: interact '{}' {} x {} — the grid belongs to scene '{}'; no-op",
            state.tick, grid.id, tool, node, grid.scene
        ));
        return Ok(());
    }
    let Some(cell) = grid.cell(tool, node) else {
        events.push(format!(
            "tick {}: interact '{}' {} x {} — no record; no-op",
            state.tick, grid.id, tool, node
        ));
        return Ok(());
    };
    match &cell.camera {
        Some(camera) => events.push(format!(
            "tick {}: interact '{}' {} x {} resolved; camera preset '{}'",
            state.tick, grid.id, tool, node, camera
        )),
        None => events.push(format!(
            "tick {}: interact '{}' {} x {} resolved",
            state.tick, grid.id, tool, node
        )),
    }
    if let Some(set) = &cell.response {
        apply_named_set(
            game,
            state,
            set,
            events,
            &format!("interaction '{}' cell {} x {}", grid.id, tool, node),
        );
    }
    Ok(())
}

/// Step 3 (offer decisions, D-464): the input names a trigger
/// whose offer is pending and decides it. Accepting applies
/// the trigger's effect set as the step's transaction and
/// settles the trigger; declining withdraws the offer and
/// defers the trigger for the rest of the day (it re-offers on
/// the next matching day). Deciding an offer that is not
/// pending refuses the run (`TG-AUTH-CONFLICT`): the log and
/// the run have diverged.
fn apply_offer_input(
    game: &Game,
    state: &mut GameState,
    entry: &LogEntry,
    events: &mut Vec<String>,
) -> Result<(), Refusal> {
    let Some(def) = game.trigger(&entry.id) else {
        return refuse(
            RefusalClass::Conflict,
            format!(
                "input decides offer '{}', but the game defines no such trigger",
                entry.id
            ),
        );
    };
    let pending = state
        .triggers
        .get(&def.id)
        .is_some_and(|trigger| trigger.offer.is_some());
    if !pending {
        return refuse(
            RefusalClass::Conflict,
            format!(
                "input decides offer '{}', but no offer from it is pending",
                def.id
            ),
        );
    }
    match entry.option.as_deref() {
        Some("accept") => {
            events.push(format!(
                "tick {}: offer from trigger '{}' accepted",
                state.tick, def.id
            ));
            apply_effect_set(game, state, def, events);
            Ok(())
        }
        Some("decline") => {
            if let Some(trigger) = state.triggers.get_mut(&def.id) {
                trigger.offer = None;
                trigger.deferred_day = Some(state.day);
            }
            events.push(format!(
                "tick {}: offer from trigger '{}' declined; deferred for the rest of day {}",
                state.tick, def.id, state.day
            ));
            Ok(())
        }
        Some(other) => refuse(
            RefusalClass::Malformed,
            format!("offer decision '{other}' is not known; use accept or decline"),
        ),
        None => refuse(
            RefusalClass::Malformed,
            "an offer input must carry an option (accept or decline)",
        ),
    }
}

/// Step 4: threshold/flag edges plus emitted events, looping
/// until a full pass fires nothing. Each trigger fires at most
/// once per tick; edge trackers track even when a trigger
/// cannot fire (the crossing is consumed).
fn reevaluate(
    game: &Game,
    state: &mut GameState,
    events: &mut Vec<String>,
    fired: &mut BTreeSet<String>,
) -> Result<(), Refusal> {
    loop {
        let mut progress = false;
        let emitted: Vec<String> = state.emitted.clone();
        for def in ordered(game) {
            if fired.contains(&def.id) {
                continue;
            }
            let due = match &def.kind {
                TriggerKind::OnThreshold {
                    character,
                    stat,
                    threshold,
                    direction,
                } => {
                    let above = stat_value(game, state, character.as_deref(), stat) >= *threshold;
                    let was = state
                        .triggers
                        .get(&def.id)
                        .is_some_and(|trigger| trigger.above);
                    if let Some(trigger) = state.triggers.get_mut(&def.id) {
                        trigger.above = above;
                    }
                    match direction {
                        crate::game::model::Direction::Rising => !was && above,
                        crate::game::model::Direction::Falling => was && !above,
                    }
                }
                TriggerKind::OnFlag { flag, edge } => {
                    let now = flag_value(game, state, flag);
                    let was = state
                        .triggers
                        .get(&def.id)
                        .is_some_and(|trigger| trigger.flag_was);
                    if let Some(trigger) = state.triggers.get_mut(&def.id) {
                        trigger.flag_was = now;
                    }
                    match edge {
                        crate::game::model::FlagEdge::Both => now != was,
                        crate::game::model::FlagEdge::Falling => was && !now,
                        crate::game::model::FlagEdge::Rising => !was && now,
                    }
                }
                TriggerKind::OnEvent { event } => emitted.contains(event),
                _ => false,
            };
            if due && eligible(state, def) && gate_passes(game, state, def)? {
                fire(game, state, def, events, fired)?;
                progress = true;
            }
        }
        if !progress {
            return Ok(());
        }
    }
}

/// Step 4b: fail-ceiling channels (P2-9). For each channel
/// guarding the scene the run is in, in id order: the gauge
/// accumulates the tick's gains (the step-3 event input and
/// every event emitted in steps 2–4 feed it, once per gain
/// row), then the ceiling check — a gauge at or above its
/// ceiling crosses, latching the channel and applying its
/// adverse set as one transaction — then, only when the
/// channel did not cross, decay toward zero at the authored
/// rate. A channel outside its scene neither accumulates nor
/// decays, and a latched channel never fires again: the
/// adverse transaction is exactly-once. Events the adverse
/// set emits do not re-enter step 4 (it has settled); like
/// step-5 emissions, they are trace-only.
fn evaluate_fail_ceilings(
    game: &Game,
    state: &mut GameState,
    entry: Option<&LogEntry>,
    events: &mut Vec<String>,
) {
    if game.fail_ceilings.is_empty() {
        return;
    }
    let mut tick_events: Vec<&str> = state.emitted.iter().map(String::as_str).collect();
    if let Some(entry) = entry
        && entry.kind == "event"
    {
        tick_events.push(entry.id.as_str());
    }
    let mut crossed: Vec<(String, String, i64, i64)> = Vec::new();
    for def in &game.fail_ceilings {
        if state.current_scene.as_deref() != Some(def.scene.as_str()) {
            continue;
        }
        let channel = state.fail_ceilings.entry(def.id.clone()).or_default();
        if channel.fired {
            continue;
        }
        let mut gained: i64 = 0;
        for gain in &def.gains {
            if tick_events.contains(&gain.event.as_str()) {
                gained = gained.saturating_add(gain.amount);
            }
        }
        channel.value = channel.value.saturating_add(gained);
        if channel.value >= def.ceiling {
            channel.fired = true;
            crossed.push((
                def.id.clone(),
                def.adverse.clone(),
                channel.value,
                def.ceiling,
            ));
        } else if def.decay_per_tick > 0 && channel.value > 0 {
            channel.value = (channel.value - def.decay_per_tick).max(0);
        }
    }
    for (id, adverse, value, ceiling) in crossed {
        events.push(format!(
            "tick {}: fail-ceiling '{id}' crossed ({value} >= {ceiling})",
            state.tick
        ));
        apply_named_set(
            game,
            state,
            &adverse,
            events,
            &format!("fail-ceiling '{id}'"),
        );
    }
}

/// Step 5 (marker lifecycle, P2-12): clears the flagged
/// markers bound to `scene` at the named point — start-clear
/// on entry, finish-clear on exit. Clearing a marker that is
/// not flagged is silent (the lifecycle is idempotent); a
/// clear that removes a flag is a trace event.
fn clear_markers(
    game: &Game,
    state: &mut GameState,
    scene: &str,
    point: crate::game::model::MarkerClear,
    events: &mut Vec<String>,
) {
    let point_name = match point {
        crate::game::model::MarkerClear::Finish => "finish",
        crate::game::model::MarkerClear::Start => "start",
    };
    for marker in &game.markers {
        if marker.scene != scene || marker.clear != point {
            continue;
        }
        let removed = match marker.scope {
            crate::game::model::FlagScope::Globals => state.markers_globals.remove(&marker.id),
            crate::game::model::FlagScope::Run => state.markers_run.remove(&marker.id),
        };
        if removed {
            events.push(format!(
                "tick {}: marker '{}' cleared ({point_name} at '{scene}')",
                state.tick, marker.id
            ));
        }
    }
}

/// Step 5b: `on_enter_scene` triggers for the scene just
/// entered. A transition they cause applies next tick (no
/// cascade inside a tick).
fn fire_enter_scene(
    game: &Game,
    state: &mut GameState,
    scene: &str,
    events: &mut Vec<String>,
    fired: &mut BTreeSet<String>,
) -> Result<(), Refusal> {
    for def in ordered(game) {
        let TriggerKind::OnEnterScene { scene: target } = &def.kind else {
            continue;
        };
        if target != scene || fired.contains(&def.id) || !eligible(state, def) {
            continue;
        }
        if gate_passes(game, state, def)? {
            fire(game, state, def, events, fired)?;
        }
    }
    // A transition caused here waits for the next tick.
    Ok(())
}

/// The `game_simulate` verb body: genesis plus a bounded run.
pub fn run_simulation(
    project: &Project,
    project_sha256: &str,
    inputs: &SimulateInput,
) -> Result<Value, Refusal> {
    if let Some(pin) = &inputs.project_sha256
        && pin != project_sha256
    {
        return refuse(
            RefusalClass::Conflict,
            format!(
                "the choice log binds project {pin}, but the loaded project is {project_sha256}"
            ),
        );
    }
    if inputs.ticks > GAME_SIM_TICKS_MAX {
        return refuse(
            RefusalClass::Range,
            format!("a simulation runs at most {GAME_SIM_TICKS_MAX} ticks"),
        );
    }
    let mut state = genesis(project, inputs.run_seed)?;
    let content_sha256 = if project.content.is_empty() {
        None
    } else {
        let locale = project
            .content_locale
            .as_deref()
            .unwrap_or(crate::game::model::CONTENT_LOCALE_CANONICAL);
        Some(sha256_hex(
            emit_content(&project.content, locale).as_bytes(),
        ))
    };
    // The run's genesis hash binds the sidecar's content hash —
    // not the prose bytes in the project — so two runs over
    // different prose are different evidence, while the project
    // and its saves never churn (D-468).
    let genesis_hash = state_hash(&state);
    let initial_hash = sha256_hex(
        format!(
            "{}:{genesis_hash}",
            content_sha256.as_deref().unwrap_or("no-content")
        )
        .as_bytes(),
    );
    let log = parse_log(&inputs.choice_log, inputs.ticks)?;
    let mut events = Vec::new();
    let mut chain = Vec::new();
    run_ticks(
        project,
        &mut state,
        &log,
        inputs.ticks,
        &mut events,
        &mut chain,
    )?;
    let game = crate::game::decide::section(project)?;
    let slot_id = game
        .clock
        .as_ref()
        .and_then(|clock| {
            clock
                .slots
                .get(usize::try_from(state.slot).unwrap_or(usize::MAX))
        })
        .cloned();
    Ok(json!({
        "chain": chain,
        "content_sha256": content_sha256,
        "ending": state.ending.clone().map_or(Value::Null, Value::String),
        "events": events,
        "final": {
            "day": state.day,
            "line": state.line,
            "scene": state.current_scene,
            "slot": state.slot,
            "slot_id": slot_id,
        },
        "initial_hash": initial_hash,
        "result": "ok",
        "run_seed": inputs.run_seed,
        "ticks": inputs.ticks,
    }))
}

// ----------------------------------------------------------------
// estate-rng/1 golden vectors (runtime plan Rung R0, 2026-11-26).
//
// These vectors are the executable form of the format registry
// record in crates/torogao-format/FORMATS.md. They were computed
// independently of this implementation (SHA-256 + the SplitMix64
// step, from the written spec) and pinned here: any drift in the
// seed derivation or the step function — the two halves of the
// `estate-rng/1` contract — fails this gate instead of silently
// changing what a saved run replays to. The raw-from-zero vector
// additionally pins the bare SplitMix64 step against the
// algorithm's published first outputs for state 0.
// ----------------------------------------------------------------
#[cfg(test)]
mod rng_golden_tests {
    use super::{rng_next, stream_seed};
    use crate::game::state::GameState;

    /// Draws `n` values from `stream` on a fresh state whose
    /// stream word starts at `word`.
    fn draws_from(word: u64, stream: &str, n: usize) -> Vec<u64> {
        let mut state = GameState::default();
        state.rng.insert(stream.to_string(), word);
        (0..n).map(|_| rng_next(&mut state, stream)).collect()
    }

    #[test]
    fn golden_stream_seeds() {
        // (run_seed, stream_id) -> genesis state word.
        let cases: [(i64, &str, u64); 5] = [
            (1, "battle", 0x4863_335D_30E0_D67E),
            (42, "battle", 0x2E19_3A6D_A86F_7A46),
            (1, "loot", 0xD791_59E0_6348_0E65),
            (-7, "battle", 0xB3D5_A4FD_82AA_FABC),
            (0, "default", 0xCA68_E982_8E7D_67C0),
        ];
        for (run_seed, stream_id, expected) in cases {
            assert_eq!(
                stream_seed(run_seed, stream_id),
                expected,
                "stream_seed({run_seed}, {stream_id:?})"
            );
        }
    }

    #[test]
    fn golden_draw_sequences() {
        // First eight draws of named streams from named run seeds.
        let cases: [(i64, &str, [u64; 8]); 4] = [
            (
                1,
                "battle",
                [
                    0x1EEF_B6B2_14EA_E1EF,
                    0xB0F1_3355_978C_89A2,
                    0x0EC2_C7F4_7409_C7E6,
                    0xC627_7026_EDBC_E037,
                    0x8B12_FDB2_FB90_9F58,
                    0x8071_1169_AFE7_DF1F,
                    0x4E05_153D_E0E0_7709,
                    0x4145_68CD_A821_37BD,
                ],
            ),
            (
                42,
                "battle",
                [
                    0xA278_FA5F_DF8D_5CA2,
                    0x6801_84E5_D632_E4E7,
                    0x194C_B546_2647_C37D,
                    0x31FC_F362_9D8A_6AF0,
                    0x56F9_3ED4_A8C7_4DB2,
                    0x2265_2CEF_E500_F2C3,
                    0xAB62_D701_08E7_3977,
                    0x99F2_BA8C_47D1_EDA0,
                ],
            ),
            (
                1,
                "loot",
                [
                    0x37E4_2B2A_A6B5_0DFE,
                    0x6148_6B50_4CD0_1A89,
                    0xF032_CFE8_4F60_1A63,
                    0xEC19_FEF4_0FE6_BA3C,
                    0xBAA2_3F99_3737_5C88,
                    0x0AD4_453E_226A_6926,
                    0xCA8D_1192_AB5A_BEC5,
                    0x8368_69C6_F4E2_74DB,
                ],
            ),
            (
                -7,
                "battle",
                [
                    0xEEDA_F4C4_E670_7E41,
                    0x280B_311F_55ED_85DE,
                    0x3A2D_F0EB_9E96_C5FE,
                    0xCD6A_5994_C30E_B0A3,
                    0xEFAA_43A0_ABCD_8F1E,
                    0x530F_37FA_5EF1_992B,
                    0xF82E_0DC1_F607_00FB,
                    0x3D41_CD81_F9DB_577B,
                ],
            ),
        ];
        for (run_seed, stream_id, expected) in cases {
            let seed = stream_seed(run_seed, stream_id);
            assert_eq!(
                draws_from(seed, stream_id, 8),
                expected,
                "draws for stream_seed({run_seed}, {stream_id:?})"
            );
        }
    }

    #[test]
    fn golden_raw_splitmix_from_zero() {
        // The bare step, no seed derivation: a stream whose word
        // starts at 0 (the `or_insert(0)` path) yields the
        // published SplitMix64 outputs for state 0.
        let mut state = GameState::default();
        let draws: Vec<u64> = (0..4).map(|_| rng_next(&mut state, "raw")).collect();
        assert_eq!(
            draws,
            [
                0xE220_A839_7B1D_CDAF,
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F,
                0xF88B_B8A8_724C_81EC,
            ]
        );
        // Four draws consumed exactly four steps of the word.
        assert_eq!(state.rng["raw"], 4u64.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    }
}
