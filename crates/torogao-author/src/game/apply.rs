//! Applying admitted game ops.
//!
//! Runs only after `decide_game` returned `Verdict::Apply`; like
//! the rig side, it asserts what the gate established instead
//! of re-checking. Record builders live here (shared with the
//! gate's identity comparisons, the `expression_record`
//! precedent).

use crate::game::model::{
    CharacterDef, DialogueLine, GAME_FORMAT_VERSION, Game, SceneDef, StatInitial,
};
use crate::game::op::{
    CharacterDefineInput, DialogueLineDefineInput, GameDefineInput, GameOp, PresentationInput,
    SceneDefineInput,
};
use crate::model::{Project, locate};
use crate::refusal::Refusal;

/// Inserts or replaces a record in an id-sorted collection.
fn upsert<T>(items: &mut Vec<T>, item: T, key: impl Fn(&T) -> &String) {
    let id = key(&item).clone();
    match locate(items, &id, &key) {
        Ok(index) => items[index] = item,
        Err(index) => items.insert(index, item),
    }
}

/// The save-schema version a `game_define` input names (an
/// absent input is version 1).
pub(crate) fn save_schema_version_of(inputs: &GameDefineInput) -> i64 {
    inputs.save_schema_version.unwrap_or(1)
}

/// The character record a `character_define` produces (initials
/// sorted by stat id).
pub(crate) fn character_record(inputs: &CharacterDefineInput) -> CharacterDef {
    let mut initials: Vec<StatInitial> = inputs
        .initials
        .iter()
        .map(|row| StatInitial {
            stat: row.stat.clone(),
            value: row.value,
        })
        .collect();
    initials.sort_by(|a, b| a.stat.cmp(&b.stat));
    CharacterDef {
        id: inputs.id.clone(),
        name_ref: inputs.name_ref.clone(),
        initials,
    }
}

/// The scene record a `scene_define` produces.
pub(crate) fn scene_record(inputs: &SceneDefineInput) -> SceneDef {
    SceneDef {
        id: inputs.id.clone(),
        presentation: match inputs.presentation {
            PresentationInput::Event => crate::game::model::Presentation::Event,
            PresentationInput::Room => crate::game::model::Presentation::Room,
            PresentationInput::Vn => crate::game::model::Presentation::Vn,
        },
    }
}

/// The dialogue record a `dialogue_line_define` produces. The
/// record carries the text reference only; the prose goes to the
/// sidecar (see `apply_game`).
pub(crate) fn dialogue_record(inputs: &DialogueLineDefineInput) -> DialogueLine {
    DialogueLine {
        id: inputs.id.clone(),
        scene: inputs.scene.clone(),
        ordinal: inputs.ordinal,
        speaker: inputs.speaker.clone(),
        text_ref: inputs.text_ref.clone(),
        next: inputs.next.clone(),
        hook_expression: inputs.hook_expression.clone(),
    }
}

/// The format migration, applied whenever a game op lands on a
/// section written under an older game format: v1 → v2 changes
/// no record (v2 only adds optional fields and the interpreter's
/// tick semantics), so the migration is the version stamp
/// itself. It is a named step so the rule is explicit and the
/// save loader's chain can share it.
pub(crate) fn migrate_game_section(game: &mut Game) {
    if game.format_version < GAME_FORMAT_VERSION {
        game.format_version = GAME_FORMAT_VERSION;
    }
}

/// Records prose under a content id (the sidecar upsert: an
/// absent id gains its entry; an identical entry is kept — the
/// gate has already refused a differing one). The first prose a
/// project gains stamps the sidecar with the canonical authored
/// locale, `en` (D-472); a loaded sidecar's locale is never
/// rewritten.
fn record_prose(project: &mut Project, id: &str, text: &str) {
    if project.content_locale.is_none() {
        project.content_locale = Some(crate::game::model::CONTENT_LOCALE_CANONICAL.to_string());
    }
    let entry = crate::game::model::ContentEntry {
        id: id.to_string(),
        text: text.to_string(),
    };
    upsert(&mut project.content, entry, |e| &e.id);
}

/// Applies one admitted game op to the project.
pub fn apply_game(project: &mut Project, op: &GameOp) -> Result<(), Refusal> {
    match op {
        GameOp::GameDefine(inputs) => {
            let game = Game {
                id: inputs.id.clone(),
                format_version: GAME_FORMAT_VERSION,
                save_schema_version: save_schema_version_of(inputs),
                start_scene: inputs.start_scene.clone(),
                ..Game::default()
            };
            project.game = Some(game);
        }
        GameOp::CharacterDefine(inputs) => {
            let record = character_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.characters, record, |c| &c.id);
            record_prose(project, &inputs.name_ref, &inputs.name_text);
        }
        GameOp::SceneDefine(inputs) => {
            let record = scene_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.scenes, record, |s| &s.id);
        }
        GameOp::DialogueLineDefine(inputs) => {
            let record = dialogue_record(inputs);
            let game = game_mut(project);
            upsert(&mut game.dialogue, record, |l| &l.id);
            record_prose(project, &inputs.text_ref, &inputs.text);
        }
    }
    if let Some(game) = &mut project.game {
        migrate_game_section(game);
    }
    Ok(())
}

fn game_mut(project: &mut Project) -> &mut Game {
    project
        .game
        .as_mut()
        .expect("decide() admitted a game op with no game section")
}
