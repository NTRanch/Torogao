//! The AUTH-G game family (Amendment 2, §G1–§G3): the project
//! as a game, authored through the same `decide()` → apply →
//! op-log path as the rig family.
//!
//! Slice G1 lands the core five verbs — `game_define`,
//! `clock_define`, `rng_stream_define`,
//! `game_save_schema_define`, and the read-only `game_simulate`
//! — plus the parts of the G2 checkers G1 makes live: the
//! §G3.1 tick skeleton with its per-tick state-hash chain, the
//! deterministic RNG, and the typed checkers behind
//! `TG-GAME-EXPR`, `TG-GAME-TIER`, and `TG-GAME-TABLE`. The
//! remaining seventeen game verbs are refused by name with
//! their slice (G2), exactly as later rig slices were refused
//! in A1; the scene/dialogue checkers behind `TG-GAME-GRAPH`
//! arrive with the scenes they check (G2.4), but the class is
//! already live: `game_simulate` refuses a `choice` input with
//! it, because no choices exist to honour yet.
//!
//! Two G1 honesties, stated once here: `game_define`'s
//! `start_scene` is a forward reference (§G2.1 delegates its
//! check to the validation sweep once scenes exist), and an
//! initial assignment to a stat or flag is refused
//! `TG-AUTH-DANGLING` because G1 authors no stat/flag
//! definitions — G2.2 wires the real collections into
//! [`stat_or_flag_defined`].

use std::collections::BTreeSet;

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::decide::{Verdict, check_id};
use crate::intake::sha256_hex;
use crate::limits;
use crate::model::{
    AssignValue, GameClock, GameDef, InitialAssignment, Migration, Project, RngStream,
    SaveContextPolicy, SaveSchema, SlotHolds,
};
use crate::op::Op;
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::ron::{Ron, emit_document};

/// One row of the game manifest: a verb's home in the AUTH-G
/// family. The rig manifest lives in `op.rs`; the game family
/// keeps its own table because Amendment 2 numbers its verbs
/// separately (§G6 counts them into the registry on their own).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GameVerbSpec {
    /// Verb name.
    pub name: &'static str,
    /// Family (Amendment 2 §G2 section order).
    pub group: &'static str,
    /// Whether the verb mutates the project (false: the read).
    pub mutating: bool,
    /// Whether this slice implements it.
    pub live: bool,
    /// Owning AUTH-G slice ("G1" or "G2").
    pub slice: &'static str,
}

macro_rules! gverb {
    ($name:literal, $group:literal, $mutating:literal, $live:literal, $slice:literal) => {
        GameVerbSpec {
            name: $name,
            group: $group,
            mutating: $mutating,
            live: $live,
            slice: $slice,
        }
    };
}

/// The game manifest: 22 verbs (Amendment 2 §G2), in family
/// order, alphabetical within family.
pub const GAME_MANIFEST: &[GameVerbSpec] = &[
    // Game core (§G2.1) — slice G1, all live.
    gverb!("clock_define", "game core", true, true, "G1"),
    gverb!("game_define", "game core", true, true, "G1"),
    gverb!("game_save_schema_define", "game core", true, true, "G1"),
    gverb!("game_simulate", "game core", false, true, "G1"),
    gverb!("rng_stream_define", "game core", true, true, "G1"),
    // Stats, flags, and derived values (§G2.2) — slice G2.
    gverb!(
        "flag_define",
        "stats, flags, and derived values",
        true,
        false,
        "G2"
    ),
    gverb!(
        "stat_define",
        "stats, flags, and derived values",
        true,
        false,
        "G2"
    ),
    gverb!(
        "stat_tier_define",
        "stats, flags, and derived values",
        true,
        false,
        "G2"
    ),
    gverb!(
        "value_define",
        "stats, flags, and derived values",
        true,
        false,
        "G2"
    ),
    // Conditions, effects, and tables (§G2.3) — slice G2.
    gverb!(
        "condition_define",
        "conditions, effects, and tables",
        true,
        false,
        "G2"
    ),
    gverb!(
        "effect_set_define",
        "conditions, effects, and tables",
        true,
        false,
        "G2"
    ),
    gverb!(
        "table_define",
        "conditions, effects, and tables",
        true,
        false,
        "G2"
    ),
    // Characters, scenes, dialogue, choices (§G2.4) — slice G2.
    gverb!(
        "character_define",
        "characters, scenes, dialogue, choices",
        true,
        false,
        "G2"
    ),
    gverb!(
        "choice_define",
        "characters, scenes, dialogue, choices",
        true,
        false,
        "G2"
    ),
    gverb!(
        "dialogue_line_define",
        "characters, scenes, dialogue, choices",
        true,
        false,
        "G2"
    ),
    gverb!(
        "schedule_define",
        "characters, scenes, dialogue, choices",
        true,
        false,
        "G2"
    ),
    gverb!(
        "scene_bind_rig",
        "characters, scenes, dialogue, choices",
        true,
        false,
        "G2"
    ),
    gverb!(
        "scene_define",
        "characters, scenes, dialogue, choices",
        true,
        false,
        "G2"
    ),
    gverb!(
        "scene_exit_define",
        "characters, scenes, dialogue, choices",
        true,
        false,
        "G2"
    ),
    // Triggers (§G2.5) — slice G2.
    gverb!("trigger_define", "triggers", true, false, "G2"),
    // Endings and gallery (§G2.6) — slice G2.
    gverb!("ending_define", "endings and gallery", true, false, "G2"),
    gverb!(
        "gallery_entry_define",
        "endings and gallery",
        true,
        false,
        "G2"
    ),
];

/// The game manifest row for this verb, if it is a game verb.
pub fn game_verb_spec(verb: &str) -> Option<&'static GameVerbSpec> {
    GAME_MANIFEST.iter().find(|spec| spec.name == verb)
}

/// The refusal for a game verb this slice does not implement:
/// the rig family's `TG-AUTH-UNKNOWN-KIND` shape, naming the
/// owning AUTH-G slice.
pub fn game_refusal(verb: &str) -> Refusal {
    match game_verb_spec(verb) {
        Some(spec) => Refusal::new(
            RefusalClass::UnknownKind,
            format!(
                "verb '{verb}' belongs to AUTH-G slice {}; it is not implemented in G1",
                spec.slice
            ),
        ),
        None => Refusal::new(
            RefusalClass::UnknownKind,
            format!("verb '{verb}' is not in the game manifest"),
        ),
    }
}

/// The canonical game-state field set (§G3.3): every state field
/// the family produces belongs to exactly one save partition,
/// so the partition sweep is an exact cover of this list.
pub const STATE_FIELDS: &[&str] = &[
    "clock",
    "ending",
    "flags_globals",
    "flags_run",
    "gallery",
    "rng_state",
    "scene",
    "stats",
    "triggers",
];

/// Hash algorithm versions the family implements; anything else
/// versioned is `TG-AUTH-UNKNOWN-KIND`, anything unversioned is
/// `TG-GAME-SAVE-SCHEMA` (the schema must name a version).
pub const HASH_ALGORITHMS: &[&str] = &["sha256/1"];

/// RNG algorithm versions the interpreter implements.
pub const RNG_ALGORITHMS: &[&str] = &["torogao-rng/1"];

// ---- inputs ------------------------------------------------------

/// Inputs of `clock_define`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockDefineInput {
    /// Ordered named slots per day, in tick order.
    pub slots: Vec<String>,
    /// Days per week, when the game uses weeks.
    #[serde(default)]
    pub days_per_week: Option<u32>,
    /// Weekday names, in order.
    #[serde(default)]
    pub weekdays: Vec<String>,
    /// Optional day limit.
    #[serde(default)]
    pub day_limit: Option<u32>,
}

/// A run-start value in an initial assignment: an integer for a
/// stat, a boolean for a flag.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(untagged)]
pub enum AssignValueInput {
    /// An integer stat value.
    Int(i64),
    /// A boolean flag value.
    Bool(bool),
}

/// One initial assignment in `game_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct InitialAssignmentInput {
    /// The stat or flag id.
    pub target: String,
    /// The value at run start.
    pub value: AssignValueInput,
}

/// A save-context policy in verb inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum SavePolicyInput {
    /// At any tick boundary.
    AnyTickBoundary,
    /// Only at scene boundaries.
    SceneBoundary,
    /// Only inside the listed safe scenes.
    SafeScenesOnly {
        /// The safe scene ids.
        scenes: Vec<String>,
    },
}

/// Inputs of `game_define`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameDefineInput {
    /// The game id.
    pub id: String,
    /// The start scene id (a forward reference at define time).
    pub start_scene: String,
    /// Stat/flag values at run start.
    #[serde(default)]
    pub initial: Vec<InitialAssignmentInput>,
    /// The alter-ego veto rule: an effect-set id.
    #[serde(default)]
    pub alter_ego_veto: Option<String>,
    /// Save-context policy default for the game.
    #[serde(default)]
    pub save_policy: Option<SavePolicyInput>,
}

/// Inputs of `rng_stream_define`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RngStreamDefineInput {
    /// Stream id.
    pub id: String,
    /// Named, versioned algorithm.
    pub algorithm: String,
}

/// One migration step in `game_save_schema_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationInput {
    /// The version this step migrates from.
    pub from: u32,
    /// The migration's name.
    pub name: String,
}

/// Inputs of `game_save_schema_define`.
#[derive(Clone, Debug, Deserialize, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSchemaDefineInput {
    /// Schema version.
    pub version: u32,
    /// Slot count.
    pub slots: u32,
    /// Whether a slot holds one run or a chapter list.
    pub slot_holds: String,
    /// State fields in the per-slot partition.
    pub slot_fields: Vec<String>,
    /// State fields in the globals partition.
    pub globals_fields: Vec<String>,
    /// The named hash algorithm version.
    pub hash_algorithm: String,
    /// Where a manual save may be taken.
    pub policy: SavePolicyInput,
    /// The migration chain.
    #[serde(default)]
    pub migrations: Vec<MigrationInput>,
}

/// One entry of a `game_simulate` choice log.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ChoiceLogEntry {
    /// The tick this input lands on (1-based, consecutive).
    pub tick: u64,
    /// The input kind: `advance`, `choice`, or `event`.
    pub kind: String,
    /// The input's id (a choice id, an event id, or an advance
    /// marker).
    pub id: String,
    /// The chosen option, for `choice` inputs.
    #[serde(default)]
    pub option: Option<String>,
}

/// Inputs of `game_simulate`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameSimulateInput {
    /// The project bytes' hash the log was authored against.
    pub project_sha256: String,
    /// The run seed.
    pub seed: u64,
    /// The choice log: one input per tick.
    pub choice_log: Vec<ChoiceLogEntry>,
}

// ---- record builders ----------------------------------------------
// Each builder canonicalizes its inputs into the stored record.
// decide() identity checks and apply() both go through them, so
// the two never disagree about what an op means (the rig
// family's expression_record pattern).

/// The canonical clock record for these inputs.
pub fn clock_record(inputs: &ClockDefineInput) -> GameClock {
    GameClock {
        slots: inputs.slots.clone(),
        days_per_week: inputs.days_per_week,
        weekdays: inputs.weekdays.clone(),
        day_limit: inputs.day_limit,
    }
}

/// The inputs that rebuild this clock record (validation sweep).
pub fn clock_inputs(clock: &GameClock) -> ClockDefineInput {
    ClockDefineInput {
        slots: clock.slots.clone(),
        days_per_week: clock.days_per_week,
        weekdays: clock.weekdays.clone(),
        day_limit: clock.day_limit,
    }
}

fn policy_record(inputs: &SavePolicyInput) -> SaveContextPolicy {
    match inputs {
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

fn policy_inputs(policy: &SaveContextPolicy) -> SavePolicyInput {
    match policy {
        SaveContextPolicy::AnyTickBoundary => SavePolicyInput::AnyTickBoundary,
        SaveContextPolicy::SceneBoundary => SavePolicyInput::SceneBoundary,
        SaveContextPolicy::SafeScenesOnly(scenes) => SavePolicyInput::SafeScenesOnly {
            scenes: scenes.clone(),
        },
    }
}

fn assign_record(value: AssignValueInput) -> AssignValue {
    match value {
        AssignValueInput::Int(n) => AssignValue::Int(n),
        AssignValueInput::Bool(b) => AssignValue::Bool(b),
    }
}

fn assign_input(value: AssignValue) -> AssignValueInput {
    match value {
        AssignValue::Int(n) => AssignValueInput::Int(n),
        AssignValue::Bool(b) => AssignValueInput::Bool(b),
    }
}

/// The canonical game record for these inputs (initial
/// assignments sorted by target).
pub fn game_record(inputs: &GameDefineInput) -> GameDef {
    let mut initial: Vec<InitialAssignment> = inputs
        .initial
        .iter()
        .map(|assignment| InitialAssignment {
            target: assignment.target.clone(),
            value: assign_record(assignment.value),
        })
        .collect();
    initial.sort_by(|a, b| a.target.cmp(&b.target));
    GameDef {
        id: inputs.id.clone(),
        start_scene: inputs.start_scene.clone(),
        initial,
        alter_ego_veto: inputs.alter_ego_veto.clone(),
        save_policy: inputs.save_policy.as_ref().map(policy_record),
    }
}

/// The inputs that rebuild this game record (validation sweep).
pub fn game_inputs(game: &GameDef) -> GameDefineInput {
    GameDefineInput {
        id: game.id.clone(),
        start_scene: game.start_scene.clone(),
        initial: game
            .initial
            .iter()
            .map(|assignment| InitialAssignmentInput {
                target: assignment.target.clone(),
                value: assign_input(assignment.value),
            })
            .collect(),
        alter_ego_veto: game.alter_ego_veto.clone(),
        save_policy: game.save_policy.as_ref().map(policy_inputs),
    }
}

/// The canonical stream record for these inputs.
pub fn rng_record(inputs: &RngStreamDefineInput) -> RngStream {
    RngStream {
        id: inputs.id.clone(),
        algorithm: inputs.algorithm.clone(),
    }
}

/// The inputs that rebuild this stream record (sweep).
pub fn rng_inputs(stream: &RngStream) -> RngStreamDefineInput {
    RngStreamDefineInput {
        id: stream.id.clone(),
        algorithm: stream.algorithm.clone(),
    }
}

/// The canonical schema record for these inputs (partitions and
/// migrations sorted). Caller guarantees `slot_holds` parsed.
pub fn schema_record(inputs: &SaveSchemaDefineInput, slot_holds: SlotHolds) -> SaveSchema {
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
        slot_holds,
        slot_fields,
        globals_fields,
        hash_algorithm: inputs.hash_algorithm.clone(),
        policy: policy_record(&inputs.policy),
        migrations,
    }
}

/// The inputs that rebuild this schema record (sweep).
pub fn schema_inputs(schema: &SaveSchema) -> SaveSchemaDefineInput {
    SaveSchemaDefineInput {
        version: schema.version,
        slots: schema.slots,
        slot_holds: match schema.slot_holds {
            SlotHolds::OneRun => "one_run".to_string(),
            SlotHolds::ChapterList => "chapter_list".to_string(),
        },
        slot_fields: schema.slot_fields.clone(),
        globals_fields: schema.globals_fields.clone(),
        hash_algorithm: schema.hash_algorithm.clone(),
        policy: policy_inputs(&schema.policy),
        migrations: schema
            .migrations
            .iter()
            .map(|step| MigrationInput {
                from: step.from,
                name: step.name.clone(),
            })
            .collect(),
    }
}

// ---- decide --------------------------------------------------------

/// Whether a stat or flag with this id is defined. G1 authors
/// no stat/flag definitions, so nothing is defined yet; G2.2
/// wires the real collections in here, and until then every
/// initial assignment is honestly dangling (§G2.1 validates
/// initial values against their definitions at define time).
fn stat_or_flag_defined(_project: &Project, _target: &str) -> bool {
    false
}

/// Decide one game op against the project (the project always
/// exists: a game verb on an uninitialized project is refused
/// by the caller before this runs).
pub fn decide_game(project: &Project, op: &Op) -> Result<Verdict, Refusal> {
    match op {
        Op::ClockDefine(inputs) => decide_clock(project, inputs),
        Op::GameDefine(inputs) => decide_game_def(project, inputs),
        Op::GameSaveSchemaDefine(inputs) => decide_schema(project, inputs),
        Op::RngStreamDefine(inputs) => decide_rng(project, inputs),
        other => unreachable!("decide_game called with {}", other.verb()),
    }
}

fn decide_clock(project: &Project, inputs: &ClockDefineInput) -> Result<Verdict, Refusal> {
    if inputs.slots.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "a clock needs at least one slot: the tick is one slot advance",
        );
    }
    if inputs.slots.len() > limits::CLOCK_SLOTS_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "clock declares {} slots, over the bound of {}",
                inputs.slots.len(),
                limits::CLOCK_SLOTS_MAX
            ),
        );
    }
    let mut seen = BTreeSet::new();
    for slot in &inputs.slots {
        check_id(slot, "slot")?;
        if !seen.insert(slot.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("clock names slot '{slot}' twice"),
            );
        }
    }
    match inputs.days_per_week {
        Some(days) => {
            if days == 0 || days > limits::CLOCK_WEEKDAYS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "days_per_week {days} is outside 1..={}",
                        limits::CLOCK_WEEKDAYS_MAX
                    ),
                );
            }
            if inputs.weekdays.len() != days as usize {
                return refuse(
                    RefusalClass::Malformed,
                    format!(
                        "clock declares {days} days per week but names {} weekdays",
                        inputs.weekdays.len()
                    ),
                );
            }
        }
        None if !inputs.weekdays.is_empty() => {
            return refuse(
                RefusalClass::Malformed,
                "clock names weekdays but declares no days_per_week",
            );
        }
        None => {}
    }
    let mut seen = BTreeSet::new();
    for weekday in &inputs.weekdays {
        if weekday.is_empty()
            || weekday.len() > limits::PROVENANCE_TEXT_BYTES_MAX
            || weekday.chars().any(char::is_control)
        {
            return refuse(
                RefusalClass::Malformed,
                format!("weekday name '{weekday}' is not usable text"),
            );
        }
        if !seen.insert(weekday.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("clock names weekday '{weekday}' twice"),
            );
        }
    }
    if let Some(limit) = inputs.day_limit {
        if limit == 0 || limit > limits::DAY_LIMIT_MAX {
            return refuse(
                RefusalClass::Range,
                format!("day_limit {limit} is outside 1..={}", limits::DAY_LIMIT_MAX),
            );
        }
    }
    let record = clock_record(inputs);
    match &project.clock {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => refuse(
            RefusalClass::Conflict,
            "a clock is already defined with different inputs",
        ),
        None => Ok(Verdict::Apply),
    }
}

fn decide_game_def(project: &Project, inputs: &GameDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "id")?;
    check_id(&inputs.start_scene, "start_scene")?;
    let mut seen = BTreeSet::new();
    for assignment in &inputs.initial {
        check_id(&assignment.target, "target")?;
        if !seen.insert(assignment.target.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("initial assignments name '{}' twice", assignment.target),
            );
        }
        if !stat_or_flag_defined(project, &assignment.target) {
            return refuse(
                RefusalClass::Dangling,
                format!(
                    "initial assignment target '{}' is not a defined stat or flag",
                    assignment.target
                ),
            );
        }
    }
    if let Some(veto) = &inputs.alter_ego_veto {
        check_id(veto, "alter_ego_veto")?;
    }
    if let Some(SavePolicyInput::SafeScenesOnly { scenes }) = &inputs.save_policy {
        if scenes.is_empty() {
            return refuse(
                RefusalClass::Malformed,
                "a safe-scenes save policy needs at least one scene",
            );
        }
        for scene in scenes {
            check_id(scene, "scene")?;
        }
    }
    let record = game_record(inputs);
    match &project.game {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => refuse(
            RefusalClass::Conflict,
            "a game is already defined with different inputs",
        ),
        None => Ok(Verdict::Apply),
    }
}

fn decide_rng(project: &Project, inputs: &RngStreamDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "id")?;
    if !RNG_ALGORITHMS.contains(&inputs.algorithm.as_str()) {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "RNG algorithm '{}' is not one the interpreter implements",
                inputs.algorithm
            ),
        );
    }
    let record = rng_record(inputs);
    match project.rng_stream(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => refuse(
            RefusalClass::Conflict,
            format!(
                "rng stream '{}' already names a different algorithm",
                inputs.id
            ),
        ),
        None => {
            if project.rng_streams.len() + 1 > limits::RNG_STREAMS_MAX {
                return refuse(
                    RefusalClass::Range,
                    format!(
                        "project already holds {} RNG streams, the bound",
                        limits::RNG_STREAMS_MAX
                    ),
                );
            }
            Ok(Verdict::Apply)
        }
    }
}

fn decide_schema(project: &Project, inputs: &SaveSchemaDefineInput) -> Result<Verdict, Refusal> {
    let schema_class = RefusalClass::GameSaveSchema;
    let slot_holds = match inputs.slot_holds.as_str() {
        "one_run" => SlotHolds::OneRun,
        "chapter_list" => SlotHolds::ChapterList,
        other => {
            return refuse(
                RefusalClass::UnknownKind,
                format!("slot_holds '{other}' is not one the family defines"),
            );
        }
    };
    let record = schema_record(inputs, slot_holds);
    if let Some(existing) = &project.save_schema {
        if *existing == record {
            return Ok(Verdict::NoOp);
        }
    }
    if record.version == 0 {
        return refuse(schema_class, "schema version must be at least 1");
    }
    if record.slots == 0 || record.slots > limits::SAVE_SLOTS_MAX {
        return refuse(
            schema_class,
            format!(
                "schema declares {} slots, outside 1..={}",
                record.slots,
                limits::SAVE_SLOTS_MAX
            ),
        );
    }
    if !record.hash_algorithm.contains('/') {
        return refuse(
            schema_class,
            format!(
                "schema names hash algorithm '{}' with no version; a schema must name a hash algorithm version",
                record.hash_algorithm
            ),
        );
    }
    if !HASH_ALGORITHMS.contains(&record.hash_algorithm.as_str()) {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "hash algorithm '{}' is not one the interpreter implements",
                record.hash_algorithm
            ),
        );
    }
    // The partition sweep: an exact cover of the canonical set.
    for field in STATE_FIELDS {
        let in_slot = record.slot_fields.iter().any(|f| f == field);
        let in_globals = record.globals_fields.iter().any(|f| f == field);
        match (in_slot, in_globals) {
            (true, true) => {
                return refuse(
                    schema_class,
                    format!("state field '{field}' is in both partitions"),
                );
            }
            (false, false) => {
                return refuse(
                    schema_class,
                    format!(
                        "state field '{field}' is in neither partition: the sweep must be exact"
                    ),
                );
            }
            _ => {}
        }
    }
    for field in record.slot_fields.iter().chain(&record.globals_fields) {
        if !STATE_FIELDS.contains(&field.as_str()) {
            return refuse(
                schema_class,
                format!("schema names state field '{field}', which the family cannot produce"),
            );
        }
    }
    // Duplicate assignment inside one partition (the record is
    // sorted, so duplicates are adjacent).
    for list in [&record.slot_fields, &record.globals_fields] {
        for pair in list.windows(2) {
            if pair[0] == pair[1] {
                return refuse(
                    schema_class,
                    format!("state field '{}' is assigned twice", pair[0]),
                );
            }
        }
    }
    // The migration chain: named single-version steps forming
    // one contiguous run that ends at version - 1. When a prior
    // schema exists, the run must start at its version, so no
    // accepted save is silently orphaned.
    for step in &record.migrations {
        check_id(&step.name, "migration")?;
        if step.from == 0 || step.from >= record.version {
            return refuse(
                schema_class,
                format!(
                    "migration '{}' goes from {}, outside 1..{}",
                    step.name, step.from, record.version
                ),
            );
        }
    }
    for pair in record.migrations.windows(2) {
        if pair[1].from != pair[0].from + 1 {
            return refuse(
                schema_class,
                format!(
                    "migration chain has a gap: no step from {}",
                    pair[0].from + 1
                ),
            );
        }
    }
    if let Some(last) = record.migrations.last() {
        if last.from + 1 != record.version {
            return refuse(
                schema_class,
                format!(
                    "migration chain ends at version {}; the schema is version {}",
                    last.from + 1,
                    record.version
                ),
            );
        }
    }
    if let Some(existing) = &project.save_schema {
        if record.version <= existing.version {
            return refuse(
                schema_class,
                format!(
                    "schema version {} does not strictly increase over the defined {}",
                    record.version, existing.version
                ),
            );
        }
        match record.migrations.first() {
            Some(first) if first.from == existing.version => {}
            _ => {
                return refuse(
                    schema_class,
                    format!(
                        "migration chain does not start at the prior version {}: its saves would be orphaned",
                        existing.version
                    ),
                );
            }
        }
    }
    Ok(Verdict::Apply)
}

// ---- apply -----------------------------------------------------------

/// Apply one decided game op to the project.
pub fn apply_game(project: &mut Project, op: &Op) {
    match op {
        Op::ClockDefine(inputs) => project.clock = Some(clock_record(inputs)),
        Op::GameDefine(inputs) => project.game = Some(game_record(inputs)),
        Op::GameSaveSchemaDefine(inputs) => {
            let slot_holds = match inputs.slot_holds.as_str() {
                "chapter_list" => SlotHolds::ChapterList,
                _ => SlotHolds::OneRun,
            };
            project.save_schema = Some(schema_record(inputs, slot_holds));
        }
        Op::RngStreamDefine(inputs) => {
            let record = rng_record(inputs);
            let at = project
                .rng_streams
                .partition_point(|stream| stream.id < record.id);
            if project.rng_streams.get(at).map(|s| &s.id) == Some(&record.id) {
                project.rng_streams[at] = record;
            } else {
                project.rng_streams.insert(at, record);
            }
        }
        other => unreachable!("apply_game called with {}", other.verb()),
    }
}

// ---- the deterministic RNG -------------------------------------------

/// One stream of `torogao-rng/1`: a SplitMix64 generator whose
/// initial state is derived from the run seed and the stream id
/// by SHA-256, so streams are independent by construction and a
/// run's draws replay bit-exactly from `(seed, stream id, draw
/// index)` alone.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
    /// Draws taken so far (part of the hashed run state).
    pub draws: u64,
}

impl Rng {
    /// Open the stream for this run seed.
    pub fn new(seed: u64, stream_id: &str) -> Rng {
        let mut hasher = Sha256::new();
        hasher.update(b"torogao-rng/1\n");
        hasher.update(seed.to_le_bytes());
        hasher.update(b"\n");
        hasher.update(stream_id.as_bytes());
        let digest = hasher.finalize();
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&digest[..8]);
        Rng {
            state: u64::from_le_bytes(bytes),
            draws: 0,
        }
    }

    /// The next draw (SplitMix64).
    pub fn next_u64(&mut self) -> u64 {
        self.draws += 1;
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

// ---- the tick interpreter (§G3.1) -------------------------------------

/// The outcome of a `game_simulate` run.
#[derive(Clone, Debug, PartialEq)]
pub struct SimReport {
    /// Ticks actually executed (the log's length, unless the
    /// run stopped early).
    pub ticks: u64,
    /// The per-tick state-hash chain, one hash per tick (§G3.2).
    pub hashes: Vec<String>,
    /// SHA-256 of the concatenated per-tick hashes.
    pub chain_sha256: String,
    /// The fired ending, when one fired (none can in G1).
    pub ending: Option<String>,
    /// Why the run stopped early (`day_limit`), when it did.
    pub stopped: Option<String>,
    /// Final clock position: day and slot name.
    pub day: u64,
    /// Final slot name.
    pub slot: String,
}

/// Run the §G3.1 skeleton over a choice log. Steps 2 (scheduled
/// triggers), 4 (threshold/flag re-evaluation), 5 (scene
/// transitions), and 6 (ending evaluation) run in their fixed
/// positions but have no authored content to act on until G2;
/// step 7 (the state hash) is live, which is what §G3.2's
/// evidence subset measures.
pub fn simulate(
    project: &Project,
    project_sha256: &str,
    inputs: &GameSimulateInput,
) -> Result<SimReport, Refusal> {
    if inputs.project_sha256 != project_sha256 {
        return refuse(
            RefusalClass::Conflict,
            "the choice log was authored against a different project hash",
        );
    }
    let Some(game) = &project.game else {
        return refuse(
            RefusalClass::UnknownTarget,
            "project defines no game; game_simulate needs game_define first",
        );
    };
    let Some(clock) = &project.clock else {
        return refuse(
            RefusalClass::UnknownTarget,
            "project defines no clock; game_simulate needs clock_define first",
        );
    };
    if inputs.choice_log.len() > limits::CHOICE_LOG_INPUTS_MAX {
        return refuse(
            RefusalClass::Range,
            format!(
                "choice log holds {} inputs, over the bound of {}",
                inputs.choice_log.len(),
                limits::CHOICE_LOG_INPUTS_MAX
            ),
        );
    }
    for (index, entry) in inputs.choice_log.iter().enumerate() {
        if entry.tick != index as u64 + 1 {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "choice log entry {} names tick {}; entries must be consecutive from 1",
                    index + 1,
                    entry.tick
                ),
            );
        }
        check_id(&entry.id, "id")?;
        match entry.kind.as_str() {
            "advance" | "event" => {
                if entry.option.is_some() {
                    return refuse(
                        RefusalClass::Malformed,
                        format!("input '{}' is not a choice but names an option", entry.id),
                    );
                }
            }
            "choice" => {}
            other => {
                return refuse(
                    RefusalClass::UnknownKind,
                    format!("input kind '{other}' is not one the interpreter consumes"),
                );
            }
        }
    }
    // Streams open at the run seed; draws are counted into the
    // hashed state so a future weighted draw is replay-visible.
    let streams: Vec<(&str, Rng)> = project
        .rng_streams
        .iter()
        .map(|stream| (stream.id.as_str(), Rng::new(inputs.seed, &stream.id)))
        .collect();
    let mut day: u64 = 1;
    let mut slot_index: usize = 0;
    let mut hashes: Vec<String> = Vec::new();
    let mut stopped: Option<String> = None;
    for entry in &inputs.choice_log {
        // Step 1: advance the clock one slot. A tick that would
        // cross the day limit does not execute: the run stops
        // with the input unconsumed and the position unchanged.
        let mut next_day = day;
        let mut next_slot = slot_index + 1;
        if next_slot == clock.slots.len() {
            next_slot = 0;
            next_day += 1;
        }
        if let Some(limit) = clock.day_limit
            && next_day > u64::from(limit)
        {
            stopped = Some("day_limit".to_string());
            break;
        }
        day = next_day;
        slot_index = next_slot;
        // Step 2: fire scheduled triggers — none authored (G2.5).
        // Step 3: consume exactly one input.
        if entry.kind == "choice" {
            return refuse(
                RefusalClass::GameGraph,
                format!(
                    "choice '{}' names no choice in this project: choices are not authored yet (AUTH-G slice G2)",
                    entry.id
                ),
            );
        }
        // Step 4: re-evaluate threshold/flag triggers — none
        // authored (G2.5).
        // Step 5: apply the requested scene transition — a scene
        // graph is not authored yet (G2.4); the run stays in the
        // start scene.
        // Step 6: evaluate endings — none authored (G2.6).
        // Step 7: hash the full state into the chain.
        let state = Ron::Struct(
            "GameState".to_string(),
            vec![
                (
                    "day".to_string(),
                    Ron::Int(i64::try_from(day).unwrap_or(i64::MAX)),
                ),
                ("ending".to_string(), Ron::Opt(None)),
                (
                    "rng".to_string(),
                    Ron::List(
                        streams
                            .iter()
                            .map(|(id, rng)| {
                                Ron::Struct(
                                    "StreamState".to_string(),
                                    vec![
                                        (
                                            "draws".to_string(),
                                            Ron::Int(i64::try_from(rng.draws).unwrap_or(i64::MAX)),
                                        ),
                                        ("id".to_string(), Ron::Str((*id).to_string())),
                                    ],
                                )
                            })
                            .collect(),
                    ),
                ),
                ("scene".to_string(), Ron::Str(game.start_scene.clone())),
                (
                    "slot".to_string(),
                    Ron::Str(clock.slots[slot_index].clone()),
                ),
                (
                    "tick".to_string(),
                    Ron::Int(i64::try_from(hashes.len() as u64 + 1).unwrap_or(i64::MAX)),
                ),
            ],
        );
        hashes.push(sha256_hex(emit_document(&state).as_bytes()));
    }
    let chain_sha256 = sha256_hex(hashes.concat().as_bytes());
    Ok(SimReport {
        ticks: hashes.len() as u64,
        chain_sha256,
        ending: None,
        stopped,
        day,
        slot: clock.slots[slot_index].clone(),
        hashes,
    })
}

/// The `game_simulate` answer document for a loaded project.
pub fn simulate_document(
    project: &Project,
    project_sha256: &str,
    inputs: &GameSimulateInput,
) -> Result<Value, Refusal> {
    let report = simulate(project, project_sha256, inputs)?;
    Ok(json!({
        "chain_sha256": report.chain_sha256,
        "day": report.day,
        "ending": report.ending,
        "hashes": report.hashes,
        "result": "ok",
        "slot": report.slot,
        "stopped": report.stopped,
        "ticks": report.ticks,
    }))
}

// ---- the G2 checkers G1 makes live (§G2.3) ----------------------------

/// The expression form (§G2.3): data, never code. Total: no
/// division or modulo node exists — a definition naming one is
/// refused `TG-GAME-EXPR` — and there are no loops, no
/// recursion, no string operations, and no RNG.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// An integer literal.
    Lit(i64),
    /// A boolean literal (flag tests compare against these).
    BoolLit(bool),
    /// A stat reference (integer).
    Stat(String),
    /// A flag reference (boolean).
    Flag(String),
    /// A named-value reference (same kind as the value).
    ValueRef(String),
    /// The tier index of a stat (integer).
    TierOf(String),
    /// The current day (integer).
    ClockDay,
    /// The current slot name's index (integer).
    ClockSlot,
    /// The current weekday index (integer).
    ClockWeekday,
    /// Days remaining before the day limit (integer).
    ClockDaysRemaining,
    /// A comparison.
    Compare(Cmp, Box<Expr>, Box<Expr>),
    /// Boolean conjunction.
    And(Box<Expr>, Box<Expr>),
    /// Boolean disjunction.
    Or(Box<Expr>, Box<Expr>),
    /// Boolean negation.
    Not(Box<Expr>),
    /// An integer operation.
    Arith(Arith, Box<Expr>, Box<Expr>),
    /// `clamp(value, lo, hi)` over integers.
    Clamp(Box<Expr>, Box<Expr>, Box<Expr>),
}

/// A comparison operator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cmp {
    /// `==`
    Eq,
    /// `!=`
    Ne,
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
}

/// An integer operator (division and modulo deliberately
/// absent — see [`Expr`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Arith {
    /// `+`
    Add,
    /// `-`
    Sub,
    /// `*`
    Mul,
    /// `min(a, b)`
    Min,
    /// `max(a, b)`
    Max,
}

/// An expression's type: the checker is total over these two.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExprType {
    /// An integer.
    Int,
    /// A boolean.
    Bool,
}

fn expr_refusal(message: String) -> Refusal {
    Refusal::new(RefusalClass::GameExpr, message)
}

/// Type- and reference-check an expression, returning its type.
/// `values` pairs each named value with its defining expression;
/// a reference cycle among them is `TG-GAME-EXPR`, as are an
/// unknown reference, a flag in an integer position (§G2.3:
/// adding to, multiplying, or ordering a flag is refused), and
/// an ordering comparison over booleans.
pub fn check_expression(
    expr: &Expr,
    stats: &[&str],
    flags: &[&str],
    values: &[(&str, &Expr)],
) -> Result<ExprType, Refusal> {
    check_expr(expr, stats, flags, values, &mut Vec::new())
}

fn check_expr(
    expr: &Expr,
    stats: &[&str],
    flags: &[&str],
    values: &[(&str, &Expr)],
    visiting: &mut Vec<String>,
) -> Result<ExprType, Refusal> {
    match expr {
        Expr::Lit(_)
        | Expr::ClockDay
        | Expr::ClockSlot
        | Expr::ClockWeekday
        | Expr::ClockDaysRemaining => Ok(ExprType::Int),
        Expr::BoolLit(_) => Ok(ExprType::Bool),
        Expr::Stat(name) => {
            if stats.contains(&name.as_str()) {
                Ok(ExprType::Int)
            } else {
                Err(expr_refusal(format!(
                    "expression references unknown stat '{name}'"
                )))
            }
        }
        Expr::Flag(name) => {
            if flags.contains(&name.as_str()) {
                Ok(ExprType::Bool)
            } else {
                Err(expr_refusal(format!(
                    "expression references unknown flag '{name}'"
                )))
            }
        }
        Expr::TierOf(name) => {
            if stats.contains(&name.as_str()) {
                Ok(ExprType::Int)
            } else {
                Err(expr_refusal(format!(
                    "tier-of references unknown stat '{name}'"
                )))
            }
        }
        Expr::ValueRef(name) => {
            let Some((_, definition)) = values.iter().find(|(id, _)| id == name) else {
                return Err(expr_refusal(format!(
                    "expression references unknown value '{name}'"
                )));
            };
            if visiting.iter().any(|seen| seen == name) {
                return Err(expr_refusal(format!(
                    "named values form a reference cycle at '{name}'"
                )));
            }
            visiting.push(name.clone());
            let kind = check_expr(definition, stats, flags, values, visiting);
            visiting.pop();
            kind
        }
        Expr::Compare(op, lhs, rhs) => {
            let lhs_kind = check_expr(lhs, stats, flags, values, visiting)?;
            let rhs_kind = check_expr(rhs, stats, flags, values, visiting)?;
            if lhs_kind != rhs_kind {
                return Err(expr_refusal(
                    "comparison operands have different kinds".to_string(),
                ));
            }
            if lhs_kind == ExprType::Bool && !matches!(op, Cmp::Eq | Cmp::Ne) {
                return Err(expr_refusal(
                    "flags compare only for equality: ordering a flag is refused".to_string(),
                ));
            }
            Ok(ExprType::Bool)
        }
        Expr::And(lhs, rhs) | Expr::Or(lhs, rhs) => {
            for side in [lhs, rhs] {
                if check_expr(side, stats, flags, values, visiting)? != ExprType::Bool {
                    return Err(expr_refusal("a logic operand is not boolean".to_string()));
                }
            }
            Ok(ExprType::Bool)
        }
        Expr::Not(inner) => {
            if check_expr(inner, stats, flags, values, visiting)? != ExprType::Bool {
                return Err(expr_refusal(
                    "a negation operand is not boolean".to_string(),
                ));
            }
            Ok(ExprType::Bool)
        }
        Expr::Arith(_, lhs, rhs) => {
            for side in [lhs, rhs] {
                if check_expr(side, stats, flags, values, visiting)? != ExprType::Int {
                    return Err(expr_refusal(
                        "an arithmetic operand is not an integer (a flag cannot be added to or multiplied)".to_string(),
                    ));
                }
            }
            Ok(ExprType::Int)
        }
        Expr::Clamp(value, lo, hi) => {
            for side in [value, lo, hi] {
                if check_expr(side, stats, flags, values, visiting)? != ExprType::Int {
                    return Err(expr_refusal(
                        "a clamp operand is not an integer".to_string(),
                    ));
                }
            }
            Ok(ExprType::Int)
        }
    }
}

/// Check a stat's tier bands (§G2.2): thresholds strictly
/// ascending, every band inside the stat's declared range, and
/// exactly one more tier name than thresholds.
pub fn check_tier_bands(
    stat: &str,
    stat_min: i64,
    stat_max: i64,
    thresholds: &[i64],
    names: &[String],
) -> Result<(), Refusal> {
    if names.len() != thresholds.len() + 1 {
        return refuse(
            RefusalClass::GameTier,
            format!(
                "stat '{stat}' declares {} thresholds but {} tier names: a tier set needs exactly one more name than thresholds",
                thresholds.len(),
                names.len()
            ),
        );
    }
    for pair in thresholds.windows(2) {
        if pair[1] <= pair[0] {
            return refuse(
                RefusalClass::GameTier,
                format!(
                    "stat '{stat}' tier thresholds are not strictly ascending at {}",
                    pair[1]
                ),
            );
        }
    }
    for threshold in thresholds {
        if *threshold <= stat_min || *threshold > stat_max {
            return refuse(
                RefusalClass::GameTier,
                format!(
                    "stat '{stat}' tier threshold {threshold} is outside its declared range {stat_min}..={stat_max}"
                ),
            );
        }
    }
    Ok(())
}

/// One weighted-table outcome for [`check_table`].
#[derive(Clone, Debug, PartialEq)]
pub struct TableOutcome {
    /// The outcome's id.
    pub id: String,
    /// Its integer weight.
    pub weight: i64,
}

/// Check a weighted table (§G2.3): a named, defined RNG stream;
/// positive weights; a non-zero total; unique outcome ids. (The
/// per-outcome effect-set existence check rides `table_define`
/// in G2, under `TG-AUTH-DANGLING`.)
pub fn check_table(
    stream: Option<&str>,
    streams: &[&str],
    outcomes: &[TableOutcome],
) -> Result<(), Refusal> {
    let Some(stream) = stream else {
        return refuse(
            RefusalClass::GameTable,
            "table names no RNG stream: nondeterministic selection is refused",
        );
    };
    if !streams.contains(&stream) {
        return refuse(
            RefusalClass::Dangling,
            format!("table names RNG stream '{stream}', which is not defined"),
        );
    }
    let mut seen = BTreeSet::new();
    let mut total: i64 = 0;
    for outcome in outcomes {
        if outcome.weight <= 0 {
            return refuse(
                RefusalClass::GameTable,
                format!(
                    "outcome '{}' has non-positive weight {}",
                    outcome.id, outcome.weight
                ),
            );
        }
        if !seen.insert(outcome.id.as_str()) {
            return refuse(
                RefusalClass::GameTable,
                format!("table names outcome '{}' twice", outcome.id),
            );
        }
        total = match total.checked_add(outcome.weight) {
            Some(sum) => sum,
            None => {
                return refuse(RefusalClass::GameTable, "table weights overflow the total");
            }
        };
    }
    if total == 0 {
        return refuse(
            RefusalClass::GameTable,
            "table weights total zero: nothing can be drawn",
        );
    }
    Ok(())
}
