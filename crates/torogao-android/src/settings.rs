//! The Settings area's store: everything the phone
//! Settings surface operates, as platform-neutral logic
//! over a storage root (BRIEF-A1 §Scope 4).
//!
//! Settings' capabilities are the Phase F enumeration for
//! the area — no registry tools home here; the area's
//! commands are the three shell natives (persona.rs
//! `native_commands`): the Nerd-mode toggle, Save state,
//! Load state. Each is implemented here against the same
//! functions the desktop shell uses:
//!
//! - the toggle reads/writes the persona preference
//!   through `persona::{load_preference, save_preference}`
//!   (temp file + rename; written only when the value
//!   changes — the desktop discipline, both halves);
//! - save/load move a session document through the `save`
//!   module and the session host's op replay, the windowed
//!   shell's own flow, with one phone substitution: the
//!   model reloads from the embedded fixture bytes instead
//!   of a filesystem path, and a document naming any other
//!   model is a typed refusal.
//!
//! Every failure is a typed `AppError` the surface renders
//! in the refusal register (§persona rule 11) — never a
//! crash, never a silent default.

use std::path::PathBuf;

use torogao_app::persona::{PreferenceState, load_preference, save_preference};
use torogao_app::player::PlayHost;
use torogao_app::save::{capture_with_defaults, read_save, restore_play, write_save};
use torogao_app::session_host::{AppBridge, project_genesis};
use torogao_app::state::ShellState;
use torogao_app::{AppError, AppResult};
use torogao_session::host::Host;
use torogao_session::session::Session;

use crate::fixture::{EMBEDDED_MODEL_LABEL, PRIMA_REPERTOIRE_BYTES, load_prima};
use crate::storage::Storage;

/// A save name's length bound (characters). Names are
/// single file leaves under the saves dir, never paths.
pub const SAVE_NAME_MAX: usize = 64;
/// The save file's suffix (the document is the desktop
/// `torogao-session-save/1` JSON).
pub const SAVE_FILE_SUFFIX: &str = ".save.json";
/// Who holds sessions this shell captures (provenance in
/// the document, studio register).
pub const HELD_BY: &str = "torogao-android shell";

/// Validates a save name: a single leaf, no separators,
/// no traversal, no control characters, bounded length.
/// The name comes from an IME text field — external input,
/// validated before it touches a path.
pub fn validate_save_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Param("save name is empty".to_string()));
    }
    if name.chars().count() > SAVE_NAME_MAX {
        return Err(AppError::Param(format!(
            "save name exceeds the {SAVE_NAME_MAX}-character bound"
        )));
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(AppError::Param(
            "save name must be a single name, not a path".to_string(),
        ));
    }
    if name.chars().any(|ch| ch.is_control()) {
        return Err(AppError::Param(
            "save name carries control characters".to_string(),
        ));
    }
    Ok(name)
}

/// The Settings store over one storage root.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    /// The preference file (Addition S state dir leaf).
    preference_file: PathBuf,
    /// The named saves' directory.
    saves_dir: PathBuf,
}

impl SettingsStore {
    /// A store rooted at `storage`'s derived paths.
    pub fn from_storage(storage: &Storage) -> SettingsStore {
        SettingsStore {
            preference_file: storage.preference_file(),
            saves_dir: storage.saves_dir(),
        }
    }

    /// A store over explicit paths (host tests).
    pub fn new(preference_file: PathBuf, saves_dir: PathBuf) -> SettingsStore {
        SettingsStore {
            preference_file,
            saves_dir,
        }
    }

    /// The preference file this store reads and writes.
    pub fn preference_file(&self) -> &std::path::Path {
        &self.preference_file
    }

    /// The persisted Nerd-mode choice (default Persona on
    /// a missing or unreadable file — the desktop rule: a
    /// preference must not brick the shell; corrupt
    /// content parses to the default inside
    /// `load_preference`).
    pub fn nerd_mode(&self) -> bool {
        load_preference(&self.preference_file).nerd_mode
    }

    /// Sets the persisted choice, writing only when the
    /// value changes. Returns whether a write happened.
    pub fn set_nerd_mode(&self, on: bool) -> AppResult<bool> {
        if self.nerd_mode() == on {
            return Ok(false);
        }
        save_preference(&self.preference_file, &PreferenceState { nerd_mode: on })?;
        Ok(true)
    }

    /// The path a save name resolves to.
    pub fn save_path(&self, name: &str) -> AppResult<PathBuf> {
        let name = validate_save_name(name)?;
        Ok(self.saves_dir.join(format!("{name}{SAVE_FILE_SUFFIX}")))
    }

    /// The existing saves' names, sorted (the surface's
    /// picker list). A missing saves dir reads as empty.
    pub fn list_saves(&self) -> Vec<String> {
        let mut names = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.saves_dir) {
            for entry in entries.flatten() {
                let file_name = entry.file_name();
                let Some(text) = file_name.to_str() else {
                    continue;
                };
                if let Some(name) = text.strip_suffix(SAVE_FILE_SUFFIX) {
                    names.push(name.to_string());
                }
            }
        }
        names.sort();
        names
    }

    /// Save state (native:save-session-state): captures
    /// the standing scene + play state into a named
    /// document under the output-guard rules (never the
    /// model itself, never an overwrite — the shell never
    /// forces, exactly as on desktop).
    pub fn save_session(
        &self,
        name: &str,
        state: &mut ShellState,
        play: &PlayHost,
    ) -> AppResult<String> {
        let path = self.save_path(name)?;
        if state.scene.is_none() {
            return Err(AppError::Param("no model loaded".to_string()));
        }
        let (genesis, defaults) = project_genesis(state)?;
        let evaluated = {
            let scene = state.scene.as_mut().expect("scene checked above");
            format!(
                "{:016x}",
                torogao_app::evaluated_state_hash(&mut scene.puppet)
            )
        };
        let play_snapshot = play.snapshot(state.scene.as_ref().expect("scene checked above"));
        let session = Session::new(genesis, &defaults, HELD_BY);
        let document = capture_with_defaults(
            &session,
            &play_snapshot,
            EMBEDDED_MODEL_LABEL,
            &evaluated,
            &defaults,
        );
        let session_id = document.session_id.clone();
        write_save(
            &path,
            &document,
            Some(std::path::Path::new(EMBEDDED_MODEL_LABEL)),
            false,
        )?;
        Ok(format!("saved {} (session {})", path.display(), session_id))
    }

    /// Load state (native:load-session-state): restores a
    /// named document — the model reloads from the
    /// embedded fixture, the op log replays through a
    /// fresh session + bridge, the play record restores.
    /// A corrupt document, a document naming another
    /// model, or a replay that lands on a different hash
    /// is a typed refusal and the caller's state is left
    /// untouched (validation completes before commit).
    pub fn load_session(&self, name: &str) -> AppResult<(ShellState, PlayHost, String)> {
        let path = self.save_path(name)?;
        let document = read_save(&path)?;
        if document.model != EMBEDDED_MODEL_LABEL {
            return Err(AppError::Param(format!(
                "save names model {:?}; this shell carries only the embedded Prima repertoire",
                document.model
            )));
        }
        let scene = load_prima(PRIMA_REPERTOIRE_BYTES)?;
        let mut shell = ShellState::new();
        shell.install_scene(scene);
        let genesis = torogao_session::from_canon_ron(&document.genesis)
            .map_err(|r| AppError::Format(format!("{}: {}", r.code, r.message)))?;
        let session = Session::new(genesis, &document.defaults, &document.held_by);
        let mut host = Host::new(session);
        let mut bridge = AppBridge::with_player(shell, PlayHost::new());
        let mut base = torogao_session::state_hash(&host.session.genesis);
        for op in &document.ops {
            let applied = host
                .call_tool(
                    "session_apply_op",
                    &serde_json::json!({"base_hash": base, "op": op}),
                    &mut bridge,
                )
                .map_err(|r| AppError::Format(format!("{}: {}", r.code, r.message)))?;
            base = applied["post"].as_str().unwrap_or_default().to_string();
        }
        if base != document.session_state_hash {
            return Err(AppError::Format(format!(
                "restored session hash {base} != saved hash {}",
                document.session_state_hash
            )));
        }
        let (mut shell, mut player) = bridge.into_parts();
        let report = {
            let scene = shell
                .scene
                .as_mut()
                .ok_or_else(|| AppError::Format("save model scene missing".to_string()))?;
            restore_play(&document, scene, &mut player)?
        };
        Ok((
            shell,
            player,
            format!(
                "loaded save {} (session {}, play steps {})",
                path.display(),
                document.session_id,
                report.steps_taken
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{SettingsStore, validate_save_name};
    use crate::fixture::{PRIMA_REPERTOIRE_BYTES, load_prima};
    use std::path::PathBuf;
    use torogao_app::player::PlayHost;
    use torogao_app::state::ShellState;

    /// A store over a fresh temp root (removed by the
    /// caller's cleanup).
    fn temp_store(tag: &str) -> (SettingsStore, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "torogao-android-settings-{tag}-{}",
            std::process::id()
        ));
        let saves = root.join("saves");
        std::fs::create_dir_all(&saves).expect("saves dir");
        (
            SettingsStore::new(root.join("persona-preference.json"), saves),
            root,
        )
    }

    /// B7's unit half: the toggle persists, a fresh store
    /// (the relaunch) reads it back, and write-on-change
    /// means a repeated set does not rewrite.
    #[test]
    fn nerd_mode_persists_across_store_instances() {
        let (store, root) = temp_store("persist");
        assert!(!store.nerd_mode());
        assert!(store.set_nerd_mode(true).expect("set on"));
        assert!(!store.set_nerd_mode(true).expect("set on again"));
        let relaunched =
            SettingsStore::new(root.join("persona-preference.json"), root.join("saves"));
        assert!(relaunched.nerd_mode());
        assert!(relaunched.set_nerd_mode(false).expect("set off"));
        assert!(!relaunched.nerd_mode());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Adversarial: a corrupt preference file yields the
    /// default (Persona), never an error or a panic.
    #[test]
    fn corrupt_preference_yields_the_default() {
        let (store, root) = temp_store("corrupt-pref");
        std::fs::write(root.join("persona-preference.json"), "{torn").expect("seed corrupt");
        assert!(!store.nerd_mode());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Adversarial: hostile save names are typed refusals.
    #[test]
    fn hostile_save_names_refuse() {
        for name in ["", "  ", "../escape", "a/b", "a\\b", "..", "a\u{7}b"] {
            assert!(
                validate_save_name(name).is_err(),
                "name {name:?} must refuse"
            );
        }
        assert!(validate_save_name(&"x".repeat(65)).is_err());
        assert_eq!(validate_save_name(" session-1 ").ok(), Some("session-1"));
    }

    /// Adversarial (cure round): IME-typed unicode names.
    /// The bound counts characters, not bytes (a 64-char
    /// multibyte name is legal, a 65-char one refuses);
    /// unicode-whitespace-only names trim to empty and
    /// refuse; a unicode-named save on disk lists back
    /// byte-exact.
    #[test]
    fn unicode_save_names_validate_by_character_and_round_trip() {
        assert_eq!(validate_save_name("セッション").ok(), Some("セッション"));
        assert!(validate_save_name(&"あ".repeat(64)).is_ok());
        assert!(validate_save_name(&"あ".repeat(65)).is_err());
        // Non-breaking and ideographic spaces are Rust
        // whitespace: a name of only those is empty.
        assert!(validate_save_name("\u{00A0}\u{3000}").is_err());
        // A combining sequence counts per character and
        // is otherwise ordinary.
        assert!(validate_save_name("e\u{0301}lan").is_ok());

        let (store, root) = temp_store("unicode");
        let path = store.save_path("セッション①").expect("path resolves");
        std::fs::write(&path, "{}").expect("seed save file");
        assert_eq!(store.list_saves(), vec!["セッション①".to_string()]);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Adversarial (cure round): the saves directory
    /// disappearing mid-run (the storage half of a
    /// surface-loss event) reads as an empty picker, not
    /// an error — and the store still resolves new save
    /// paths against the same root.
    #[test]
    fn missing_saves_dir_reads_empty() {
        let (store, root) = temp_store("missing-saves");
        std::fs::remove_dir_all(root.join("saves")).expect("remove saves dir");
        assert!(store.list_saves().is_empty());
        assert!(store.save_path("after-loss").is_ok());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Save → load restores the session and lands on the
    /// same evaluated state the capture recorded; a second
    /// save under the same name refuses (never forced).
    #[test]
    fn save_load_round_trip_restores_state() {
        let (store, root) = temp_store("roundtrip");
        let mut state = ShellState::new();
        state.install_scene(load_prima(PRIMA_REPERTOIRE_BYTES).expect("prima loads"));
        let mut play = PlayHost::new();
        {
            let scene = state.scene.as_ref().expect("scene");
            play.play(scene, Some("Greeting"), true, None)
                .expect("play");
        }
        let scene = state.scene.as_mut().expect("scene");
        play.step(scene, 12).expect("steps");
        let before = torogao_app::evaluated_state_hash(&mut scene.puppet);
        store
            .save_session("walk", &mut state, &play)
            .expect("save succeeds");
        assert!(
            store.save_session("walk", &mut state, &play).is_err(),
            "overwrite must refuse"
        );
        let (mut restored, _player, message) = store.load_session("walk").expect("load succeeds");
        assert!(message.contains("session"));
        let scene = restored.scene.as_mut().expect("restored scene");
        assert_eq!(torogao_app::evaluated_state_hash(&mut scene.puppet), before);
        std::fs::remove_dir_all(&root).expect("cleanup");
    }

    /// Adversarial: a corrupt save document is a typed
    /// refusal, and so is a document naming another model.
    #[test]
    fn corrupt_and_foreign_saves_refuse() {
        let (store, root) = temp_store("corrupt-save");
        let path = store.save_path("broken").expect("path");
        std::fs::write(&path, "{\"format\":").expect("seed corrupt");
        assert!(store.load_session("broken").is_err());
        // A well-formed document for another model: take a
        // real save and rewrite its model field.
        let mut state = ShellState::new();
        state.install_scene(load_prima(PRIMA_REPERTOIRE_BYTES).expect("prima loads"));
        let play = PlayHost::new();
        store.save_session("real", &mut state, &play).expect("save");
        let real_path = store.save_path("real").expect("path");
        let text = std::fs::read_to_string(&real_path).expect("read save");
        let foreign = text.replace(
            "torogao-android://prima-repertoire.inx",
            "someone-elses-model.inx",
        );
        assert_ne!(text, foreign);
        std::fs::write(&real_path, foreign).expect("rewrite");
        assert!(store.load_session("real").is_err());
        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
