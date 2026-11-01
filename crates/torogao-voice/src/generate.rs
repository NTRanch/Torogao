//! The grammar generator: sources of truth in, grammar
//! document out.
//!
//! Tool entries derive mechanically from the live registry
//! (`torogao_cli::registry`), so a tool cannot exist without
//! a spoken form. Verb entries derive from the manifests in
//! `BRIEF-AUTHORING.md` Item 4 and
//! `AMENDMENT-2-GAME-SYSTEMS.md` Item G2, carried here as data
//! tables. CLI modes, app commands, and the safety class are
//! hand-authored tables below.
//!
//! The exclusion list is part of the generator: targets that
//! must never become voice-completable are named here, and a
//! test asserts their absence from every generated grammar.

use crate::grammar::{
    ConfirmClass, GRAMMAR_VERSION, GrammarDoc, GrammarEntry, SlotKind, SlotSpec, Target,
};
use serde_json::Value;

/// Targets that never get a grammar entry, by design law:
///
/// - anything that arms live device output (the haptics S4
///   rung) — voice may request through other surfaces; the
///   Owner's go + presence decides, and there is nothing in
///   the grammar to parse to;
/// - Purge/Reset *execution* — the rite's screen is
///   voice-reachable (`app.open-purge-reset`), but the
///   destructive confirm is a deliberate on-screen act, never
///   a spoken word.
///
/// No current registry tool or verb carries these names; the
/// list exists so a future surface that adds one fails the
/// exclusion test instead of silently inheriting voice
/// coverage.
pub const EXCLUDED_TARGET_NAMES: &[&str] = &["haptics-arm-live", "purge-execute", "purge"];

/// The 47 canonical authoring verbs (BRIEF-AUTHORING.md
/// Item 4), alphabetical, as the manifest lists them.
pub const AUTHOR_VERBS: &[&str] = &[
    "audio_bind_set",
    "audio_track_define",
    "curve_from_samples",
    "curve_set",
    "deformer_glue_set",
    "deformer_parent_set",
    "deformer_remove",
    "deformer_rotation_create",
    "deformer_warp_create",
    "export_bundle",
    "export_manifest",
    "export_native",
    "expression_define",
    "expression_remove",
    "expression_test",
    "intake_image",
    "intake_psd",
    "keyform_set",
    "mesh_set",
    "mesh_trim",
    "motion_bake",
    "motion_create",
    "param_define",
    "param_grid_define",
    "param_link_define",
    "param_pose_default_set",
    "param_remove",
    "part_art_bind",
    "part_clip_set",
    "part_create",
    "part_draw_order_set",
    "part_parent_set",
    "part_remove",
    "part_visibility_set",
    "physics_bake",
    "physics_forces_set",
    "physics_group_define",
    "physics_group_remove",
    "project_diff",
    "project_import",
    "project_init",
    "project_inspect",
    "project_replay",
    "project_validate",
    "render_range_sweep",
    "render_still",
    "render_take",
];

/// The 22 AUTH-G game verbs (AMENDMENT-2-GAME-SYSTEMS.md
/// Item G2), alphabetical.
pub const GAME_VERBS: &[&str] = &[
    "character_define",
    "choice_define",
    "clock_define",
    "condition_define",
    "dialogue_line_define",
    "effect_set_define",
    "ending_define",
    "flag_define",
    "gallery_entry_define",
    "game_define",
    "game_save_schema_define",
    "game_simulate",
    "rng_stream_define",
    "scene_bind_rig",
    "scene_define",
    "scene_exit_define",
    "schedule_define",
    "stat_define",
    "stat_tier_define",
    "table_define",
    "trigger_define",
    "value_define",
];

/// Verbs that mutate nothing: they execute on parse, with no
/// confirmation. `expression_test` is read-only by the brief
/// (it appends no op-log entry) but renders a golden file to
/// disk, so it keeps the confirm class — see
/// [`verb_confirm_class`].
const READ_ONLY_VERBS: &[&str] = &[
    "expression_test",
    "game_simulate",
    "project_diff",
    "project_inspect",
    "project_validate",
];

/// CLI modes that are not registry tools.
const CLI_MODES: &[&str] = &["a2a", "mcp", "paths", "registry-dump"];

/// Schema properties that hold paths (spoken as one verbatim
/// token) rather than bare names.
const PATH_PROPERTIES: &[&str] = &[
    "controllers",
    "drivers",
    "out",
    "override",
    "recipe",
    "track",
    "wav",
];

/// Every authoring verb (47 + 22), sorted.
pub fn all_verbs() -> Vec<&'static str> {
    let mut verbs: Vec<&'static str> = AUTHOR_VERBS
        .iter()
        .chain(GAME_VERBS.iter())
        .copied()
        .collect();
    verbs.sort_unstable();
    verbs
}

/// The CLI mode names, for coverage checks.
pub fn cli_modes() -> &'static [&'static str] {
    CLI_MODES
}

/// Builds the full grammar document from its sources.
pub fn build_grammar() -> GrammarDoc {
    let mut entries: Vec<GrammarEntry> = Vec::new();
    entries.push(stop_entry());
    entries.extend(app_entries());
    entries.extend(mode_entries());
    entries.extend(verb_entries());
    entries.extend(tool_entries());
    entries.sort_by(|a, b| a.id.cmp(&b.id));
    GrammarDoc {
        version: GRAMMAR_VERSION.to_string(),
        entries,
    }
}

/// The reserved stop class.
fn stop_entry() -> GrammarEntry {
    GrammarEntry {
        id: "safety.stop".to_string(),
        target: Target::SafetyStop,
        posture: "safety".to_string(),
        confirm: ConfirmClass::None,
        templates: ["stop", "halt", "cancel", "safeword"]
            .iter()
            .map(ToString::to_string)
            .collect(),
        slots: Vec::new(),
    }
}

/// Hand-authored app-shell commands. These parse from day
/// one; they bind when the app shell lands (dispatch returns
/// `TG-VOICE-NOT-BOUND` until then).
fn app_entries() -> Vec<GrammarEntry> {
    let table: &[(&str, &str, &[&str])] = &[
        ("app.confirm-pending", "confirm-pending", &["confirm"]),
        (
            "app.open-code-ledger",
            "open-code-ledger",
            &["open code ledger"],
        ),
        (
            "app.open-creator",
            "open-creator",
            &["open creator", "creator"],
        ),
        (
            "app.open-playroom",
            "open-playroom",
            &["open playroom", "playroom"],
        ),
        (
            "app.open-purge-reset",
            "open-purge-reset",
            &["open purge and reset", "open purge"],
        ),
        (
            "app.open-settings",
            "open-settings",
            &["open settings", "settings"],
        ),
        ("app.open-study", "open-study", &["open study", "the study"]),
        (
            "app.toggle-nerd-mode",
            "toggle-nerd-mode",
            &["toggle nerd mode", "nerd mode"],
        ),
    ];
    table
        .iter()
        .map(|(id, name, templates)| GrammarEntry {
            id: id.to_string(),
            target: Target::AppCommand {
                name: name.to_string(),
            },
            posture: "app".to_string(),
            confirm: ConfirmClass::None,
            templates: templates.iter().map(ToString::to_string).collect(),
            slots: Vec::new(),
        })
        .collect()
}

/// CLI modes as voice entries. `paths` and `registry-dump`
/// execute through the CLI library; the two server modes
/// parse but stay unbound (a server is a session, not a
/// command outcome) and confirm-class, as starting a server
/// is a state change.
fn mode_entries() -> Vec<GrammarEntry> {
    let table: &[(&str, &[&str], ConfirmClass)] = &[
        (
            "a2a",
            &["start a2a server", "a2a server"],
            ConfirmClass::Confirm,
        ),
        (
            "mcp",
            &["start mcp server", "mcp server"],
            ConfirmClass::Confirm,
        ),
        ("paths", &["show paths", "paths"], ConfirmClass::None),
        (
            "registry-dump",
            &["dump registry", "registry dump"],
            ConfirmClass::None,
        ),
    ];
    table
        .iter()
        .map(|(name, templates, confirm)| GrammarEntry {
            id: format!("mode.{name}"),
            target: Target::CliMode {
                name: name.to_string(),
            },
            posture: "app".to_string(),
            confirm: *confirm,
            templates: templates.iter().map(ToString::to_string).collect(),
            slots: Vec::new(),
        })
        .collect()
}

/// The confirmation class of an authoring verb.
fn verb_confirm_class(verb: &str) -> ConfirmClass {
    if verb == "expression_test" {
        // Read-only per the brief, but it writes a golden
        // render; a spoken slip should not write files
        // unconfirmed.
        return ConfirmClass::Confirm;
    }
    if READ_ONLY_VERBS.contains(&verb) {
        ConfirmClass::None
    } else {
        ConfirmClass::Confirm
    }
}

/// Verb entries: spoken form is "author" + the verb name with
/// underscores as spaces, plus an optional trailing argument
/// span. Execution binds when torogao-author lands; the
/// grammar coverage is real from day one.
fn verb_entries() -> Vec<GrammarEntry> {
    all_verbs()
        .iter()
        .map(|verb| {
            let spoken = verb.replace('_', " ");
            let read_only = READ_ONLY_VERBS.contains(verb);
            GrammarEntry {
                id: format!("verb.{verb}"),
                target: Target::AuthorVerb {
                    name: verb.to_string(),
                },
                posture: if read_only { "read-only" } else { "mutating" }.to_string(),
                confirm: verb_confirm_class(verb),
                templates: vec![
                    format!("author {spoken}"),
                    format!("author {spoken} {{args}}"),
                ],
                slots: vec![SlotSpec {
                    name: "args".to_string(),
                    kind: SlotKind::Rest,
                    required: false,
                    phrase: String::new(),
                }],
            }
        })
        .collect()
}

/// Maps one input-schema property to its slot kind.
fn slot_kind_for(property: &str, schema: &Value) -> SlotKind {
    let type_name = schema
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string");
    if property == "project" {
        return SlotKind::Project;
    }
    match type_name {
        "boolean" => SlotKind::Flag,
        "integer" | "number" => SlotKind::Number,
        "array" => SlotKind::List,
        _ if PATH_PROPERTIES.contains(&property) => SlotKind::Path,
        _ => SlotKind::Name,
    }
}

/// Tool entries, derived from the live registry: canonical
/// spoken form is the tool name as words, then one phrase per
/// schema property in schema order (`<property> {<slot>}`),
/// optional properties and flags bracketed. A second,
/// required-only template is emitted when optional slots
/// exist.
fn tool_entries() -> Vec<GrammarEntry> {
    let mut entries = Vec::new();
    for tool in torogao_cli::registry::list_tools() {
        let spec = torogao_cli::registry::spec_json(tool);
        let schema = &spec["inputSchema"];
        let properties = schema.get("properties").and_then(Value::as_object);
        let required: Vec<String> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let mut slots: Vec<SlotSpec> = Vec::new();
        if let Some(properties) = properties {
            for (name, prop_schema) in properties {
                let kind = slot_kind_for(name, prop_schema);
                slots.push(SlotSpec {
                    name: name.clone(),
                    kind,
                    required: required.contains(name),
                    phrase: if kind == SlotKind::Flag {
                        name.replace('_', " ")
                    } else {
                        String::new()
                    },
                });
            }
        }
        slots.sort_by(|a, b| a.name.cmp(&b.name));
        let command_words = tool.name.replace('-', " ");
        let full = render_template(&command_words, &slots, true);
        let mut templates = vec![full.clone()];
        let required_only = render_template(&command_words, &slots, false);
        if required_only != full {
            templates.push(required_only);
        }
        // Short form: when `project` is the only required
        // slot, it may be spoken bare, without its phrase
        // word ("inspect prima" for "inspect project prima").
        let required_slots: Vec<&SlotSpec> = slots.iter().filter(|slot| slot.required).collect();
        if required_slots.len() == 1 && required_slots[0].kind == SlotKind::Project {
            let mut short = format!("{command_words} {{project}}");
            for slot in &slots {
                if !slot.required && slot.kind != SlotKind::Flag {
                    short.push_str(&format!(
                        " [{} {{{}}}]",
                        slot.name.replace('_', " "),
                        slot.name
                    ));
                }
            }
            if !templates.contains(&short) {
                templates.push(short);
            }
        }
        entries.push(GrammarEntry {
            id: format!("tool.{}", tool.name),
            target: Target::RegistryTool {
                name: tool.name.to_string(),
            },
            posture: tool.posture.to_string(),
            confirm: if tool.posture == "export" {
                ConfirmClass::Confirm
            } else {
                ConfirmClass::None
            },
            templates,
            slots,
        });
    }
    entries
}

/// Renders one template. With `include_optional`, optional
/// slots appear bracketed; without, they are omitted. Flag
/// slots render as their bare phrase (bracketed — a flag is
/// always optional); other slots render as
/// `<property words> {<name>}`.
fn render_template(command_words: &str, slots: &[SlotSpec], include_optional: bool) -> String {
    let mut parts: Vec<String> = vec![command_words.to_string()];
    for slot in slots {
        if slot.kind == SlotKind::Flag {
            parts.push(format!("[{}]", slot.phrase));
            continue;
        }
        let segment = format!("{} {{{}}}", slot.name.replace('_', " "), slot.name);
        if slot.required {
            parts.push(segment);
        } else if include_optional {
            parts.push(format!("[{segment}]"));
        }
    }
    parts.join(" ")
}
