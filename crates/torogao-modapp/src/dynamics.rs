//! The measured tables for the Modification App systems, as data.
//!
//! The tables ship as RON (`data/modapp-dynamics.ron`, schema
//! `torogao-modapp-dynamics/1`) following the estate's RON
//! contract discipline: the file is the contract, this module
//! validates it at load, and any value outside the measured
//! shape is a typed refusal — never a silent default. The
//! human record of record is `games/modification-app-dynamics.md`
//! in the Lane D programme tree; every value here is transcribed
//! from it (and through it, from the decompiled 0.12.3 sources).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

/// The embedded measured tables (see the file header for
/// provenance and the spec sections behind every value).
pub const DYNAMICS_RON: &str = include_str!("../data/modapp-dynamics.ron");

/// Bounds on the measured tables. The source counts are 39
/// nodes, 38 state rows, 4 items, 10 reward rows, 47 tuning
/// rows; the bounds exist so a corrupt file cannot allocate
/// unbounded tables, not as claims about the source.
pub const NODE_COUNT_MAX: usize = 128;
/// A node has at most 3 children in the source graph.
pub const CHILDREN_COUNT_MAX: usize = 8;
/// State rows: the Default row plus one per path node.
pub const STATE_COUNT_MAX: usize = 128;
/// Store items in the source table.
pub const ITEM_COUNT_MAX: usize = 64;
/// Minigame tuning rows across all classes.
pub const TUNING_ROW_COUNT_MAX: usize = 256;
/// Named flags in Constants.Flags that the systems consult.
pub const FLAG_COUNT_MAX: usize = 32;
/// Room ids run 0..=7 in Constants.RoomIds.
pub const ROOM_ID_MAX: u32 = 7;
/// A single item price bound (source maximum: 600 TP).
pub const ITEM_PRICE_MAX: u32 = 100_000;
/// The minigame classes the loader accepts. A class outside
/// this set is UNKNOWN and refused, never extrapolated.
pub const KNOWN_MINIGAME_CLASSES: [&str; 11] = [
    "Setup",
    "AE",
    "BE",
    "Bimbo",
    "Goth",
    "Growth",
    "Milk",
    "MindControl",
    "Pet",
    "Shrink",
    "Vampire",
];

/// One node of the story graph: its menu gates and its
/// minigame binding, exactly as tabled in StoryFlow.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    pub path: String,
    /// The single day-slot at which the node may start, if gated.
    pub time_gate: Option<u8>,
    /// Name of a flag (see `Dynamics::flag_event`) that must be
    /// seen before this node appears in the menu, if any.
    pub req_flag: Option<String>,
    pub minigame_class: Option<String>,
    pub minigame_level: u32,
    pub children: Vec<String>,
    /// File-wide AddTime calls across all dialogue branches.
    pub branch_add_time_total: u32,
}

/// A face placement: position, rotation, scale (MiaStates
/// FACE_POSITION records).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FacePosition {
    pub position: (f64, f64),
    pub rotation: f64,
    pub scale: f64,
}

/// One row of the state table: a delta over the Default row.
/// `None` keys inherit Default at resolve time.
#[derive(Clone, Debug, PartialEq)]
pub struct StateDelta {
    pub id: String,
    pub body: Option<String>,
    pub face: Option<String>,
    pub extras: Option<String>,
    pub position: Option<(f64, f64)>,
    /// Uniform scale (the source stores [s, s]).
    pub scale: Option<f64>,
    pub face_position: Option<FacePosition>,
}

/// A fully merged state: the Default row with one node's
/// delta applied (DialogueAction.GetMiaState semantics).
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedState {
    pub body: String,
    pub face: String,
    pub extras: String,
    pub position: (f64, f64),
    pub scale: f64,
    pub face_position: FacePosition,
    /// Subject room per time slot, from the Default row.
    pub schedule: Vec<u32>,
}

/// One store item: price and rule flags. Names and
/// descriptions are original text and are not carried.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    pub price: u32,
    pub unique: bool,
    pub consumable: bool,
    pub use_immediately: bool,
    pub gift: bool,
}

/// A minigame win reward: `base + span * ratio`, ratio in
/// [0, 1] supplied per class semantics (spec section 7).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reward {
    pub base: u32,
    pub span: u32,
}

/// One console-game payout row.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsolePayout {
    pub game: String,
    pub tier: String,
    pub payout: u32,
    pub setup: Vec<f64>,
}

/// One minigame tuning row: the values its setup assigns at
/// one level, as measured key/value pairs. Level 0 is the
/// class defaults row.
#[derive(Clone, Debug, PartialEq)]
pub struct TuningRow {
    pub class: String,
    pub level: u32,
    pub teleport: bool,
    pub params: BTreeMap<String, f64>,
}

/// Failure modes of loading, validating, or running the
/// systems. Invalid data is an error, never an assertion: the
/// RON file is an external input.
#[derive(Debug)]
pub enum ModappError {
    /// The RON text did not parse as the dynamics schema.
    Parse(ron::de::SpannedError),
    /// A parsed value fell outside the measured shape.
    Invalid {
        field: &'static str,
        reason: &'static str,
    },
    /// A node id the table does not carry.
    UnknownNode { id: String },
    /// A flag name the table does not carry.
    UnknownFlag { name: String },
    /// An item id the table does not carry.
    UnknownItem { id: String },
    /// A minigame class the table does not carry.
    UnknownMinigame { class: String },
    /// A node that is not a child of the current node.
    NotAChild { node: String, current: String },
    /// A node's required flag event has not been seen.
    FlagNotMet { flag: String },
    /// A node attempted outside its gated day-slot.
    WrongTime {
        node: String,
        required: u8,
        actual: u8,
    },
    /// The subject is not in the player's current room.
    SubjectAbsent { subject_room: u32, player_room: u32 },
    /// A purchase or trip the balance cannot cover.
    InsufficientPoints { needed: u32, have: i64 },
    /// A unique item bought twice.
    AlreadyOwned { id: String },
    /// Points moved while the store unlock event is unseen.
    StoreLocked,
    /// Quick travel attempted without the path-tp item.
    TeleportItemMissing,
    /// A graph target no route from Default reaches.
    UnreachableNode { id: String },
    /// A step input (ratio, room, clock) broke the contract.
    InvalidStep { reason: &'static str },
}

impl std::fmt::Display for ModappError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModappError::Parse(e) => write!(f, "dynamics RON parse failed: {e}"),
            ModappError::Invalid { field, reason } => {
                write!(f, "invalid dynamics field {field}: {reason}")
            }
            ModappError::UnknownNode { id } => write!(f, "unknown node id {id}"),
            ModappError::UnknownFlag { name } => write!(f, "unknown flag name {name}"),
            ModappError::UnknownItem { id } => write!(f, "unknown item id {id}"),
            ModappError::UnknownMinigame { class } => {
                write!(f, "unknown minigame class {class}")
            }
            ModappError::NotAChild { node, current } => {
                write!(f, "node {node} is not a child of current node {current}")
            }
            ModappError::FlagNotMet { flag } => {
                write!(f, "required flag {flag} has not been seen")
            }
            ModappError::WrongTime {
                node,
                required,
                actual,
            } => write!(
                f,
                "node {node} requires time slot {required}, current slot is {actual}"
            ),
            ModappError::SubjectAbsent {
                subject_room,
                player_room,
            } => write!(
                f,
                "subject is in room {subject_room}, player is in room {player_room}"
            ),
            ModappError::InsufficientPoints { needed, have } => {
                write!(f, "need {needed} TP, have {have} TP")
            }
            ModappError::AlreadyOwned { id } => write!(f, "unique item {id} already owned"),
            ModappError::StoreLocked => {
                write!(f, "points cannot move before the store unlock event")
            }
            ModappError::TeleportItemMissing => {
                write!(f, "quick travel requires the path-tp item")
            }
            ModappError::UnreachableNode { id } => {
                write!(f, "no route from Default reaches node {id}")
            }
            ModappError::InvalidStep { reason } => write!(f, "invalid step input: {reason}"),
        }
    }
}

impl std::error::Error for ModappError {}

/// The raw deserialized shape of `torogao-modapp-dynamics/1`.
#[derive(Debug, Deserialize)]
struct DynamicsWire {
    slots_per_day: u8,
    teleport_difficulty: u32,
    teleport_price_per_node: u32,
    new_event_points: u32,
    store_unlock_min_high_events: u32,
    store_unlock_min_level_digit: u8,
    store_catchup_per_seen_node: u32,
    cafe_unlock_points: u32,
    schedule: Vec<u32>,
    flags: BTreeMap<String, String>,
    nodes: Vec<NodeWire>,
    states: Vec<StateWire>,
    items: Vec<ItemWire>,
    rewards: Vec<RewardWire>,
    console_payouts: Vec<ConsolePayoutWire>,
    tuning: Vec<TuningWire>,
}

#[derive(Debug, Deserialize)]
struct NodeWire {
    id: String,
    path: String,
    time_gate: Option<u8>,
    req_flag: Option<String>,
    minigame_class: Option<String>,
    minigame_level: u32,
    children: Vec<String>,
    branch_add_time_total: u32,
}

#[derive(Debug, Deserialize)]
struct FacePositionWire {
    position: (f64, f64),
    rotation: f64,
    scale: f64,
}

#[derive(Debug, Deserialize)]
struct StateWire {
    id: String,
    body: Option<String>,
    face: Option<String>,
    extras: Option<String>,
    position: Option<(f64, f64)>,
    scale: Option<f64>,
    face_position: Option<FacePositionWire>,
}

#[derive(Debug, Deserialize)]
struct ItemWire {
    id: String,
    price: u32,
    unique: bool,
    consumable: bool,
    use_immediately: bool,
    gift: bool,
}

#[derive(Debug, Deserialize)]
struct RewardWire {
    class: String,
    base: u32,
    span: u32,
}

#[derive(Debug, Deserialize)]
struct ConsolePayoutWire {
    game: String,
    tier: String,
    payout: u32,
    setup: Vec<f64>,
}

#[derive(Debug, Deserialize)]
struct TuningWire {
    class: String,
    level: u32,
    teleport: bool,
    params: BTreeMap<String, f64>,
}

/// The validated dynamics tables. Construct only through
/// `Dynamics::from_ron` or `measured_dynamics`, so every
/// instance satisfies the measured shape.
#[derive(Clone, Debug)]
pub struct Dynamics {
    slots_per_day: u8,
    teleport_difficulty: u32,
    teleport_price_per_node: u32,
    new_event_points: u32,
    store_unlock_min_high_events: u32,
    store_unlock_min_level_digit: u8,
    store_catchup_per_seen_node: u32,
    cafe_unlock_points: u32,
    schedule: Vec<u32>,
    flags: BTreeMap<String, String>,
    nodes: Vec<Node>,
    node_index: BTreeMap<String, usize>,
    states: Vec<StateDelta>,
    state_index: BTreeMap<String, usize>,
    items: Vec<Item>,
    rewards: BTreeMap<String, Reward>,
    console_payouts: Vec<ConsolePayout>,
    tuning: Vec<TuningRow>,
}

impl Dynamics {
    /// Parse and validate a dynamics table from RON text.
    pub fn from_ron(text: &str) -> Result<Dynamics, ModappError> {
        let wire: DynamicsWire = ron::from_str(text).map_err(ModappError::Parse)?;
        let mut node_index = BTreeMap::new();
        for (i, n) in wire.nodes.iter().enumerate() {
            node_index.insert(n.id.clone(), i);
        }
        let mut state_index = BTreeMap::new();
        for (i, s) in wire.states.iter().enumerate() {
            state_index.insert(s.id.clone(), i);
        }
        let d = Dynamics {
            slots_per_day: wire.slots_per_day,
            teleport_difficulty: wire.teleport_difficulty,
            teleport_price_per_node: wire.teleport_price_per_node,
            new_event_points: wire.new_event_points,
            store_unlock_min_high_events: wire.store_unlock_min_high_events,
            store_unlock_min_level_digit: wire.store_unlock_min_level_digit,
            store_catchup_per_seen_node: wire.store_catchup_per_seen_node,
            cafe_unlock_points: wire.cafe_unlock_points,
            schedule: wire.schedule,
            flags: wire.flags,
            nodes: wire
                .nodes
                .into_iter()
                .map(|n| Node {
                    id: n.id,
                    path: n.path,
                    time_gate: n.time_gate,
                    req_flag: n.req_flag,
                    minigame_class: n.minigame_class,
                    minigame_level: n.minigame_level,
                    children: n.children,
                    branch_add_time_total: n.branch_add_time_total,
                })
                .collect(),
            node_index,
            states: wire
                .states
                .into_iter()
                .map(|s| StateDelta {
                    id: s.id,
                    body: s.body,
                    face: s.face,
                    extras: s.extras,
                    position: s.position,
                    scale: s.scale,
                    face_position: s.face_position.map(|fp| FacePosition {
                        position: fp.position,
                        rotation: fp.rotation,
                        scale: fp.scale,
                    }),
                })
                .collect(),
            state_index,
            items: wire
                .items
                .into_iter()
                .map(|i| Item {
                    id: i.id,
                    price: i.price,
                    unique: i.unique,
                    consumable: i.consumable,
                    use_immediately: i.use_immediately,
                    gift: i.gift,
                })
                .collect(),
            rewards: wire
                .rewards
                .into_iter()
                .map(|r| {
                    (
                        r.class,
                        Reward {
                            base: r.base,
                            span: r.span,
                        },
                    )
                })
                .collect(),
            console_payouts: wire
                .console_payouts
                .into_iter()
                .map(|c| ConsolePayout {
                    game: c.game,
                    tier: c.tier,
                    payout: c.payout,
                    setup: c.setup,
                })
                .collect(),
            tuning: wire
                .tuning
                .into_iter()
                .map(|t| TuningRow {
                    class: t.class,
                    level: t.level,
                    teleport: t.teleport,
                    params: t.params,
                })
                .collect(),
        };
        d.validate()?;
        Ok(d)
    }

    fn invalid(field: &'static str, reason: &'static str) -> ModappError {
        ModappError::Invalid { field, reason }
    }

    fn validate(&self) -> Result<(), ModappError> {
        if self.slots_per_day == 0 || self.slots_per_day > 8 {
            return Err(Self::invalid("slots_per_day", "outside 1..=8"));
        }
        if self.schedule.len() != usize::from(self.slots_per_day) {
            return Err(Self::invalid(
                "schedule",
                "must carry one room per time slot",
            ));
        }
        for room in &self.schedule {
            if *room > ROOM_ID_MAX {
                return Err(Self::invalid("schedule", "room id outside 0..=7"));
            }
        }
        if self.teleport_difficulty == 0 {
            return Err(Self::invalid("teleport_difficulty", "must be positive"));
        }
        if self.flags.is_empty() || self.flags.len() > FLAG_COUNT_MAX {
            return Err(Self::invalid("flags", "count outside 1..=32"));
        }
        for event in self.flags.values() {
            if event.is_empty() {
                return Err(Self::invalid("flags", "event strings must be non-empty"));
            }
        }
        // The store gate consults this flag by name; a table
        // without it cannot run the measured economy.
        if !self.flags.contains_key("STORE") {
            return Err(Self::invalid("flags", "the STORE flag is required"));
        }
        if self.nodes.is_empty() || self.nodes.len() > NODE_COUNT_MAX {
            return Err(Self::invalid("nodes", "count outside 1..=128"));
        }
        let mut ids = BTreeSet::new();
        for n in &self.nodes {
            if !ids.insert(n.id.as_str()) {
                return Err(Self::invalid("nodes", "duplicate node id"));
            }
            if n.id.is_empty() {
                return Err(Self::invalid("nodes", "node ids must be non-empty"));
            }
            if n.children.len() > CHILDREN_COUNT_MAX {
                return Err(Self::invalid("nodes", "child count above 8"));
            }
            for child in &n.children {
                if !self.node_index.contains_key(child) {
                    return Err(Self::invalid(
                        "nodes",
                        "child reference is not a known node",
                    ));
                }
            }
            if let Some(gate) = n.time_gate {
                if gate >= self.slots_per_day {
                    return Err(Self::invalid("nodes", "time gate is not a day slot"));
                }
            }
            if let Some(flag) = &n.req_flag {
                if !self.flags.contains_key(flag) {
                    return Err(Self::invalid("nodes", "required flag is not a known flag"));
                }
            }
            if let Some(class) = &n.minigame_class {
                if !KNOWN_MINIGAME_CLASSES.contains(&class.as_str()) {
                    return Err(Self::invalid(
                        "nodes",
                        "minigame class is not a known class",
                    ));
                }
            }
        }
        if !self.node_index.contains_key("INTRO") || !self.node_index.contains_key("Default") {
            return Err(Self::invalid(
                "nodes",
                "the graph must carry INTRO and Default",
            ));
        }
        if self.states.is_empty() || self.states.len() > STATE_COUNT_MAX {
            return Err(Self::invalid("states", "count outside 1..=128"));
        }
        let mut state_ids = BTreeSet::new();
        for s in &self.states {
            if !state_ids.insert(s.id.as_str()) {
                return Err(Self::invalid("states", "duplicate state id"));
            }
            if s.id != "Default" && !self.node_index.contains_key(&s.id) {
                return Err(Self::invalid("states", "state row is not a known node"));
            }
            if let Some(scale) = s.scale {
                if !scale.is_finite() || scale <= 0.0 {
                    return Err(Self::invalid("states", "scale must be finite and positive"));
                }
            }
            if let Some((x, y)) = s.position {
                if !x.is_finite() || !y.is_finite() {
                    return Err(Self::invalid("states", "position must be finite"));
                }
            }
            if let Some(fp) = &s.face_position {
                if !fp.position.0.is_finite()
                    || !fp.position.1.is_finite()
                    || !fp.rotation.is_finite()
                    || !fp.scale.is_finite()
                    || fp.scale <= 0.0
                {
                    return Err(Self::invalid(
                        "states",
                        "face position must be finite with positive scale",
                    ));
                }
            }
        }
        // The Default row is the merge base: it must be complete.
        let default = self
            .state_index
            .get("Default")
            .map(|i| &self.states[*i])
            .ok_or(Self::invalid("states", "the Default row is required"))?;
        if default.body.is_none()
            || default.face.is_none()
            || default.extras.is_none()
            || default.position.is_none()
            || default.scale.is_none()
            || default.face_position.is_none()
        {
            return Err(Self::invalid(
                "states",
                "the Default row must specify every key",
            ));
        }
        if self.items.is_empty() || self.items.len() > ITEM_COUNT_MAX {
            return Err(Self::invalid("items", "count outside 1..=64"));
        }
        let mut item_ids = BTreeSet::new();
        for item in &self.items {
            if !item_ids.insert(item.id.as_str()) {
                return Err(Self::invalid("items", "duplicate item id"));
            }
            if item.price > ITEM_PRICE_MAX {
                return Err(Self::invalid("items", "price above bound"));
            }
        }
        for class in self.rewards.keys() {
            if !KNOWN_MINIGAME_CLASSES.contains(&class.as_str()) {
                return Err(Self::invalid("rewards", "reward row is not a known class"));
            }
        }
        if self.tuning.len() > TUNING_ROW_COUNT_MAX {
            return Err(Self::invalid("tuning", "row count above 256"));
        }
        let mut tuning_keys = BTreeSet::new();
        for row in &self.tuning {
            if !KNOWN_MINIGAME_CLASSES.contains(&row.class.as_str()) {
                return Err(Self::invalid("tuning", "tuning row is not a known class"));
            }
            if !tuning_keys.insert((row.class.as_str(), row.level, row.teleport)) {
                return Err(Self::invalid("tuning", "duplicate tuning row"));
            }
            if row.params.is_empty() {
                return Err(Self::invalid("tuning", "tuning row carries no params"));
            }
            for value in row.params.values() {
                if !value.is_finite() {
                    return Err(Self::invalid("tuning", "tuning values must be finite"));
                }
            }
        }
        Ok(())
    }

    pub fn slots_per_day(&self) -> u8 {
        self.slots_per_day
    }

    pub fn teleport_difficulty(&self) -> u32 {
        self.teleport_difficulty
    }

    pub fn teleport_price_per_node(&self) -> u32 {
        self.teleport_price_per_node
    }

    pub fn new_event_points(&self) -> u32 {
        self.new_event_points
    }

    pub fn store_unlock_min_high_events(&self) -> u32 {
        self.store_unlock_min_high_events
    }

    pub fn store_unlock_min_level_digit(&self) -> u8 {
        self.store_unlock_min_level_digit
    }

    pub fn store_catchup_per_seen_node(&self) -> u32 {
        self.store_catchup_per_seen_node
    }

    pub fn cafe_unlock_points(&self) -> u32 {
        self.cafe_unlock_points
    }

    /// Subject room per time slot (index = time slot).
    pub fn schedule(&self) -> &[u32] {
        &self.schedule
    }

    /// The SeenEvents string satisfying a named flag, or a
    /// typed refusal for a flag the table does not carry.
    pub fn flag_event(&self, name: &str) -> Result<&str, ModappError> {
        self.flags
            .get(name)
            .map(String::as_str)
            .ok_or_else(|| ModappError::UnknownFlag {
                name: name.to_string(),
            })
    }

    /// A graph node by id, or a typed refusal.
    pub fn node(&self, id: &str) -> Result<&Node, ModappError> {
        self.node_index
            .get(id)
            .map(|i| &self.nodes[*i])
            .ok_or_else(|| ModappError::UnknownNode { id: id.to_string() })
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn nodes(&self) -> &[Node] {
        &self.nodes
    }

    /// The raw state-table delta for a node id, or a typed
    /// refusal. `Default` resolves to the complete base row.
    pub fn state_delta(&self, id: &str) -> Result<&StateDelta, ModappError> {
        self.state_index
            .get(id)
            .map(|i| &self.states[*i])
            .ok_or_else(|| ModappError::UnknownNode { id: id.to_string() })
    }

    /// A store item by id, or a typed refusal.
    pub fn item(&self, id: &str) -> Result<&Item, ModappError> {
        self.items
            .iter()
            .find(|i| i.id == id)
            .ok_or_else(|| ModappError::UnknownItem { id: id.to_string() })
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// The win reward for a minigame class, or a typed
    /// refusal. Classes with no measured reward (Setup) are
    /// refused, not zeroed.
    pub fn reward(&self, class: &str) -> Result<Reward, ModappError> {
        if !KNOWN_MINIGAME_CLASSES.contains(&class) {
            return Err(ModappError::UnknownMinigame {
                class: class.to_string(),
            });
        }
        self.rewards
            .get(class)
            .copied()
            .ok_or_else(|| ModappError::UnknownMinigame {
                class: class.to_string(),
            })
    }

    pub fn console_payouts(&self) -> &[ConsolePayout] {
        &self.console_payouts
    }

    /// The tuning params for a class at a level: the class's
    /// level-0 defaults row merged under the exact row. A
    /// class/level the table does not carry is a typed
    /// refusal, never an extrapolation.
    pub fn tuning(
        &self,
        class: &str,
        level: u32,
        teleport: bool,
    ) -> Result<BTreeMap<String, f64>, ModappError> {
        if !KNOWN_MINIGAME_CLASSES.contains(&class) {
            return Err(ModappError::UnknownMinigame {
                class: class.to_string(),
            });
        }
        let mut merged = BTreeMap::new();
        let mut found = false;
        for row in &self.tuning {
            if row.class == class && row.level == 0 && row.teleport == teleport {
                // The teleport variants carry no separate
                // defaults row in the source; a teleport row at
                // level 0 IS the variant's full assignment set
                // over the normal defaults.
                if teleport {
                    if let Some(base) = self.tuning_row(class, 0, false) {
                        merged.extend(base.params.iter().map(|(k, v)| (k.clone(), *v)));
                    }
                }
                merged.extend(row.params.iter().map(|(k, v)| (k.clone(), *v)));
                found = true;
            }
        }
        if level != 0 {
            match self.tuning_row(class, level, teleport) {
                Some(row) => {
                    merged.extend(row.params.iter().map(|(k, v)| (k.clone(), *v)));
                    found = true;
                }
                None => {
                    return Err(ModappError::UnknownMinigame {
                        class: format!("{class} level {level}"),
                    });
                }
            }
        }
        if !found {
            return Err(ModappError::UnknownMinigame {
                class: class.to_string(),
            });
        }
        Ok(merged)
    }

    fn tuning_row(&self, class: &str, level: u32, teleport: bool) -> Option<&TuningRow> {
        self.tuning
            .iter()
            .find(|r| r.class == class && r.level == level && r.teleport == teleport)
    }
}

/// Load the embedded measured tables (data/modapp-dynamics.ron).
pub fn measured_dynamics() -> Result<Dynamics, ModappError> {
    Dynamics::from_ron(DYNAMICS_RON)
}
