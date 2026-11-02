//! The AUTH-G gate: `decide_game`, a pure function of (project,
//! game op) → Apply | NoOp | Refusal, under the same laws as the
//! rig gate — definition verbs are upserts (identical inputs are
//! a no-op, differing inputs are `TG-AUTH-CONFLICT`), every
//! bound lives here, and deny is final.

use crate::decide::{Verdict, check_id};
use crate::game::apply::{
    appearance_record, character_record, choice_record, clock_record, condition_record,
    dialogue_record, effect_set_record, ending_record, flag_record, gallery_record,
    save_schema_record, save_schema_version_of, scene_binding_record, scene_exit_record,
    scene_record, schedule_record, stat_record, stream_record, table_record, tier_record,
    trigger_record, value_record, visual_state_record,
};
use crate::game::model::{
    AppearanceKind, GAME_FORMAT_VERSION, GAME_FORMAT_VERSION_FIRST, GAME_HASH_ALGO, Game,
    SAVE_STATE_FIELDS,
};
use crate::game::op::{
    AppearanceDefineInput, AppearanceEntryInput, AppearanceValueInput, AtomInput,
    CharacterDefineInput, ChoiceDefineInput, ClockDefineInput, ConditionDefineInput,
    DialogueLineDefineInput, EffectSetDefineInput, EndingDefineInput, ExprInput, FlagDefineInput,
    GalleryEntryDefineInput, GalleryUnlockInput, GameDefineInput, GameOp, RngStreamDefineInput,
    SaveSchemaDefineInput, SceneBindRigInput, SceneDefineInput, SceneExitDefineInput,
    ScheduleDefineInput, StatDefineInput, StatKindInput, StatTierDefineInput, TableDefineInput,
    TriggerDefineInput, TriggerKindInput, ValueDefineInput, VisualStateDefineInput, WeightInput,
};
use crate::limits::{
    GAME_APPEARANCE_SLOTS_MAX, GAME_CATEGORY_TEXT_BYTES_MAX, GAME_CHARACTERS_MAX,
    GAME_CHOICE_OPTIONS_MAX, GAME_CHOICES_MAX, GAME_CONDITIONS_MAX, GAME_CONTENT_TEXT_BYTES_MAX,
    GAME_DIALOGUE_LINES_MAX, GAME_EFFECT_ATOMS_MAX, GAME_EFFECT_SETS_MAX, GAME_ENDINGS_MAX,
    GAME_FLAGS_MAX, GAME_GALLERY_ENTRIES_MAX, GAME_LIFETIME_DAYS_MAX, GAME_MIGRATIONS_MAX,
    GAME_OFFER_DAYS_MAX, GAME_RNG_STREAMS_MAX, GAME_SAVE_SLOTS_MAX, GAME_SCENE_BINDINGS_MAX,
    GAME_SCENE_EXITS_MAX, GAME_SCENES_MAX, GAME_SCHEDULE_OVERRIDES_MAX, GAME_SCHEDULES_MAX,
    GAME_SLOTS_MAX, GAME_STATS_MAX, GAME_TABLE_OUTCOMES_MAX, GAME_TABLES_MAX,
    GAME_TIER_THRESHOLDS_MAX, GAME_TRIGGERS_MAX, GAME_VALUES_MAX, GAME_VISUAL_STATES_MAX,
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
        GameOp::RngStreamDefine(inputs) => {
            let game = section(project)?;
            decide_stream(game, inputs)
        }
        GameOp::TableDefine(inputs) => {
            let game = section(project)?;
            decide_table(game, inputs)
        }
        GameOp::AppearanceDefine(inputs) => {
            let game = section(project)?;
            decide_appearance(game, inputs)
        }
        GameOp::ChoiceDefine(inputs) => decide_choice(project, inputs),
        GameOp::EndingDefine(inputs) => {
            let game = section(project)?;
            decide_ending(game, inputs)
        }
        GameOp::GalleryEntryDefine(inputs) => decide_gallery_entry(project, inputs),
        GameOp::GameSaveSchemaDefine(inputs) => {
            let game = section(project)?;
            decide_save_schema(game, inputs)
        }
        GameOp::ScheduleDefine(inputs) => {
            let game = section(project)?;
            decide_schedule(game, inputs)
        }
        GameOp::SceneBindRig(inputs) => decide_scene_bind_rig(project, inputs),
        GameOp::SceneExitDefine(inputs) => {
            let game = section(project)?;
            decide_scene_exit(game, inputs)
        }
        GameOp::VisualStateDefine(inputs) => {
            let game = section(project)?;
            decide_visual_state(game, inputs)
        }
    }
}

fn game_table<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::GameTable, message)
}

fn decide_stream(game: &Game, inputs: &RngStreamDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "stream id")?;
    if inputs.algorithm != crate::game::model::RNG_ALGORITHM {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "RNG algorithm '{}' is not implemented; this build implements '{}'",
                inputs.algorithm,
                crate::game::model::RNG_ALGORITHM
            ),
        );
    }
    let record = stream_record(inputs);
    upsert_verdict(
        game.stream(&inputs.id),
        &record,
        "rng stream",
        &inputs.id,
        game.streams.len() + 1,
        GAME_RNG_STREAMS_MAX,
    )
}

fn decide_table(game: &Game, inputs: &TableDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "table id")?;
    if game.stream(&inputs.stream).is_none() {
        return game_table(format!(
            "table '{}' names RNG stream '{}', which is not defined",
            inputs.id, inputs.stream
        ));
    }
    if inputs.outcomes.is_empty() {
        return game_table(format!("table '{}' has no outcomes", inputs.id));
    }
    check_count(
        inputs.outcomes.len(),
        GAME_TABLE_OUTCOMES_MAX,
        "table outcomes",
    )?;
    let mut seen: Vec<&str> = Vec::new();
    let mut all_static_total: i64 = 0;
    let mut all_static = true;
    for outcome in &inputs.outcomes {
        check_id(&outcome.id, "outcome id")?;
        if seen.contains(&outcome.id.as_str()) {
            return game_table(format!(
                "table '{}' names outcome '{}' twice",
                inputs.id, outcome.id
            ));
        }
        seen.push(&outcome.id);
        if game.effect_set(&outcome.effect_set).is_none() {
            return dangling("effect set", &outcome.effect_set);
        }
        if let Some(gate) = &outcome.gate
            && game.condition(gate).is_none()
        {
            return dangling("gate condition", gate);
        }
        match &outcome.weight {
            WeightInput::Static(weight) => {
                if *weight <= 0 {
                    return game_table(format!(
                        "outcome '{}' in table '{}' has non-positive weight {weight}",
                        outcome.id, inputs.id
                    ));
                }
                all_static_total = all_static_total.saturating_add(*weight);
            }
            WeightInput::Expr(expr) => {
                all_static = false;
                let tree = crate::game::expr::expr_from_input(expr)?;
                if crate::game::expr::type_of(game, &tree)? != crate::game::model::ExprType::Int {
                    return game_expr(format!(
                        "the weight of outcome '{}' in table '{}' must be an integer expression",
                        outcome.id, inputs.id
                    ));
                }
                if let crate::game::model::Expr::Lit(value) = tree
                    && value <= 0
                {
                    return game_table(format!(
                        "outcome '{}' in table '{}' has non-positive weight {value}",
                        outcome.id, inputs.id
                    ));
                }
            }
        }
    }
    if all_static && all_static_total <= 0 {
        return game_table(format!("table '{}' has a zero total weight", inputs.id));
    }
    let record = table_record(inputs)?;
    upsert_verdict(
        game.table(&inputs.id),
        &record,
        "table",
        &inputs.id,
        game.tables.len() + 1,
        GAME_TABLES_MAX,
    )
}

/// Checks appearance assignments against a slot vocabulary:
/// every entry names a declared slot (dangling otherwise),
/// no slot is assigned twice, and each value's form matches
/// its slot's declared kind (an ill-typed value is an
/// expression-class refusal, like an ill-typed table weight).
fn check_appearance_entries(
    slots: &[crate::game::model::AppearanceSlot],
    entries: &[AppearanceEntryInput],
    what: &str,
) -> Result<(), Refusal> {
    let mut seen: Vec<&str> = Vec::new();
    for entry in entries {
        check_id(&entry.slot, "appearance slot id")?;
        let Some(slot) = slots.iter().find(|slot| slot.id == entry.slot) else {
            return dangling("appearance slot", &entry.slot);
        };
        if seen.contains(&entry.slot.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("{what} assigns slot '{}' twice", entry.slot),
            );
        }
        seen.push(entry.slot.as_str());
        let matches = matches!(
            (&slot.kind, &entry.value),
            (AppearanceKind::Ints, AppearanceValueInput::Ints(_))
                | (AppearanceKind::Text, AppearanceValueInput::Text(_))
        );
        if !matches {
            return game_expr(format!(
                "the value for appearance slot '{}' does not match its declared kind",
                entry.slot
            ));
        }
    }
    Ok(())
}

fn decide_appearance(game: &Game, inputs: &AppearanceDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.character, "character id")?;
    if game.character(&inputs.character).is_none() {
        return dangling("character", &inputs.character);
    }
    if inputs.slots.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "an appearance table declares at least one slot",
        );
    }
    check_count(
        inputs.slots.len(),
        GAME_APPEARANCE_SLOTS_MAX,
        "appearance slots",
    )?;
    let mut seen: Vec<&str> = Vec::new();
    for slot in &inputs.slots {
        check_id(&slot.id, "appearance slot id")?;
        if seen.contains(&slot.id.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("appearance slot '{}' is declared twice", slot.id),
            );
        }
        seen.push(slot.id.as_str());
    }
    let record = appearance_record(inputs);
    check_appearance_entries(&record.slots, &inputs.base, "the baseline appearance")?;
    // The baseline is total: every declared slot has a value,
    // so any delta — including the empty one — resolves.
    for slot in &record.slots {
        if record.base.iter().all(|entry| entry.slot != slot.id) {
            return refuse(
                RefusalClass::Malformed,
                format!("the baseline appearance omits slot '{}'", slot.id),
            );
        }
    }
    upsert_verdict(
        game.appearance(&inputs.character),
        &record,
        "appearance table",
        &inputs.character,
        game.appearances.len() + 1,
        GAME_CHARACTERS_MAX,
    )
}

fn decide_visual_state(game: &Game, inputs: &VisualStateDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.character, "character id")?;
    check_id(&inputs.state, "visual state name")?;
    if game.character(&inputs.character).is_none() {
        return dangling("character", &inputs.character);
    }
    let Some(appearance) = game.appearance(&inputs.character) else {
        return dangling("appearance table for character", &inputs.character);
    };
    check_appearance_entries(&appearance.slots, &inputs.delta, "the delta")?;
    let record = visual_state_record(inputs);
    match game.visual_state(&inputs.character, &inputs.state) {
        Some(found) if *found == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "visual state '{}' for character '{}' already exists with different inputs",
            inputs.state, inputs.character
        )),
        None => {
            check_count(
                game.visual_states.len() + 1,
                GAME_VISUAL_STATES_MAX,
                "visual states",
            )?;
            Ok(Verdict::Apply)
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
        AtomInput::RollTable { table } => {
            if game.table(table).is_none() {
                return dangling("table", table);
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
    if let Some(veto) = &inputs.alter_ego_veto {
        check_id(veto, "alter-ego veto effect set")?;
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
                && game.alter_ego_veto == inputs.alter_ego_veto
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
    let mut seen: Vec<&str> = Vec::with_capacity(inputs.participants.len());
    for participant in &inputs.participants {
        check_id(participant, "scene participant")?;
        if seen.contains(&participant.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "participant '{participant}' is named twice in scene '{}'",
                    inputs.id
                ),
            );
        }
        seen.push(participant);
        if game.character(participant).is_none() {
            return dangling("participant character", participant);
        }
    }
    if let Some(backdrop) = &inputs.backdrop {
        check_id(backdrop, "scene backdrop")?;
    }
    if let Some(condition) = &inputs.entry_condition {
        check_id(condition, "entry condition")?;
        if game.condition(condition).is_none() {
            return dangling("entry condition", condition);
        }
    }
    for (what, set) in [
        ("on_enter effect set", &inputs.on_enter),
        ("on_exit effect set", &inputs.on_exit),
    ] {
        if let Some(set) = set {
            check_id(set, what)?;
            if game.effect_set(set).is_none() {
                return dangling(what, set);
            }
        }
    }
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

fn decide_save_schema(game: &Game, inputs: &SaveSchemaDefineInput) -> Result<Verdict, Refusal> {
    if inputs.version == 0 {
        return refuse(
            RefusalClass::GameSaveSchema,
            "save schema version 0 is below 1",
        );
    }
    if inputs.slots == 0 || inputs.slots > GAME_SAVE_SLOTS_MAX {
        return refuse(
            RefusalClass::GameSaveSchema,
            format!(
                "save schema slot count {} is outside 1..={GAME_SAVE_SLOTS_MAX}",
                inputs.slots
            ),
        );
    }
    match inputs.slot_holds.as_str() {
        "chapter_list" | "one_run" => {}
        other => {
            return refuse(
                RefusalClass::UnknownKind,
                format!("slot_holds '{other}' is not a known slot shape"),
            );
        }
    }
    if !inputs.hash_algorithm.contains('/') {
        return refuse(
            RefusalClass::GameSaveSchema,
            format!(
                "hash algorithm '{}' is unversioned; name a versioned algorithm",
                inputs.hash_algorithm
            ),
        );
    }
    if inputs.hash_algorithm != GAME_HASH_ALGO {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "hash algorithm '{}' is not a known algorithm",
                inputs.hash_algorithm
            ),
        );
    }
    // The partition sweep: an exact cover of the canonical
    // state-field set (Amendment 2 §G3.3).
    let mut covered: Vec<&str> = Vec::with_capacity(SAVE_STATE_FIELDS.len());
    for field in inputs
        .slot_fields
        .iter()
        .chain(inputs.globals_fields.iter())
    {
        if !SAVE_STATE_FIELDS.contains(&field.as_str()) {
            return refuse(
                RefusalClass::GameSaveSchema,
                format!("state field '{field}' is not a game-state field"),
            );
        }
        if covered.contains(&field.as_str()) {
            return refuse(
                RefusalClass::GameSaveSchema,
                format!("state field '{field}' is assigned twice (partitions overlap)"),
            );
        }
        covered.push(field);
    }
    if covered.len() != SAVE_STATE_FIELDS.len() {
        let missing: Vec<&str> = SAVE_STATE_FIELDS
            .iter()
            .copied()
            .filter(|field| !covered.contains(field))
            .collect();
        return refuse(
            RefusalClass::GameSaveSchema,
            format!(
                "the partition leaves state fields unassigned: {}",
                missing.join(", ")
            ),
        );
    }
    // The migration chain segment: named steps, contiguous,
    // ending at version - 1 (authoring rule per Amendment 2).
    check_count(inputs.migrations.len(), GAME_MIGRATIONS_MAX, "migrations")?;
    let mut steps: Vec<u32> = inputs.migrations.iter().map(|step| step.from).collect();
    steps.sort_unstable();
    if inputs.version == 1 && !steps.is_empty() {
        return refuse(
            RefusalClass::GameSaveSchema,
            "a version-1 schema has nothing to migrate from",
        );
    }
    if inputs.version > 1 && steps.is_empty() {
        return refuse(
            RefusalClass::GameSaveSchema,
            format!(
                "schema version {} needs a migration chain, and none was given",
                inputs.version
            ),
        );
    }
    for (index, from) in steps.iter().enumerate() {
        let expected = steps[0] + u32::try_from(index).unwrap_or(u32::MAX);
        if *from != expected {
            return refuse(
                RefusalClass::GameSaveSchema,
                format!("migration from {from} does not continue the chain (expected {expected})"),
            );
        }
    }
    if let Some(last) = steps.last()
        && *last != inputs.version - 1
    {
        return refuse(
            RefusalClass::GameSaveSchema,
            format!(
                "the migration chain ends at version {last}; it must end at {}",
                inputs.version - 1
            ),
        );
    }
    for step in &inputs.migrations {
        if step.name.trim().is_empty() {
            return refuse(RefusalClass::Malformed, "a migration step needs a name");
        }
    }
    let record = save_schema_record(inputs);
    match &game.save_schema {
        None => Ok(Verdict::Apply),
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(existing) => {
            if inputs.version <= existing.version {
                return refuse(
                    RefusalClass::GameSaveSchema,
                    format!(
                        "save schema version {} does not strictly increase over the authored {}",
                        inputs.version, existing.version
                    ),
                );
            }
            if steps.first() != Some(&existing.version) {
                return refuse(
                    RefusalClass::GameSaveSchema,
                    format!(
                        "the migration chain starts at {:?}; it must start at the prior schema's version {}",
                        steps.first(),
                        existing.version
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

fn decide_choice(project: &Project, inputs: &ChoiceDefineInput) -> Result<Verdict, Refusal> {
    let game = section(project)?;
    check_id(&inputs.id, "choice id")?;
    check_id(&inputs.scene, "choice scene")?;
    if game.scene(&inputs.scene).is_none() {
        return dangling("scene", &inputs.scene);
    }
    if let Some(prompt) = &inputs.prompt_line {
        check_id(prompt, "prompt content id")?;
        if !project.content.iter().any(|entry| entry.id == *prompt) {
            return refuse(
                RefusalClass::Dangling,
                format!("prompt content '{prompt}' is not in the content sidecar"),
            );
        }
    }
    if inputs.options.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            format!("choice '{}' has no options", inputs.id),
        );
    }
    check_count(
        inputs.options.len(),
        GAME_CHOICE_OPTIONS_MAX,
        "choice options",
    )?;
    let mut seen: Vec<&str> = Vec::with_capacity(inputs.options.len());
    let mut unconditional = false;
    for option in &inputs.options {
        check_id(&option.id, "choice option id")?;
        check_id(&option.label_ref, "option label reference")?;
        if seen.contains(&option.id.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "option id '{}' is named twice in choice '{}'",
                    option.id, inputs.id
                ),
            );
        }
        seen.push(&option.id);
        if !project
            .content
            .iter()
            .any(|entry| entry.id == option.label_ref)
        {
            return refuse(
                RefusalClass::Dangling,
                format!(
                    "label content '{}' is not in the content sidecar",
                    option.label_ref
                ),
            );
        }
        if option.visible_if.is_none() && option.enabled_if.is_none() {
            unconditional = true;
        }
        for (what, condition) in [
            ("visible_if condition", &option.visible_if),
            ("enabled_if condition", &option.enabled_if),
        ] {
            if let Some(condition) = condition {
                check_id(condition, what)?;
                if game.condition(condition).is_none() {
                    return dangling(what, condition);
                }
            }
        }
        match option.origin.as_str() {
            "alter_ego" | "player" => {}
            other => {
                return refuse(
                    RefusalClass::UnknownKind,
                    format!("choice option origin '{other}' is not a known origin"),
                );
            }
        }
        if let Some(sort) = &option.sort {
            let expr = crate::game::expr::expr_from_input(sort)?;
            let ty = crate::game::expr::type_of(game, &expr)?;
            if ty != crate::game::model::ExprType::Int {
                return refuse(
                    RefusalClass::GameExpr,
                    format!(
                        "choice '{}' option '{}' sort is not an integer expression",
                        inputs.id, option.id
                    ),
                );
            }
        }
        if let Some(set) = &option.effect_set {
            check_id(set, "option effect set")?;
            if game.effect_set(set).is_none() {
                return dangling("option effect set", set);
            }
        }
        match &option.target {
            crate::game::op::ChoiceTargetInput::EndingCheck => {}
            crate::game::op::ChoiceTargetInput::Exit { id } => {
                check_id(id, "exit target")?;
                if game.scene_exit(&inputs.scene, id).is_none() {
                    return dangling("scene exit", id);
                }
            }
            crate::game::op::ChoiceTargetInput::Line { id } => {
                check_id(id, "line target")?;
                match game.dialogue_line(id) {
                    None => return dangling("dialogue line", id),
                    Some(line) if line.scene != inputs.scene => {
                        return refuse(
                            RefusalClass::GameGraph,
                            format!(
                                "choice '{}' option '{}' targets line '{id}' of scene '{}'; a line target must stay in the choice's scene '{}'",
                                inputs.id, option.id, line.scene, inputs.scene
                            ),
                        );
                    }
                    Some(_) => {}
                }
            }
            crate::game::op::ChoiceTargetInput::Scene { id } => {
                check_id(id, "scene target")?;
                if game.scene(id).is_none() {
                    return dangling("scene", id);
                }
            }
        }
    }
    if !unconditional {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "choice '{}' has no unconditional option: a player could face an empty choice",
                inputs.id
            ),
        );
    }
    let record = choice_record(inputs)?;
    upsert_verdict(
        game.choice(&inputs.id),
        &record,
        "choice",
        &inputs.id,
        game.choices.len() + 1,
        GAME_CHOICES_MAX,
    )
}

fn decide_ending(game: &Game, inputs: &EndingDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "ending id")?;
    check_id(&inputs.condition, "ending condition")?;
    check_id(&inputs.epilogue_scene, "epilogue scene")?;
    if game.condition(&inputs.condition).is_none() {
        return dangling("ending condition", &inputs.condition);
    }
    if game.scene(&inputs.epilogue_scene).is_none() {
        return dangling("epilogue scene", &inputs.epilogue_scene);
    }
    if game
        .endings
        .iter()
        .any(|ending| ending.id != inputs.id && ending.priority == inputs.priority)
    {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "ending priority {} is already taken: priorities are unique",
                inputs.priority
            ),
        );
    }
    let record = ending_record(inputs);
    upsert_verdict(
        game.ending(&inputs.id),
        &record,
        "ending",
        &inputs.id,
        game.endings.len() + 1,
        GAME_ENDINGS_MAX,
    )
}

fn decide_gallery_entry(
    project: &Project,
    inputs: &GalleryEntryDefineInput,
) -> Result<Verdict, Refusal> {
    let game = section(project)?;
    check_id(&inputs.id, "gallery entry id")?;
    if inputs.category.trim().is_empty() || inputs.category.len() > GAME_CATEGORY_TEXT_BYTES_MAX {
        return refuse(
            RefusalClass::Malformed,
            format!(
                "gallery category must be usable text of at most {GAME_CATEGORY_TEXT_BYTES_MAX} bytes"
            ),
        );
    }
    check_id(&inputs.replay_scene, "replay scene")?;
    check_id(&inputs.replay_line, "replay line")?;
    if game.scene(&inputs.replay_scene).is_none() {
        return dangling("replay scene", &inputs.replay_scene);
    }
    if game.dialogue_line(&inputs.replay_line).is_none() {
        return dangling("replay line", &inputs.replay_line);
    }
    if let GalleryUnlockInput::Condition { id } = &inputs.unlock {
        check_id(id, "unlock condition")?;
        if game.condition(id).is_none() {
            return dangling("unlock condition", id);
        }
    }
    if let Some(thumbnail) = &inputs.thumbnail {
        check_id(thumbnail, "thumbnail asset")?;
        if project.asset(thumbnail).is_none() {
            return dangling("thumbnail asset", thumbnail);
        }
    }
    let record = gallery_record(inputs);
    upsert_verdict(
        game.gallery_entry(&inputs.id),
        &record,
        "gallery entry",
        &inputs.id,
        game.gallery_entries.len() + 1,
        GAME_GALLERY_ENTRIES_MAX,
    )
}

fn decide_schedule(game: &Game, inputs: &ScheduleDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.character, "schedule character")?;
    if game.character(&inputs.character).is_none() {
        return dangling("character", &inputs.character);
    }
    let Some(clock) = &game.clock else {
        return refuse(
            RefusalClass::GameGraph,
            "a schedule needs the game's clock (the slot set is the clock's)",
        );
    };
    let mut seen: Vec<&str> = Vec::with_capacity(inputs.slots.len());
    for row in &inputs.slots {
        check_id(&row.scene, "schedule scene")?;
        if !clock.slots.iter().any(|slot| slot == &row.slot) {
            return refuse(
                RefusalClass::GameGraph,
                format!("slot '{}' is not a clock slot", row.slot),
            );
        }
        if seen.contains(&row.slot.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("slot '{}' is given twice in the schedule", row.slot),
            );
        }
        seen.push(&row.slot);
        if game.scene(&row.scene).is_none() {
            return dangling("schedule scene", &row.scene);
        }
    }
    for slot in &clock.slots {
        if !seen.contains(&slot.as_str()) {
            return refuse(
                RefusalClass::GameGraph,
                format!("the schedule leaves clock slot '{slot}' unassigned"),
            );
        }
    }
    check_count(
        inputs.overrides.len(),
        GAME_SCHEDULE_OVERRIDES_MAX,
        "schedule overrides",
    )?;
    let mut conditions: Vec<&str> = Vec::with_capacity(inputs.overrides.len());
    for row in &inputs.overrides {
        check_id(&row.condition, "override condition")?;
        check_id(&row.scene, "override scene")?;
        if conditions.contains(&row.condition.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("override condition '{}' is given twice", row.condition),
            );
        }
        conditions.push(&row.condition);
        if game.condition(&row.condition).is_none() {
            return dangling("override condition", &row.condition);
        }
        if game.scene(&row.scene).is_none() {
            return dangling("override scene", &row.scene);
        }
    }
    let record = schedule_record(inputs);
    upsert_verdict(
        game.schedule(&inputs.character),
        &record,
        "schedule",
        &inputs.character,
        game.schedules.len() + 1,
        GAME_SCHEDULES_MAX,
    )
}

fn decide_scene_bind_rig(
    project: &Project,
    inputs: &SceneBindRigInput,
) -> Result<Verdict, Refusal> {
    let game = section(project)?;
    check_id(&inputs.scene, "scene id")?;
    check_id(&inputs.participant, "participant id")?;
    check_id(&inputs.rig_model, "rig model")?;
    check_id(&inputs.entry_expression, "entry expression")?;
    let Some(scene) = game.scene(&inputs.scene) else {
        return dangling("scene", &inputs.scene);
    };
    if !scene.participants.iter().any(|p| p == &inputs.participant) {
        return refuse(
            RefusalClass::GameGraph,
            format!(
                "'{}' is not a participant of scene '{}'",
                inputs.participant, inputs.scene
            ),
        );
    }
    if project.expression(&inputs.entry_expression).is_none() {
        return dangling("entry expression", &inputs.entry_expression);
    }
    if let Some(motion) = &inputs.entry_motion {
        check_id(motion, "entry motion")?;
        if project.motion(motion).is_none() {
            return dangling("entry motion", motion);
        }
    }
    let record = scene_binding_record(inputs);
    // Set semantics (the branch's rule): rebinding is a set,
    // identical bindings are a no-op, differing ones APPLY —
    // never a conflict.
    match game.scene_binding(&inputs.scene, &inputs.participant) {
        Some(found) if *found == record => Ok(Verdict::NoOp),
        Some(_) => Ok(Verdict::Apply),
        None => {
            check_count(
                game.scene_bindings.len() + 1,
                GAME_SCENE_BINDINGS_MAX,
                "scene bindings",
            )?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_scene_exit(game: &Game, inputs: &SceneExitDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.scene, "scene id")?;
    check_id(&inputs.id, "exit id")?;
    check_id(&inputs.target, "exit target")?;
    if game.scene(&inputs.scene).is_none() {
        return dangling("scene", &inputs.scene);
    }
    if game.scene(&inputs.target).is_none() {
        return dangling("exit target scene", &inputs.target);
    }
    if let Some(condition) = &inputs.condition {
        check_id(condition, "exit condition")?;
        if game.condition(condition).is_none() {
            return dangling("exit condition", condition);
        }
    }
    if let Some(set) = &inputs.effect_set {
        check_id(set, "exit effect set")?;
        if game.effect_set(set).is_none() {
            return dangling("exit effect set", set);
        }
    }
    let record = scene_exit_record(inputs);
    upsert_verdict(
        game.scene_exit(&inputs.scene, &inputs.id),
        &record,
        "scene exit",
        &inputs.id,
        game.scene_exits.len() + 1,
        GAME_SCENE_EXITS_MAX,
    )
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
