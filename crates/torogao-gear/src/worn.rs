//! The worn set: which gear is on the body, in what state,
//! and the session op log its transitions append to (§§2.6,
//! 3.2, 4.5).
//!
//! Physics never silently deletes an item: a displacement is
//! a latched state with an op-log event and its cause, and
//! only a user operation (re-seat, remove, remove-all) moves
//! an item out of a state physics put it in.

use std::collections::BTreeMap;

use crate::document::{GearClass, GearDocument, StateVariant};

/// One op-log event: a worn-set or safety-act transition.
/// Events render as canonical one-line records so a session's
/// history is diffable and hashable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpEvent {
    /// The §3.4 act completed and was appended.
    RemoveAllCompleted {
        /// Tick of the act.
        tick: u64,
    },
    /// The engine was reset to baseline (§3.4 step 4).
    EngineReset {
        /// Tick of the reset.
        tick: u64,
    },
    /// An item was applied to the body.
    ItemApplied {
        /// Tick of the application.
        tick: u64,
        /// Item id.
        id: String,
    },
    /// An item latched into `displaced` (§4.5).
    ItemDisplaced {
        /// Tick of the transition.
        tick: u64,
        /// Item id.
        id: String,
        /// Anchor whose attachment displaced.
        anchor: String,
        /// Cause: `drift-threshold` or `contact-event`.
        cause: String,
    },
    /// An item was removed from the body.
    ItemRemoved {
        /// Tick of the removal.
        tick: u64,
        /// Item id.
        id: String,
    },
    /// A displaced item was re-seated by a user operation.
    ItemReseated {
        /// Tick of the re-seat.
        tick: u64,
        /// Item id.
        id: String,
    },
    /// An ache mirror stop was issued (§3.4 step 2).
    MirrorStopIssued {
        /// Tick of the stop.
        tick: u64,
    },
    /// One side of a paired item was removed (§2.3 B).
    SideRemoved {
        /// Tick of the removal.
        tick: u64,
        /// Item id.
        id: String,
        /// Anchor of the removed side.
        anchor: String,
    },
    /// Local stimulation was latched to zero (§3.4 step 1).
    StimulationLatchedZero {
        /// Tick of the latch.
        tick: u64,
    },
}

impl OpEvent {
    /// The event's tick.
    pub fn tick(&self) -> u64 {
        match self {
            Self::EngineReset { tick }
            | Self::ItemApplied { tick, .. }
            | Self::ItemDisplaced { tick, .. }
            | Self::ItemRemoved { tick, .. }
            | Self::ItemReseated { tick, .. }
            | Self::MirrorStopIssued { tick }
            | Self::RemoveAllCompleted { tick }
            | Self::SideRemoved { tick, .. }
            | Self::StimulationLatchedZero { tick } => *tick,
        }
    }

    /// The canonical one-line record of this event.
    pub fn canonical(&self) -> String {
        match self {
            Self::EngineReset { tick } => format!("t{tick} engine-reset"),
            Self::ItemApplied { tick, id } => format!("t{tick} applied {id}"),
            Self::ItemDisplaced {
                tick,
                id,
                anchor,
                cause,
            } => {
                format!("t{tick} displaced {id} @{anchor} cause={cause}")
            }
            Self::ItemRemoved { tick, id } => format!("t{tick} removed {id}"),
            Self::ItemReseated { tick, id } => format!("t{tick} reseated {id}"),
            Self::MirrorStopIssued { tick } => format!("t{tick} mirror-stop"),
            Self::RemoveAllCompleted { tick } => format!("t{tick} remove-all-completed"),
            Self::SideRemoved { tick, id, anchor } => {
                format!("t{tick} side-removed {id} @{anchor}")
            }
            Self::StimulationLatchedZero { tick } => {
                format!("t{tick} stimulation-latched-zero")
            }
        }
    }
}

/// One attachment's runtime state inside a worn entry.
#[derive(Clone, Debug, PartialEq)]
pub struct AttachmentState {
    /// The anchor this state belongs to.
    pub anchor: String,
    /// The attachment's current variant.
    pub variant: StateVariant,
    /// Latest contact quality from the §4.3 model (0.0 until
    /// the first solve).
    pub contact_quality: f64,
}

/// One worn item: its document and per-attachment states.
/// A paired item keeps both sides separately, so a one-sided
/// displacement is representable (§2.3 B).
#[derive(Clone, Debug, PartialEq)]
pub struct WornEntry {
    /// The validated document worn.
    pub document: GearDocument,
    /// Per-attachment states, in document anchor order.
    pub attachments: Vec<AttachmentState>,
}

impl WornEntry {
    fn new(document: GearDocument) -> Self {
        let worn_variant = match document.class {
            GearClass::Apparel => StateVariant::Worn,
            GearClass::Effect => StateVariant::Active,
            _ => StateVariant::Attached,
        };
        let attachments = document
            .anchors
            .iter()
            .map(|a| AttachmentState {
                anchor: a.anchor.clone(),
                variant: worn_variant,
                contact_quality: 0.0,
            })
            .collect();
        Self {
            document,
            attachments,
        }
    }

    /// Whether every attachment is still on the body in its
    /// worn state (none displaced, none removed).
    pub fn fully_seated(&self) -> bool {
        self.attachments
            .iter()
            .all(|a| !matches!(a.variant, StateVariant::Displaced | StateVariant::Removed))
    }
}

/// The worn set plus its op log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WornSet {
    entries: BTreeMap<String, WornEntry>,
    log: Vec<OpEvent>,
}

impl WornSet {
    /// An empty worn set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an event to the op log.
    pub fn record(&mut self, event: OpEvent) {
        self.log.push(event);
    }

    /// The op log, in order.
    pub fn log(&self) -> &[OpEvent] {
        &self.log
    }

    /// The op log as canonical text lines.
    pub fn log_lines(&self) -> Vec<String> {
        self.log.iter().map(|e| e.canonical()).collect()
    }

    /// Applies a validated item. Applying an already-worn item
    /// replaces its entry (a fresh seating) and logs again.
    pub fn apply(&mut self, document: GearDocument, tick: u64) {
        let id = document.id.clone();
        self.entries.insert(id.clone(), WornEntry::new(document));
        self.record(OpEvent::ItemApplied { tick, id });
    }

    /// Removes one item entirely. Returns whether it was worn.
    pub fn remove(&mut self, id: &str, tick: u64) -> bool {
        if self.entries.remove(id).is_some() {
            self.record(OpEvent::ItemRemoved {
                tick,
                id: id.to_string(),
            });
            true
        } else {
            false
        }
    }

    /// Removes one side of a paired item (§2.3 B): that
    /// attachment's variant becomes `Removed`; the item stays
    /// worn on its remaining sides.
    pub fn remove_side(&mut self, id: &str, anchor: &str, tick: u64) -> bool {
        let Some(entry) = self.entries.get_mut(id) else {
            return false;
        };
        let Some(state) = entry.attachments.iter_mut().find(|a| a.anchor == anchor) else {
            return false;
        };
        state.variant = StateVariant::Removed;
        state.contact_quality = 0.0;
        self.record(OpEvent::SideRemoved {
            tick,
            id: id.to_string(),
            anchor: anchor.to_string(),
        });
        true
    }

    /// Latches one attachment into `displaced` (§4.5), with
    /// its cause. The transition is an event; it happens at
    /// most once per seating.
    pub fn mark_displaced(&mut self, id: &str, anchor: &str, cause: &str, tick: u64) -> bool {
        let Some(entry) = self.entries.get_mut(id) else {
            return false;
        };
        let Some(state) = entry.attachments.iter_mut().find(|a| a.anchor == anchor) else {
            return false;
        };
        if state.variant == StateVariant::Displaced || state.variant == StateVariant::Removed {
            return false;
        }
        state.variant = StateVariant::Displaced;
        self.record(OpEvent::ItemDisplaced {
            tick,
            id: id.to_string(),
            anchor: anchor.to_string(),
            cause: cause.to_string(),
        });
        true
    }

    /// Re-seats every displaced attachment of an item (a user
    /// operation; the solve never does this silently, §4.5).
    pub fn reseat(&mut self, id: &str, tick: u64) -> bool {
        let Some(entry) = self.entries.get_mut(id) else {
            return false;
        };
        let mut changed = false;
        let worn_variant = match entry.document.class {
            GearClass::Apparel => StateVariant::Worn,
            GearClass::Effect => StateVariant::Active,
            _ => StateVariant::Attached,
        };
        for state in &mut entry.attachments {
            if state.variant == StateVariant::Displaced {
                state.variant = worn_variant;
                changed = true;
            }
        }
        if changed {
            self.record(OpEvent::ItemReseated {
                tick,
                id: id.to_string(),
            });
        }
        changed
    }

    /// Removes every worn item (all classes, including
    /// fixtures), logging each removal in id order. This is
    /// §3.4 step 3; the surfaces drive the full act.
    pub fn remove_all_items(&mut self, tick: u64) -> Vec<String> {
        let ids: Vec<String> = self.entries.keys().cloned().collect();
        for id in &ids {
            self.entries.remove(id);
            self.record(OpEvent::ItemRemoved {
                tick,
                id: id.clone(),
            });
        }
        ids
    }

    /// The current arousal floor from worn gear (§5.4):
    /// the MAXIMUM floor over fully-seated items — floors
    /// take max, never sum. A displaced or partially removed
    /// item contributes no floor until re-seated.
    pub fn arousal_floor_permille(&self) -> i64 {
        let mut floor = 0;
        for entry in self.entries.values() {
            if entry.fully_seated()
                && let Some(item_floor) = entry.document.arousal_floor_permille()
            {
                floor = floor.max(item_floor);
            }
        }
        floor
    }

    /// Looks up a worn entry by item id.
    pub fn get(&self, id: &str) -> Option<&WornEntry> {
        self.entries.get(id)
    }

    /// Worn item ids, sorted.
    pub fn ids(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// Number of worn items.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is worn.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
