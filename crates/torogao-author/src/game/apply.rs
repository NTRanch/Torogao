//! Applying admitted game ops.
//!
//! Runs only after `decide_game` returned `Verdict::Apply`; like
//! the rig side, it asserts what the gate established instead
//! of re-checking. Record builders live here (shared with the
//! gate's identity comparisons, the `expression_record`
//! precedent).

use crate::game::model::{
    AppearanceDef, AppearanceEntry, AppearanceKind, AppearanceSlot, AppearanceValue, Atom,
    CharacterDef, ChoiceDef, ChoiceOption, ChoiceOrigin, ChoiceTarget, Clock, ConditionDef,
    DialogueLine, Direction, EffectSetDef, EndingDef, FlagDef, FlagEdge, FlagScope,
    GAME_FORMAT_VERSION, GalleryEntryDef, GalleryUnlock, Game, Lifetime, Migration, OfferDef,
    Overflow, Repetition, RngStreamDef, SaveContextPolicy, SaveSchema, SceneBindingDef, SceneDef,
    SceneExitDef, ScheduleDef, ScheduleOverride, ScheduleSlot, SlotHolds, StatDef, StatInitial,
    StatKind, StatScope, TableDef, TableOutcome, TableWeight, ThreadDef, ThreadInterludeDef,
    ThreadMessage, TierDef, TriggerDef, TriggerKind, ValueDef, VisualStateDef,
};
use crate::game::op::{
    AppearanceDefineInput, AppearanceEntryInput, AppearanceKindInput, AppearanceSlotInput,
    AppearanceValueInput, AtomInput, CharacterDefineInput, ChoiceDefineInput, ChoiceOptionInput,
    ChoiceTargetInput, ClockDefineInput, ConditionDefineInput, DialogueLineDefineInput,
    DirectionInput, EffectSetDefineInput, EndingDefineInput, FlagDefineInput, FlagEdgeInput,
    FlagScopeInput, GalleryEntryDefineInput, GalleryUnlockInput, GameDefineInput, GameOp,
    OverflowInput, PresentationInput, RepetitionInput, RngStreamDefineInput, SavePolicyInput,
    SaveSchemaDefineInput, SceneBindRigInput, SceneDefineInput, SceneExitDefineInput,
    ScheduleDefineInput, StatDefineInput, StatKindInput, StatScopeInput, StatTierDefineInput,
    TableDefineInput, TableOutcomeInput, ThreadCreateInput, ThreadPostInput, TriggerDefineInput,
    TriggerKindInput, ValueDefineInput, VisualStateDefineInput, WeightInput,
};
use crate::model::{Project, locate};
use crate::refusal::Refusal;

/// Inserts or replaces a record in an id-sorted collection.
fn upsert<T>(items: &mut Vec<T>, item: T, key: impl Fn(&T) -> &String) {
    let id = key(&item).clone();
    match locate(items, &id, &key) {
        Ok(index) => items[index] = item,
        Err(index) => items.insert(index, item),
    }
}

/// The save-schema version a `game_define` input names (an
/// absent input is version 1).
pub(crate) fn save_schema_version_of(inputs: &GameDefineInput) -> i64 {
    inputs.save_schema_version.unwrap_or(1)
}

/// The character record a `character_define` produces (initials
/// sorted by stat id).
pub(crate) fn character_record(inputs: &CharacterDefineInput) -> CharacterDef {
    let mut initials: Vec<StatInitial> = inputs
        .initials
        .iter()
        .map(|row| StatInitial {
            stat: row.stat.clone(),
            value: row.value,
        })
        .collect();
    initials.sort_by(|a, b| a.stat.cmp(&b.stat));
    CharacterDef {
        id: inputs.id.clone(),
        name_ref: inputs.name_ref.clone(),
        initials,
    }
}

/// The scene record a `scene_define` produces.
pub(crate) fn scene_record(inputs: &SceneDefineInput) -> SceneDef {
    let mut participants = inputs.participants.clone();
    participants.sort();
    SceneDef {
        id: inputs.id.clone(),
        presentation: match inputs.presentation {
            PresentationInput::Event => crate::game::model::Presentation::Event,
            PresentationInput::Room => crate::game::model::Presentation::Room,
            PresentationInput::Vn => crate::game::model::Presentation::Vn,
        },
        participants,
        backdrop: inputs.backdrop.clone(),
        entry_condition: inputs.entry_condition.clone(),
        on_enter: inputs.on_enter.clone(),
        on_exit: inputs.on_exit.clone(),
        thread_interlude: inputs
            .thread_interlude
            .as_ref()
            .map(|interlude| ThreadInterludeDef {
                thread: interlude.thread.clone(),
                reply_delay_ms: interlude.reply_delay_ms,
            }),
    }
}

/// The thread record a `thread_create` produces (participants
/// sorted, so the record is canonical however the author
/// ordered the inputs; messages start empty and grow by
/// `thread_post`).
pub(crate) fn thread_record(inputs: &ThreadCreateInput) -> ThreadDef {
    let mut participants = inputs.participants.clone();
    participants.sort();
    ThreadDef {
        id: inputs.id.clone(),
        participants,
        messages: Vec::new(),
    }
}

/// The inputs a stored thread's creation rebuilds from (for
/// the validation sweep).
pub(crate) fn thread_create_input_of(record: &ThreadDef) -> ThreadCreateInput {
    ThreadCreateInput {
        id: record.id.clone(),
        participants: record.participants.clone(),
    }
}

/// The message record a `thread_post` produces.
pub(crate) fn thread_message_record(inputs: &ThreadPostInput) -> ThreadMessage {
    ThreadMessage {
        id: inputs.id.clone(),
        speaker: inputs.speaker.clone(),
        line: inputs.line.clone(),
        at: inputs.at,
    }
}

/// The inputs a stored message rebuilds from (for the
/// validation sweep).
pub(crate) fn thread_post_input_of(thread: &str, message: &ThreadMessage) -> ThreadPostInput {
    ThreadPostInput {
        thread: thread.to_string(),
        id: message.id.clone(),
        speaker: message.speaker.clone(),
        line: message.line.clone(),
        at: message.at,
    }
}

/// The dialogue record a `dialogue_line_define` produces. The
/// record carries the text reference only; the prose goes to the
/// sidecar (see `apply_game`).
pub(crate) fn dialogue_record(inputs: &DialogueLineDefineInput) -> DialogueLine {
    DialogueLine {
        id: inputs.id.clone(),
        scene: inputs.scene.clone(),
        ordinal: inputs.ordinal,
        speaker: inputs.speaker.clone(),
        text_ref: inputs.text_ref.clone(),
        next: inputs.next.clone(),
        hook_expression: inputs.hook_expression.clone(),
    }
}

/// The clock record a `clock_define` produces.
pub(crate) fn clock_record(inputs: &ClockDefineInput) -> Clock {
    Clock {
        slots: inputs.slots.clone(),
        days_per_week: inputs.days_per_week,
        day_limit: inputs.day_limit,
    }
}

/// The stat record a `stat_define` produces.
pub(crate) fn stat_record(inputs: &StatDefineInput) -> StatDef {
    StatDef {
        id: inputs.id.clone(),
        scope: match inputs.scope {
            StatScopeInput::Character => StatScope::Character,
            StatScopeInput::Game => StatScope::Game,
            StatScopeInput::Party => StatScope::Party,
        },
        kind: match &inputs.kind {
            StatKindInput::Enum { values } => StatKind::Enum(values.clone()),
            StatKindInput::Fixed => StatKind::Fixed,
            StatKindInput::Int => StatKind::Int,
        },
        min: inputs.min,
        max: inputs.max,
        default: inputs.default,
        overflow: match inputs.overflow {
            OverflowInput::Clamp => Overflow::Clamp,
            OverflowInput::Refuse => Overflow::Refuse,
        },
        lifetime: inputs.lifetime_days.map(|days| Lifetime { days }),
    }
}

/// The input form of a stored stat (for the validation sweep).
pub(crate) fn stat_input_of(stat: &StatDef) -> StatDefineInput {
    StatDefineInput {
        id: stat.id.clone(),
        scope: match stat.scope {
            StatScope::Character => StatScopeInput::Character,
            StatScope::Game => StatScopeInput::Game,
            StatScope::Party => StatScopeInput::Party,
        },
        kind: match &stat.kind {
            StatKind::Enum(values) => StatKindInput::Enum {
                values: values.clone(),
            },
            StatKind::Fixed => StatKindInput::Fixed,
            StatKind::Int => StatKindInput::Int,
        },
        min: stat.min,
        max: stat.max,
        default: stat.default,
        overflow: match stat.overflow {
            Overflow::Clamp => OverflowInput::Clamp,
            Overflow::Refuse => OverflowInput::Refuse,
        },
        lifetime_days: stat.lifetime.map(|lifetime| lifetime.days),
    }
}

/// The tier record a `stat_tier_define` produces.
pub(crate) fn tier_record(inputs: &StatTierDefineInput) -> TierDef {
    TierDef {
        stat: inputs.stat.clone(),
        thresholds: inputs.thresholds.clone(),
        names: inputs.names.clone(),
    }
}

/// The flag record a `flag_define` produces.
pub(crate) fn flag_record(inputs: &FlagDefineInput) -> FlagDef {
    FlagDef {
        id: inputs.id.clone(),
        scope: match inputs.scope {
            FlagScopeInput::Globals => FlagScope::Globals,
            FlagScopeInput::Run => FlagScope::Run,
        },
        default: inputs.default,
    }
}

/// The value record a `value_define` produces; the expression's
/// inferred type is stored on the record.
pub(crate) fn value_record(game: &Game, inputs: &ValueDefineInput) -> Result<ValueDef, Refusal> {
    let expr = crate::game::expr::expr_from_input(&inputs.expr)?;
    let value_type = crate::game::expr::type_of(game, &expr)?;
    Ok(ValueDef {
        id: inputs.id.clone(),
        expr,
        value_type,
    })
}

/// The condition record a `condition_define` produces.
pub(crate) fn condition_record(inputs: &ConditionDefineInput) -> Result<ConditionDef, Refusal> {
    Ok(ConditionDef {
        id: inputs.id.clone(),
        expr: crate::game::expr::expr_from_input(&inputs.expr)?,
    })
}

/// The stored atom an input atom converts to.
pub(crate) fn atom_of(input: &AtomInput) -> Result<Atom, Refusal> {
    Ok(match input {
        AtomInput::EmitEvent { event } => Atom::EmitEvent {
            event: event.clone(),
        },
        AtomInput::EnterScene { scene } => Atom::EnterScene {
            scene: scene.clone(),
        },
        AtomInput::FlagClear { flag } => Atom::FlagClear { flag: flag.clone() },
        AtomInput::FlagSet { flag } => Atom::FlagSet { flag: flag.clone() },
        AtomInput::RollTable { table } => Atom::RollTable {
            table: table.clone(),
        },
        AtomInput::StatAdd {
            stat,
            character,
            amount,
        } => Atom::StatAdd {
            character: character.clone(),
            stat: stat.clone(),
            amount: Box::new(crate::game::expr::expr_from_input(amount)?),
        },
        AtomInput::StatSet {
            stat,
            character,
            value,
        } => Atom::StatSet {
            character: character.clone(),
            stat: stat.clone(),
            value: Box::new(crate::game::expr::expr_from_input(value)?),
        },
    })
}

/// The input form of a stored atom (for the validation sweep).
pub(crate) fn atom_input_of(atom: &Atom) -> AtomInput {
    match atom {
        Atom::EmitEvent { event } => AtomInput::EmitEvent {
            event: event.clone(),
        },
        Atom::EnterScene { scene } => AtomInput::EnterScene {
            scene: scene.clone(),
        },
        Atom::FlagClear { flag } => AtomInput::FlagClear { flag: flag.clone() },
        Atom::FlagSet { flag } => AtomInput::FlagSet { flag: flag.clone() },
        Atom::RollTable { table } => AtomInput::RollTable {
            table: table.clone(),
        },
        Atom::StatAdd {
            character,
            stat,
            amount,
        } => AtomInput::StatAdd {
            stat: stat.clone(),
            character: character.clone(),
            amount: crate::game::expr::expr_to_input(amount),
        },
        Atom::StatSet {
            character,
            stat,
            value,
        } => AtomInput::StatSet {
            stat: stat.clone(),
            character: character.clone(),
            value: crate::game::expr::expr_to_input(value),
        },
    }
}

/// The effect-set record an `effect_set_define` produces.
pub(crate) fn effect_set_record(inputs: &EffectSetDefineInput) -> Result<EffectSetDef, Refusal> {
    let mut atoms = Vec::new();
    for atom in &inputs.atoms {
        atoms.push(atom_of(atom)?);
    }
    Ok(EffectSetDef {
        id: inputs.id.clone(),
        atoms,
    })
}

/// The stored trigger kind an input kind converts to.
pub(crate) fn trigger_kind_of(input: &TriggerKindInput) -> TriggerKind {
    match input {
        TriggerKindInput::OnEnterScene { scene } => TriggerKind::OnEnterScene {
            scene: scene.clone(),
        },
        TriggerKindInput::OnEvent { event } => TriggerKind::OnEvent {
            event: event.clone(),
        },
        TriggerKindInput::OnFlag { flag, edge } => TriggerKind::OnFlag {
            flag: flag.clone(),
            edge: match edge {
                FlagEdgeInput::Both => FlagEdge::Both,
                FlagEdgeInput::Falling => FlagEdge::Falling,
                FlagEdgeInput::Rising => FlagEdge::Rising,
            },
        },
        TriggerKindInput::OnThreshold {
            stat,
            character,
            threshold,
            direction,
        } => TriggerKind::OnThreshold {
            character: character.clone(),
            stat: stat.clone(),
            threshold: *threshold,
            direction: match direction {
                DirectionInput::Falling => Direction::Falling,
                DirectionInput::Rising => Direction::Rising,
            },
        },
        TriggerKindInput::OnTick {
            slots,
            weekdays,
            day_modulo,
            exact_day,
        } => TriggerKind::OnTick {
            slots: slots.clone(),
            weekdays: weekdays.clone(),
            day_modulo: *day_modulo,
            exact_day: *exact_day,
        },
    }
}

/// The input form of a stored trigger kind (for the validation
/// sweep).
pub(crate) fn trigger_kind_input_of(kind: &TriggerKind) -> TriggerKindInput {
    match kind {
        TriggerKind::OnEnterScene { scene } => TriggerKindInput::OnEnterScene {
            scene: scene.clone(),
        },
        TriggerKind::OnEvent { event } => TriggerKindInput::OnEvent {
            event: event.clone(),
        },
        TriggerKind::OnFlag { flag, edge } => TriggerKindInput::OnFlag {
            flag: flag.clone(),
            edge: match edge {
                FlagEdge::Both => FlagEdgeInput::Both,
                FlagEdge::Falling => FlagEdgeInput::Falling,
                FlagEdge::Rising => FlagEdgeInput::Rising,
            },
        },
        TriggerKind::OnThreshold {
            character,
            stat,
            threshold,
            direction,
        } => TriggerKindInput::OnThreshold {
            stat: stat.clone(),
            character: character.clone(),
            threshold: *threshold,
            direction: match direction {
                Direction::Falling => DirectionInput::Falling,
                Direction::Rising => DirectionInput::Rising,
            },
        },
        TriggerKind::OnTick {
            slots,
            weekdays,
            day_modulo,
            exact_day,
        } => TriggerKindInput::OnTick {
            slots: slots.clone(),
            weekdays: weekdays.clone(),
            day_modulo: *day_modulo,
            exact_day: *exact_day,
        },
    }
}

/// The trigger record a `trigger_define` produces.
pub(crate) fn trigger_record(inputs: &TriggerDefineInput) -> TriggerDef {
    TriggerDef {
        id: inputs.id.clone(),
        kind: trigger_kind_of(&inputs.kind),
        gate: inputs.gate.clone(),
        effect_set: inputs.effect_set.clone(),
        priority: inputs.priority,
        repetition: match inputs.repetition {
            RepetitionInput::Once => Repetition::Once,
            RepetitionInput::Repeat => Repetition::Repeat,
        },
        cooldown_ticks: inputs.cooldown_ticks,
        offer: inputs.offer.as_ref().map(|offer| OfferDef {
            expires_days: offer.expires_days,
        }),
    }
}

/// The stream record an `rng_stream_define` produces.
pub(crate) fn stream_record(inputs: &RngStreamDefineInput) -> RngStreamDef {
    RngStreamDef {
        id: inputs.id.clone(),
        algorithm: inputs.algorithm.clone(),
    }
}

/// The table record a `table_define` produces.
pub(crate) fn table_record(inputs: &TableDefineInput) -> Result<TableDef, Refusal> {
    let mut outcomes = Vec::new();
    for outcome in &inputs.outcomes {
        outcomes.push(TableOutcome {
            id: outcome.id.clone(),
            weight: match &outcome.weight {
                WeightInput::Expr(expr) => {
                    TableWeight::Expr(crate::game::expr::expr_from_input(expr)?)
                }
                WeightInput::Static(weight) => TableWeight::Static(*weight),
            },
            gate: outcome.gate.clone(),
            effect_set: outcome.effect_set.clone(),
        });
    }
    Ok(TableDef {
        id: inputs.id.clone(),
        stream: inputs.stream.clone(),
        outcomes,
    })
}

/// The input form of a stored table (for the validation sweep).
pub(crate) fn table_input_of(table: &TableDef) -> TableDefineInput {
    TableDefineInput {
        id: table.id.clone(),
        stream: table.stream.clone(),
        outcomes: table
            .outcomes
            .iter()
            .map(|outcome| TableOutcomeInput {
                id: outcome.id.clone(),
                weight: match &outcome.weight {
                    TableWeight::Expr(expr) => {
                        WeightInput::Expr(crate::game::expr::expr_to_input(expr))
                    }
                    TableWeight::Static(weight) => WeightInput::Static(*weight),
                },
                gate: outcome.gate.clone(),
                effect_set: outcome.effect_set.clone(),
            })
            .collect(),
    }
}

fn appearance_kind_of(input: AppearanceKindInput) -> AppearanceKind {
    match input {
        AppearanceKindInput::Ints => AppearanceKind::Ints,
        AppearanceKindInput::Text => AppearanceKind::Text,
    }
}

fn appearance_kind_input(kind: AppearanceKind) -> AppearanceKindInput {
    match kind {
        AppearanceKind::Ints => AppearanceKindInput::Ints,
        AppearanceKind::Text => AppearanceKindInput::Text,
    }
}

fn appearance_entry_of(input: &AppearanceEntryInput) -> AppearanceEntry {
    AppearanceEntry {
        slot: input.slot.clone(),
        value: match &input.value {
            AppearanceValueInput::Ints(values) => AppearanceValue::Ints(values.clone()),
            AppearanceValueInput::Text(token) => AppearanceValue::Text(token.clone()),
        },
    }
}

fn appearance_entry_input(entry: &AppearanceEntry) -> AppearanceEntryInput {
    AppearanceEntryInput {
        slot: entry.slot.clone(),
        value: match &entry.value {
            AppearanceValue::Ints(values) => AppearanceValueInput::Ints(values.clone()),
            AppearanceValue::Text(token) => AppearanceValueInput::Text(token.clone()),
        },
    }
}

/// The appearance record an `appearance_define` produces:
/// slots and baseline entries are stored sorted by id, so the
/// record is canonical however the author ordered the inputs.
pub(crate) fn appearance_record(inputs: &AppearanceDefineInput) -> AppearanceDef {
    let mut slots: Vec<AppearanceSlot> = inputs
        .slots
        .iter()
        .map(|slot| AppearanceSlot {
            id: slot.id.clone(),
            kind: appearance_kind_of(slot.kind),
        })
        .collect();
    slots.sort_by(|left, right| left.id.cmp(&right.id));
    let mut base: Vec<AppearanceEntry> = inputs.base.iter().map(appearance_entry_of).collect();
    base.sort_by(|left, right| left.slot.cmp(&right.slot));
    AppearanceDef {
        base,
        character: inputs.character.clone(),
        slots,
    }
}

/// The input form of a stored appearance (for the sweep).
pub(crate) fn appearance_input_of(appearance: &AppearanceDef) -> AppearanceDefineInput {
    AppearanceDefineInput {
        character: appearance.character.clone(),
        slots: appearance
            .slots
            .iter()
            .map(|slot| AppearanceSlotInput {
                id: slot.id.clone(),
                kind: appearance_kind_input(slot.kind),
            })
            .collect(),
        base: appearance.base.iter().map(appearance_entry_input).collect(),
    }
}

/// The visual-state record a `visual_state_define` produces:
/// delta entries are stored sorted by slot id.
pub(crate) fn visual_state_record(inputs: &VisualStateDefineInput) -> VisualStateDef {
    let mut delta: Vec<AppearanceEntry> = inputs.delta.iter().map(appearance_entry_of).collect();
    delta.sort_by(|left, right| left.slot.cmp(&right.slot));
    VisualStateDef {
        character: inputs.character.clone(),
        delta,
        state: inputs.state.clone(),
    }
}

/// The input form of a stored visual state (for the sweep).
pub(crate) fn visual_state_input_of(state: &VisualStateDef) -> VisualStateDefineInput {
    VisualStateDefineInput {
        character: state.character.clone(),
        state: state.state.clone(),
        delta: state.delta.iter().map(appearance_entry_input).collect(),
    }
}

/// Inserts or replaces a visual state in the (character,
/// state)-sorted collection.
fn upsert_visual_state(states: &mut Vec<VisualStateDef>, record: VisualStateDef) {
    let key = (record.character.as_str(), record.state.as_str());
    match states
        .iter()
        .position(|row| row.character == record.character && row.state == record.state)
    {
        Some(index) => states[index] = record,
        None => {
            let index = states
                .iter()
                .position(|row| (row.character.as_str(), row.state.as_str()) > key)
                .unwrap_or(states.len());
            states.insert(index, record);
        }
    }
}

/// The format migration, applied whenever a game op lands on a
/// section written under an older game format: v1 → v2 changes
/// no record (v2 only adds optional fields and the interpreter's
/// tick semantics), so the migration is the version stamp
/// itself. It is a named step so the rule is explicit and the
/// save loader's chain can share it.
pub(crate) fn migrate_game_section(game: &mut Game) {
    if game.format_version < GAME_FORMAT_VERSION {
        game.format_version = GAME_FORMAT_VERSION;
    }
}

/// Records prose under a content id (the sidecar upsert: an
/// absent id gains its entry; an identical entry is kept — the
/// gate has already refused a differing one). The first prose a
/// project gains stamps the sidecar with the canonical authored
/// locale, `en` (D-472); a loaded sidecar's locale is never
/// rewritten.
fn record_prose(project: &mut Project, id: &str, text: &str) {
    if project.content_locale.is_none() {
        project.content_locale = Some(crate::game::model::CONTENT_LOCALE_CANONICAL.to_string());
    }
    let entry = crate::game::model::ContentEntry {
        id: id.to_string(),
        text: text.to_string(),
    };
    upsert(&mut project.content, entry, |e| &e.id);
}

/// Applies one admitted game op to the project.
pub fn apply_game(project: &mut Project, op: &GameOp) -> Result<(), Refusal> {
    match op {
        GameOp::GameDefine(inputs) => {
            let game = Game {
                id: inputs.id.clone(),
                format_version: GAME_FORMAT_VERSION,
                save_schema_version: save_schema_version_of(inputs),
                start_scene: inputs.start_scene.clone(),
                alter_ego_veto: inputs.alter_ego_veto.clone(),
                ..Game::default()
            };
            project.game = Some(game);
        }
        GameOp::CharacterDefine(inputs) => {
            let record = character_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.characters, record, |c| &c.id);
            record_prose(project, &inputs.name_ref, &inputs.name_text);
        }
        GameOp::SceneDefine(inputs) => {
            let record = scene_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.scenes, record, |s| &s.id);
        }
        GameOp::DialogueLineDefine(inputs) => {
            let record = dialogue_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.dialogue, record, |l| &l.id);
            record_prose(project, &inputs.text_ref, &inputs.text);
        }
        GameOp::ClockDefine(inputs) => {
            let game = game_mut(project);
            game.clock = Some(clock_record(inputs));
        }
        GameOp::StatDefine(inputs) => {
            let record = stat_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.stats, record, |s| &s.id);
        }
        GameOp::StatTierDefine(inputs) => {
            let record = tier_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.tiers, record, |t| &t.stat);
        }
        GameOp::FlagDefine(inputs) => {
            let record = flag_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.flags, record, |f| &f.id);
        }
        GameOp::ValueDefine(inputs) => {
            let game = game_mut(project);
            let record = value_record(game, inputs)?;
            upsert(&mut game.values, record, |v| &v.id);
        }
        GameOp::ConditionDefine(inputs) => {
            let record = condition_record(inputs)?;
            let game = game_mut(project);
            upsert(&mut game.conditions, record, |c| &c.id);
        }
        GameOp::EffectSetDefine(inputs) => {
            let record = effect_set_record(inputs)?;
            let game = game_mut(project);
            upsert(&mut game.effect_sets, record, |s| &s.id);
        }
        GameOp::TriggerDefine(inputs) => {
            let record = trigger_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.triggers, record, |t| &t.id);
        }
        GameOp::RngStreamDefine(inputs) => {
            let record = stream_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.streams, record, |s| &s.id);
        }
        GameOp::TableDefine(inputs) => {
            let record = table_record(inputs)?;
            let game = game_mut(project);
            upsert(&mut game.tables, record, |t| &t.id);
        }
        GameOp::AppearanceDefine(inputs) => {
            let record = appearance_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.appearances, record, |a| &a.character);
        }
        GameOp::VisualStateDefine(inputs) => {
            let record = visual_state_record(inputs);
            let game = game_mut(project);
            upsert_visual_state(&mut game.visual_states, record);
        }
        GameOp::ChoiceDefine(inputs) => {
            let record = choice_record(inputs)?;
            let game = game_mut(project);
            upsert(&mut game.choices, record, |c| &c.id);
        }
        GameOp::EndingDefine(inputs) => {
            let record = ending_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.endings, record, |e| &e.id);
        }
        GameOp::GalleryEntryDefine(inputs) => {
            let record = gallery_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.gallery_entries, record, |e| &e.id);
        }
        GameOp::GameSaveSchemaDefine(inputs) => {
            let record = save_schema_record(inputs);
            let game = game_mut(project);
            game.save_schema = Some(record);
        }
        GameOp::ScheduleDefine(inputs) => {
            let record = schedule_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.schedules, record, |s| &s.character);
        }
        GameOp::SceneBindRig(inputs) => {
            let record = scene_binding_record(inputs);
            let game = game_mut(project);
            upsert_scene_binding(&mut game.scene_bindings, record);
        }
        GameOp::SceneExitDefine(inputs) => {
            let record = scene_exit_record(inputs);
            let game = game_mut(project);
            upsert_scene_exit(&mut game.scene_exits, record);
        }
        GameOp::ThreadCreate(inputs) => {
            let record = thread_record(inputs);
            let game = game_mut(project);
            // Decide admits a create only for a fresh id (an
            // identical re-create is a no-op, a differing one a
            // conflict), so this upsert always inserts.
            upsert(&mut game.threads, record, |t| &t.id);
        }
        GameOp::ThreadPost(inputs) => {
            let record = thread_message_record(inputs);
            let game = game_mut(project);
            let thread = game
                .threads
                .iter_mut()
                .find(|row| row.id == inputs.thread)
                .expect("decide() admitted a post to a missing thread");
            thread.messages.push(record);
        }
    }
    if let Some(game) = &mut project.game {
        migrate_game_section(game);
    }
    Ok(())
}

/// The save-context policy a `SavePolicyInput` denotes (safe
/// scenes sorted, the record's canonical shape).
pub(crate) fn policy_of(input: &SavePolicyInput) -> SaveContextPolicy {
    match input {
        SavePolicyInput::AnyTickBoundary => SaveContextPolicy::AnyTickBoundary,
        SavePolicyInput::SceneBoundary => SaveContextPolicy::SceneBoundary,
        SavePolicyInput::SafeScenesOnly { scenes } => {
            let mut scenes = scenes.clone();
            scenes.sort();
            scenes.dedup();
            SaveContextPolicy::SafeScenesOnly(scenes)
        }
    }
}

/// The save-schema record a `game_save_schema_define`
/// produces (partitions and migrations sorted to canonical
/// shape).
pub(crate) fn save_schema_record(inputs: &SaveSchemaDefineInput) -> SaveSchema {
    let mut slot_fields = inputs.slot_fields.clone();
    slot_fields.sort();
    let mut globals_fields = inputs.globals_fields.clone();
    globals_fields.sort();
    let mut migrations: Vec<Migration> = inputs
        .migrations
        .iter()
        .map(|step| Migration {
            from: step.from,
            name: step.name.clone(),
        })
        .collect();
    migrations.sort_by_key(|step| step.from);
    SaveSchema {
        version: inputs.version,
        slots: inputs.slots,
        slot_holds: match inputs.slot_holds.as_str() {
            "chapter_list" => SlotHolds::ChapterList,
            _ => SlotHolds::OneRun,
        },
        slot_fields,
        globals_fields,
        hash_algorithm: inputs.hash_algorithm.clone(),
        policy: policy_of(&inputs.policy),
        migrations,
    }
}

/// The record a stored schema rebuilds to (validate sweep).
pub(crate) fn save_schema_input_of(schema: &SaveSchema) -> SaveSchemaDefineInput {
    SaveSchemaDefineInput {
        version: schema.version,
        slots: schema.slots,
        slot_holds: match schema.slot_holds {
            SlotHolds::ChapterList => "chapter_list".to_string(),
            SlotHolds::OneRun => "one_run".to_string(),
        },
        slot_fields: schema.slot_fields.clone(),
        globals_fields: schema.globals_fields.clone(),
        hash_algorithm: schema.hash_algorithm.clone(),
        policy: match &schema.policy {
            SaveContextPolicy::AnyTickBoundary => SavePolicyInput::AnyTickBoundary,
            SaveContextPolicy::SceneBoundary => SavePolicyInput::SceneBoundary,
            SaveContextPolicy::SafeScenesOnly(scenes) => SavePolicyInput::SafeScenesOnly {
                scenes: scenes.clone(),
            },
        },
        migrations: schema
            .migrations
            .iter()
            .map(|step| crate::game::op::MigrationInput {
                from: step.from,
                name: step.name.clone(),
            })
            .collect(),
    }
}

/// The choice target an input denotes.
pub(crate) fn choice_target_of(input: &ChoiceTargetInput) -> ChoiceTarget {
    match input {
        ChoiceTargetInput::EndingCheck => ChoiceTarget::EndingCheck,
        ChoiceTargetInput::Exit { id } => ChoiceTarget::Exit(id.clone()),
        ChoiceTargetInput::Line { id } => ChoiceTarget::Line(id.clone()),
        ChoiceTargetInput::Scene { id } => ChoiceTarget::Scene(id.clone()),
    }
}

/// The choice record a `choice_define` produces (options in
/// authored order; sort expressions converted).
pub(crate) fn choice_record(inputs: &ChoiceDefineInput) -> Result<ChoiceDef, Refusal> {
    let mut options: Vec<ChoiceOption> = Vec::with_capacity(inputs.options.len());
    for option in &inputs.options {
        options.push(ChoiceOption {
            id: option.id.clone(),
            label_ref: option.label_ref.clone(),
            visible_if: option.visible_if.clone(),
            enabled_if: option.enabled_if.clone(),
            sort: option
                .sort
                .as_ref()
                .map(super::expr::expr_from_input)
                .transpose()?,
            origin: match option.origin.as_str() {
                "alter_ego" => ChoiceOrigin::AlterEgo,
                _ => ChoiceOrigin::Player,
            },
            effect_set: option.effect_set.clone(),
            target: choice_target_of(&option.target),
        });
    }
    Ok(ChoiceDef {
        id: inputs.id.clone(),
        scene: inputs.scene.clone(),
        prompt_line: inputs.prompt_line.clone(),
        options,
    })
}

/// The inputs a stored choice rebuilds from (validate sweep).
pub(crate) fn choice_input_of(record: &ChoiceDef) -> ChoiceDefineInput {
    ChoiceDefineInput {
        id: record.id.clone(),
        scene: record.scene.clone(),
        prompt_line: record.prompt_line.clone(),
        options: record
            .options
            .iter()
            .map(|option| ChoiceOptionInput {
                id: option.id.clone(),
                label_ref: option.label_ref.clone(),
                visible_if: option.visible_if.clone(),
                enabled_if: option.enabled_if.clone(),
                sort: option.sort.as_ref().map(super::expr::expr_to_input),
                origin: match option.origin {
                    ChoiceOrigin::AlterEgo => "alter_ego".to_string(),
                    ChoiceOrigin::Player => "player".to_string(),
                },
                effect_set: option.effect_set.clone(),
                target: match &option.target {
                    ChoiceTarget::EndingCheck => ChoiceTargetInput::EndingCheck,
                    ChoiceTarget::Exit(id) => ChoiceTargetInput::Exit { id: id.clone() },
                    ChoiceTarget::Line(id) => ChoiceTargetInput::Line { id: id.clone() },
                    ChoiceTarget::Scene(id) => ChoiceTargetInput::Scene { id: id.clone() },
                },
            })
            .collect(),
    }
}

/// The ending record an `ending_define` produces.
pub(crate) fn ending_record(inputs: &EndingDefineInput) -> EndingDef {
    EndingDef {
        id: inputs.id.clone(),
        condition: inputs.condition.clone(),
        priority: inputs.priority,
        epilogue_scene: inputs.epilogue_scene.clone(),
    }
}

/// The inputs a stored ending rebuilds from (validate sweep).
pub(crate) fn ending_input_of(record: &EndingDef) -> EndingDefineInput {
    EndingDefineInput {
        id: record.id.clone(),
        condition: record.condition.clone(),
        priority: record.priority,
        epilogue_scene: record.epilogue_scene.clone(),
    }
}

/// The gallery record a `gallery_entry_define` produces.
pub(crate) fn gallery_record(inputs: &GalleryEntryDefineInput) -> GalleryEntryDef {
    GalleryEntryDef {
        id: inputs.id.clone(),
        category: inputs.category.clone(),
        unlock: match &inputs.unlock {
            GalleryUnlockInput::Condition { id } => GalleryUnlock::Condition(id.clone()),
            GalleryUnlockInput::Effect => GalleryUnlock::Effect,
        },
        replay_scene: inputs.replay_scene.clone(),
        replay_line: inputs.replay_line.clone(),
        thumbnail: inputs.thumbnail.clone(),
    }
}

/// The inputs a stored gallery entry rebuilds from (validate
/// sweep).
pub(crate) fn gallery_input_of(record: &GalleryEntryDef) -> GalleryEntryDefineInput {
    GalleryEntryDefineInput {
        id: record.id.clone(),
        category: record.category.clone(),
        unlock: match &record.unlock {
            GalleryUnlock::Condition(id) => GalleryUnlockInput::Condition { id: id.clone() },
            GalleryUnlock::Effect => GalleryUnlockInput::Effect,
        },
        replay_scene: record.replay_scene.clone(),
        replay_line: record.replay_line.clone(),
        thumbnail: record.thumbnail.clone(),
    }
}

/// The schedule record a `schedule_define` produces (slot rows
/// sorted by slot name; overrides keep authored order).
pub(crate) fn schedule_record(inputs: &ScheduleDefineInput) -> ScheduleDef {
    let mut slots: Vec<ScheduleSlot> = inputs
        .slots
        .iter()
        .map(|row| ScheduleSlot {
            slot: row.slot.clone(),
            scene: row.scene.clone(),
        })
        .collect();
    slots.sort_by(|a, b| a.slot.cmp(&b.slot));
    ScheduleDef {
        character: inputs.character.clone(),
        slots,
        overrides: inputs
            .overrides
            .iter()
            .map(|row| ScheduleOverride {
                condition: row.condition.clone(),
                scene: row.scene.clone(),
            })
            .collect(),
    }
}

/// The inputs a stored schedule rebuilds from (validate
/// sweep): the rows are already in canonical sorted order, so
/// the rebuilt record matches.
pub(crate) fn schedule_input_of(record: &ScheduleDef) -> ScheduleDefineInput {
    ScheduleDefineInput {
        character: record.character.clone(),
        slots: record
            .slots
            .iter()
            .map(|row| crate::game::op::ScheduleSlotInput {
                slot: row.slot.clone(),
                scene: row.scene.clone(),
            })
            .collect(),
        overrides: record
            .overrides
            .iter()
            .map(|row| crate::game::op::ScheduleOverrideInput {
                condition: row.condition.clone(),
                scene: row.scene.clone(),
            })
            .collect(),
    }
}

/// The binding record a `scene_bind_rig` produces.
pub(crate) fn scene_binding_record(inputs: &SceneBindRigInput) -> SceneBindingDef {
    SceneBindingDef {
        scene: inputs.scene.clone(),
        participant: inputs.participant.clone(),
        rig_model: inputs.rig_model.clone(),
        entry_expression: inputs.entry_expression.clone(),
        entry_motion: inputs.entry_motion.clone(),
    }
}

/// The inputs a stored binding rebuilds from (validate sweep).
pub(crate) fn scene_binding_input_of(record: &SceneBindingDef) -> SceneBindRigInput {
    SceneBindRigInput {
        scene: record.scene.clone(),
        participant: record.participant.clone(),
        rig_model: record.rig_model.clone(),
        entry_expression: record.entry_expression.clone(),
        entry_motion: record.entry_motion.clone(),
    }
}

/// The exit record a `scene_exit_define` produces.
pub(crate) fn scene_exit_record(inputs: &SceneExitDefineInput) -> SceneExitDef {
    SceneExitDef {
        scene: inputs.scene.clone(),
        id: inputs.id.clone(),
        target: inputs.target.clone(),
        condition: inputs.condition.clone(),
        effect_set: inputs.effect_set.clone(),
    }
}

/// The inputs a stored exit rebuilds from (validate sweep).
pub(crate) fn scene_exit_input_of(record: &SceneExitDef) -> SceneExitDefineInput {
    SceneExitDefineInput {
        scene: record.scene.clone(),
        id: record.id.clone(),
        target: record.target.clone(),
        condition: record.condition.clone(),
        effect_set: record.effect_set.clone(),
    }
}

/// Inserts or replaces a binding in the (scene,
/// participant)-sorted collection (scene_bind_rig's set
/// semantics land here: an admitted rebind simply replaces).
fn upsert_scene_binding(items: &mut Vec<SceneBindingDef>, item: SceneBindingDef) {
    match items
        .iter()
        .position(|row| row.scene == item.scene && row.participant == item.participant)
    {
        Some(index) => items[index] = item,
        None => {
            items.push(item);
            items.sort_by(|a, b| (&a.scene, &a.participant).cmp(&(&b.scene, &b.participant)));
        }
    }
}

/// Inserts or replaces an exit in the (scene, id)-sorted
/// collection.
fn upsert_scene_exit(items: &mut Vec<SceneExitDef>, item: SceneExitDef) {
    match items
        .iter()
        .position(|row| row.scene == item.scene && row.id == item.id)
    {
        Some(index) => items[index] = item,
        None => {
            items.push(item);
            items.sort_by(|a, b| (&a.scene, &a.id).cmp(&(&b.scene, &b.id)));
        }
    }
}

fn game_mut(project: &mut Project) -> &mut Game {
    project
        .game
        .as_mut()
        .expect("decide() admitted a game op with no game section")
}
