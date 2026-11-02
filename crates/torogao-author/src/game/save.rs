//! Saves: the §G3.3 snapshot document, its load checks, and
//! the format migration chain.
//!
//! A save is a canonical RON document binding: the game format
//! version it was written under, the authored save-schema
//! version, the project hash it belongs to, the run seed, the
//! full state (slot partition plus the globals partition), and
//! the state hash. Load is a pipeline of typed refusals — parse,
//! project binding (`TG-AUTH-CONFLICT`), schema currency
//! (`TG-GAME-SAVE-SCHEMA`), stepwise migration, state
//! verification against the project's definitions, and the
//! state-hash recompute. A failed load mutates nothing: it
//! produces no state at all.
//!
//! The format migration chain currently has one step, v1 → v2
//! (the P2-2/P2-3 tick-contract bump): v2 added optional state
//! fields (lifetime counters, offer state), so a v1 state reads
//! with those fields absent — counters disarmed, no offers
//! pending — which the state parser already encodes. The step
//! is a named function so the rule is authored, inspectable,
//! and tested, not an accident of the parser.
//!
//! The save-SCHEMA version is a separate axis (C-4/D-544): the
//! version a save binds to is derived — the authored save
//! schema's version when a schema exists, else the stored
//! `save_schema_version` scalar (the pre-schema default). A
//! save older than the derived version walks the authored
//! schema's migration chain stepwise: each step is a named
//! acceptance (a `Migration` is `{from, name}` — it carries no
//! field mapping, so the state document carries over unchanged
//! and the step's effect is the version advance, recorded by
//! name in `SavedGame::migrations_applied`). A missing step —
//! including a save older than the chain segment the current
//! schema carries, or any older save when no schema exists —
//! refuses `TG-GAME-SAVE-SCHEMA`: there is no silent upgrade.

use crate::codec::{open, record, text};
use crate::game::model::{GAME_FORMAT_VERSION, GAME_FORMAT_VERSION_FIRST, GAME_HASH_ALGO, Game};
use crate::game::state::{GameState, state_from_ron, state_hash, state_to_ron};
use crate::model::Project;
use crate::refusal::{Refusal, RefusalClass, refuse};
use crate::ron::{Ron, emit_document, parse_document};

fn schema_refusal<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::GameSaveSchema, message)
}

/// A loaded save: the verified state and its run seed.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedGame {
    /// The run seed recorded in the save.
    pub run_seed: i64,
    /// The verified runtime state.
    pub state: GameState,
    /// The migration steps applied to bring the save's schema
    /// version up to the project's derived version, by name,
    /// in order (empty when the save was already current).
    pub migrations_applied: Vec<String>,
}

/// Serializes a save for `state` under `project`'s authored
/// schema. Saving mid-tick is refused: the transient fields
/// must be empty, as they are at every tick boundary.
pub fn save_game(
    project: &Project,
    project_sha256: &str,
    state: &GameState,
    run_seed: i64,
) -> Result<String, Refusal> {
    let game = crate::game::decide::section(project)?;
    if state.pending_scene.is_some() || !state.emitted.is_empty() {
        return refuse(
            RefusalClass::Malformed,
            "cannot save mid-tick: transient tick state is not empty",
        );
    }
    let doc = record(
        "Save",
        vec![
            ("format_version", Ron::Int(GAME_FORMAT_VERSION)),
            ("hash_algo", text(GAME_HASH_ALGO)),
            ("project_sha256", text(project_sha256)),
            ("run_seed", Ron::Int(run_seed)),
            (
                "schema_version",
                Ron::Int(game.derived_save_schema_version()),
            ),
            ("state", state_to_ron(state)),
            ("state_hash", text(&state_hash(state))),
        ],
    );
    Ok(emit_document(&doc))
}

/// The v1 → v2 migration step: a v1 state document, parsed with
/// its v2-only fields absent, IS the migrated state — lifetime
/// counters disarmed (no v1 run could arm one: the primitive
/// did not exist), no offers pending (the lifecycle did not
/// exist), deferrals none. Anything else in a v1 state carries
/// over unchanged.
pub fn migrate_state_v1_to_v2(state: GameState) -> GameState {
    state
}

/// Loads and verifies a save against the project it claims to
/// belong to.
pub fn load_game(
    project: &Project,
    project_sha256: &str,
    text: &str,
) -> Result<SavedGame, Refusal> {
    let game = crate::game::decide::section(project)?;
    let value = parse_document(text)?;
    let mut fields = open(value, "Save")?;
    let format_version = fields.int("format_version")?;
    let hash_algo = fields.string("hash_algo")?;
    let saved_project = fields.string("project_sha256")?;
    let run_seed = fields.int("run_seed")?;
    let schema_version = fields.int("schema_version")?;
    let state_ron = fields.take("state")?;
    let recorded_hash = fields.string("state_hash")?;
    fields.done()?;
    if hash_algo != GAME_HASH_ALGO {
        return schema_refusal(format!(
            "save hash algorithm '{hash_algo}' is not '{GAME_HASH_ALGO}'"
        ));
    }
    if saved_project != project_sha256 {
        return refuse(
            RefusalClass::Conflict,
            format!(
                "the save belongs to project {saved_project}, not {project_sha256}; saves do not cross projects"
            ),
        );
    }
    let derived = game.derived_save_schema_version();
    if schema_version > derived {
        return schema_refusal(format!(
            "save schema_version {schema_version} is newer than the project's authored {derived}"
        ));
    }
    // An older save walks the authored migration chain
    // stepwise (see the module header): every step from the
    // save's version up to the derived version must be named
    // in the current schema's chain.
    let mut migrations_applied: Vec<String> = Vec::new();
    if schema_version < derived {
        let Some(schema) = &game.save_schema else {
            return schema_refusal(format!(
                "save schema_version {schema_version} predates the project's {derived}, and the project authors no save schema to migrate it"
            ));
        };
        let mut current = schema_version;
        while current < derived {
            let Some(step) = schema
                .migrations
                .iter()
                .find(|step| i64::from(step.from) == current)
            else {
                return schema_refusal(format!(
                    "save schema_version {schema_version} cannot reach the project's {derived}: the migration chain has no step from version {current}"
                ));
            };
            migrations_applied.push(step.name.clone());
            current += 1;
        }
    }
    if format_version > GAME_FORMAT_VERSION {
        return schema_refusal(format!(
            "save format_version {format_version} is newer than this build supports ({GAME_FORMAT_VERSION})"
        ));
    }
    if format_version < GAME_FORMAT_VERSION_FIRST {
        return schema_refusal(format!(
            "save format_version {format_version} is not a known version"
        ));
    }
    let mut state = state_from_ron(state_ron, format_version)?;
    if format_version == GAME_FORMAT_VERSION_FIRST {
        state = migrate_state_v1_to_v2(state);
    }
    verify_state_against_project(game, &state)?;
    let recomputed = state_hash(&state);
    if recomputed != recorded_hash {
        return schema_refusal(format!(
            "save state_hash {recorded_hash} does not recompute ({recomputed}); the save is corrupt or hand-edited"
        ));
    }
    Ok(SavedGame {
        run_seed,
        state,
        migrations_applied,
    })
}

/// A save's state must name only what the project defines: a
/// stat instance for an undefined stat (or character), a flag,
/// a trigger, a scene, or a line the project does not carry
/// refuses the load.
fn verify_state_against_project(game: &Game, state: &GameState) -> Result<(), Refusal> {
    for key in state.stats.keys().chain(state.lifetimes.keys()) {
        let Some(stat) = game.stat(&key.stat) else {
            return schema_refusal(format!("save names undefined stat '{}'", key.stat));
        };
        match (stat.scope, &key.character) {
            (crate::game::model::StatScope::Character, Some(who)) => {
                if game.character(who).is_none() {
                    return schema_refusal(format!(
                        "save names undefined character '{who}' for stat '{}'",
                        key.stat
                    ));
                }
            }
            (crate::game::model::StatScope::Character, None) => {
                return schema_refusal(format!(
                    "save carries character-scoped stat '{}' with no character",
                    key.stat
                ));
            }
            (_, Some(_)) => {
                return schema_refusal(format!(
                    "save carries stat '{}' with a character it does not scope to",
                    key.stat
                ));
            }
            (_, None) => {}
        }
        if state.lifetimes.contains_key(key) && stat.lifetime.is_none() {
            return schema_refusal(format!(
                "save arms a lifetime on stat '{}', which defines none",
                key.stat
            ));
        }
    }
    for flag in state.flags_run.keys().chain(state.flags_globals.keys()) {
        if game.flag(flag).is_none() {
            return schema_refusal(format!("save names undefined flag '{flag}'"));
        }
    }
    for id in state.triggers.keys() {
        if game.trigger(id).is_none() {
            return schema_refusal(format!("save names undefined trigger '{id}'"));
        }
    }
    for id in state.rng.keys() {
        if game.stream(id).is_none() {
            return schema_refusal(format!("save names undefined RNG stream '{id}'"));
        }
    }
    if let Some(scene) = &state.current_scene
        && game.scene(scene).is_none()
    {
        return schema_refusal(format!("save names undefined scene '{scene}'"));
    }
    if let Some(line) = &state.line
        && game.dialogue_line(line).is_none()
    {
        return schema_refusal(format!("save names undefined dialogue line '{line}'"));
    }
    if let Some(ending) = &state.ending
        && game.ending(ending).is_none()
    {
        return schema_refusal(format!("save names undefined ending '{ending}'"));
    }
    Ok(())
}
