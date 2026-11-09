//! The day/session state machine over the measured tables:
//! the phone-hub menu resolver, the day clock, node play, the
//! TP economy, and quick travel.
//!
//! Modeled simplifications, stated plainly (the spec is the
//! record of record): the store unlock is evaluated at node
//! completion rather than at the original's next room
//! transition; a failed minigame changes nothing (the original
//! plays a `failed` scene whose clock effects are branch data,
//! not a systems constant); teleport applies the goal node's
//! state without marking it seen, mirroring
//! Teleporter.Finish's direct field writes.

use std::collections::{BTreeMap, BTreeSet};

use crate::dynamics::{Dynamics, Item, ModappError, ResolvedState};
use crate::world;

/// One menu entry after gating: the node, whether it can be
/// started now, and the issue blocking it if not.
#[derive(Clone, Debug, PartialEq)]
pub struct MenuOption {
    pub node_id: String,
    pub enabled: bool,
    /// True when the entry's starting slot differs from the
    /// current slot (the source's INCORRECT_TIME issue).
    pub wrong_time: bool,
    /// True when the node is flag-gated (the source's
    /// "sub-path" sort key).
    pub sub_path: bool,
}

/// The hub menu for the current node, or the reason the menu
/// cannot be shown at all.
#[derive(Clone, Debug, PartialEq)]
pub enum Menu {
    /// The subject is not in the player's room; the source
    /// replaces the menu with a not-found notice.
    SubjectAbsent {
        subject_room: u32,
    },
    /// The current node has no children (end of content).
    EndOfContent,
    Options(Vec<MenuOption>),
}

/// The result of playing a node to completion (or failing
/// its minigame).
#[derive(Clone, Debug, PartialEq)]
pub struct PlayOutcome {
    pub completed: bool,
    /// TP actually granted by the minigame win (0 while the
    /// store is locked or on teleport-class runs).
    pub reward_granted: i64,
    /// TP granted by the first-view event bonus.
    pub new_event_bonus: i64,
    /// TP granted by the store-unlock catch-up, when this
    /// completion triggered the unlock.
    pub unlock_catchup: i64,
    pub state: Option<ResolvedState>,
    pub day: u32,
    pub time: u8,
}

/// The result of a quick-travel trip.
#[derive(Clone, Debug, PartialEq)]
pub struct TeleportOutcome {
    pub route: Vec<String>,
    pub price: u32,
    pub state: ResolvedState,
}

/// A running game. All state transitions validate first and
/// commit after, so a refused step leaves the game unchanged.
#[derive(Clone, Debug)]
pub struct Game {
    dynamics: Dynamics,
    day: u32,
    time: u8,
    points: i64,
    current_node: String,
    player_room: u32,
    seen_nodes: BTreeSet<String>,
    seen_events: BTreeSet<String>,
    inventory: BTreeMap<String, u32>,
}

impl Game {
    /// A new game just after the intro: day 0, slot 0
    /// (Morning), at the Default node with Default seen, in
    /// the Kitchen (the slot-0 subject room, so the first
    /// menu can open).
    pub fn new(dynamics: Dynamics) -> Game {
        let start_room = dynamics.schedule().first().copied().unwrap_or(0);
        let mut seen_nodes = BTreeSet::new();
        seen_nodes.insert("Default".to_string());
        Game {
            dynamics,
            day: 0,
            time: 0,
            points: 0,
            current_node: "Default".to_string(),
            player_room: start_room,
            seen_nodes,
            seen_events: BTreeSet::new(),
            inventory: BTreeMap::new(),
        }
    }

    pub fn day(&self) -> u32 {
        self.day
    }

    pub fn time(&self) -> u8 {
        self.time
    }

    pub fn points(&self) -> i64 {
        self.points
    }

    pub fn current_node(&self) -> &str {
        &self.current_node
    }

    pub fn player_room(&self) -> u32 {
        self.player_room
    }

    pub fn seen_nodes(&self) -> &BTreeSet<String> {
        &self.seen_nodes
    }

    pub fn seen_events(&self) -> &BTreeSet<String> {
        &self.seen_events
    }

    pub fn inventory_count(&self, item_id: &str) -> u32 {
        self.inventory.get(item_id).copied().unwrap_or(0)
    }

    /// The subject's room at the current slot, from the
    /// measured schedule.
    pub fn subject_room(&self) -> u32 {
        self.dynamics.schedule()[usize::from(self.time)]
    }

    /// Whether the subject is in the player's room (the menu
    /// presence gate).
    pub fn subject_present(&self) -> bool {
        self.subject_room() == self.player_room
    }

    /// Whether the store unlock event has been seen; points
    /// cannot move before it (the AddPoints gate).
    pub fn store_unlocked(&self) -> bool {
        match self.dynamics.flag_event("STORE") {
            Ok(event) => self.seen_events.contains(event),
            Err(_) => false,
        }
    }

    /// Move the player to a room (bounded to known room ids).
    pub fn set_room(&mut self, room: u32) -> Result<(), ModappError> {
        if room > crate::dynamics::ROOM_ID_MAX {
            return Err(ModappError::InvalidStep {
                reason: "room id outside 0..=7",
            });
        }
        self.player_room = room;
        Ok(())
    }

    /// Advance the clock one slot (GameOrchestration.AddTime):
    /// slot + 1, wrapping past the last slot into the next
    /// day's slot 0. This is also the phone's Wait button.
    pub fn wait(&mut self) {
        self.time += 1;
        if self.time >= self.dynamics.slots_per_day() {
            self.time = 0;
            self.day += 1;
        }
    }

    /// The hub menu for the current node, gated exactly as
    /// Phone.GetAvailableEvents gates it: flag-unmet children
    /// are excluded entirely; time-mismatched children are
    /// shown disabled; entries sort by issue count, then
    /// non-flag-gated before flag-gated.
    pub fn menu(&self) -> Result<Menu, ModappError> {
        // Gate order as in Phone._on_tf_app_pressed: end of
        // content is reported before the presence check.
        let current = self.dynamics.node(&self.current_node)?;
        if current.children.is_empty() {
            return Ok(Menu::EndOfContent);
        }
        if !self.subject_present() {
            return Ok(Menu::SubjectAbsent {
                subject_room: self.subject_room(),
            });
        }
        let mut options = Vec::new();
        for child_id in &current.children {
            let child = self.dynamics.node(child_id)?;
            if let Some(flag) = &child.req_flag {
                let event = self.dynamics.flag_event(flag)?;
                if !self.seen_events.contains(event) {
                    continue;
                }
            }
            let wrong_time = matches!(child.time_gate, Some(gate) if gate != self.time);
            options.push(MenuOption {
                node_id: child.id.clone(),
                enabled: !wrong_time,
                wrong_time,
                sub_path: child.req_flag.is_some(),
            });
        }
        options.sort_by_key(|o| (o.wrong_time, o.sub_path));
        Ok(Menu::Options(options))
    }

    /// Move points through the measured gate: while the store
    /// unlock event is unseen, AddPoints is a no-op returning
    /// 0. Returns the amount actually applied.
    pub fn add_points(&mut self, qty: i64) -> Result<i64, ModappError> {
        if !self.store_unlocked() {
            return Ok(0);
        }
        self.points = self
            .points
            .checked_add(qty)
            .ok_or(ModappError::InvalidStep {
                reason: "points overflow",
            })?;
        Ok(qty)
    }

    /// Play a child node: validate the menu gates, run the
    /// (caller-resolved) minigame outcome, and on success
    /// apply the measured completion sequence — win reward,
    /// first-view bonus, state application, the node's
    /// `<id>-enter` event, the roll to next-day slot 0, and
    /// the store-unlock check. `performance_ratio` is the
    /// class-specific ratio of the spec's reward formulas,
    /// in [0, 1].
    pub fn play_node(
        &mut self,
        node_id: &str,
        minigame_success: bool,
        performance_ratio: f64,
    ) -> Result<PlayOutcome, ModappError> {
        if !performance_ratio.is_finite() {
            return Err(ModappError::InvalidStep {
                reason: "performance ratio must be finite",
            });
        }
        let node = self.dynamics.node(node_id)?.clone();
        let current = self.dynamics.node(&self.current_node)?;
        if !current.children.iter().any(|c| c == node_id) {
            return Err(ModappError::NotAChild {
                node: node_id.to_string(),
                current: self.current_node.clone(),
            });
        }
        if let Some(flag) = &node.req_flag {
            let event = self.dynamics.flag_event(flag)?;
            if !self.seen_events.contains(event) {
                return Err(ModappError::FlagNotMet { flag: flag.clone() });
            }
        }
        if let Some(gate) = node.time_gate {
            if gate != self.time {
                return Err(ModappError::WrongTime {
                    node: node_id.to_string(),
                    required: gate,
                    actual: self.time,
                });
            }
        }
        if !self.subject_present() {
            return Err(ModappError::SubjectAbsent {
                subject_room: self.subject_room(),
                player_room: self.player_room,
            });
        }
        if !minigame_success {
            return Ok(PlayOutcome {
                completed: false,
                reward_granted: 0,
                new_event_bonus: 0,
                unlock_catchup: 0,
                state: None,
                day: self.day,
                time: self.time,
            });
        }
        let ratio = performance_ratio.clamp(0.0, 1.0);
        let mut reward_granted = 0_i64;
        if let Some(class) = &node.minigame_class {
            if let Ok(reward) = self.dynamics.reward(class) {
                let amount = i64::from(reward.base) + (f64::from(reward.span) * ratio) as i64;
                reward_granted = self.add_points(amount)?;
            }
        }
        let first_view = !self.seen_nodes.contains(node_id);
        let mut new_event_bonus = 0_i64;
        if first_view {
            new_event_bonus = self.add_points(i64::from(self.dynamics.new_event_points()))?;
            self.seen_nodes.insert(node_id.to_string());
        }
        self.seen_events.insert(format!("{node_id}-enter"));
        self.current_node = node_id.to_string();
        let state = world::resolve_state(&self.dynamics, node_id)?;
        self.time = 0;
        self.day += 1;
        let unlock_catchup = self.maybe_unlock_store()?;
        Ok(PlayOutcome {
            completed: true,
            reward_granted,
            new_event_bonus,
            unlock_catchup,
            state: Some(state),
            day: self.day,
            time: self.time,
        })
    }

    /// The store-unlock check (TIGHTLY_STORE): once the
    /// measured count of "level >= digit node-enter" events
    /// is seen, the STORE event lands and the retroactive
    /// catch-up (seen nodes * rate) is granted. Returns the
    /// catch-up granted, 0 when no unlock happened.
    pub fn maybe_unlock_store(&mut self) -> Result<i64, ModappError> {
        if self.store_unlocked() {
            return Ok(0);
        }
        let min_digit = char::from(b'0' + self.dynamics.store_unlock_min_level_digit());
        let high_events = self
            .seen_events
            .iter()
            .filter(|e| {
                e.strip_suffix("-enter")
                    .and_then(|stem| stem.chars().last())
                    .is_some_and(|c| c >= min_digit && c <= '9')
            })
            .count();
        if high_events < self.dynamics.store_unlock_min_high_events() as usize {
            return Ok(0);
        }
        let event = self.dynamics.flag_event("STORE")?.to_string();
        self.seen_events.insert(event);
        let catchup =
            i64::from(self.dynamics.store_catchup_per_seen_node()) * self.seen_nodes.len() as i64;
        let granted = self.add_points(catchup)?;
        Ok(granted)
    }

    /// Buy a store item (Inventory.CanBuy + Store.Buy):
    /// refuses unknown items, a locked store, unique items
    /// already owned, and insufficient balance; charges the
    /// price and records the item.
    pub fn buy(&mut self, item_id: &str) -> Result<Item, ModappError> {
        let item = self.dynamics.item(item_id)?.clone();
        if !self.store_unlocked() {
            return Err(ModappError::StoreLocked);
        }
        if item.unique && self.inventory_count(item_id) > 0 {
            return Err(ModappError::AlreadyOwned {
                id: item_id.to_string(),
            });
        }
        if self.points < i64::from(item.price) {
            return Err(ModappError::InsufficientPoints {
                needed: item.price,
                have: self.points,
            });
        }
        self.add_points(-i64::from(item.price))?;
        *self.inventory.entry(item_id.to_string()).or_insert(0) += 1;
        Ok(item)
    }

    /// The quick-travel price to a node: per-node rate times
    /// route length. Affordability is the only charge-side
    /// behavior the 0.12.3 sources implement (spec REVIEW
    /// #2), so this is also the teleport gate.
    pub fn teleport_price(&self, target: &str) -> Result<u32, ModappError> {
        let route = world::route(&self.dynamics, &self.current_node, target)?;
        Ok(self.dynamics.teleport_price_per_node() * route.len() as u32)
    }

    /// Quick travel to a node (PathsMap + Teleporter):
    /// requires the path-tp item and an affordable route;
    /// applies the goal node's state directly, without
    /// marking the node seen or saving its event (the
    /// source's Finish writes the fields directly).
    pub fn teleport(&mut self, target: &str) -> Result<TeleportOutcome, ModappError> {
        if self.inventory_count("path-tp") == 0 {
            return Err(ModappError::TeleportItemMissing);
        }
        let route = world::route(&self.dynamics, &self.current_node, target)?;
        let price = self.dynamics.teleport_price_per_node() * route.len() as u32;
        if self.points < i64::from(price) {
            return Err(ModappError::InsufficientPoints {
                needed: price,
                have: self.points,
            });
        }
        let state = world::resolve_state(&self.dynamics, target)?;
        self.current_node = target.to_string();
        Ok(TeleportOutcome {
            route,
            price,
            state,
        })
    }

    /// The resolved state table row for the current node.
    pub fn current_state(&self) -> Result<ResolvedState, ModappError> {
        world::resolve_state(&self.dynamics, &self.current_node)
    }
}
