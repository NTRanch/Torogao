//! The AUTH-G gate: `decide_game`, a pure function of (project,
//! game op) → Apply | NoOp | Refusal, under the same laws as the
//! rig gate — definition verbs are upserts (identical inputs are
//! a no-op, differing inputs are `TG-AUTH-CONFLICT`), every
//! bound lives here, and deny is final.

use crate::decide::{Verdict, check_id};
use crate::game::apply::{character_record, dialogue_record, save_schema_version_of, scene_record};
use crate::game::model::{GAME_FORMAT_VERSION, GAME_FORMAT_VERSION_FIRST, Game};
use crate::game::op::{
    CharacterDefineInput, DialogueLineDefineInput, GameDefineInput, GameOp, SceneDefineInput,
};
use crate::limits::{
    GAME_CHARACTERS_MAX, GAME_CONTENT_TEXT_BYTES_MAX, GAME_DIALOGUE_LINES_MAX, GAME_SCENES_MAX,
};
use crate::model::Project;
use crate::refusal::{Refusal, RefusalClass, refuse};

fn conflict<T>(message: impl Into<String>) -> Result<T, Refusal> {
    refuse(RefusalClass::Conflict, message)
}

fn dangling<T>(what: &str, id: &str) -> Result<T, Refusal> {
    refuse(
        RefusalClass::Dangling,
        format!("{what} '{id}' does not exist"),
    )
}

fn check_count(count: usize, max: usize, what: &str) -> Result<(), Refusal> {
    if count > max {
        return refuse(
            RefusalClass::Range,
            format!("{what} would number {count}; the bound is {max}"),
        );
    }
    Ok(())
}

/// Prose law for sidecar text: non-empty, bounded, no control
/// characters other than newline and tab (dialogue prose may
/// break lines; it may not smuggle terminal controls).
fn check_prose(text: &str, what: &str) -> Result<(), Refusal> {
    if text.trim().is_empty() {
        return refuse(
            RefusalClass::Malformed,
            format!("{what} is missing or empty"),
        );
    }
    if text.len() > GAME_CONTENT_TEXT_BYTES_MAX {
        return refuse(
            RefusalClass::Range,
            format!("{what} exceeds {GAME_CONTENT_TEXT_BYTES_MAX} bytes"),
        );
    }
    if text
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        return refuse(
            RefusalClass::Malformed,
            format!("{what} carries control characters"),
        );
    }
    Ok(())
}

/// The prose upsert law, shared by every op that authors prose:
/// a content id with different prose already recorded is a
/// conflict; editorial changes are made by editing `content.ron`
/// itself, never by re-defining a line.
fn check_prose_upsert(project: &Project, id: &str, text: &str) -> Result<(), Refusal> {
    match project.content.iter().find(|entry| entry.id == id) {
        Some(entry) if entry.text != text => conflict(format!(
            "content '{id}' already carries different prose; edit content.ron to change it"
        )),
        _ => Ok(()),
    }
}

/// The game section, refusing when the project authors no game
/// yet (every game op but `game_define` needs one).
fn section(project: &Project) -> Result<&Game, Refusal> {
    let Some(game) = &project.game else {
        return refuse(
            RefusalClass::UnknownTarget,
            "the project has no game section; run game_define first",
        );
    };
    if game.format_version > GAME_FORMAT_VERSION {
        return refuse(
            RefusalClass::UnknownKind,
            format!(
                "game format_version {} is newer than this build supports ({GAME_FORMAT_VERSION})",
                game.format_version
            ),
        );
    }
    if game.format_version < GAME_FORMAT_VERSION_FIRST {
        return refuse(
            RefusalClass::Malformed,
            format!(
                "game format_version {} is not a known version",
                game.format_version
            ),
        );
    }
    Ok(game)
}

/// THE game gate. Pure: reads only its arguments.
pub fn decide_game(project: &Project, op: &GameOp) -> Result<Verdict, Refusal> {
    match op {
        GameOp::GameDefine(inputs) => decide_game_define(project, inputs),
        GameOp::CharacterDefine(inputs) => {
            section(project)?;
            decide_character(project, inputs)
        }
        GameOp::SceneDefine(inputs) => {
            let game = section(project)?;
            decide_scene(game, inputs)
        }
        GameOp::DialogueLineDefine(inputs) => {
            section(project)?;
            decide_dialogue(project, inputs)
        }
    }
}

fn decide_game_define(project: &Project, inputs: &GameDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "game id")?;
    if let Some(scene) = &inputs.start_scene {
        check_id(scene, "start scene")?;
    }
    let schema = save_schema_version_of(inputs);
    if schema < 1 {
        return refuse(
            RefusalClass::Range,
            format!("save_schema_version {schema} is below 1"),
        );
    }
    match &project.game {
        None => Ok(Verdict::Apply),
        Some(game) => {
            if game.format_version > GAME_FORMAT_VERSION {
                return refuse(
                    RefusalClass::UnknownKind,
                    format!(
                        "game format_version {} is newer than this build supports ({GAME_FORMAT_VERSION})",
                        game.format_version
                    ),
                );
            }
            if game.id == inputs.id
                && game.start_scene == inputs.start_scene
                && game.save_schema_version == schema
            {
                Ok(Verdict::NoOp)
            } else {
                conflict("a game with different defining inputs already exists here")
            }
        }
    }
}

fn decide_character(project: &Project, inputs: &CharacterDefineInput) -> Result<Verdict, Refusal> {
    let game = section(project)?;
    check_id(&inputs.id, "character id")?;
    check_id(&inputs.name_ref, "name reference")?;
    check_prose(&inputs.name_text, "character name")?;
    check_prose_upsert(project, &inputs.name_ref, &inputs.name_text)?;
    let mut seen: Vec<&str> = Vec::with_capacity(inputs.initials.len());
    for row in &inputs.initials {
        if seen.contains(&row.stat.as_str()) {
            return refuse(
                RefusalClass::Malformed,
                format!("stat '{}' is given twice in the initials", row.stat),
            );
        }
        seen.push(&row.stat);
        let Some(stat) = game.stat(&row.stat) else {
            return dangling("stat", &row.stat);
        };
        if stat.scope != crate::game::model::StatScope::Character {
            return refuse(
                RefusalClass::Malformed,
                format!(
                    "initial for stat '{}' needs a character-scoped stat",
                    row.stat
                ),
            );
        }
        if row.value < stat.min || row.value > stat.max {
            return refuse(
                RefusalClass::Range,
                format!(
                    "initial {} for stat '{}' is outside its range [{}, {}]",
                    row.value, stat.id, stat.min, stat.max
                ),
            );
        }
    }
    let record = character_record(inputs);
    match game.character(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "character '{}' already exists with different inputs",
            inputs.id
        )),
        None => {
            check_count(game.characters.len() + 1, GAME_CHARACTERS_MAX, "characters")?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_scene(game: &Game, inputs: &SceneDefineInput) -> Result<Verdict, Refusal> {
    check_id(&inputs.id, "scene id")?;
    let record = scene_record(inputs);
    match game.scene(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "scene '{}' already exists with different inputs",
            inputs.id
        )),
        None => {
            check_count(game.scenes.len() + 1, GAME_SCENES_MAX, "scenes")?;
            Ok(Verdict::Apply)
        }
    }
}

fn decide_dialogue(
    project: &Project,
    inputs: &DialogueLineDefineInput,
) -> Result<Verdict, Refusal> {
    let game = section(project)?;
    check_id(&inputs.id, "dialogue line id")?;
    check_id(&inputs.text_ref, "text reference")?;
    if game.scene(&inputs.scene).is_none() {
        return dangling("scene", &inputs.scene);
    }
    if inputs.speaker != "narrator" && game.character(&inputs.speaker).is_none() {
        return dangling("speaker character", &inputs.speaker);
    }
    if let Some(next) = &inputs.next {
        check_id(next, "next line")?;
    }
    if let Some(hook) = &inputs.hook_expression {
        check_id(hook, "hook expression")?;
    }
    check_prose(&inputs.text, "dialogue text")?;
    check_prose_upsert(project, &inputs.text_ref, &inputs.text)?;
    let record = dialogue_record(inputs);
    match game.dialogue_line(&inputs.id) {
        Some(existing) if *existing == record => Ok(Verdict::NoOp),
        Some(_) => conflict(format!(
            "dialogue line '{}' already exists with different inputs",
            inputs.id
        )),
        None => {
            let in_scene = game
                .dialogue
                .iter()
                .filter(|line| line.scene == inputs.scene)
                .count();
            if usize::try_from(inputs.ordinal).unwrap_or(usize::MAX) < in_scene {
                return refuse(
                    RefusalClass::GameGraph,
                    format!(
                        "ordinal {} in scene '{}' is already taken ({} lines authored)",
                        inputs.ordinal, inputs.scene, in_scene
                    ),
                );
            }
            if usize::try_from(inputs.ordinal).unwrap_or(usize::MAX) > in_scene {
                return refuse(
                    RefusalClass::GameGraph,
                    format!(
                        "ordinal {} in scene '{}' would leave a gap ({} lines authored)",
                        inputs.ordinal, inputs.scene, in_scene
                    ),
                );
            }
            check_count(
                game.dialogue.len() + 1,
                GAME_DIALOGUE_LINES_MAX,
                "dialogue lines",
            )?;
            Ok(Verdict::Apply)
        }
    }
}
