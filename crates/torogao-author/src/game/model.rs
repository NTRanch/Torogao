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
    pub save_schema_version: i64,
    /// Scene the game starts in (a forward reference at define
    /// time; the validation sweep requires it to exist).
    pub start_scene: Option<String>,
    /// The authored clock, once defined.
    pub clock: Option<Clock>,
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
}

impl Game {
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

    /// The tier bands over this stat, if authored.
    pub fn tier(&self, stat: &str) -> Option<&TierDef> {
        find(&self.tiers, stat, |item| &item.stat)
    }

    /// The trigger with this id.
    pub fn trigger(&self, id: &str) -> Option<&TriggerDef> {
        find(&self.triggers, id, |item| &item.id)
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

/// One scene.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneDef {
    /// Stable id.
    pub id: String,
    /// The presentation class.
    pub presentation: Presentation,
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
