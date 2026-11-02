//! The AUTH-G game records: what the `game` section of
//! `project.ron` holds (Amendment 2, §G2/§G4).
//!
//! The record layout is designed whole here; the Phase 2 slices
//! add verbs and interpreter semantics, never record shapes, so a
//! project authored before a slice re-emits byte-identically
//! after it. Absent means absent: a project with no game content
//! carries no `game` field at all, and inside the section an
//! absent lifetime, gate, or offer is a missing field, never an
//! empty scaffold record.
//!
//! Game values are integers only (fixed-point milliunits where a
//! design needs fractions): no float ever enters game state, so
//! game records hold no `f64`.

use crate::model::find;

/// The game section's format version. Version 1 carried the
/// records only; version 2 is the tick-contract ruling shared by
/// Phase 2 slices P2-2 (state lifetime) and P2-3 (deferral): the
/// interpreter evaluates lifetimes and offer expiry inside the
/// tick. The bump is one ruling for the pair, not one per slice.
pub const GAME_FORMAT_VERSION: i64 = 2;
/// The first game format version (records only, no interpreter).
pub const GAME_FORMAT_VERSION_FIRST: i64 = 1;
/// The canonical authored locale of `content.ron` (D-472):
/// English is the authored sidecar; any other locale is a
/// translation of it, never the reverse.
pub const CONTENT_LOCALE_CANONICAL: &str = "en";
/// The hash algorithm name saves and state hashes carry.
pub const GAME_HASH_ALGO: &str = "sha256/1";

/// One authored game: the whole `game` section of a project.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Game {
    /// Game id (the project's game identity).
    pub id: String,
    /// Game format version of this section (see
    /// [`GAME_FORMAT_VERSION`]).
    pub format_version: i64,
    /// The authored save-schema version saves must not exceed.
    /// This stored scalar is only the pre-schema default: once
    /// a save schema is authored, the binding version is
    /// derived from it (see `derived_save_schema_version`).
    pub save_schema_version: i64,
    /// Scene the game starts in (a forward reference at define
    /// time; the validation sweep requires it to exist).
    pub start_scene: Option<String>,
    /// The alter-ego veto rule (C-9/D-549, re-homed from the
    /// AUTH-G branch's `GameDef`): an effect-set id the
    /// interpreter applies as its own transaction when the
    /// player picks a non-alter-ego option while an alter-ego
    /// option was offered (Amendment 2 §G5). A forward
    /// reference at define time; the validation sweep requires
    /// it to exist.
    pub alter_ego_veto: Option<String>,
    /// The authored clock, once defined.
    pub clock: Option<Clock>,
    /// Named RNG streams, sorted by id. (Serialized only when
    /// non-empty: absent means absent, so a project authored
    /// before this surface keeps its bytes.)
    pub streams: Vec<RngStreamDef>,
    /// Weighted outcome tables, sorted by id. (Serialized only
    /// when non-empty, like `streams`.)
    pub tables: Vec<TableDef>,
    /// Stats, sorted by id.
    pub stats: Vec<StatDef>,
    /// Tier bands over stats, sorted by stat id.
    pub tiers: Vec<TierDef>,
    /// Flags, sorted by id.
    pub flags: Vec<FlagDef>,
    /// Named derived values, sorted by id.
    pub values: Vec<ValueDef>,
    /// Named gate conditions, sorted by id.
    pub conditions: Vec<ConditionDef>,
    /// Named effect sets, sorted by id.
    pub effect_sets: Vec<EffectSetDef>,
    /// Characters, sorted by id.
    pub characters: Vec<CharacterDef>,
    /// Scenes, sorted by id.
    pub scenes: Vec<SceneDef>,
    /// Dialogue lines, sorted by id.
    pub dialogue: Vec<DialogueLine>,
    /// Triggers, sorted by id.
    pub triggers: Vec<TriggerDef>,
    /// Branch-point choices, sorted by id. (Serialized only
    /// when non-empty, like `streams`.)
    pub choices: Vec<ChoiceDef>,
    /// Ending rules, sorted by id. (Serialized only when
    /// non-empty.)
    pub endings: Vec<EndingDef>,
    /// Gallery replay registrations, sorted by id. (Serialized
    /// only when non-empty.)
    pub gallery_entries: Vec<GalleryEntryDef>,
    /// Per-character schedules, sorted by character.
    /// (Serialized only when non-empty.)
    pub schedules: Vec<ScheduleDef>,
    /// Scene-participant rig bindings, sorted by (scene,
    /// participant). (Serialized only when non-empty.)
    pub scene_bindings: Vec<SceneBindingDef>,
    /// Scene exits, sorted by (scene, id). (Serialized only
    /// when non-empty.)
    pub scene_exits: Vec<SceneExitDef>,
    /// The authored save schema, once defined (C-4/D-544).
    /// (Serialized only when present.)
    pub save_schema: Option<SaveSchema>,
    /// Per-character appearance tables, sorted by character
    /// id. (Serialized only when non-empty, like `streams`.)
    pub appearances: Vec<AppearanceDef>,
    /// Progression-state appearance deltas, sorted by
    /// (character, state). (Serialized only when non-empty.)
    pub visual_states: Vec<VisualStateDef>,
}

impl Game {
    /// The save-schema version saves bind to: the authored
    /// schema's version when a schema exists, otherwise the
    /// stored `save_schema_version` scalar (the pre-schema
    /// default for projects that never author one) (C-4/D-544).
    pub fn derived_save_schema_version(&self) -> i64 {
        self.save_schema
            .as_ref()
            .map_or(self.save_schema_version, |schema| i64::from(schema.version))
    }

    /// The appearance table of this character.
    pub fn appearance(&self, character: &str) -> Option<&AppearanceDef> {
        find(&self.appearances, character, |item| &item.character)
    }

    /// The choice with this id.
    pub fn choice(&self, id: &str) -> Option<&ChoiceDef> {
        find(&self.choices, id, |item| &item.id)
    }

    /// The ending with this id.
    pub fn ending(&self, id: &str) -> Option<&EndingDef> {
        find(&self.endings, id, |item| &item.id)
    }

    /// The gallery entry with this id.
    pub fn gallery_entry(&self, id: &str) -> Option<&GalleryEntryDef> {
        find(&self.gallery_entries, id, |item| &item.id)
    }

    /// The schedule of this character.
    pub fn schedule(&self, character: &str) -> Option<&ScheduleDef> {
        find(&self.schedules, character, |item| &item.character)
    }

    /// The rig binding of this (scene, participant) pair.
    pub fn scene_binding(&self, scene: &str, participant: &str) -> Option<&SceneBindingDef> {
        self.scene_bindings
            .iter()
            .find(|row| row.scene == scene && row.participant == participant)
    }

    /// The exit with this id in this scene.
    pub fn scene_exit(&self, scene: &str, id: &str) -> Option<&SceneExitDef> {
        self.scene_exits
            .iter()
            .find(|row| row.scene == scene && row.id == id)
    }

    /// The character with this id.
    pub fn character(&self, id: &str) -> Option<&CharacterDef> {
        find(&self.characters, id, |item| &item.id)
    }

    /// The condition with this id.
    pub fn condition(&self, id: &str) -> Option<&ConditionDef> {
        find(&self.conditions, id, |item| &item.id)
    }

    /// The dialogue line with this id.
    pub fn dialogue_line(&self, id: &str) -> Option<&DialogueLine> {
        find(&self.dialogue, id, |item| &item.id)
    }

    /// The effect set with this id.
    pub fn effect_set(&self, id: &str) -> Option<&EffectSetDef> {
        find(&self.effect_sets, id, |item| &item.id)
    }

    /// The flag with this id.
    pub fn flag(&self, id: &str) -> Option<&FlagDef> {
        find(&self.flags, id, |item| &item.id)
    }

    /// The scene with this id.
    pub fn scene(&self, id: &str) -> Option<&SceneDef> {
        find(&self.scenes, id, |item| &item.id)
    }

    /// The stat with this id.
    pub fn stat(&self, id: &str) -> Option<&StatDef> {
        find(&self.stats, id, |item| &item.id)
    }

    /// The RNG stream with this id.
    pub fn stream(&self, id: &str) -> Option<&RngStreamDef> {
        find(&self.streams, id, |item| &item.id)
    }

    /// The weighted table with this id.
    pub fn table(&self, id: &str) -> Option<&TableDef> {
        find(&self.tables, id, |item| &item.id)
    }

    /// The tier bands over this stat, if authored.
    pub fn tier(&self, stat: &str) -> Option<&TierDef> {
        find(&self.tiers, stat, |item| &item.stat)
    }

    /// The trigger with this id.
    pub fn trigger(&self, id: &str) -> Option<&TriggerDef> {
        find(&self.triggers, id, |item| &item.id)
    }

    /// The visual-state delta of this character in this state.
    pub fn visual_state(&self, character: &str, state: &str) -> Option<&VisualStateDef> {
        self.visual_states
            .iter()
            .find(|row| row.character == character && row.state == state)
    }

    /// The named value with this id.
    pub fn value(&self, id: &str) -> Option<&ValueDef> {
        find(&self.values, id, |item| &item.id)
    }

    /// Dialogue lines of one scene, in ordinal order.
    pub fn lines_in_scene<'a>(&'a self, scene: &str) -> Vec<&'a DialogueLine> {
        let mut lines: Vec<&DialogueLine> = self
            .dialogue
            .iter()
            .filter(|line| line.scene == scene)
            .collect();
        lines.sort_by_key(|line| line.ordinal);
        lines
    }
}

/// The authored clock: named slots per day, an optional weekday
/// cycle, and an optional day limit. Time is data; the
/// interpreter never reads a wall clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Clock {
    /// Slot ids in day order (the day's periods).
    pub slots: Vec<String>,
    /// Weekday cycle length, when the design names weekdays.
    pub days_per_week: Option<u32>,
    /// Last day of the run, when the design bounds it.
    pub day_limit: Option<u32>,
}

/// Stat scope: where instances of the stat live.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatScope {
    /// One instance for the whole run.
    Game,
    /// One instance for the party (modelled as a single instance
    /// until party membership is authored).
    Party,
    /// One instance per character.
    Character,
}

/// Stat kind. All values are integers at runtime; `Fixed` values
/// are milliunits (value / 1000) by authoring convention, and an
/// `Enum` stat's value is the index of its named value.
#[derive(Clone, Debug, PartialEq)]
pub enum StatKind {
    /// A plain integer.
    Int,
    /// A fixed-point integer (milliunits).
    Fixed,
    /// One of the named values (stored as its index).
    Enum(Vec<String>),
}

/// What an out-of-range write to a stat does.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Overflow {
    /// Saturate to the declared range.
    Clamp,
    /// Refuse the whole effect-set transaction (a runtime event
    /// in the trace, never a partial state).
    Refuse,
}

/// An authored lifetime/decay window on a state record (D-463).
/// While a lifetime-bearing stat's value differs from its
/// default, a counter of `days` days is armed; each day rollover
/// decrements it, and at zero the stat hard-resets to its
/// default. Any authored write to the stat re-arms the counter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lifetime {
    /// The decay window, in days (>= 1).
    pub days: u32,
}

/// One stat definition.
#[derive(Clone, Debug, PartialEq)]
pub struct StatDef {
    /// Stable id.
    pub id: String,
    /// Where instances live.
    pub scope: StatScope,
    /// Value kind.
    pub kind: StatKind,
    /// Smallest admitted value.
    pub min: i64,
    /// Largest admitted value.
    pub max: i64,
    /// Value at genesis (and the lifetime reset target).
    pub default: i64,
    /// Out-of-range write policy.
    pub overflow: Overflow,
    /// The decay window, when the stat carries one.
    pub lifetime: Option<Lifetime>,
}

/// Tier bands over one stat: ascending thresholds; the tier of a
/// value is the count of thresholds it reaches, so the band
/// count is thresholds + 1 and `names` numbers exactly that.
#[derive(Clone, Debug, PartialEq)]
pub struct TierDef {
    /// The stat the bands range over (the record's key).
    pub stat: String,
    /// Ascending thresholds, each within (stat.min, stat.max].
    pub thresholds: Vec<i64>,
    /// Band names, lowest first; thresholds + 1 of them.
    pub names: Vec<String>,
}

impl TierDef {
    /// The band index of a value: thresholds reached.
    pub fn band_of(&self, value: i64) -> usize {
        self.thresholds.iter().filter(|t| value >= **t).count()
    }
}

/// Flag scope: which save partition the flag lives in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlagScope {
    /// Per-run state (the slot partition).
    Run,
    /// Cross-run state (the globals partition).
    Globals,
}

/// One flag definition.
#[derive(Clone, Debug, PartialEq)]
pub struct FlagDef {
    /// Stable id.
    pub id: String,
    /// Which save partition holds it.
    pub scope: FlagScope,
    /// Value at genesis.
    pub default: bool,
}

/// A named derived value: an expression other records may read.
/// Its type is fixed at define time by the gate's inference and
/// stored, so the record is self-describing.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueDef {
    /// Stable id.
    pub id: String,
    /// The defining expression.
    pub expr: Expr,
    /// The inferred type (`Int` or `Bool`).
    pub value_type: ExprType,
}

/// A named gate condition: a boolean expression.
#[derive(Clone, Debug, PartialEq)]
pub struct ConditionDef {
    /// Stable id.
    pub id: String,
    /// The boolean expression.
    pub expr: Expr,
}

/// The two expression value types.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExprType {
    /// A boolean.
    Bool,
    /// An integer (milliunit fixed-point included).
    Int,
}

/// A clock field an expression may read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockField {
    /// The current day (1-based).
    Day,
    /// Days left before the clock's day limit.
    DaysRemaining,
    /// The current slot index (0-based).
    Slot,
    /// The current weekday (1-based in the authored cycle).
    Weekday,
}

/// A comparison operator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CmpOp {
    /// Equal.
    Eq,
    /// Greater than or equal.
    Ge,
    /// Greater than.
    Gt,
    /// Less than or equal.
    Le,
    /// Less than.
    Lt,
    /// Not equal.
    Ne,
}

/// An expression tree: data, not code. Total by construction —
/// no division, no modulo, no loops, no recursion, no strings,
/// no RNG. Arithmetic saturates; comparisons and logic are
/// exact. Randomness enters a design only through tables on
/// named streams, never through expressions.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    /// Integer addition (saturating).
    Add(Box<Expr>, Box<Expr>),
    /// Conjunction of one or more booleans.
    And(Vec<Expr>),
    /// A boolean literal.
    BoolLit(bool),
    /// Clamp an integer between two integer bounds.
    Clamp(Box<Expr>, Box<Expr>, Box<Expr>),
    /// A clock field read.
    Clock(ClockField),
    /// A comparison.
    Cmp(CmpOp, Box<Expr>, Box<Expr>),
    /// A flag read.
    Flag(String),
    /// An integer literal.
    Lit(i64),
    /// The larger of two integers.
    Max(Box<Expr>, Box<Expr>),
    /// The smaller of two integers.
    Min(Box<Expr>, Box<Expr>),
    /// Integer multiplication (saturating).
    Mul(Box<Expr>, Box<Expr>),
    /// Integer negation (saturating).
    Neg(Box<Expr>),
    /// Boolean negation.
    Not(Box<Expr>),
    /// Disjunction of one or more booleans.
    Or(Vec<Expr>),
    /// A stat read; `character` is required exactly when the
    /// stat is character-scoped.
    Stat {
        /// The character instance, for character-scoped stats.
        character: Option<String>,
        /// The stat id.
        stat: String,
    },
    /// Integer subtraction (saturating).
    Sub(Box<Expr>, Box<Expr>),
    /// The tier band index of a stat's current value.
    TierOf {
        /// The character instance, for character-scoped stats.
        character: Option<String>,
        /// The stat id.
        stat: String,
    },
    /// A named value read.
    Value(String),
}

/// One effect set: atoms applied as one transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectSetDef {
    /// Stable id.
    pub id: String,
    /// The atoms, in application order.
    pub atoms: Vec<Atom>,
}

/// One effect atom. The atom set is closed: an unknown atom kind
/// is refused at define time (`TG-AUTH-UNKNOWN-KIND`).
#[derive(Clone, Debug, PartialEq)]
pub enum Atom {
    /// Emit an engine event (the seam engine results re-enter
    /// through; `on_event` triggers see it in the same tick's
    /// re-evaluation pass).
    EmitEvent {
        /// The event name.
        event: String,
    },
    /// Move to a scene (applied at the tick's transition step;
    /// the last `enter_scene` of a tick wins).
    EnterScene {
        /// The target scene.
        scene: String,
    },
    /// Clear a flag.
    FlagClear {
        /// The flag id.
        flag: String,
    },
    /// Set a flag.
    FlagSet {
        /// The flag id.
        flag: String,
    },
    /// Draw on a weighted table (one step of the table's named
    /// stream) and apply the drawn outcome's effect set inside
    /// the same transaction.
    RollTable {
        /// The table id.
        table: String,
    },
    /// Add an evaluated amount to a stat.
    StatAdd {
        /// The character instance, for character-scoped stats.
        character: Option<String>,
        /// The stat id.
        stat: String,
        /// The amount expression (integer).
        amount: Box<Expr>,
    },
    /// Set a stat to an evaluated value.
    StatSet {
        /// The character instance, for character-scoped stats.
        character: Option<String>,
        /// The stat id.
        stat: String,
        /// The value expression (integer).
        value: Box<Expr>,
    },
}

/// One character.
#[derive(Clone, Debug, PartialEq)]
pub struct CharacterDef {
    /// Stable id.
    pub id: String,
    /// Content id of the display name (`content.ron` holds the
    /// prose; the authored prose travels in the op inputs).
    pub name_ref: String,
    /// Genesis overrides for character-scoped stats.
    pub initials: Vec<StatInitial>,
}

/// One genesis stat override for a character.
#[derive(Clone, Debug, PartialEq)]
pub struct StatInitial {
    /// The stat id.
    pub stat: String,
    /// The genesis value.
    pub value: i64,
}

/// Scene presentation classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Presentation {
    /// An event scene (focus card; e.g. a phone screen).
    Event,
    /// A room scene (background plus sprites).
    Room,
    /// A visual-novel scene (background, sprites, dialogue box).
    Vn,
}

/// One scene. The C-8/D-548 growth fields (`participants`,
/// `backdrop`, `entry_condition`, `on_enter`, `on_exit`) are
/// absent-means-absent: a scene authored before the growth
/// carries none of them and keeps its bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDef {
    /// Stable id.
    pub id: String,
    /// The presentation class.
    pub presentation: Presentation,
    /// The characters present in the scene (character ids,
    /// sorted). `scene_bind_rig` keys on (scene, participant)
    /// against this list.
    pub participants: Vec<String>,
    /// The backdrop: an intaken asset id or a rig scene id
    /// (Amendment 2 §Item G4); resolved at validation where
    /// the naming collection is known, forward here.
    pub backdrop: Option<String>,
    /// A condition id gating entry to the scene.
    pub entry_condition: Option<String>,
    /// An effect set applied when the scene is entered (the
    /// interpreter applies it at the transition step).
    pub on_enter: Option<String>,
    /// An effect set applied when the scene is left (applied
    /// at the transition step, before the entry effects).
    pub on_exit: Option<String>,
}

/// One dialogue line. The record carries a text *reference*
/// only; the prose lives in `content.ron` under that reference
/// (D-468), so an editorial prose edit never churns this record
/// or the project hash.
#[derive(Clone, Debug, PartialEq)]
pub struct DialogueLine {
    /// Stable id.
    pub id: String,
    /// The scene the line belongs to.
    pub scene: String,
    /// Position in the scene's chain (0-based, gapless).
    pub ordinal: u32,
    /// The speaking character id, or `narrator`.
    pub speaker: String,
    /// The content id of the line's prose in `content.ron`.
    pub text_ref: String,
    /// The next line in the chain, when the chain does not end
    /// here. (Choice and exit targets join this pointer when
    /// their verbs land.)
    pub next: Option<String>,
    /// A rig expression the line asks the stage to show; the id
    /// resolves against the project's rig expressions at
    /// validation time.
    pub hook_expression: Option<String>,
}

/// When a threshold or flag trigger edges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    /// The value/flag goes from below/off to at-or-above/on.
    Rising,
    /// The value/flag goes from at-or-above/on to below/off.
    Falling,
}

/// A trigger's firing pattern/kind.
#[derive(Clone, Debug, PartialEq)]
pub enum TriggerKind {
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
        edge: FlagEdge,
    },
    /// Fires when a stat crosses a threshold.
    OnThreshold {
        /// The character instance, for character-scoped stats.
        character: Option<String>,
        /// The stat id.
        stat: String,
        /// The threshold value.
        threshold: i64,
        /// Which crossing fires it.
        direction: Direction,
    },
    /// Fires on clock positions matching the pattern: every
    /// present field must match (an absent field matches all).
    OnTick {
        /// Slot ids that match (absent: every slot).
        slots: Option<Vec<String>>,
        /// Weekdays that match (absent: every weekday).
        weekdays: Option<Vec<u32>>,
        /// `(period, remainder)`: matches when
        /// `day % period == remainder`.
        day_modulo: Option<(u32, u32)>,
        /// One exact day that matches.
        exact_day: Option<i64>,
    },
}

/// Which edge of a flag an `on_flag` trigger fires on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FlagEdge {
    /// Either edge.
    Both,
    /// Off to on.
    Rising,
    /// On to off.
    Falling,
}

/// Whether a trigger may fire again.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Repetition {
    /// Fires at most once per run.
    Once,
    /// May fire whenever its pattern matches again (subject to
    /// cooldown).
    Repeat,
}

/// The offered/deferred lifecycle a trigger may carry (D-464).
/// When a trigger with an offer matches, it does not fire: it
/// becomes *offered*. The run's next offer input accepts it
/// (its effect set applies) or declines it (it is deferred —
/// suppressed for the rest of the day, re-offered on the next
/// matching day). An offer may also expire: its age advances on
/// the same day rollover that drives stat lifetimes (one clock),
/// and at `expires_days` the offer is withdrawn unfired.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OfferDef {
    /// Days an offer may stay open before it expires; absent
    /// means the offer does not expire.
    pub expires_days: Option<u32>,
}

/// One trigger.
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerDef {
    /// Stable id.
    pub id: String,
    /// When it fires.
    pub kind: TriggerKind,
    /// An optional named gate condition.
    pub gate: Option<String>,
    /// The effect set applied on firing (or on offer acceptance).
    pub effect_set: String,
    /// Firing order key: higher fires first; ties break by id.
    pub priority: i64,
    /// Once or repeat.
    pub repetition: Repetition,
    /// Ticks the trigger sits out after firing (0: none).
    pub cooldown_ticks: u32,
    /// The offered lifecycle, when the trigger offers instead of
    /// firing outright.
    pub offer: Option<OfferDef>,
}

/// The value kind an appearance slot carries: a text token
/// (a part name, an expression id) or an integer list
/// (transforms and per-slot vectors, in fixed point where the
/// source data is fractional — floats never enter the format).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppearanceKind {
    /// An integer list (transforms, vectors).
    Ints,
    /// A text token (a part or expression name).
    Text,
}

/// One declared appearance slot of a character's layered
/// doll: an id and the kind of value it carries.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceSlot {
    /// Stable id.
    pub id: String,
    /// The kind of value the slot carries.
    pub kind: AppearanceKind,
}

/// One appearance assignment: a slot and its value.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceEntry {
    /// The slot being assigned.
    pub slot: String,
    /// The assigned value.
    pub value: AppearanceValue,
}

/// An appearance value. Text tokens name parts and
/// expressions; integer lists carry transforms and vectors.
#[derive(Clone, Debug, PartialEq)]
pub enum AppearanceValue {
    /// An integer list (fixed point where fractional).
    Ints(Vec<i64>),
    /// A text token naming a part or expression.
    Text(String),
}

/// A character's appearance table (P2-6): the declared slots
/// of its layered doll and the baseline ("Default") value of
/// every slot. The base is total by construction — every
/// declared slot has a baseline — so an empty delta resolves
/// to exactly this.
#[derive(Clone, Debug, PartialEq)]
pub struct AppearanceDef {
    /// The baseline value of every declared slot, sorted by
    /// slot id.
    pub base: Vec<AppearanceEntry>,
    /// The character this table belongs to.
    pub character: String,
    /// The declared slots, sorted by id.
    pub slots: Vec<AppearanceSlot>,
}

/// One progression state's sparse appearance delta (P2-6):
/// the slots this state overrides on the character's baseline
/// doll. The delta is sparse by design, and an EMPTY delta is
/// a first-class value — a state that changes no visual
/// property at all (the Modification App's MindControl path)
/// is a row with no entries, not a missing row.
#[derive(Clone, Debug, PartialEq)]
pub struct VisualStateDef {
    /// The character whose appearance this state overrides.
    pub character: String,
    /// The sparse overrides, sorted by slot id. Empty is a
    /// first-class value.
    pub delta: Vec<AppearanceEntry>,
    /// The progression state's name.
    pub state: String,
}

/// The one RNG algorithm this build implements: SplitMix64,
/// seeded per stream from the run seed and the stream id by a
/// fixed derivation (see `game::simulate`). The algorithm is
/// part of the project's format record — a stream names it, and
/// an unknown algorithm refuses at authoring time — so a future
/// algorithm change is a project migration, not a silent drift.
pub const RNG_ALGORITHM: &str = "estate-rng/1";

/// One named randomness stream. Draws on a stream are the only
/// randomness in the family; a draw consumes exactly one step
/// of the stream regardless of the weights it is drawn against.
#[derive(Clone, Debug, PartialEq)]
pub struct RngStreamDef {
    /// Stable id.
    pub id: String,
    /// The algorithm name (see [`RNG_ALGORITHM`]).
    pub algorithm: String,
}

/// One outcome's weight in a weighted table: a positive static
/// integer, or a bounded integer expression evaluated at draw
/// time against the drawing state (D-469, adopting AUTH-D10).
#[derive(Clone, Debug, PartialEq)]
pub enum TableWeight {
    /// An expression tree (integer-typed at authoring time);
    /// a draw-time value of zero or less makes the outcome
    /// ineligible for that draw.
    Expr(Expr),
    /// A positive integer literal.
    Static(i64),
}

/// One outcome of a weighted table.
#[derive(Clone, Debug, PartialEq)]
pub struct TableOutcome {
    /// Stable id (unique within the table).
    pub id: String,
    /// The outcome's weight.
    pub weight: TableWeight,
    /// An optional named gate condition; a false gate makes the
    /// outcome ineligible for the draw.
    pub gate: Option<String>,
    /// The effect set applied when the outcome is drawn.
    pub effect_set: String,
}

/// A weighted outcome table on a named RNG stream.
#[derive(Clone, Debug, PartialEq)]
pub struct TableDef {
    /// Stable id.
    pub id: String,
    /// The RNG stream draws consume.
    pub stream: String,
    /// The outcomes, in authored order (draws walk them in this
    /// order against cumulative weights).
    pub outcomes: Vec<TableOutcome>,
}

/// Where a manual save may be taken (Amendment 2 §G3.3). This
/// is the save-schema family's policy surface: the AUTH-G
/// branch also carried a `save_policy` on its `game_define`
/// inputs, and D-549 folds that surface in here — the schema's
/// policy is the one home, and `game_define` carries no second
/// copy to disagree with it.
#[derive(Clone, Debug, PartialEq)]
pub enum SaveContextPolicy {
    /// At any tick boundary (the first-class default posture).
    AnyTickBoundary,
    /// Only at scene boundaries.
    SceneBoundary,
    /// Only inside the listed safe scenes (sorted).
    SafeScenesOnly(Vec<String>),
}

/// Whether a save slot holds one run or a chapter list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotHolds {
    /// A chapter list per slot.
    ChapterList,
    /// One run per slot.
    OneRun,
}

/// One named migration step in a save schema's chain: it
/// migrates saves of version `from` to `from + 1`. Applying a
/// step at load time is acceptance under the step's name: the
/// step carries no field mapping (the branch's `Migration` is
/// `{from, name}`), so the state document carries over
/// unchanged and the step's effect is the version advance
/// itself, recorded by name in the loaded save's audit trail.
#[derive(Clone, Debug, PartialEq)]
pub struct Migration {
    /// The version this step migrates from.
    pub from: u32,
    /// The migration's name (its id).
    pub name: String,
}

/// The authored save schema (`game_save_schema_define`,
/// Amendment 2 §G3.3): saving and loading are runtime acts;
/// what a save must contain is authored data.
#[derive(Clone, Debug, PartialEq)]
pub struct SaveSchema {
    /// Schema version: strictly increases on every change.
    pub version: u32,
    /// Slot count, 1..=64.
    pub slots: u32,
    /// Whether a slot holds one run or a chapter list.
    pub slot_holds: SlotHolds,
    /// State fields in the per-slot partition, sorted.
    pub slot_fields: Vec<String>,
    /// State fields in the globals partition, sorted.
    pub globals_fields: Vec<String>,
    /// The named hash algorithm version saves carry.
    pub hash_algorithm: String,
    /// Where a manual save may be taken.
    pub policy: SaveContextPolicy,
    /// The migration chain segment: named steps, sorted by
    /// `from`, contiguous, ending at `version - 1`. A
    /// redefinition's segment starts at the prior schema's
    /// version (the branch's rule), so the chain a load walks
    /// is exactly the steps the current schema carries.
    pub migrations: Vec<Migration>,
}

/// The canonical game-state field set a save schema partitions
/// (Amendment 2 §G3.3): every field belongs to exactly one
/// partition, so the schema sweep is an exact cover of this
/// list.
pub const SAVE_STATE_FIELDS: &[&str] = &[
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

/// Who a choice option belongs to (Amendment 2 §G2.4): the
/// alter-ego origin is what the game's veto rule
/// (`Game::alter_ego_veto`) acts on when the player declines
/// it by picking another option.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChoiceOrigin {
    /// An alter-ego option.
    AlterEgo,
    /// A player option.
    Player,
}

/// Where a choice option leads (Amendment 2 §G2.4, reconciled
/// with the trunk's chain pointer under C-8/D-548): a `Line`
/// target joins the scene's dialogue chain (the target line
/// must belong to the choice's scene, as a `next` pointer
/// must stay in its scene); an `Exit` target resolves through
/// the scene's authored exit record to its target scene; a
/// `Scene` target names that scene directly. Both scene
/// targets drive the interpreter's one transition mechanism
/// (`pending_scene`, step 5) — no parallel exit machinery.
#[derive(Clone, Debug, PartialEq)]
pub enum ChoiceTarget {
    /// The ending evaluation point (no id: the check itself).
    EndingCheck,
    /// A scene exit, by its id within the choice's scene.
    Exit(String),
    /// A dialogue line, by id (in the choice's scene).
    Line(String),
    /// A scene, by id (a direct transition target).
    Scene(String),
}

/// One option of a choice (Amendment 2 §G2.4). Authored order
/// is the ordinal the runtime's sort falls back to.
#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceOption {
    /// Option id, unique within the choice.
    pub id: String,
    /// Content id of the label text: a `content.ron` sidecar
    /// key (C-7/D-547), refused at decide time when absent.
    pub label_ref: String,
    /// Condition id gating visibility.
    pub visible_if: Option<String>,
    /// Condition id gating enablement: visible-but-disabled
    /// is a distinct, authored state.
    pub enabled_if: Option<String>,
    /// Integer sort expression: the runtime orders presented
    /// options by (sort value, authored ordinal).
    pub sort: Option<Expr>,
    /// Who the option belongs to.
    pub origin: ChoiceOrigin,
    /// Effect set applied when the option is taken (absent:
    /// no effects).
    pub effect_set: Option<String>,
    /// Where the option leads.
    pub target: ChoiceTarget,
}

/// A branch point (`choice_define`, Amendment 2 §G2.4).
#[derive(Clone, Debug, PartialEq)]
pub struct ChoiceDef {
    /// Choice id.
    pub id: String,
    /// The scene the choice belongs to.
    pub scene: String,
    /// The prompt's content id (a `content.ron` sidecar key,
    /// C-7/D-547), when the choice has one.
    pub prompt_line: Option<String>,
    /// The options, in authored order.
    pub options: Vec<ChoiceOption>,
}

/// An ending rule (`ending_define`, Amendment 2 §G2.6):
/// evaluation is a runtime act; the rule is data. Priorities
/// are unique across endings and evaluate in ascending order
/// (the lowest priority number is checked first), so the
/// runtime never coin-flips an ending.
#[derive(Clone, Debug, PartialEq)]
pub struct EndingDef {
    /// Ending id.
    pub id: String,
    /// The condition id that must hold.
    pub condition: String,
    /// Evaluation priority: unique across endings.
    pub priority: i64,
    /// The epilogue scene id.
    pub epilogue_scene: String,
}

/// How a gallery entry unlocks (Amendment 2 §G2.6).
#[derive(Clone, Debug, PartialEq)]
pub enum GalleryUnlock {
    /// A condition id under which the entry auto-unlocks.
    Condition(String),
    /// An effect atom names this entry (the unlock is an
    /// effect, not a condition).
    Effect,
}

/// A replay registration (`gallery_entry_define`, Amendment 2
/// §G2.6).
#[derive(Clone, Debug, PartialEq)]
pub struct GalleryEntryDef {
    /// Entry id.
    pub id: String,
    /// The gallery category the entry files under.
    pub category: String,
    /// How the entry unlocks.
    pub unlock: GalleryUnlock,
    /// The replay target scene id.
    pub replay_scene: String,
    /// The replay target entry line id.
    pub replay_line: String,
    /// Thumbnail asset id; resolves against the project's
    /// intaken assets at define time.
    pub thumbnail: Option<String>,
}

/// One slot → scene row of a schedule.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleSlot {
    /// The clock slot name.
    pub slot: String,
    /// The scene id the character is in.
    pub scene: String,
}

/// One ordered condition override of a schedule: when the
/// condition holds, the character is in this scene instead of
/// the slot's.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleOverride {
    /// The condition id.
    pub condition: String,
    /// The scene id.
    pub scene: String,
}

/// A character's recurring placement (`schedule_define`,
/// Amendment 2 §G2.4): the slot map covers every clock slot —
/// the runtime must never guess a location.
#[derive(Clone, Debug, PartialEq)]
pub struct ScheduleDef {
    /// The character id.
    pub character: String,
    /// Slot → scene rows, sorted by slot name (the map is the
    /// meaning; row order is not).
    pub slots: Vec<ScheduleSlot>,
    /// Ordered condition overrides: first match wins.
    pub overrides: Vec<ScheduleOverride>,
}

/// A scene participant's rig binding (`scene_bind_rig`,
/// Amendment 2 §G2.4): kept apart from the scene record so art
/// can be rebound without re-authoring the scene's logic.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneBindingDef {
    /// The scene id.
    pub scene: String,
    /// The participant (character) id: a member of the scene's
    /// `participants` (C-8/D-548).
    pub participant: String,
    /// The rig model id (a forward reference: the model
    /// registry of §Item G4 is not authored in this crate).
    pub rig_model: String,
    /// The entry expression id: resolves against the project's
    /// rig expressions at define time.
    pub entry_expression: String,
    /// The entry motion id, when the binding enters moving:
    /// resolves against the project's motions at define time.
    pub entry_motion: Option<String>,
}

/// One exit from a scene (`scene_exit_define`, Amendment 2
/// §G2.4): exits are separate records so graphs grow
/// incrementally and the reachability sweep has named edges.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneExitDef {
    /// The scene the exit leaves.
    pub scene: String,
    /// Exit id, unique within the scene.
    pub id: String,
    /// The target scene id.
    pub target: String,
    /// Condition id gating the exit.
    pub condition: Option<String>,
    /// Effect set applied on traversal.
    pub effect_set: Option<String>,
}

/// One prose entry of `content.ron`, keyed by content id. Entries
/// are sidecar state: they ride in the loaded project so the
/// gate and the interpreter can see them, but they are never
/// serialized into `project.ron`.
#[derive(Clone, Debug, PartialEq)]
pub struct ContentEntry {
    /// The content id (a dialogue `text_ref` or a character
    /// `name_ref`).
    pub id: String,
    /// The prose, in the sidecar's locale.
    pub text: String,
}
