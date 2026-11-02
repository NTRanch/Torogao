//! Appearance resolution (P2-6): a character's appearance is
//! never edited directly; it is *resolved* — the baseline
//! doll plus the current progression state's sparse delta.
//! Resolution is a pure function of the authored records: the
//! baseline's value for every slot the delta does not name,
//! the delta's value for every slot it does. An empty delta
//! resolves to exactly the baseline — the Modification App's
//! MindControl path made visible in the data: the row exists,
//! and it changes nothing.

use crate::game::model::{AppearanceValue, Game};
use crate::refusal::{Refusal, RefusalClass, refuse};

/// Resolves a character's appearance: the baseline values in
/// slot-declaration order, overridden by the named state's
/// delta entries. `state` of `None` is the baseline itself.
/// A character with no appearance table, or a state the table
/// does not carry, refuses typed — a missing row is an
/// authoring error, never a silent baseline.
pub fn resolve_appearance(
    game: &Game,
    character: &str,
    state: Option<&str>,
) -> Result<Vec<(String, AppearanceValue)>, Refusal> {
    let Some(appearance) = game.appearance(character) else {
        return refuse(
            RefusalClass::UnknownTarget,
            format!("character '{character}' has no appearance table"),
        );
    };
    let delta: &[crate::game::model::AppearanceEntry] = match state {
        None => &[],
        Some(name) => match game.visual_state(character, name) {
            Some(row) => &row.delta,
            None => {
                return refuse(
                    RefusalClass::UnknownTarget,
                    format!("character '{character}' has no visual state '{name}'"),
                );
            }
        },
    };
    let mut resolved: Vec<(String, AppearanceValue)> = Vec::with_capacity(appearance.slots.len());
    for slot in &appearance.slots {
        let value = delta
            .iter()
            .find(|entry| entry.slot == slot.id)
            .or_else(|| appearance.base.iter().find(|entry| entry.slot == slot.id))
            .map(|entry| entry.value.clone())
            .expect("decide() keeps the baseline total over the declared slots");
        resolved.push((slot.id.clone(), value));
    }
    Ok(resolved)
}
