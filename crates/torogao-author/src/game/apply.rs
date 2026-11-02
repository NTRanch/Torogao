//! Applying admitted game ops.
//!
//! Runs only after `decide_game` returned `Verdict::Apply`; like
//! the rig side, it asserts what the gate established instead
//! of re-checking. Record builders live here (shared with the
//! gate's identity comparisons, the `expression_record`
//! precedent).

use crate::game::model::{
    Atom, CharacterDef, Clock, ConditionDef, DialogueLine, Direction, EffectSetDef, FlagDef,
    FlagEdge, FlagScope, GAME_FORMAT_VERSION, Game, Lifetime, Overflow, Repetition, SceneDef,
    StatDef, StatInitial, StatKind, StatScope, TierDef, TriggerDef, TriggerKind, ValueDef,
};
use crate::game::op::{
    AtomInput, CharacterDefineInput, ClockDefineInput, ConditionDefineInput,
    DialogueLineDefineInput, DirectionInput, EffectSetDefineInput, FlagDefineInput, FlagEdgeInput,
    FlagScopeInput, GameDefineInput, GameOp, OverflowInput, PresentationInput, RepetitionInput,
    SceneDefineInput, StatDefineInput, StatKindInput, StatScopeInput, StatTierDefineInput,
    TriggerDefineInput, TriggerKindInput, ValueDefineInput,
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
    SceneDef {
        id: inputs.id.clone(),
        presentation: match inputs.presentation {
            PresentationInput::Event => crate::game::model::Presentation::Event,
            PresentationInput::Room => crate::game::model::Presentation::Room,
            PresentationInput::Vn => crate::game::model::Presentation::Vn,
        },
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

/// The trigger record a `trigger_define` produces. (The `offer`
/// field joins the input record in slice P2-3; until then a
/// stored offer cannot be authored through ops.)
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
        offer: None,
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
    }
    if let Some(game) = &mut project.game {
        migrate_game_section(game);
    }
    Ok(())
}

fn game_mut(project: &mut Project) -> &mut Game {
    project
        .game
        .as_mut()
        .expect("decide() admitted a game op with no game section")
}
