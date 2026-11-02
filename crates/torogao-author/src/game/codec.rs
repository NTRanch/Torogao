//! Game record ↔ RON mapping: the `game` section of
//! `project.ron` and the `content.ron` sidecar document.
//!
//! Same discipline as the rig codec: one emitter, field order
//! `id` first then alphabetical, unknown kinds are
//! `TG-AUTH-UNKNOWN-KIND`, missing/unknown/ill-typed fields are
//! `TG-AUTH-MALFORMED`, and canonicality is decided by
//! re-emitting and comparing bytes. Record names are dispatched
//! positionally (an expression `Stat(...)` node and a stat
//! definition `Stat(...)` are different records in different
//! positions), the same convention the rig codec uses for its
//! `Value` rows.

use crate::codec::{
    Fields, as_i64, as_strings, as_u32, collect, open, open_any, opt, record, sorted_unique, text,
    texts, unknown_variant, wrong_type,
};
use crate::game::model::{
    AppearanceDef, AppearanceEntry, AppearanceKind, AppearanceSlot, AppearanceValue, Atom,
    CharacterDef, Clock, ClockField, CmpOp, ConditionDef, ContentEntry, DialogueLine, Direction,
    EffectSetDef, Expr, FlagDef, FlagEdge, FlagScope, Game, Lifetime, OfferDef, Overflow,
    Presentation, Repetition, RngStreamDef, SceneDef, StatDef, StatInitial, StatKind, StatScope,
    TableDef, TableOutcome, TableWeight, TierDef, TriggerDef, TriggerKind, ValueDef,
    VisualStateDef,
};
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::ron::Ron;

// ---- game section emission ---------------------------------------

/// Emits the game section as a `Game(...)` RON value.
pub(crate) fn game_to_ron(game: &Game) -> Ron {
    let mut fields: Vec<(&str, Ron)> = vec![
        ("id", text(&game.id)),
        (
            "characters",
            Ron::List(game.characters.iter().map(character_to_ron).collect()),
        ),
        (
            "clock",
            opt(game.clock.as_ref(), |clock| {
                record(
                    "Clock",
                    vec![
                        (
                            "day_limit",
                            opt(clock.day_limit.as_ref(), |v| Ron::Int(i64::from(*v))),
                        ),
                        (
                            "days_per_week",
                            opt(clock.days_per_week.as_ref(), |v| Ron::Int(i64::from(*v))),
                        ),
                        ("slots", texts(&clock.slots)),
                    ],
                )
            }),
        ),
        (
            "conditions",
            Ron::List(game.conditions.iter().map(condition_to_ron).collect()),
        ),
        (
            "dialogue",
            Ron::List(game.dialogue.iter().map(dialogue_to_ron).collect()),
        ),
        (
            "effect_sets",
            Ron::List(game.effect_sets.iter().map(effect_set_to_ron).collect()),
        ),
        (
            "flags",
            Ron::List(game.flags.iter().map(flag_to_ron).collect()),
        ),
        ("format_version", Ron::Int(game.format_version)),
        ("save_schema_version", Ron::Int(game.save_schema_version)),
        (
            "scenes",
            Ron::List(game.scenes.iter().map(scene_to_ron).collect()),
        ),
        ("start_scene", opt(game.start_scene.as_ref(), |id| text(id))),
        (
            "stats",
            Ron::List(game.stats.iter().map(stat_to_ron).collect()),
        ),
        (
            "tiers",
            Ron::List(game.tiers.iter().map(tier_to_ron).collect()),
        ),
        (
            "triggers",
            Ron::List(game.triggers.iter().map(trigger_to_ron).collect()),
        ),
        (
            "values",
            Ron::List(game.values.iter().map(value_def_to_ron).collect()),
        ),
    ];
    // The later surfaces — appearances, streams, tables,
    // visual states — are emitted only when present (absent
    // means absent): a project authored before a surface — the
    // plain-VN fixture predates all four — keeps its exact
    // bytes.
    if !game.tables.is_empty() {
        insert_field(
            &mut fields,
            "tiers",
            "tables",
            Ron::List(game.tables.iter().map(table_to_ron).collect()),
        );
    }
    if !game.streams.is_empty() {
        insert_field(
            &mut fields,
            "tiers",
            "streams",
            Ron::List(game.streams.iter().map(stream_to_ron).collect()),
        );
    }
    if !game.appearances.is_empty() {
        insert_field(
            &mut fields,
            "characters",
            "appearances",
            Ron::List(game.appearances.iter().map(appearance_to_ron).collect()),
        );
    }
    if !game.visual_states.is_empty() {
        fields.push((
            "visual_states",
            Ron::List(game.visual_states.iter().map(visual_state_to_ron).collect()),
        ));
    }
    record("Game", fields)
}

/// Inserts one field just before the named anchor field (or
/// at the end when the anchor is absent), preserving the
/// section's alphabetical order.
fn insert_field(fields: &mut Vec<(&str, Ron)>, anchor: &str, name: &'static str, value: Ron) {
    let pos = fields
        .iter()
        .position(|(field, _)| *field == anchor)
        .unwrap_or(fields.len());
    fields.insert(pos, (name, value));
}

fn appearance_to_ron(appearance: &AppearanceDef) -> Ron {
    record(
        "Appearance",
        vec![
            (
                "base",
                Ron::List(
                    appearance
                        .base
                        .iter()
                        .map(appearance_entry_to_ron)
                        .collect(),
                ),
            ),
            ("character", text(&appearance.character)),
            (
                "slots",
                Ron::List(
                    appearance
                        .slots
                        .iter()
                        .map(appearance_slot_to_ron)
                        .collect(),
                ),
            ),
        ],
    )
}

fn appearance_slot_to_ron(slot: &AppearanceSlot) -> Ron {
    record(
        "AppearanceSlot",
        vec![
            ("id", text(&slot.id)),
            (
                "kind",
                Ron::Ident(match slot.kind {
                    AppearanceKind::Ints => "Ints".to_string(),
                    AppearanceKind::Text => "Text".to_string(),
                }),
            ),
        ],
    )
}

fn appearance_entry_to_ron(entry: &AppearanceEntry) -> Ron {
    record(
        "AppearanceEntry",
        vec![
            ("slot", text(&entry.slot)),
            (
                "value",
                match &entry.value {
                    AppearanceValue::Ints(values) => {
                        Ron::List(values.iter().map(|v| Ron::Int(*v)).collect())
                    }
                    AppearanceValue::Text(token) => text(token),
                },
            ),
        ],
    )
}

fn visual_state_to_ron(state: &VisualStateDef) -> Ron {
    record(
        "VisualState",
        vec![
            ("character", text(&state.character)),
            (
                "delta",
                Ron::List(state.delta.iter().map(appearance_entry_to_ron).collect()),
            ),
            ("state", text(&state.state)),
        ],
    )
}

fn stream_to_ron(stream: &RngStreamDef) -> Ron {
    record(
        "RngStream",
        vec![
            ("algorithm", text(&stream.algorithm)),
            ("id", text(&stream.id)),
        ],
    )
}

fn table_to_ron(table: &TableDef) -> Ron {
    record(
        "Table",
        vec![
            ("id", text(&table.id)),
            (
                "outcomes",
                Ron::List(table.outcomes.iter().map(outcome_to_ron).collect()),
            ),
            ("stream", text(&table.stream)),
        ],
    )
}

fn outcome_to_ron(outcome: &TableOutcome) -> Ron {
    record(
        "TableOutcome",
        vec![
            ("effect_set", text(&outcome.effect_set)),
            ("gate", opt(outcome.gate.as_ref(), |id| text(id))),
            ("id", text(&outcome.id)),
            (
                "weight",
                match &outcome.weight {
                    TableWeight::Expr(expr) => expr_to_ron(expr),
                    TableWeight::Static(weight) => Ron::Int(*weight),
                },
            ),
        ],
    )
}

fn character_to_ron(character: &CharacterDef) -> Ron {
    record(
        "Character",
        vec![
            ("id", text(&character.id)),
            (
                "initials",
                Ron::List(
                    character
                        .initials
                        .iter()
                        .map(|initial| {
                            record(
                                "Initial",
                                vec![
                                    ("stat", text(&initial.stat)),
                                    ("value", Ron::Int(initial.value)),
                                ],
                            )
                        })
                        .collect(),
                ),
            ),
            ("name_ref", text(&character.name_ref)),
        ],
    )
}

fn condition_to_ron(condition: &ConditionDef) -> Ron {
    record(
        "Condition",
        vec![
            ("id", text(&condition.id)),
            ("expr", expr_to_ron(&condition.expr)),
        ],
    )
}

fn dialogue_to_ron(line: &DialogueLine) -> Ron {
    record(
        "DialogueLine",
        vec![
            ("id", text(&line.id)),
            (
                "hook_expression",
                opt(line.hook_expression.as_ref(), |id| text(id)),
            ),
            ("next", opt(line.next.as_ref(), |id| text(id))),
            ("ordinal", Ron::Int(i64::from(line.ordinal))),
            ("scene", text(&line.scene)),
            ("speaker", text(&line.speaker)),
            ("text_ref", text(&line.text_ref)),
        ],
    )
}

fn effect_set_to_ron(set: &EffectSetDef) -> Ron {
    record(
        "EffectSet",
        vec![
            ("id", text(&set.id)),
            (
                "atoms",
                Ron::List(set.atoms.iter().map(atom_to_ron).collect()),
            ),
        ],
    )
}

fn atom_to_ron(atom: &Atom) -> Ron {
    match atom {
        Atom::EmitEvent { event } => record("EmitEvent", vec![("event", text(event))]),
        Atom::EnterScene { scene } => record("EnterScene", vec![("scene", text(scene))]),
        Atom::FlagClear { flag } => record("FlagClear", vec![("flag", text(flag))]),
        Atom::FlagSet { flag } => record("FlagSet", vec![("flag", text(flag))]),
        Atom::RollTable { table } => record("RollTable", vec![("table", text(table))]),
        Atom::StatAdd {
            character,
            stat,
            amount,
        } => record(
            "StatAdd",
            vec![
                ("amount", expr_to_ron(amount)),
                ("character", opt(character.as_ref(), |id| text(id))),
                ("stat", text(stat)),
            ],
        ),
        Atom::StatSet {
            character,
            stat,
            value,
        } => record(
            "StatSet",
            vec![
                ("character", opt(character.as_ref(), |id| text(id))),
                ("stat", text(stat)),
                ("value", expr_to_ron(value)),
            ],
        ),
    }
}

fn flag_to_ron(flag: &FlagDef) -> Ron {
    record(
        "Flag",
        vec![
            ("id", text(&flag.id)),
            ("default", Ron::Bool(flag.default)),
            (
                "scope",
                Ron::Ident(
                    match flag.scope {
                        FlagScope::Globals => "Globals",
                        FlagScope::Run => "Run",
                    }
                    .to_string(),
                ),
            ),
        ],
    )
}

fn scene_to_ron(scene: &SceneDef) -> Ron {
    record(
        "Scene",
        vec![
            ("id", text(&scene.id)),
            (
                "presentation",
                Ron::Ident(
                    match scene.presentation {
                        Presentation::Event => "Event",
                        Presentation::Room => "Room",
                        Presentation::Vn => "Vn",
                    }
                    .to_string(),
                ),
            ),
        ],
    )
}

fn stat_to_ron(stat: &StatDef) -> Ron {
    let kind = match &stat.kind {
        StatKind::Enum(values) => record("Enum", vec![("values", texts(values))]),
        StatKind::Fixed => Ron::Ident("Fixed".to_string()),
        StatKind::Int => Ron::Ident("Int".to_string()),
    };
    record(
        "Stat",
        vec![
            ("id", text(&stat.id)),
            ("default", Ron::Int(stat.default)),
            ("kind", kind),
            (
                "lifetime",
                opt(stat.lifetime.as_ref(), |lifetime| {
                    record(
                        "Lifetime",
                        vec![("days", Ron::Int(i64::from(lifetime.days)))],
                    )
                }),
            ),
            ("max", Ron::Int(stat.max)),
            ("min", Ron::Int(stat.min)),
            (
                "overflow",
                Ron::Ident(
                    match stat.overflow {
                        Overflow::Clamp => "Clamp",
                        Overflow::Refuse => "Refuse",
                    }
                    .to_string(),
                ),
            ),
            (
                "scope",
                Ron::Ident(
                    match stat.scope {
                        StatScope::Character => "Character",
                        StatScope::Game => "Game",
                        StatScope::Party => "Party",
                    }
                    .to_string(),
                ),
            ),
        ],
    )
}

fn tier_to_ron(tier: &TierDef) -> Ron {
    record(
        "Tier",
        vec![
            ("stat", text(&tier.stat)),
            ("names", texts(&tier.names)),
            (
                "thresholds",
                Ron::List(
                    tier.thresholds
                        .iter()
                        .map(|value| Ron::Int(*value))
                        .collect(),
                ),
            ),
        ],
    )
}

fn trigger_to_ron(trigger: &TriggerDef) -> Ron {
    record(
        "Trigger",
        vec![
            ("id", text(&trigger.id)),
            (
                "cooldown_ticks",
                Ron::Int(i64::from(trigger.cooldown_ticks)),
            ),
            ("effect_set", text(&trigger.effect_set)),
            ("gate", opt(trigger.gate.as_ref(), |id| text(id))),
            ("kind", trigger_kind_to_ron(&trigger.kind)),
            (
                "offer",
                opt(trigger.offer.as_ref(), |offer| {
                    record(
                        "Offer",
                        vec![(
                            "expires_days",
                            opt(offer.expires_days.as_ref(), |v| Ron::Int(i64::from(*v))),
                        )],
                    )
                }),
            ),
            ("priority", Ron::Int(trigger.priority)),
            (
                "repetition",
                Ron::Ident(
                    match trigger.repetition {
                        Repetition::Once => "Once",
                        Repetition::Repeat => "Repeat",
                    }
                    .to_string(),
                ),
            ),
        ],
    )
}

fn trigger_kind_to_ron(kind: &TriggerKind) -> Ron {
    match kind {
        TriggerKind::OnEnterScene { scene } => record("OnEnterScene", vec![("scene", text(scene))]),
        TriggerKind::OnEvent { event } => record("OnEvent", vec![("event", text(event))]),
        TriggerKind::OnFlag { flag, edge } => record(
            "OnFlag",
            vec![
                (
                    "edge",
                    Ron::Ident(
                        match edge {
                            FlagEdge::Both => "Both",
                            FlagEdge::Falling => "Falling",
                            FlagEdge::Rising => "Rising",
                        }
                        .to_string(),
                    ),
                ),
                ("flag", text(flag)),
            ],
        ),
        TriggerKind::OnThreshold {
            character,
            stat,
            threshold,
            direction,
        } => record(
            "OnThreshold",
            vec![
                ("character", opt(character.as_ref(), |id| text(id))),
                (
                    "direction",
                    Ron::Ident(
                        match direction {
                            Direction::Falling => "Falling",
                            Direction::Rising => "Rising",
                        }
                        .to_string(),
                    ),
                ),
                ("stat", text(stat)),
                ("threshold", Ron::Int(*threshold)),
            ],
        ),
        TriggerKind::OnTick {
            slots,
            weekdays,
            day_modulo,
            exact_day,
        } => record(
            "OnTick",
            vec![
                (
                    "day_modulo",
                    opt(day_modulo.as_ref(), |(period, remainder)| {
                        Ron::Tuple(vec![
                            Ron::Int(i64::from(*period)),
                            Ron::Int(i64::from(*remainder)),
                        ])
                    }),
                ),
                ("exact_day", opt(exact_day.as_ref(), |day| Ron::Int(*day))),
                ("slots", opt(slots.as_ref(), |slots| texts(slots))),
                (
                    "weekdays",
                    opt(weekdays.as_ref(), |days| {
                        Ron::List(days.iter().map(|day| Ron::Int(i64::from(*day))).collect())
                    }),
                ),
            ],
        ),
    }
}

fn value_def_to_ron(value: &ValueDef) -> Ron {
    record(
        "Value",
        vec![
            ("id", text(&value.id)),
            ("expr", expr_to_ron(&value.expr)),
            (
                "value_type",
                Ron::Ident(
                    match value.value_type {
                        crate::game::model::ExprType::Bool => "Bool",
                        crate::game::model::ExprType::Int => "Int",
                    }
                    .to_string(),
                ),
            ),
        ],
    )
}

/// Emits one expression tree.
pub(crate) fn expr_to_ron(expr: &Expr) -> Ron {
    match expr {
        Expr::Add(left, right) => binary("Add", left, right),
        Expr::And(args) => record("And", vec![("args", exprs_to_ron(args))]),
        Expr::BoolLit(value) => record("BoolLit", vec![("value", Ron::Bool(*value))]),
        Expr::Clamp(value, lo, hi) => record(
            "Clamp",
            vec![
                ("hi", expr_to_ron(hi)),
                ("lo", expr_to_ron(lo)),
                ("value", expr_to_ron(value)),
            ],
        ),
        Expr::Clock(field) => record(
            "Clock",
            vec![(
                "field",
                Ron::Ident(
                    match field {
                        ClockField::Day => "Day",
                        ClockField::DaysRemaining => "DaysRemaining",
                        ClockField::Slot => "Slot",
                        ClockField::Weekday => "Weekday",
                    }
                    .to_string(),
                ),
            )],
        ),
        Expr::Cmp(op, left, right) => record(
            "Cmp",
            vec![
                ("left", expr_to_ron(left)),
                (
                    "op",
                    Ron::Ident(
                        match op {
                            CmpOp::Eq => "Eq",
                            CmpOp::Ge => "Ge",
                            CmpOp::Gt => "Gt",
                            CmpOp::Le => "Le",
                            CmpOp::Lt => "Lt",
                            CmpOp::Ne => "Ne",
                        }
                        .to_string(),
                    ),
                ),
                ("right", expr_to_ron(right)),
            ],
        ),
        Expr::Flag(flag) => record("Flag", vec![("flag", text(flag))]),
        Expr::Lit(value) => record("Lit", vec![("value", Ron::Int(*value))]),
        Expr::Max(left, right) => binary("Max", left, right),
        Expr::Min(left, right) => binary("Min", left, right),
        Expr::Mul(left, right) => binary("Mul", left, right),
        Expr::Neg(inner) => record("Neg", vec![("expr", expr_to_ron(inner))]),
        Expr::Not(inner) => record("Not", vec![("expr", expr_to_ron(inner))]),
        Expr::Or(args) => record("Or", vec![("args", exprs_to_ron(args))]),
        Expr::Stat { character, stat } => record(
            "Stat",
            vec![
                ("character", opt(character.as_ref(), |id| text(id))),
                ("stat", text(stat)),
            ],
        ),
        Expr::Sub(left, right) => binary("Sub", left, right),
        Expr::TierOf { character, stat } => record(
            "TierOf",
            vec![
                ("character", opt(character.as_ref(), |id| text(id))),
                ("stat", text(stat)),
            ],
        ),
        Expr::Value(id) => record("Value", vec![("value", text(id))]),
    }
}

fn binary(name: &str, left: &Expr, right: &Expr) -> Ron {
    record(
        name,
        vec![("left", expr_to_ron(left)), ("right", expr_to_ron(right))],
    )
}

fn exprs_to_ron(exprs: &[Expr]) -> Ron {
    Ron::List(exprs.iter().map(expr_to_ron).collect())
}

// ---- game section parsing ----------------------------------------

/// Parses a `Game(...)` value into the game section.
pub(crate) fn game_from_ron(value: Ron) -> Result<Game, Refusal> {
    let mut fields = open(value, "Game")?;
    let characters = collect(fields.list("characters")?, character_from_ron)?;
    let clock = fields.option("clock")?.map(clock_from_ron).transpose()?;
    let conditions = collect(fields.list("conditions")?, condition_from_ron)?;
    let dialogue = collect(fields.list("dialogue")?, dialogue_from_ron)?;
    let effect_sets = collect(fields.list("effect_sets")?, effect_set_from_ron)?;
    let flags = collect(fields.list("flags")?, flag_from_ron)?;
    let scenes = collect(fields.list("scenes")?, scene_from_ron)?;
    let stats = collect(fields.list("stats")?, stat_from_ron)?;
    // Streams/tables are absent-means-absent fields: a section
    // written before this surface has neither key.
    let streams = match fields.try_take("streams") {
        None => Vec::new(),
        Some(Ron::List(items)) => collect(items, stream_from_ron)?,
        Some(_) => return wrong_type("streams", "a list"),
    };
    let tables = match fields.try_take("tables") {
        None => Vec::new(),
        Some(Ron::List(items)) => collect(items, table_from_ron)?,
        Some(_) => return wrong_type("tables", "a list"),
    };
    // Appearances/visual states are absent-means-absent too.
    let appearances = match fields.try_take("appearances") {
        None => Vec::new(),
        Some(Ron::List(items)) => collect(items, appearance_from_ron)?,
        Some(_) => return wrong_type("appearances", "a list"),
    };
    let visual_states = match fields.try_take("visual_states") {
        None => Vec::new(),
        Some(Ron::List(items)) => collect(items, visual_state_from_ron)?,
        Some(_) => return wrong_type("visual_states", "a list"),
    };
    let tiers = collect(fields.list("tiers")?, tier_from_ron)?;
    let triggers = collect(fields.list("triggers")?, trigger_from_ron)?;
    let values = collect(fields.list("values")?, value_def_from_ron)?;
    let game = Game {
        id: fields.string("id")?,
        format_version: fields.int("format_version")?,
        save_schema_version: fields.int("save_schema_version")?,
        start_scene: fields.opt_string("start_scene")?,
        clock,
        streams: sorted_unique(streams, "streams", |item| &item.id)?,
        tables: sorted_unique(tables, "tables", |item| &item.id)?,
        stats: sorted_unique(stats, "stats", |item| &item.id)?,
        tiers: sorted_unique(tiers, "tiers", |item| &item.stat)?,
        flags: sorted_unique(flags, "flags", |item| &item.id)?,
        values: sorted_unique(values, "values", |item| &item.id)?,
        conditions: sorted_unique(conditions, "conditions", |item| &item.id)?,
        effect_sets: sorted_unique(effect_sets, "effect sets", |item| &item.id)?,
        characters: sorted_unique(characters, "characters", |item| &item.id)?,
        scenes: sorted_unique(scenes, "scenes", |item| &item.id)?,
        dialogue: sorted_unique(dialogue, "dialogue lines", |item| &item.id)?,
        triggers: sorted_unique(triggers, "triggers", |item| &item.id)?,
        appearances: sorted_unique(appearances, "appearances", |item| &item.character)?,
        visual_states: sorted_visual_states(visual_states)?,
    };
    fields.done()?;
    Ok(game)
}

/// Sorts visual states by (character, state) and refuses
/// duplicate (character, state) pairs.
fn sorted_visual_states(mut items: Vec<VisualStateDef>) -> Result<Vec<VisualStateDef>, Refusal> {
    items.sort_by(|left, right| {
        (left.character.as_str(), left.state.as_str())
            .cmp(&(right.character.as_str(), right.state.as_str()))
    });
    for pair in items.windows(2) {
        if pair[0].character == pair[1].character && pair[0].state == pair[1].state {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "visual states carry duplicate state '{}' for character '{}'",
                    pair[0].state, pair[0].character
                ),
            );
        }
    }
    Ok(items)
}

fn appearance_from_ron(value: Ron) -> Result<AppearanceDef, Refusal> {
    let mut fields = open(value, "Appearance")?;
    let appearance = AppearanceDef {
        base: collect(fields.list("base")?, appearance_entry_from_ron)?,
        character: fields.string("character")?,
        slots: collect(fields.list("slots")?, appearance_slot_from_ron)?,
    };
    fields.done()?;
    Ok(appearance)
}

fn appearance_slot_from_ron(value: Ron) -> Result<AppearanceSlot, Refusal> {
    let mut fields = open(value, "AppearanceSlot")?;
    let slot = AppearanceSlot {
        id: fields.string("id")?,
        kind: match fields.ident("kind")?.as_str() {
            "Ints" => AppearanceKind::Ints,
            "Text" => AppearanceKind::Text,
            other => {
                return refuse(
                    RefusalClass::UnknownKind,
                    format!("appearance slot kind '{other}' is not implemented"),
                );
            }
        },
    };
    fields.done()?;
    Ok(slot)
}

fn appearance_entry_from_ron(value: Ron) -> Result<AppearanceEntry, Refusal> {
    let mut fields = open(value, "AppearanceEntry")?;
    let raw = fields.take("value")?;
    let entry = AppearanceEntry {
        slot: fields.string("slot")?,
        value: match raw {
            Ron::Str(token) => AppearanceValue::Text(token),
            Ron::List(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    match item {
                        Ron::Int(v) => values.push(v),
                        _ => return wrong_type("appearance value", "an integer"),
                    }
                }
                AppearanceValue::Ints(values)
            }
            _ => return wrong_type("appearance value", "a string or an integer list"),
        },
    };
    fields.done()?;
    Ok(entry)
}

fn visual_state_from_ron(value: Ron) -> Result<VisualStateDef, Refusal> {
    let mut fields = open(value, "VisualState")?;
    let state = VisualStateDef {
        character: fields.string("character")?,
        delta: collect(fields.list("delta")?, appearance_entry_from_ron)?,
        state: fields.string("state")?,
    };
    fields.done()?;
    Ok(state)
}

fn stream_from_ron(value: Ron) -> Result<RngStreamDef, Refusal> {
    let mut fields = open(value, "RngStream")?;
    let stream = RngStreamDef {
        algorithm: fields.string("algorithm")?,
        id: fields.string("id")?,
    };
    fields.done()?;
    Ok(stream)
}

fn table_from_ron(value: Ron) -> Result<TableDef, Refusal> {
    let mut fields = open(value, "Table")?;
    let outcomes = collect(fields.list("outcomes")?, outcome_from_ron)?;
    let table = TableDef {
        id: fields.string("id")?,
        stream: fields.string("stream")?,
        outcomes,
    };
    fields.done()?;
    Ok(table)
}

fn outcome_from_ron(value: Ron) -> Result<TableOutcome, Refusal> {
    let mut fields = open(value, "TableOutcome")?;
    // The weight is a bare integer (static form) or an
    // expression record (the D-469 form); the two encodings are
    // disjoint by construction.
    let weight = match fields.take("weight")? {
        Ron::Int(weight) => TableWeight::Static(weight),
        other => TableWeight::Expr(expr_from_ron(other)?),
    };
    let outcome = TableOutcome {
        effect_set: fields.string("effect_set")?,
        gate: fields.opt_string("gate")?,
        id: fields.string("id")?,
        weight,
    };
    fields.done()?;
    Ok(outcome)
}

fn clock_from_ron(value: Ron) -> Result<Clock, Refusal> {
    let mut fields = open(value, "Clock")?;
    let clock = Clock {
        slots: as_strings(fields.list("slots")?, "slots")?,
        days_per_week: fields
            .option("days_per_week")?
            .map(|v| as_u32(v, "days_per_week"))
            .transpose()?,
        day_limit: fields
            .option("day_limit")?
            .map(|v| as_u32(v, "day_limit"))
            .transpose()?,
    };
    fields.done()?;
    Ok(clock)
}

fn character_from_ron(value: Ron) -> Result<CharacterDef, Refusal> {
    let mut fields = open(value, "Character")?;
    let mut initials = Vec::new();
    for item in fields.list("initials")? {
        let mut row = open(item, "Initial")?;
        initials.push(StatInitial {
            stat: row.string("stat")?,
            value: row.int("value")?,
        });
        row.done()?;
    }
    initials.sort_by(|a, b| a.stat.cmp(&b.stat));
    let character = CharacterDef {
        id: fields.string("id")?,
        name_ref: fields.string("name_ref")?,
        initials,
    };
    fields.done()?;
    Ok(character)
}

fn condition_from_ron(value: Ron) -> Result<ConditionDef, Refusal> {
    let mut fields = open(value, "Condition")?;
    let condition = ConditionDef {
        id: fields.string("id")?,
        expr: expr_from_ron(fields.take("expr")?)?,
    };
    fields.done()?;
    Ok(condition)
}

fn dialogue_from_ron(value: Ron) -> Result<DialogueLine, Refusal> {
    let mut fields = open(value, "DialogueLine")?;
    let line = DialogueLine {
        id: fields.string("id")?,
        scene: fields.string("scene")?,
        ordinal: fields.uint("ordinal")?,
        speaker: fields.string("speaker")?,
        text_ref: fields.string("text_ref")?,
        next: fields.opt_string("next")?,
        hook_expression: fields.opt_string("hook_expression")?,
    };
    fields.done()?;
    Ok(line)
}

fn effect_set_from_ron(value: Ron) -> Result<EffectSetDef, Refusal> {
    let mut fields = open(value, "EffectSet")?;
    let atoms = collect(fields.list("atoms")?, atom_from_ron)?;
    let set = EffectSetDef {
        id: fields.string("id")?,
        atoms,
    };
    fields.done()?;
    Ok(set)
}

fn atom_from_ron(value: Ron) -> Result<Atom, Refusal> {
    let (name, mut fields) = open_any(value)?;
    let atom = match name.as_str() {
        "EmitEvent" => Atom::EmitEvent {
            event: fields.string("event")?,
        },
        "EnterScene" => Atom::EnterScene {
            scene: fields.string("scene")?,
        },
        "FlagClear" => Atom::FlagClear {
            flag: fields.string("flag")?,
        },
        "FlagSet" => Atom::FlagSet {
            flag: fields.string("flag")?,
        },
        "RollTable" => Atom::RollTable {
            table: fields.string("table")?,
        },
        "StatAdd" => Atom::StatAdd {
            character: fields.opt_string("character")?,
            stat: fields.string("stat")?,
            amount: Box::new(expr_from_ron(fields.take("amount")?)?),
        },
        "StatSet" => Atom::StatSet {
            character: fields.opt_string("character")?,
            stat: fields.string("stat")?,
            value: Box::new(expr_from_ron(fields.take("value")?)?),
        },
        other => {
            return refuse(
                RefusalClass::UnknownKind,
                format!("effect atom kind '{other}' is not known"),
            );
        }
    };
    fields.done()?;
    Ok(atom)
}

fn flag_from_ron(value: Ron) -> Result<FlagDef, Refusal> {
    let mut fields = open(value, "Flag")?;
    let scope = match fields.ident("scope")?.as_str() {
        "Globals" => FlagScope::Globals,
        "Run" => FlagScope::Run,
        other => return unknown_variant("scope", other),
    };
    let flag = FlagDef {
        id: fields.string("id")?,
        scope,
        default: fields.boolean("default")?,
    };
    fields.done()?;
    Ok(flag)
}

fn scene_from_ron(value: Ron) -> Result<SceneDef, Refusal> {
    let mut fields = open(value, "Scene")?;
    let presentation = match fields.ident("presentation")?.as_str() {
        "Event" => Presentation::Event,
        "Room" => Presentation::Room,
        "Vn" => Presentation::Vn,
        other => return unknown_variant("presentation", other),
    };
    let scene = SceneDef {
        id: fields.string("id")?,
        presentation,
    };
    fields.done()?;
    Ok(scene)
}

fn stat_from_ron(value: Ron) -> Result<StatDef, Refusal> {
    let mut fields = open(value, "Stat")?;
    let kind = match fields.take("kind")? {
        Ron::Ident(name) => match name.as_str() {
            "Fixed" => StatKind::Fixed,
            "Int" => StatKind::Int,
            other => return unknown_variant("kind", other),
        },
        value @ Ron::Struct(..) => {
            let mut kind_fields = open(value, "Enum")?;
            let values = as_strings(kind_fields.list("values")?, "values")?;
            kind_fields.done()?;
            StatKind::Enum(values)
        }
        _ => return wrong_type("kind", "Int, Fixed, or Enum(values: [...])"),
    };
    let lifetime = fields
        .option("lifetime")?
        .map(|inner| {
            let mut lifetime_fields = open(inner, "Lifetime")?;
            let lifetime = Lifetime {
                days: lifetime_fields.uint("days")?,
            };
            lifetime_fields.done()?;
            Ok::<Lifetime, Refusal>(lifetime)
        })
        .transpose()?;
    let overflow = match fields.ident("overflow")?.as_str() {
        "Clamp" => Overflow::Clamp,
        "Refuse" => Overflow::Refuse,
        other => return unknown_variant("overflow", other),
    };
    let scope = match fields.ident("scope")?.as_str() {
        "Character" => StatScope::Character,
        "Game" => StatScope::Game,
        "Party" => StatScope::Party,
        other => return unknown_variant("scope", other),
    };
    let stat = StatDef {
        id: fields.string("id")?,
        scope,
        kind,
        min: fields.int("min")?,
        max: fields.int("max")?,
        default: fields.int("default")?,
        overflow,
        lifetime,
    };
    fields.done()?;
    Ok(stat)
}

fn tier_from_ron(value: Ron) -> Result<TierDef, Refusal> {
    let mut fields = open(value, "Tier")?;
    let thresholds = fields
        .list("thresholds")?
        .into_iter()
        .map(|item| as_i64(item, "thresholds"))
        .collect::<Result<Vec<i64>, Refusal>>()?;
    let tier = TierDef {
        stat: fields.string("stat")?,
        thresholds,
        names: as_strings(fields.list("names")?, "names")?,
    };
    fields.done()?;
    Ok(tier)
}

fn trigger_from_ron(value: Ron) -> Result<TriggerDef, Refusal> {
    let mut fields = open(value, "Trigger")?;
    let kind = trigger_kind_from_ron(fields.take("kind")?)?;
    let offer = fields
        .option("offer")?
        .map(|inner| {
            let mut offer_fields = open(inner, "Offer")?;
            let offer = OfferDef {
                expires_days: offer_fields
                    .option("expires_days")?
                    .map(|v| as_u32(v, "expires_days"))
                    .transpose()?,
            };
            offer_fields.done()?;
            Ok::<OfferDef, Refusal>(offer)
        })
        .transpose()?;
    let repetition = match fields.ident("repetition")?.as_str() {
        "Once" => Repetition::Once,
        "Repeat" => Repetition::Repeat,
        other => return unknown_variant("repetition", other),
    };
    let trigger = TriggerDef {
        id: fields.string("id")?,
        kind,
        gate: fields.opt_string("gate")?,
        effect_set: fields.string("effect_set")?,
        priority: fields.int("priority")?,
        repetition,
        cooldown_ticks: fields.uint("cooldown_ticks")?,
        offer,
    };
    fields.done()?;
    Ok(trigger)
}

fn trigger_kind_from_ron(value: Ron) -> Result<TriggerKind, Refusal> {
    let (name, mut fields) = open_any(value)?;
    let kind = match name.as_str() {
        "OnEnterScene" => TriggerKind::OnEnterScene {
            scene: fields.string("scene")?,
        },
        "OnEvent" => TriggerKind::OnEvent {
            event: fields.string("event")?,
        },
        "OnFlag" => {
            let edge = match fields.ident("edge")?.as_str() {
                "Both" => FlagEdge::Both,
                "Falling" => FlagEdge::Falling,
                "Rising" => FlagEdge::Rising,
                other => return unknown_variant("edge", other),
            };
            TriggerKind::OnFlag {
                flag: fields.string("flag")?,
                edge,
            }
        }
        "OnThreshold" => {
            let direction = match fields.ident("direction")?.as_str() {
                "Falling" => Direction::Falling,
                "Rising" => Direction::Rising,
                other => return unknown_variant("direction", other),
            };
            TriggerKind::OnThreshold {
                character: fields.opt_string("character")?,
                stat: fields.string("stat")?,
                threshold: fields.int("threshold")?,
                direction,
            }
        }
        "OnTick" => {
            let day_modulo = match fields.option("day_modulo")? {
                None => None,
                Some(Ron::Tuple(items)) if items.len() == 2 => {
                    let mut items = items.into_iter();
                    let (Some(period), Some(remainder)) = (items.next(), items.next()) else {
                        return wrong_type("day_modulo", "a (period, remainder) pair");
                    };
                    Some((
                        as_u32(period, "day_modulo")?,
                        as_u32(remainder, "day_modulo")?,
                    ))
                }
                Some(_) => return wrong_type("day_modulo", "a (period, remainder) pair"),
            };
            let weekdays = fields
                .option("weekdays")?
                .map(|inner| match inner {
                    Ron::List(items) => items
                        .into_iter()
                        .map(|item| as_u32(item, "weekdays"))
                        .collect::<Result<Vec<u32>, Refusal>>(),
                    _ => wrong_type("weekdays", "a list of weekday numbers"),
                })
                .transpose()?;
            TriggerKind::OnTick {
                slots: fields
                    .option("slots")?
                    .map(|inner| as_strings_list(inner, "slots"))
                    .transpose()?,
                weekdays,
                day_modulo,
                exact_day: fields
                    .option("exact_day")?
                    .map(|v| as_i64(v, "exact_day"))
                    .transpose()?,
            }
        }
        other => {
            return refuse(
                RefusalClass::UnknownKind,
                format!("trigger kind '{other}' is not known"),
            );
        }
    };
    fields.done()?;
    Ok(kind)
}

fn as_strings_list(value: Ron, key: &str) -> Result<Vec<String>, Refusal> {
    match value {
        Ron::List(items) => as_strings(items, key),
        _ => wrong_type(key, "a list of strings"),
    }
}

fn value_def_from_ron(value: Ron) -> Result<ValueDef, Refusal> {
    let mut fields = open(value, "Value")?;
    let value_type = match fields.ident("value_type")?.as_str() {
        "Bool" => crate::game::model::ExprType::Bool,
        "Int" => crate::game::model::ExprType::Int,
        other => return unknown_variant("value_type", other),
    };
    let value = ValueDef {
        id: fields.string("id")?,
        expr: expr_from_ron(fields.take("expr")?)?,
        value_type,
    };
    fields.done()?;
    Ok(value)
}

/// Parses one expression tree.
pub(crate) fn expr_from_ron(value: Ron) -> Result<Expr, Refusal> {
    let (name, mut fields) = open_any(value)?;
    let expr = match name.as_str() {
        "Add" => binary_from(&mut fields, Expr::Add)?,
        "And" => Expr::And(exprs_from(&mut fields)?),
        "BoolLit" => Expr::BoolLit(fields.boolean("value")?),
        "Clamp" => Expr::Clamp(
            Box::new(expr_from_ron(fields.take("value")?)?),
            Box::new(expr_from_ron(fields.take("lo")?)?),
            Box::new(expr_from_ron(fields.take("hi")?)?),
        ),
        "Clock" => {
            let field = match fields.ident("field")?.as_str() {
                "Day" => ClockField::Day,
                "DaysRemaining" => ClockField::DaysRemaining,
                "Slot" => ClockField::Slot,
                "Weekday" => ClockField::Weekday,
                other => return unknown_variant("field", other),
            };
            Expr::Clock(field)
        }
        "Cmp" => {
            let op = match fields.ident("op")?.as_str() {
                "Eq" => CmpOp::Eq,
                "Ge" => CmpOp::Ge,
                "Gt" => CmpOp::Gt,
                "Le" => CmpOp::Le,
                "Lt" => CmpOp::Lt,
                "Ne" => CmpOp::Ne,
                other => return unknown_variant("op", other),
            };
            Expr::Cmp(
                op,
                Box::new(expr_from_ron(fields.take("left")?)?),
                Box::new(expr_from_ron(fields.take("right")?)?),
            )
        }
        "Flag" => Expr::Flag(fields.string("flag")?),
        "Lit" => Expr::Lit(fields.int("value")?),
        "Max" => binary_from(&mut fields, Expr::Max)?,
        "Min" => binary_from(&mut fields, Expr::Min)?,
        "Mul" => binary_from(&mut fields, Expr::Mul)?,
        "Neg" => Expr::Neg(Box::new(expr_from_ron(fields.take("expr")?)?)),
        "Not" => Expr::Not(Box::new(expr_from_ron(fields.take("expr")?)?)),
        "Or" => Expr::Or(exprs_from(&mut fields)?),
        "Stat" => Expr::Stat {
            character: fields.opt_string("character")?,
            stat: fields.string("stat")?,
        },
        "Sub" => binary_from(&mut fields, Expr::Sub)?,
        "TierOf" => Expr::TierOf {
            character: fields.opt_string("character")?,
            stat: fields.string("stat")?,
        },
        "Value" => Expr::Value(fields.string("value")?),
        other => {
            return refuse(
                RefusalClass::UnknownKind,
                format!("expression kind '{other}' is not known"),
            );
        }
    };
    fields.done()?;
    Ok(expr)
}

fn binary_from(
    fields: &mut Fields,
    make: fn(Box<Expr>, Box<Expr>) -> Expr,
) -> Result<Expr, Refusal> {
    let left = expr_from_ron(fields.take("left")?)?;
    let right = expr_from_ron(fields.take("right")?)?;
    Ok(make(Box::new(left), Box::new(right)))
}

fn exprs_from(fields: &mut Fields) -> Result<Vec<Expr>, Refusal> {
    fields
        .list("args")?
        .into_iter()
        .map(expr_from_ron)
        .collect()
}

// ---- content.ron --------------------------------------------------

/// The parsed `content.ron` sidecar document.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentDoc {
    /// The sidecar's locale (`en` is the canonical authored
    /// locale, D-472).
    pub locale: String,
    /// Prose entries, sorted by id.
    pub entries: Vec<ContentEntry>,
}

/// Emits a `content.ron` document from entries and a locale.
pub fn emit_content(entries: &[ContentEntry], locale: &str) -> String {
    let doc = record(
        "Content",
        vec![
            (
                "entries",
                Ron::List(
                    entries
                        .iter()
                        .map(|entry| {
                            record(
                                "Entry",
                                vec![("id", text(&entry.id)), ("text", text(&entry.text))],
                            )
                        })
                        .collect(),
                ),
            ),
            ("locale", text(locale)),
        ],
    );
    crate::ron::emit_document(&doc)
}

/// Parses `content.ron` text (entries re-sorted by id; duplicate
/// ids refused).
pub fn parse_content(text: &str) -> Result<ContentDoc, Refusal> {
    let value = crate::ron::parse_document(text)?;
    let mut fields = open(value, "Content")?;
    let entries = collect(fields.list("entries")?, |item| {
        let mut entry_fields = open(item, "Entry")?;
        let entry = ContentEntry {
            id: entry_fields.string("id")?,
            text: entry_fields.string("text")?,
        };
        entry_fields.done()?;
        Ok(entry)
    })?;
    let doc = ContentDoc {
        locale: fields.string("locale")?,
        entries: sorted_unique(entries, "content entries", |item| &item.id)?,
    };
    fields.done()?;
    Ok(doc)
}
