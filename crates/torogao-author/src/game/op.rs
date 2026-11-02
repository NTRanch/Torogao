//! The AUTH-G verb manifest and the typed game-op inputs.
//!
//! All 22 verbs of Amendment 2 §G1/§G2 are named here, each
//! tagged with the slice that delivers it. The Phase 2 slices
//! implement the verbs they need (see `implemented`); every
//! other verb is refused by name (`TG-AUTH-UNKNOWN-KIND`), never
//! silently ignored — the rig manifest's discipline, carried
//! over. The registry fold (the A4 delta) consumes this table
//! when it lands.
//!
//! Expression and atom inputs are JSON trees tagged by `kind`;
//! conversion to the stored records (with the gate's typing)
//! lives in `game::expr` and `game::apply`.

use crate::refusal::{Refusal, RefusalClass, refuse};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The slice that delivers a game verb.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GameSlice {
    /// Phase 2 slice 1: `content.ron` sidecar + dialogue
    /// emission discipline (D-468).
    P21,
    /// Phase 2 slice 2: state lifetime primitive (D-463).
    P22,
    /// Phase 2 slice 3: deferral semantics (D-464).
    P23,
    /// Phase 2 slice 4: expression table weights (D-469).
    P24,
    /// Phase 2 slice 6: visual-state delta table
    /// (Modification App parity).
    P26,
    /// AUTH-G slice G1 remainder (core records not yet needed by
    /// a Phase 2 slice).
    G1,
    /// AUTH-G slice G2 remainder (presentation/flow records not
    /// yet needed by a Phase 2 slice).
    G2,
}

impl GameSlice {
    /// The slice label used in listings and refusals.
    pub fn label(self) -> &'static str {
        match self {
            GameSlice::P21 => "Phase 2 slice 1 (content sidecar)",
            GameSlice::P22 => "Phase 2 slice 2 (state lifetime)",
            GameSlice::P23 => "Phase 2 slice 3 (deferral)",
            GameSlice::P24 => "Phase 2 slice 4 (expression table weights)",
            GameSlice::P26 => "Phase 2 slice 6 (visual-state deltas)",
            GameSlice::G1 => "AUTH-G G1",
            GameSlice::G2 => "AUTH-G G2",
        }
    }
}

/// One game-manifest row.
#[derive(Clone, Copy, Debug)]
pub struct GameVerbSpec {
    /// Canonical verb name.
    pub name: &'static str,
    /// Manifest group.
    pub group: &'static str,
    /// Delivering slice.
    pub slice: GameSlice,
    /// True when the verb mutates the project.
    pub mutating: bool,
    /// True when this build implements the verb.
    pub implemented: bool,
}

const fn verb(
    name: &'static str,
    group: &'static str,
    slice: GameSlice,
    mutating: bool,
    implemented: bool,
) -> GameVerbSpec {
    GameVerbSpec {
        name,
        group,
        slice,
        mutating,
        implemented,
    }
}

/// The AUTH-G manifest: the Amendment's 22 verbs plus the
/// visual-state pair P2-6 adds (24), alphabetical within
/// group.
pub const GAME_MANIFEST: [GameVerbSpec; 24] = [
    verb("clock_define", "game core", GameSlice::P22, true, true),
    verb("game_define", "game core", GameSlice::P21, true, true),
    verb(
        "game_save_schema_define",
        "game core",
        GameSlice::G1,
        true,
        true,
    ),
    verb("rng_stream_define", "game core", GameSlice::P24, true, true),
    verb("condition_define", "game state", GameSlice::P22, true, true),
    verb(
        "effect_set_define",
        "game state",
        GameSlice::P22,
        true,
        true,
    ),
    verb("flag_define", "game state", GameSlice::P22, true, true),
    verb("stat_define", "game state", GameSlice::P22, true, true),
    verb("stat_tier_define", "game state", GameSlice::P22, true, true),
    verb("table_define", "game state", GameSlice::P24, true, true),
    verb("value_define", "game state", GameSlice::P22, true, true),
    verb("character_define", "game flow", GameSlice::P21, true, true),
    verb("choice_define", "game flow", GameSlice::G2, true, true),
    verb(
        "dialogue_line_define",
        "game flow",
        GameSlice::P21,
        true,
        true,
    ),
    verb("ending_define", "game flow", GameSlice::G2, true, true),
    verb(
        "gallery_entry_define",
        "game flow",
        GameSlice::G2,
        true,
        true,
    ),
    verb("scene_bind_rig", "game flow", GameSlice::G2, true, true),
    verb("scene_define", "game flow", GameSlice::P21, true, true),
    verb("scene_exit_define", "game flow", GameSlice::G2, true, true),
    verb("schedule_define", "game flow", GameSlice::G2, true, true),
    verb("trigger_define", "game flow", GameSlice::P22, true, true),
    verb(
        "appearance_define",
        "game appearance",
        GameSlice::P26,
        true,
        true,
    ),
    verb(
        "visual_state_define",
        "game appearance",
        GameSlice::P26,
        true,
        true,
    ),
    verb("game_simulate", "game runtime", GameSlice::P22, false, true),
];

/// The game-manifest row for a verb name.
pub fn game_verb_spec(name: &str) -> Option<&'static GameVerbSpec> {
    GAME_MANIFEST.iter().find(|spec| spec.name == name)
}

/// Refuses a game verb this build does not implement, by name.
pub fn refuse_game_unimplemented<T>(name: &str) -> Result<T, Refusal> {
    match game_verb_spec(name) {
        Some(spec) => refuse(
            RefusalClass::UnknownKind,
            format!(
                "verb '{name}' belongs to {}; it is not implemented in this build",
                spec.slice.label()
            ),
        ),
        None => refuse(
            RefusalClass::UnknownKind,
            format!("verb '{name}' is not in the authoring manifest"),
        ),
    }
}

// ---- shared input shapes -------------------------------------------

/// An expression tree, as input (tagged by `kind`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum ExprInput {
    /// `{"kind": "add", "left": ..., "right": ...}`.
    Add {
        /// Left operand.
        left: Box<ExprInput>,
        /// Right operand.
        right: Box<ExprInput>,
    },
    /// `{"kind": "and", "args": [...]}`.
    And {
        /// The arguments (one or more).
        args: Vec<ExprInput>,
    },
    /// `{"kind": "bool_lit", "value": true}`.
    BoolLit {
        /// The literal.
        value: bool,
    },
    /// `{"kind": "clamp", "value": ..., "lo": ..., "hi": ...}`.
    Clamp {
        /// The value.
        value: Box<ExprInput>,
        /// The lower bound.
        lo: Box<ExprInput>,
        /// The upper bound.
        hi: Box<ExprInput>,
    },
    /// `{"kind": "clock", "field": "day" | "slot" | "weekday" |
    /// "days_remaining"}`.
    Clock {
        /// The clock field name.
        field: String,
    },
    /// `{"kind": "cmp", "op": "eq"|"ne"|"lt"|"le"|"gt"|"ge",
    /// "left": ..., "right": ...}`.
    Cmp {
        /// The operator name.
        op: String,
        /// Left operand.
        left: Box<ExprInput>,
        /// Right operand.
        right: Box<ExprInput>,
    },
    /// `{"kind": "flag", "flag": "id"}`.
    Flag {
        /// The flag id.
        flag: String,
    },
    /// `{"kind": "lit", "value": 3}`.
    Lit {
        /// The literal.
        value: i64,
    },
    /// `{"kind": "max", "left": ..., "right": ...}`.
    Max {
        /// Left operand.
        left: Box<ExprInput>,
        /// Right operand.
        right: Box<ExprInput>,
    },
    /// `{"kind": "min", "left": ..., "right": ...}`.
    Min {
        /// Left operand.
        left: Box<ExprInput>,
        /// Right operand.
        right: Box<ExprInput>,
    },
    /// `{"kind": "mul", "left": ..., "right": ...}`.
    Mul {
        /// Left operand.
        left: Box<ExprInput>,
        /// Right operand.
        right: Box<ExprInput>,
    },
    /// `{"kind": "neg", "expr": ...}`.
    Neg {
        /// The operand.
        expr: Box<ExprInput>,
    },
    /// `{"kind": "not", "expr": ...}`.
    Not {
        /// The operand.
        expr: Box<ExprInput>,
    },
    /// `{"kind": "or", "args": [...]}`.
    Or {
        /// The arguments (one or more).
        args: Vec<ExprInput>,
    },
    /// `{"kind": "stat", "stat": "id", "character": "id"?}`.
    Stat {
        /// The stat id.
        stat: String,
        /// The character instance, for character-scoped stats.
        #[serde(default)]
        character: Option<String>,
    },
    /// `{"kind": "sub", "left": ..., "right": ...}`.
    Sub {
        /// Left operand.
        left: Box<ExprInput>,
        /// Right operand.
        right: Box<ExprInput>,
    },
    /// `{"kind": "tier_of", "stat": "id", "character": "id"?}`.
    TierOf {
        /// The stat id.
        stat: String,
        /// The character instance, for character-scoped stats.
        #[serde(default)]
        character: Option<String>,
    },
    /// `{"kind": "value", "value": "id"}`.
    Value {
        /// The named value's id.
        value: String,
    },
}

/// One effect atom, as input (tagged by `kind`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum AtomInput {
    /// `{"kind": "emit_event", "event": "name"}`.
    EmitEvent {
        /// The event name.
        event: String,
    },
    /// `{"kind": "enter_scene", "scene": "id"}`.
    EnterScene {
        /// The target scene.
        scene: String,
    },
    /// `{"kind": "flag_clear", "flag": "id"}`.
    FlagClear {
        /// The flag id.
        flag: String,
    },
    /// `{"kind": "flag_set", "flag": "id"}`.
    FlagSet {
        /// The flag id.
        flag: String,
    },
    /// `{"kind": "roll_table", "table": "id"}`.
    RollTable {
        /// The table id.
        table: String,
    },
    /// `{"kind": "stat_add", "stat": "id", "character": "id"?,
    /// "amount": expr}`.
    StatAdd {
        /// The stat id.
        stat: String,
        /// The character instance, for character-scoped stats.
        #[serde(default)]
        character: Option<String>,
        /// The amount expression.
        amount: ExprInput,
    },
    /// `{"kind": "stat_set", "stat": "id", "character": "id"?,
    /// "value": expr}`.
    StatSet {
        /// The stat id.
        stat: String,
        /// The character instance, for character-scoped stats.
        #[serde(default)]
        character: Option<String>,
        /// The value expression.
        value: ExprInput,
    },
}

// ---- input records --------------------------------------------------

/// `game_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GameDefineInput {
    /// The game id.
    pub id: String,
    /// The scene the game starts in (a forward reference;
    /// validation requires it to exist).
    #[serde(default)]
    pub start_scene: Option<String>,
    /// The authored save-schema version (default 1).
    #[serde(default)]
    pub save_schema_version: Option<i64>,
    /// The alter-ego veto rule: an effect-set id (a forward
    /// reference at define time), applied when the player
    /// picks a non-alter-ego option while an alter-ego option
    /// was offered (C-9/D-549).
    #[serde(default)]
    pub alter_ego_veto: Option<String>,
}

/// `clock_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ClockDefineInput {
    /// Slot ids in day order (the day's periods).
    pub slots: Vec<String>,
    /// Weekday cycle length, when the design names weekdays.
    #[serde(default)]
    pub days_per_week: Option<u32>,
    /// Last day of the run, when the design bounds it.
    #[serde(default)]
    pub day_limit: Option<u32>,
}

/// Stat scope, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatScopeInput {
    /// One instance per character.
    Character,
    /// One instance for the whole run.
    Game,
    /// One instance for the party.
    Party,
}

/// Stat kind, as input (tagged by `kind`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum StatKindInput {
    /// One of the named values (stored as its index); `min`
    /// must be 0 and `max` the value count minus one.
    Enum {
        /// The named values, in index order.
        values: Vec<String>,
    },
    /// A fixed-point integer (milliunits).
    Fixed,
    /// A plain integer.
    Int,
}

/// Overflow policy, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowInput {
    /// Saturate to the declared range.
    Clamp,
    /// Refuse the whole effect-set transaction.
    Refuse,
}

/// `stat_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatDefineInput {
    /// Stat id.
    pub id: String,
    /// Where instances live.
    pub scope: StatScopeInput,
    /// Value kind.
    pub kind: StatKindInput,
    /// Smallest admitted value.
    pub min: i64,
    /// Largest admitted value.
    pub max: i64,
    /// Value at genesis (and the lifetime reset target).
    pub default: i64,
    /// Out-of-range write policy.
    pub overflow: OverflowInput,
    /// The authored lifetime/decay window in days (D-463), when
    /// the stat carries one.
    #[serde(default)]
    pub lifetime_days: Option<u32>,
}

/// `stat_tier_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatTierDefineInput {
    /// The stat the bands range over.
    pub stat: String,
    /// Ascending thresholds, each within (min, max] of the stat.
    pub thresholds: Vec<i64>,
    /// Band names, lowest first; thresholds + 1 of them.
    pub names: Vec<String>,
}

/// Flag scope, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagScopeInput {
    /// Cross-run state (the globals partition).
    Globals,
    /// Per-run state (the slot partition).
    Run,
}

/// `flag_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FlagDefineInput {
    /// Flag id.
    pub id: String,
    /// Which save partition holds it.
    pub scope: FlagScopeInput,
    /// Value at genesis.
    #[serde(default)]
    pub default: bool,
}

/// `value_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ValueDefineInput {
    /// Value id.
    pub id: String,
    /// The defining expression.
    pub expr: ExprInput,
}

/// `condition_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionDefineInput {
    /// Condition id.
    pub id: String,
    /// The boolean expression.
    pub expr: ExprInput,
}

/// `effect_set_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EffectSetDefineInput {
    /// Effect set id.
    pub id: String,
    /// The atoms, in application order.
    pub atoms: Vec<AtomInput>,
}

/// One genesis stat override, as input.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StatInitialInput {
    /// The stat id.
    pub stat: String,
    /// The genesis value.
    pub value: i64,
}

/// `character_define` inputs. The display name's prose travels
/// with the op (and is recorded in the op log); the character
/// record itself carries only the content reference.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CharacterDefineInput {
    /// Character id.
    pub id: String,
    /// Content id of the display name.
    pub name_ref: String,
    /// The display-name prose (written to `content.ron`).
    pub name_text: String,
    /// Genesis stat overrides.
    #[serde(default)]
    pub initials: Vec<StatInitialInput>,
}

/// Scene presentation class, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationInput {
    /// An event scene (focus card).
    Event,
    /// A room scene.
    Room,
    /// A visual-novel scene.
    Vn,
}

/// `scene_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneDefineInput {
    /// Scene id.
    pub id: String,
    /// Presentation class.
    pub presentation: PresentationInput,
    /// The characters present in the scene (C-8/D-548).
    #[serde(default)]
    pub participants: Vec<String>,
    /// The backdrop: an asset id or a rig scene id.
    #[serde(default)]
    pub backdrop: Option<String>,
    /// A condition id gating entry to the scene.
    #[serde(default)]
    pub entry_condition: Option<String>,
    /// An effect set applied when the scene is entered.
    #[serde(default)]
    pub on_enter: Option<String>,
    /// An effect set applied when the scene is left.
    #[serde(default)]
    pub on_exit: Option<String>,
}

/// `dialogue_line_define` inputs. The prose travels with the op
/// (and is recorded in the op log); the line record itself
/// carries only the content reference, so later editorial edits
/// to `content.ron` never churn `project.ron` (D-468).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueLineDefineInput {
    /// Line id.
    pub id: String,
    /// The scene the line belongs to.
    pub scene: String,
    /// Position in the scene's chain (0-based, gapless; a new
    /// line's ordinal must be the scene's current line count).
    pub ordinal: u32,
    /// The speaking character id, or `narrator`.
    pub speaker: String,
    /// The content id of the line's prose.
    pub text_ref: String,
    /// The line's prose (written to `content.ron`).
    pub text: String,
    /// The next line in the chain, when the chain does not end
    /// here.
    #[serde(default)]
    pub next: Option<String>,
    /// A rig expression the line asks the stage to show
    /// (resolved at validation time).
    #[serde(default)]
    pub hook_expression: Option<String>,
}

/// Direction/edge names, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectionInput {
    /// At-or-above/on to below/off.
    Falling,
    /// Below/off to at-or-above/on.
    Rising,
}

/// Flag edge names, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlagEdgeInput {
    /// Either edge.
    Both,
    /// On to off.
    Falling,
    /// Off to on.
    Rising,
}

/// A trigger's kind, as input (tagged by `kind`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum TriggerKindInput {
    /// Fires when a scene is entered.
    OnEnterScene {
        /// The scene whose entry fires it.
        scene: String,
    },
    /// Fires when an engine input event (or an emitted event)
    /// arrives.
    OnEvent {
        /// The event name.
        event: String,
    },
    /// Fires when a flag edges.
    OnFlag {
        /// The flag id.
        flag: String,
        /// Which edge fires it.
        edge: FlagEdgeInput,
    },
    /// Fires when a stat crosses a threshold.
    OnThreshold {
        /// The stat id.
        stat: String,
        /// The character instance, for character-scoped stats.
        #[serde(default)]
        character: Option<String>,
        /// The threshold value.
        threshold: i64,
        /// Which crossing fires it.
        direction: DirectionInput,
    },
    /// Fires on clock positions matching the pattern: every
    /// present field must match.
    OnTick {
        /// Slot ids that match (absent: every slot).
        #[serde(default)]
        slots: Option<Vec<String>>,
        /// Weekdays that match (absent: every weekday).
        #[serde(default)]
        weekdays: Option<Vec<u32>>,
        /// `[period, remainder]`: matches when
        /// `day % period == remainder`.
        #[serde(default)]
        day_modulo: Option<(u32, u32)>,
        /// One exact day that matches.
        #[serde(default)]
        exact_day: Option<i64>,
    },
}

/// Repetition, as input.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepetitionInput {
    /// Fires at most once per run.
    Once,
    /// May fire whenever its pattern matches again.
    Repeat,
}

/// The offered lifecycle, as input (D-464): when present on a
/// trigger, a match offers instead of firing.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OfferInput {
    /// Days an offer may stay open before it expires; absent
    /// means the offer does not expire.
    #[serde(default)]
    pub expires_days: Option<u32>,
}

/// `trigger_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDefineInput {
    /// Trigger id.
    pub id: String,
    /// When it fires.
    pub kind: TriggerKindInput,
    /// The effect set applied on firing.
    pub effect_set: String,
    /// An optional named gate condition.
    #[serde(default)]
    pub gate: Option<String>,
    /// Firing order key: higher fires first; ties break by id.
    #[serde(default)]
    pub priority: i64,
    /// Once or repeat (default repeat).
    #[serde(default = "default_repetition")]
    pub repetition: RepetitionInput,
    /// Ticks the trigger sits out after firing (default 0).
    #[serde(default)]
    pub cooldown_ticks: u32,
    /// The offered lifecycle (D-464), when the trigger offers
    /// instead of firing outright.
    #[serde(default)]
    pub offer: Option<OfferInput>,
}

fn default_repetition() -> RepetitionInput {
    RepetitionInput::Repeat
}

/// `rng_stream_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RngStreamDefineInput {
    /// Stream id.
    pub id: String,
    /// The algorithm name; this build implements exactly
    /// `estate-rng/1`.
    pub algorithm: String,
}

/// One table outcome's weight, as input: a bare positive
/// integer (the static form), or an expression tree evaluated
/// at draw time (the D-469 form).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum WeightInput {
    /// An expression tree.
    Expr(ExprInput),
    /// A positive integer literal.
    Static(i64),
}

/// One table outcome, as input.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TableOutcomeInput {
    /// Outcome id (unique within the table).
    pub id: String,
    /// The outcome's weight.
    pub weight: WeightInput,
    /// An optional named gate condition.
    #[serde(default)]
    pub gate: Option<String>,
    /// The effect set applied when the outcome is drawn.
    pub effect_set: String,
}

/// `table_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TableDefineInput {
    /// Table id.
    pub id: String,
    /// The RNG stream draws consume.
    pub stream: String,
    /// The outcomes, in authored (draw-walk) order.
    pub outcomes: Vec<TableOutcomeInput>,
}

/// An appearance slot's value kind, as input.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceKindInput {
    /// An integer list.
    Ints,
    /// A text token.
    Text,
}

/// One appearance slot declaration, as input.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceSlotInput {
    /// Slot id.
    pub id: String,
    /// The kind of value the slot carries.
    pub kind: AppearanceKindInput,
}

/// An appearance value, as input: a text token, or an integer
/// list.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum AppearanceValueInput {
    /// A text token.
    Text(String),
    /// An integer list.
    Ints(Vec<i64>),
}

/// One appearance assignment, as input.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceEntryInput {
    /// The slot being assigned.
    pub slot: String,
    /// The assigned value.
    pub value: AppearanceValueInput,
}

/// `appearance_define` inputs: a character's slot vocabulary
/// and the baseline value of every slot.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppearanceDefineInput {
    /// The character the table belongs to.
    pub character: String,
    /// The declared slots.
    pub slots: Vec<AppearanceSlotInput>,
    /// The baseline ("Default") value of every slot.
    pub base: Vec<AppearanceEntryInput>,
}

/// `visual_state_define` inputs: one progression state's
/// sparse delta over a character's baseline appearance. The
/// delta may be empty — an empty delta is a first-class
/// value, not a missing row.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisualStateDefineInput {
    /// The character whose appearance the state overrides.
    pub character: String,
    /// The progression state's name.
    pub state: String,
    /// The sparse overrides.
    #[serde(default)]
    pub delta: Vec<AppearanceEntryInput>,
}

/// A save-context policy, as input (tagged by `kind`): the
/// policy surface of the save-schema family. The branch's
/// `game_define.save_policy` folds into this one surface
/// (D-549): it is authored by `game_save_schema_define`, never
/// by `game_define`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
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

/// One migration step in `game_save_schema_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationInput {
    /// The version this step migrates from.
    pub from: u32,
    /// The migration's name.
    pub name: String,
}

/// `game_save_schema_define` inputs (Amendment 2 §G3.3).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSchemaDefineInput {
    /// Schema version.
    pub version: u32,
    /// Slot count, 1..=64.
    pub slots: u32,
    /// Whether a slot holds one run (`one_run`) or a chapter
    /// list (`chapter_list`).
    pub slot_holds: String,
    /// State fields in the per-slot partition.
    pub slot_fields: Vec<String>,
    /// State fields in the globals partition.
    pub globals_fields: Vec<String>,
    /// The named hash algorithm version.
    pub hash_algorithm: String,
    /// Where a manual save may be taken.
    pub policy: SavePolicyInput,
    /// The migration chain segment.
    #[serde(default)]
    pub migrations: Vec<MigrationInput>,
}

/// Where a choice option leads, as input (tagged by `kind`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum ChoiceTargetInput {
    /// The ending evaluation point.
    EndingCheck,
    /// A scene exit, by its id within the choice's scene.
    Exit {
        /// The exit id.
        id: String,
    },
    /// A dialogue line, by id.
    Line {
        /// The line id.
        id: String,
    },
    /// A scene, by id (a direct transition target).
    Scene {
        /// The scene id.
        id: String,
    },
}

/// One option in `choice_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceOptionInput {
    /// Option id.
    pub id: String,
    /// Content id of the label text (a `content.ron` key,
    /// C-7/D-547).
    pub label_ref: String,
    /// Condition id gating visibility.
    #[serde(default)]
    pub visible_if: Option<String>,
    /// Condition id gating enablement.
    #[serde(default)]
    pub enabled_if: Option<String>,
    /// Integer sort expression.
    #[serde(default)]
    pub sort: Option<ExprInput>,
    /// Origin tag: `player` or `alter_ego`.
    pub origin: String,
    /// Effect set id applied when the option is taken.
    #[serde(default)]
    pub effect_set: Option<String>,
    /// Where the option leads.
    pub target: ChoiceTargetInput,
}

/// `choice_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChoiceDefineInput {
    /// Choice id.
    pub id: String,
    /// The scene the choice belongs to.
    pub scene: String,
    /// The prompt's content id (a `content.ron` key,
    /// C-7/D-547), when the choice has one.
    #[serde(default)]
    pub prompt_line: Option<String>,
    /// The options, in authored order.
    pub options: Vec<ChoiceOptionInput>,
}

/// `ending_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EndingDefineInput {
    /// Ending id.
    pub id: String,
    /// The condition id that must hold.
    pub condition: String,
    /// Evaluation priority (unique across endings).
    pub priority: i64,
    /// The epilogue scene id.
    pub epilogue_scene: String,
}

/// How a gallery entry unlocks, as input (tagged by `kind`).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case", tag = "kind")]
pub enum GalleryUnlockInput {
    /// Auto-unlock under this condition id.
    Condition {
        /// The condition id.
        id: String,
    },
    /// An effect names this entry.
    Effect,
}

/// `gallery_entry_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GalleryEntryDefineInput {
    /// Entry id.
    pub id: String,
    /// The gallery category.
    pub category: String,
    /// How the entry unlocks.
    pub unlock: GalleryUnlockInput,
    /// The replay target scene id.
    pub replay_scene: String,
    /// The replay target entry line id.
    pub replay_line: String,
    /// Thumbnail asset id.
    #[serde(default)]
    pub thumbnail: Option<String>,
}

/// One slot → scene row in `schedule_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleSlotInput {
    /// The clock slot name.
    pub slot: String,
    /// The scene id.
    pub scene: String,
}

/// One condition override in `schedule_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleOverrideInput {
    /// The condition id.
    pub condition: String,
    /// The scene id.
    pub scene: String,
}

/// `schedule_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDefineInput {
    /// The character id.
    pub character: String,
    /// Slot → scene rows: every clock slot, exactly once.
    pub slots: Vec<ScheduleSlotInput>,
    /// Ordered condition overrides.
    #[serde(default)]
    pub overrides: Vec<ScheduleOverrideInput>,
}

/// `scene_bind_rig` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneBindRigInput {
    /// The scene id.
    pub scene: String,
    /// The participant (character) id.
    pub participant: String,
    /// The rig model id.
    pub rig_model: String,
    /// The entry expression id (a rig expression).
    pub entry_expression: String,
    /// The entry motion id (a motion clip).
    #[serde(default)]
    pub entry_motion: Option<String>,
}

/// `scene_exit_define` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneExitDefineInput {
    /// The scene the exit leaves.
    pub scene: String,
    /// Exit id, unique within the scene.
    pub id: String,
    /// The target scene id.
    pub target: String,
    /// Condition id gating the exit.
    #[serde(default)]
    pub condition: Option<String>,
    /// Effect set id applied on traversal.
    #[serde(default)]
    pub effect_set: Option<String>,
}

/// One choice-log entry, as input to `game_simulate`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntryInput {
    /// The tick the input lands on (1-based).
    pub tick: u64,
    /// `advance`, `event`, `offer`, or `choice`.
    pub kind: String,
    /// The event name or trigger id, by kind (empty for
    /// `advance`).
    pub id: String,
    /// The offer decision (`accept` / `decline`), for offers.
    #[serde(default)]
    pub option: Option<String>,
}

/// `game_simulate` inputs.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SimulateInput {
    /// The run seed (recorded in the run's evidence; draws on
    /// named streams consume it once tables land).
    pub run_seed: i64,
    /// Ticks to run.
    pub ticks: u64,
    /// The choice log's project binding (§G3.2): when given, it
    /// must equal the loaded project's sha256.
    #[serde(default)]
    pub project_sha256: Option<String>,
    /// The choice log.
    #[serde(default)]
    pub choice_log: Vec<LogEntryInput>,
}

/// A mutating game op: one game verb with its typed inputs.
#[derive(Clone, Debug, PartialEq)]
pub enum GameOp {
    /// `appearance_define`.
    AppearanceDefine(AppearanceDefineInput),
    /// `character_define`.
    CharacterDefine(CharacterDefineInput),
    /// `clock_define`.
    ClockDefine(ClockDefineInput),
    /// `condition_define`.
    ConditionDefine(ConditionDefineInput),
    /// `dialogue_line_define`.
    DialogueLineDefine(DialogueLineDefineInput),
    /// `effect_set_define`.
    EffectSetDefine(EffectSetDefineInput),
    /// `flag_define`.
    FlagDefine(FlagDefineInput),
    /// `game_define`.
    GameDefine(GameDefineInput),
    /// `rng_stream_define`.
    RngStreamDefine(RngStreamDefineInput),
    /// `scene_define`.
    SceneDefine(SceneDefineInput),
    /// `stat_define`.
    StatDefine(StatDefineInput),
    /// `stat_tier_define`.
    StatTierDefine(StatTierDefineInput),
    /// `table_define`.
    TableDefine(TableDefineInput),
    /// `trigger_define`.
    TriggerDefine(TriggerDefineInput),
    /// `value_define`.
    ValueDefine(ValueDefineInput),
    /// `visual_state_define`.
    VisualStateDefine(VisualStateDefineInput),
    /// `choice_define`.
    ChoiceDefine(ChoiceDefineInput),
    /// `ending_define`.
    EndingDefine(EndingDefineInput),
    /// `gallery_entry_define`.
    GalleryEntryDefine(GalleryEntryDefineInput),
    /// `game_save_schema_define`.
    GameSaveSchemaDefine(SaveSchemaDefineInput),
    /// `schedule_define`.
    ScheduleDefine(ScheduleDefineInput),
    /// `scene_bind_rig`.
    SceneBindRig(SceneBindRigInput),
    /// `scene_exit_define`.
    SceneExitDefine(SceneExitDefineInput),
}

fn typed<T: for<'de> Deserialize<'de>>(verb: &str, inputs: &Value) -> Result<T, Refusal> {
    serde_json::from_value(inputs.clone()).or_else(|err| {
        refuse(
            RefusalClass::Malformed,
            format!("inputs for '{verb}' do not match its schema: {err}"),
        )
    })
}

/// Parses a mutating game verb and its JSON inputs into the
/// crate's `Op`. Deferred game verbs are refused by name;
/// `game_simulate` is read-only and never parses as an op.
pub fn parse_game_op(verb: &str, inputs: &Value) -> Result<crate::op::Op, Refusal> {
    let op = match verb {
        "appearance_define" => GameOp::AppearanceDefine(typed(verb, inputs)?),
        "character_define" => GameOp::CharacterDefine(typed(verb, inputs)?),
        "clock_define" => GameOp::ClockDefine(typed(verb, inputs)?),
        "condition_define" => GameOp::ConditionDefine(typed(verb, inputs)?),
        "dialogue_line_define" => GameOp::DialogueLineDefine(typed(verb, inputs)?),
        "effect_set_define" => GameOp::EffectSetDefine(typed(verb, inputs)?),
        "flag_define" => GameOp::FlagDefine(typed(verb, inputs)?),
        "choice_define" => GameOp::ChoiceDefine(typed(verb, inputs)?),
        "ending_define" => GameOp::EndingDefine(typed(verb, inputs)?),
        "gallery_entry_define" => GameOp::GalleryEntryDefine(typed(verb, inputs)?),
        "game_define" => GameOp::GameDefine(typed(verb, inputs)?),
        "game_save_schema_define" => GameOp::GameSaveSchemaDefine(typed(verb, inputs)?),
        "rng_stream_define" => GameOp::RngStreamDefine(typed(verb, inputs)?),
        "schedule_define" => GameOp::ScheduleDefine(typed(verb, inputs)?),
        "scene_bind_rig" => GameOp::SceneBindRig(typed(verb, inputs)?),
        "scene_exit_define" => GameOp::SceneExitDefine(typed(verb, inputs)?),
        "scene_define" => GameOp::SceneDefine(typed(verb, inputs)?),
        "stat_define" => GameOp::StatDefine(typed(verb, inputs)?),
        "stat_tier_define" => GameOp::StatTierDefine(typed(verb, inputs)?),
        "table_define" => GameOp::TableDefine(typed(verb, inputs)?),
        "trigger_define" => GameOp::TriggerDefine(typed(verb, inputs)?),
        "value_define" => GameOp::ValueDefine(typed(verb, inputs)?),
        "visual_state_define" => GameOp::VisualStateDefine(typed(verb, inputs)?),
        other => return refuse_game_unimplemented(other),
    };
    Ok(crate::op::Op::Game(op))
}

impl GameOp {
    /// The canonical verb name.
    pub fn verb(&self) -> &'static str {
        match self {
            GameOp::AppearanceDefine(_) => "appearance_define",
            GameOp::ChoiceDefine(_) => "choice_define",
            GameOp::EndingDefine(_) => "ending_define",
            GameOp::GalleryEntryDefine(_) => "gallery_entry_define",
            GameOp::GameSaveSchemaDefine(_) => "game_save_schema_define",
            GameOp::CharacterDefine(_) => "character_define",
            GameOp::ClockDefine(_) => "clock_define",
            GameOp::ConditionDefine(_) => "condition_define",
            GameOp::DialogueLineDefine(_) => "dialogue_line_define",
            GameOp::EffectSetDefine(_) => "effect_set_define",
            GameOp::FlagDefine(_) => "flag_define",
            GameOp::GameDefine(_) => "game_define",
            GameOp::RngStreamDefine(_) => "rng_stream_define",
            GameOp::ScheduleDefine(_) => "schedule_define",
            GameOp::SceneBindRig(_) => "scene_bind_rig",
            GameOp::SceneExitDefine(_) => "scene_exit_define",
            GameOp::SceneDefine(_) => "scene_define",
            GameOp::StatDefine(_) => "stat_define",
            GameOp::StatTierDefine(_) => "stat_tier_define",
            GameOp::TableDefine(_) => "table_define",
            GameOp::TriggerDefine(_) => "trigger_define",
            GameOp::ValueDefine(_) => "value_define",
            GameOp::VisualStateDefine(_) => "visual_state_define",
        }
    }

    /// The normalized inputs as JSON (what the op log records).
    pub fn inputs_json(&self) -> Result<Value, Refusal> {
        let result = match self {
            GameOp::AppearanceDefine(inputs) => serde_json::to_value(inputs),
            GameOp::ChoiceDefine(inputs) => serde_json::to_value(inputs),
            GameOp::EndingDefine(inputs) => serde_json::to_value(inputs),
            GameOp::GalleryEntryDefine(inputs) => serde_json::to_value(inputs),
            GameOp::GameSaveSchemaDefine(inputs) => serde_json::to_value(inputs),
            GameOp::CharacterDefine(inputs) => serde_json::to_value(inputs),
            GameOp::ClockDefine(inputs) => serde_json::to_value(inputs),
            GameOp::ConditionDefine(inputs) => serde_json::to_value(inputs),
            GameOp::DialogueLineDefine(inputs) => serde_json::to_value(inputs),
            GameOp::EffectSetDefine(inputs) => serde_json::to_value(inputs),
            GameOp::FlagDefine(inputs) => serde_json::to_value(inputs),
            GameOp::GameDefine(inputs) => serde_json::to_value(inputs),
            GameOp::RngStreamDefine(inputs) => serde_json::to_value(inputs),
            GameOp::ScheduleDefine(inputs) => serde_json::to_value(inputs),
            GameOp::SceneBindRig(inputs) => serde_json::to_value(inputs),
            GameOp::SceneExitDefine(inputs) => serde_json::to_value(inputs),
            GameOp::SceneDefine(inputs) => serde_json::to_value(inputs),
            GameOp::StatDefine(inputs) => serde_json::to_value(inputs),
            GameOp::StatTierDefine(inputs) => serde_json::to_value(inputs),
            GameOp::TableDefine(inputs) => serde_json::to_value(inputs),
            GameOp::TriggerDefine(inputs) => serde_json::to_value(inputs),
            GameOp::ValueDefine(inputs) => serde_json::to_value(inputs),
            GameOp::VisualStateDefine(inputs) => serde_json::to_value(inputs),
        };
        result.or_else(|err| {
            refuse(
                RefusalClass::Malformed,
                format!("inputs for '{}' cannot be recorded: {err}", self.verb()),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{GAME_MANIFEST, GameSlice};

    #[test]
    fn game_manifest_has_24_verbs_alphabetical_within_group() {
        assert_eq!(GAME_MANIFEST.len(), 24);
        let mut groups: Vec<&str> = Vec::new();
        for spec in GAME_MANIFEST {
            if groups.last() != Some(&spec.group) {
                assert!(!groups.contains(&spec.group), "group {} split", spec.group);
                groups.push(spec.group);
            }
        }
        assert_eq!(groups.len(), 5);
        for pair in GAME_MANIFEST.windows(2) {
            if pair[0].group == pair[1].group {
                assert!(
                    pair[0].name < pair[1].name,
                    "{} !< {}",
                    pair[0].name,
                    pair[1].name
                );
            }
        }
        let p21 = GAME_MANIFEST
            .iter()
            .filter(|spec| spec.slice == GameSlice::P21 && spec.implemented)
            .count();
        assert_eq!(p21, 4);
        let implemented = GAME_MANIFEST.iter().filter(|spec| spec.implemented).count();
        assert_eq!(implemented, 24);
    }
}
