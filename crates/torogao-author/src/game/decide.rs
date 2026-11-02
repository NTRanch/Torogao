//! The AUTH-G gate: `decide_game`, a pure function of (project,
//! game op) → Apply | NoOp | Refusal, under the same laws as the
//! rig gate — definition verbs are upserts (identical inputs are
//! a no-op, differing inputs are `TG-AUTH-CONFLICT`), every
//! bound lives here, and deny is final.

use crate::decide::{Verdict, check_id};
use crate::game::apply::{
    character_record, clock_record, condition_record, dialogue_record, effect_set_record,
    flag_record, save_schema_version_of, scene_record, stat_record, tier_record, trigger_record,
    value_record,
};
use crate::game::model::{GAME_FORMAT_VERSION, GAME_FORMAT_VERSION_FIRST, Game};
use crate::game::op::{
    AtomInput, CharacterDefineInput, ClockDefineInput, ConditionDefineInput,
    DialogueLineDefineInput, EffectSetDefineInput, ExprInput, FlagDefineInput, GameDefineInput,
    GameOp, SceneDefineInput, StatDefineInput, StatKindInput, StatTierDefineInput,
    TriggerDefineInput, TriggerKindInput, ValueDefineInput,
};
use crate::limits::{
    GAME_CHARACTERS_MAX, GAME_CONDITIONS_MAX, GAME_CONTENT_TEXT_BYTES_MAX, GAME_DIALOGUE_LINES_MAX,
    GAME_EFFECT_ATOMS_MAX, GAME_EFFECT_SETS_MAX, GAME_FLAGS_MAX, GAME_LIFETIME_DAYS_MAX,
    GAME_OFFER_DAYS_MAX, GAME_SCENES_MAX, GAME_SLOTS_MAX, GAME_STATS_MAX, GAME_TIER_THRESHOLDS_MAX,
    GAME_TRIGGERS_MAX, GAME_VALUES_MAX,
};
use crate::model::Project;
use crate::refusal::{Refusal, RefusalClass, refuse};

fn conflict<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::Conflict, message)
}

fn dangling<T>(what: &str, id: &str) -> Result<T, Refusal> {
    refuse(
        RefusalClass::Dangling,
        format!("{what} '{id}' does not exist"),
    )
}

fn check_count(count: usize, max: usize, what: &str) -> Result<(), Refusal> {
    if count > max {
        return refuse(
            RefusalClass::Range,
            format!("{what} would number {count}; the bound is {max}"),
        );
    }
    Ok(())
}

/// Prose law for sidecar text: non-empty, bounded, no control
/// characters other than newline and tab (dialogue prose may
/// break lines; it may not smuggle terminal controls).
fn check_prose(text: &str, what: &str) -> Result<(), Refusal> {
    if text.trim().is_empty() {
        return refuse(
            RefusalClass::Malformed,
            format!("{what} is missing or empty"),
        );
    }
    if text.len() > GAME_CONTENT_TEXT_BYTES_MAX {
        return refuse(
            RefusalClass::Range,
            format!("{what} exceeds {GAME_CONTENT_TEXT_BYTES_MAX} bytes"),
        );
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return refuse(
            RefusalClass::Malformed,
            format!("{what} carries control characters"),
        );
    }
    Ok(())
}

/// The prose upsert law, shared by every op that authors prose:
/// a content id with different prose already recorded is a
/// conflict; editorial changes are made by editing `content.ron`
/// itself, never by re-defining a line.
fn check_prose_upsert(project: &Project, id: &str, text: &str) -> Result<(), Refusal> {
    match project.content.iter().find(|entry| entry.id == id) {
        Some(entry) if entry.text != text => conflict(format!(
            "content '{id}' already carries different prose; edit content.ron to change it"
        )),
        _ => Ok(()),
    }
}

/// The game section, refusing when the project authors no game
/// yet (every game op but `game_define` needs one).
pub(crate) fn section(project: &Project) -> Result<&Game, Refusal> {
    let Some(game) = &project.game else {
        return refuse(
            RefusalClass::UnknownTarget,
            "the project has no game section; run game_define first",
        );
    };
    if game.format_version > GAME_FORMAT_VERSION {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "game format_version {} is newer than this build supports ({GAME_FORMAT_VERSION})",
                game.format_version
            ),
        );
    }
    if game.format_version < GAME_FORMAT_VERSION_FIRST {
        return refuse(
            RefusalClass::Malformed,
            format!(
                "game format_version {} is not a known version",
                game.format_version
            ),
        );
    }
    Ok(game)
}

/// THE game gate. Pure: reads only its arguments.
pub fn decide_game(project: &Project, op: &GameOp) -> Result<Verdict, Refusal> {
    match op {
        GameOp::GameDefine(inputs) => decide_game_define(project, inputs),
        GameOp::CharacterDefine(inputs) => {
            section(project)?;
            decide_character(project, inputs)
        }
        GameOp::SceneDefine(inputs) => {
            let game = section(project)?;
            decide_scene(game, inputs)
        }
        GameOp::DialogueLineDefine(inputs) => {
            section(project)?;
            decide_dialogue(project, inputs)
        }
        GameOp::ClockDefine(inputs) => {
            let game = section(project)?;
            decide_clock(game, inputs)
        }
        GameOp::StatDefine(inputs) => {
            let game = section(project)?;
            decide_stat(game, inputs)
        }
        GameOp::StatTierDefine(inputs) => {
            let game = section(project)?;
            decide_tier(game, inputs)
        }
        GameOp::FlagDefine(inputs) => {
            let game = section(project)?;
            decide_flag(game, inputs)
        }
        GameOp::ValueDefine(inputs) => {
            let game = section(project)?;
            decide_value(game, inputs)
        }
        GameOp::ConditionDefine(inputs) => {
            let game = section(project)?;
            decide_condition(game, inputs)
        }
        GameOp::EffectSetDefine(inputs) => {
            let game = section(project)?;
            decide_effect_set(game, inputs)
        }
        GameOp::TriggerDefine(inputs) => {
            let game = section(project)?;
            decide_trigger(game, inputs)
        }
    }
}

fn game_expr<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::GameExpr, message)
}

fn game_tier<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::GameTier, message)
}

/// The upsert law for a singleton or keyed record: identical is
/// a no-op, differing is a conflict, absent applies (subject to
/// the caller's count bound).
fn upsert_verdict<T: PartialEq>(
    existing: Option<&T>,
    record: &T,
    what: &str,
    id: &str,
    count: usize,
    max: usize,
) -> Result<Verdict, Refusal> {
    match existing {
        Some(found) if found == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "{what} '{id}' already exists with different inputs"
        )),
        None => {
            check_count(count, max, what)?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_clock(game: &Game, inputs: &ClockDefineInput) -> Result<Verdict, Refusal> {
    if inputs.slots.is_empty() {
        return refuse(RefusalClass::Malformed, "a clock needs at least one slot");
    }
    check_count(inputs.slots.len(), GAME_SLOTS_MAX, "clock slots")?;
    let mut seen: Vec<&str> = Vec::new();
    for slot in &inputs.slots {
        check_id(slot, "clock slot")?;
        if seen.contains(&slot.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("clock slot '{slot}' is named twice"),
            );
        }
        seen.push(slot);
    }
    if let Some(cycle) = inputs.days_per_week
        && cycle == 0
    {
        return refuse(RefusalClass::Range, "days_per_week 0 is below 1");
    }
    if let Some(limit) = inputs.day_limit
        && limit == 0
    {
        return refuse(RefusalClass::Range, "day_limit 0 is below 1");
    }
    let record = clock_record(inputs);
    match &game.clock {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => conflict("the game already defines a different clock"),
        None => Ok(Verdict::Apply),
    }
}

fn decide_stat(game: &Game, inputs: &StatDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "stat id")?;
    if inputs.min > inputs.max {
        return refuse(
            RefusalClass::Range,
            format!(
                "stat '{}' has min {} above max {}",
                inputs.id, inputs.min, inputs.max
            ),
        );
    }
    if inputs.default < inputs.min || inputs.default > inputs.max {
        return refuse(
            RefusalClass::Range,
            format!(
                "stat '{}' default {} is outside [{}, {}]",
                inputs.id, inputs.default, inputs.min, inputs.max
            ),
        );
    }
    if let StatKindInput::Enum { values } = &inputs.kind {
        if values.is_empty() {
            return refuse(
                RefusalClass::Malformed,
                format!("enum stat '{}' names no values", inputs.id),
            );
        }
        let mut seen: Vec<&str> = Vec::new();
        for value in values {
            check_id(value, "enum value")?;
            if seen.contains(&value.as_str()) {
                return refuse(
                    RefusalClass::Malformed,
                    format!("enum stat '{}' names value '{value}' twice", inputs.id),
                );
            }
            seen.push(value);
        }
        let top = i64::try_from(values.len() - 1).unwrap_or(i64::MAX);
        if inputs.min != 0 || inputs.max != top {
            return refuse(
                RefusalClass::Range,
                format!(
                    "enum stat '{}' must range over [0, {}] (its value count minus one)",
                    inputs.id, top
                ),
            );
        }
    }
    if let Some(days) = inputs.lifetime_days
        && (days == 0 || days > GAME_LIFETIME_DAYS_MAX)
    {
        return refuse(
            RefusalClass::Range,
            format!(
                "stat '{}' lifetime {days} days is outside 1..={GAME_LIFETIME_DAYS_MAX}",
                inputs.id
            ),
        );
    }
    let record = stat_record(inputs);
    upsert_verdict(
        game.stat(&inputs.id),
        &record,
        "stat",
        &inputs.id,
        game.stats.len() + 1,
        GAME_STATS_MAX,
    )
}

fn decide_tier(game: &Game, inputs: &StatTierDefineInput) -> Result<Verdict, Refusal> {
    let Some(stat) = game.stat(&inputs.stat) else {
        return dangling("stat", &inputs.stat);
    };
    check_count(
        inputs.thresholds.len(),
        GAME_TIER_THRESHOLDS_MAX,
        "tier thresholds",
    )?;
    for pair in inputs.thresholds.windows(2) {
        if pair[1] <= pair[0] {
            return game_tier(format!(
                "tier thresholds for stat '{}' are not strictly ascending",
                inputs.stat
            ));
        }
    }
    for threshold in &inputs.thresholds {
        if *threshold <= stat.min || *threshold > stat.max {
            return game_tier(format!(
                "tier threshold {threshold} for stat '{}' is outside ({}, {}]",
                inputs.stat, stat.min, stat.max
            ));
        }
    }
    if inputs.names.len() != inputs.thresholds.len() + 1 {
        return game_tier(format!(
            "tier for stat '{}' names {} bands for {} thresholds (bands are thresholds + 1)",
            inputs.stat,
            inputs.names.len(),
            inputs.thresholds.len()
        ));
    }
    for name in &inputs.names {
        if name.trim().is_empty() {
            return game_tier(format!(
                "tier for stat '{}' carries an empty band name",
                inputs.stat
            ));
        }
    }
    let record = tier_record(inputs);
    upsert_verdict(
        game.tier(&inputs.stat),
        &record,
        "tier definition",
        &inputs.stat,
        game.tiers.len() + 1,
        GAME_STATS_MAX,
    )
}

fn decide_flag(game: &Game, inputs: &FlagDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "flag id")?;
    let record = flag_record(inputs);
    upsert_verdict(
        game.flag(&inputs.id),
        &record,
        "flag",
        &inputs.id,
        game.flags.len() + 1,
        GAME_FLAGS_MAX,
    )
}

fn decide_value(game: &Game, inputs: &ValueDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "value id")?;
    let record = value_record(game, inputs)?;
    upsert_verdict(
        game.value(&inputs.id),
        &record,
        "value",
        &inputs.id,
        game.values.len() + 1,
        GAME_VALUES_MAX,
    )
}

fn decide_condition(game: &Game, inputs: &ConditionDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "condition id")?;
    let record = condition_record(inputs)?;
    if crate::game::expr::type_of(game, &record.expr)? != crate::game::model::ExprType::Bool {
        return game_expr(format!(
            "condition '{}' must be a boolean expression",
            inputs.id
        ));
    }
    upsert_verdict(
        game.condition(&inputs.id),
        &record,
        "condition",
        &inputs.id,
        game.conditions.len() + 1,
        GAME_CONDITIONS_MAX,
    )
}

fn decide_effect_set(game: &Game, inputs: &EffectSetDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "effect set id")?;
    check_count(inputs.atoms.len(), GAME_EFFECT_ATOMS_MAX, "effect atoms")?;
    for atom in &inputs.atoms {
        check_atom(game, atom)?;
    }
    let record = effect_set_record(inputs)?;
    upsert_verdict(
        game.effect_set(&inputs.id),
        &record,
        "effect set",
        &inputs.id,
        game.effect_sets.len() + 1,
        GAME_EFFECT_SETS_MAX,
    )
}

fn check_atom(game: &Game, atom: &AtomInput) -> Result<(), Refusal> {
    match atom {
        AtomInput::EmitEvent { event } => check_id(event, "event name"),
        AtomInput::EnterScene { scene } => {
            if game.scene(scene).is_none() {
                return dangling("scene", scene);
            }
            Ok(())
        }
        AtomInput::FlagClear { flag } | AtomInput::FlagSet { flag } => {
            if game.flag(flag).is_none() {
                return dangling("flag", flag);
            }
            Ok(())
        }
        AtomInput::StatAdd {
            stat,
            character,
            amount,
        } => check_stat_atom(game, stat, character.as_deref(), amount, false),
        AtomInput::StatSet {
            stat,
            character,
            value,
        } => check_stat_atom(game, stat, character.as_deref(), value, true),
    }
}

fn check_stat_atom(
    game: &Game,
    stat_id: &str,
    character: Option<&str>,
    amount: &ExprInput,
    is_set: bool,
) -> Result<(), Refusal> {
    let Some(stat) = game.stat(stat_id) else {
        return dangling("stat", stat_id);
    };
    crate::game::expr::check_stat_target(game, stat_id, character)?;
    let expr = crate::game::expr::expr_from_input(amount)?;
    if crate::game::expr::type_of(game, &expr)? != crate::game::model::ExprType::Int {
        return game_expr(format!(
            "the amount for stat '{stat_id}' must be an integer expression"
        ));
    }
    if is_set
        && let ExprInput::Lit { value } = amount
        && (*value < stat.min || *value > stat.max)
    {
        return refuse(
            RefusalClass::Range,
            format!(
                "stat_set writes literal {value} to stat '{stat_id}', outside its range [{}, {}]",
                stat.min, stat.max
            ),
        );
    }
    Ok(())
}

fn decide_trigger(game: &Game, inputs: &TriggerDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "trigger id")?;
    if game.effect_set(&inputs.effect_set).is_none() {
        return dangling("effect set", &inputs.effect_set);
    }
    if let Some(gate) = &inputs.gate
        && game.condition(gate).is_none()
    {
        return dangling("gate condition", gate);
    }
    match &inputs.kind {
        TriggerKindInput::OnEnterScene { scene } => {
            if game.scene(scene).is_none() {
                return dangling("scene", scene);
            }
        }
        TriggerKindInput::OnEvent { event } => {
            check_id(event, "event name")?;
        }
        TriggerKindInput::OnFlag { flag, .. } => {
            if game.flag(flag).is_none() {
                return dangling("flag", flag);
            }
        }
        TriggerKindInput::OnThreshold {
            stat,
            character,
            threshold,
            ..
        } => {
            let Some(def) = game.stat(stat) else {
                return dangling("stat", stat);
            };
            crate::game::expr::check_stat_target(game, stat, character.as_deref())?;
            if *threshold < def.min || *threshold > def.max {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "threshold {threshold} for stat '{stat}' is outside its range [{}, {}]",
                        def.min, def.max
                    ),
                );
            }
        }
        TriggerKindInput::OnTick {
            slots,
            weekdays,
            day_modulo,
            exact_day,
        } => {
            let Some(clock) = &game.clock else {
                return refuse(
                    RefusalClass::Malformed,
                    "an on_tick trigger needs a clock; run clock_define first",
                );
            };
            if let Some(set) = slots {
                for slot in set {
                    if !clock.slots.contains(slot) {
                        return dangling("clock slot", slot);
                    }
                }
            }
            if let Some(set) = weekdays {
                let Some(cycle) = clock.days_per_week else {
                    return refuse(
                        RefusalClass::Malformed,
                        "the trigger matches weekdays, but the clock names none",
                    );
                };
                for day in set {
                    if *day == 0 || *day > cycle {
                        return refuse(
                            RefusalClass::Range,
                            format!("weekday {day} is outside 1..={cycle}"),
                        );
                    }
                }
            }
            if let Some((period, remainder)) = day_modulo {
                if *period == 0 {
                    return refuse(RefusalClass::Range, "day_modulo period 0 is below 1");
                }
                if remainder >= period {
                    return refuse(
                        RefusalClass::Range,
                        format!("day_modulo remainder {remainder} must be below period {period}"),
                    );
                }
            }
            if let Some(day) = exact_day
                && *day < 1
            {
                return refuse(RefusalClass::Range, format!("exact_day {day} is below 1"));
            }
        }
    }
    if let Some(offer) = &inputs.offer
        && let Some(days) = offer.expires_days
        && (days == 0 || days > GAME_OFFER_DAYS_MAX)
    {
        return refuse(
            RefusalClass::Range,
            format!(
                "trigger '{}' offer expiry {days} days is outside 1..={GAME_OFFER_DAYS_MAX}",
                inputs.id
            ),
        );
    }
    let record = trigger_record(inputs);
    upsert_verdict(
        game.trigger(&inputs.id),
        &record,
        "trigger",
        &inputs.id,
        game.triggers.len() + 1,
        GAME_TRIGGERS_MAX,
    )
}

fn decide_game_define(project: &Project, inputs: &GameDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "game id")?;
    if let Some(scene) = &inputs.start_scene {
        check_id(scene, "start scene")?;
    }
    let schema = save_schema_version_of(inputs);
    if schema < 1 {
        return refuse(
            RefusalClass::Range,
            format!("save_schema_version {schema} is below 1"),
        );
    }
    match &project.game {
        None => Ok(Verdict::Apply),
        Some(game) => {
            if game.format_version > GAME_FORMAT_VERSION {
                return refuse(
                    RefusalClass::UnknownKind,
                    format!(
                        "game format_version {} is newer than this build supports ({GAME_FORMAT_VERSION})",
                        game.format_version
                    ),
                );
            }
            if game.id == inputs.id
                && game.start_scene == inputs.start_scene
                && game.save_schema_version == schema
            {
                Ok(Verdict::NoOp)
            } else {
                conflict("a game with different defining inputs already exists here")
            }
        }
    }
}

fn decide_character(project: &Project, inputs: &CharacterDefineInput) -> Result<Verdict, Refusal> {
    let game = section(project)?;
    check_id(&inputs.id, "character id")?;
    check_id(&inputs.name_ref, "name reference")?;
    check_prose(&inputs.name_text, "character name")?;
    check_prose_upsert(project, &inputs.name_ref, &inputs.name_text)?;
    let mut seen: Vec<&str> = Vec::with_capacity(inputs.initials.len());
    for row in &inputs.initials {
        if seen.contains(&row.stat.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("stat '{}' is given twice in the initials", row.stat),
            );
        }
        seen.push(&row.stat);
        let Some(stat) = game.stat(&row.stat) else {
            return dangling("stat", &row.stat);
        };
        if stat.scope != crate::game::model::StatScope::Character {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "initial for stat '{}' needs a character-scoped stat",
                    row.stat
                ),
            );
        }
        if row.value < stat.min || row.value > stat.max {
            return refuse(
                RefusalClass::Range,
                format!(
                    "initial {} for stat '{}' is outside its range [{}, {}]",
                    row.value, stat.id, stat.min, stat.max
                ),
            );
        }
    }
    let record = character_record(inputs);
    match game.character(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "character '{}' already exists with different inputs",
            inputs.id
        )),
        None => {
            check_count(game.characters.len() + 1, GAME_CHARACTERS_MAX, "characters")?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_scene(game: &Game, inputs: &SceneDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "scene id")?;
    let record = scene_record(inputs);
    match game.scene(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "scene '{}' already exists with different inputs",
            inputs.id
        )),
        None => {
            check_count(game.scenes.len() + 1, GAME_SCENES_MAX, "scenes")?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_dialogue(
    project: &Project,
    inputs: &DialogueLineDefineInput,
) -> Result<Verdict, Refusal> {
    let game = section(project)?;
    check_id(&inputs.id, "dialogue line id")?;
    check_id(&inputs.text_ref, "text reference")?;
    if game.scene(&inputs.scene).is_none() {
        return dangling("scene", &inputs.scene);
    }
    if inputs.speaker != "narrator" && game.character(&inputs.speaker).is_none() {
        return dangling("speaker character", &inputs.speaker);
    }
    if let Some(next) = &inputs.next {
        check_id(next, "next line")?;
    }
    if let Some(hook) = &inputs.hook_expression {
        check_id(hook, "hook expression")?;
    }
    check_prose(&inputs.text, "dialogue text")?;
    check_prose_upsert(project, &inputs.text_ref, &inputs.text)?;
    let record = dialogue_record(inputs);
    match game.dialogue_line(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "dialogue line '{}' already exists with different inputs",
            inputs.id
        )),
        None => {
            let in_scene = game
                .dialogue
                .iter()
                .filter(|line| line.scene == inputs.scene)
                .count();
            if usize::try_from(inputs.ordinal).unwrap_or(usize::MAX) < in_scene {
                return refuse(
                    RefusalClass::GameGraph,
                    format!(
                        "ordinal {} in scene '{}' is already taken ({} lines authored)",
                        inputs.ordinal, inputs.scene, in_scene
                    ),
                );
            }
            if usize::try_from(inputs.ordinal).unwrap_or(usize::MAX) > in_scene {
                return refuse(
                    RefusalClass::GameGraph,
                    format!(
                        "ordinal {} in scene '{}' would leave a gap ({} lines authored)",
                        inputs.ordinal, inputs.scene, in_scene
                    ),
                );
            }
            check_count(
                game.dialogue.len() + 1,
                GAME_DIALOGUE_LINES_MAX,
                "dialogue lines",
            )?;
            Ok(Verdict::Apply)
        }
    }
}
