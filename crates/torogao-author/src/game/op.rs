//! The AUTH-G verb manifest and the typed game-op inputs.
//!
//! All 22 verbs of Amendment 2 §G1/§G2 are named here, each
//! tagged with the slice that delivers it. The Phase 2 slices
//! implement the verbs they need (see `implemented`); every
//! other verb is refused by name (`TG-AUTH-UNKNOWN-KIND`), never
//! silently ignored — the rig manifest's discipline, carried
//! over. The registry fold (the A4 delta) consumes this table
//! when it lands.

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

/// The AUTH-G manifest: 22 verbs, alphabetical within group.
pub const GAME_MANIFEST: [GameVerbSpec; 22] = [
    verb("clock_define", "game core", GameSlice::P22, true, false),
    verb("game_define", "game core", GameSlice::P21, true, true),
    verb(
        "game_save_schema_define",
        "game core",
        GameSlice::G1,
        true,
        false,
    ),
    verb("rng_stream_define", "game core", GameSlice::G1, true, false),
    verb(
        "condition_define",
        "game state",
        GameSlice::P22,
        true,
        false,
    ),
    verb(
        "effect_set_define",
        "game state",
        GameSlice::P22,
        true,
        false,
    ),
    verb("flag_define", "game state", GameSlice::P22, true, false),
    verb("stat_define", "game state", GameSlice::P22, true, false),
    verb(
        "stat_tier_define",
        "game state",
        GameSlice::P22,
        true,
        false,
    ),
    verb("table_define", "game state", GameSlice::G1, true, false),
    verb("value_define", "game state", GameSlice::P22, true, false),
    verb("character_define", "game flow", GameSlice::P21, true, true),
    verb("choice_define", "game flow", GameSlice::G2, true, false),
    verb(
        "dialogue_line_define",
        "game flow",
        GameSlice::P21,
        true,
        true,
    ),
    verb("ending_define", "game flow", GameSlice::G2, true, false),
    verb(
        "gallery_entry_define",
        "game flow",
        GameSlice::G2,
        true,
        false,
    ),
    verb("scene_bind_rig", "game flow", GameSlice::G2, true, false),
    verb("scene_define", "game flow", GameSlice::P21, true, true),
    verb("scene_exit_define", "game flow", GameSlice::G2, true, false),
    verb("schedule_define", "game flow", GameSlice::G2, true, false),
    verb("trigger_define", "game flow", GameSlice::P22, true, false),
    verb(
        "game_simulate",
        "game runtime",
        GameSlice::P22,
        false,
        false,
    ),
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

// ---- input records -----------------------------------------------

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

/// A mutating game op: one game verb with its typed inputs.
#[derive(Clone, Debug, PartialEq)]
pub enum GameOp {
    /// `character_define`.
    CharacterDefine(CharacterDefineInput),
    /// `dialogue_line_define`.
    DialogueLineDefine(DialogueLineDefineInput),
    /// `game_define`.
    GameDefine(GameDefineInput),
    /// `scene_define`.
    SceneDefine(SceneDefineInput),
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
        "character_define" => GameOp::CharacterDefine(typed(verb, inputs)?),
        "dialogue_line_define" => GameOp::DialogueLineDefine(typed(verb, inputs)?),
        "game_define" => GameOp::GameDefine(typed(verb, inputs)?),
        "scene_define" => GameOp::SceneDefine(typed(verb, inputs)?),
        other => return refuse_game_unimplemented(other),
    };
    Ok(crate::op::Op::Game(op))
}

impl GameOp {
    /// The canonical verb name.
    pub fn verb(&self) -> &'static str {
        match self {
            GameOp::CharacterDefine(_) => "character_define",
            GameOp::DialogueLineDefine(_) => "dialogue_line_define",
            GameOp::GameDefine(_) => "game_define",
            GameOp::SceneDefine(_) => "scene_define",
        }
    }

    /// The normalized inputs as JSON (what the op log records).
    pub fn inputs_json(&self) -> Result<Value, Refusal> {
        let result = match self {
            GameOp::CharacterDefine(inputs) => serde_json::to_value(inputs),
            GameOp::DialogueLineDefine(inputs) => serde_json::to_value(inputs),
            GameOp::GameDefine(inputs) => serde_json::to_value(inputs),
            GameOp::SceneDefine(inputs) => serde_json::to_value(inputs),
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
    fn game_manifest_has_22_verbs_alphabetical_within_group() {
        assert_eq!(GAME_MANIFEST.len(), 22);
        let mut groups: Vec<&str> = Vec::new();
        for spec in GAME_MANIFEST {
            if groups.last() != Some(&spec.group) {
                assert!(!groups.contains(&spec.group), "group {} split", spec.group);
                groups.push(spec.group);
            }
        }
        assert_eq!(groups.len(), 4);
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
    }
}
