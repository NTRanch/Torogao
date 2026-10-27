//! Platform paths (Addition S) and data-file lookup: XDG
//! base directories with forced `TOROGAO_*_PATH`
//! overrides, and the pack/template/contract search
//! order (environment override, then exe-relative and
//! working-directory candidates). An unresolvable
//! directory is None — nothing falls back to the working
//! directory, ever.

use std::path::{Path, PathBuf};

/// The four XDG base kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathKind {
    /// Configuration.
    Config,
    /// Data.
    Data,
    /// State.
    State,
    /// Cache.
    Cache,
}

impl PathKind {
    /// The environment override variable for this kind.
    pub fn override_var(&self) -> &'static str {
        match self {
            PathKind::Config => "TOROGAO_CONFIG_PATH",
            PathKind::Data => "TOROGAO_DATA_PATH",
            PathKind::State => "TOROGAO_STATE_PATH",
            PathKind::Cache => "TOROGAO_CACHE_PATH",
        }
    }

    /// The XDG variable for this kind.
    pub fn xdg_var(&self) -> &'static str {
        match self {
            PathKind::Config => "XDG_CONFIG_HOME",
            PathKind::Data => "XDG_DATA_HOME",
            PathKind::State => "XDG_STATE_HOME",
            PathKind::Cache => "XDG_CACHE_HOME",
        }
    }

    /// The home-relative fallback for this kind.
    pub fn fallback(&self) -> &'static str {
        match self {
            PathKind::Config => ".config",
            PathKind::Data => ".local/share",
            PathKind::State => ".local/state",
            PathKind::Cache => ".cache",
        }
    }
}

fn absolute_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if value.starts_with('/') => Some(value),
        _ => None,
    }
}

/// The app directory for a kind (`<base>/torogao`), or the
/// forced override verbatim; None when no per-user
/// directory can be resolved.
pub fn base_dir(kind: PathKind) -> Option<String> {
    if let Some(forced) = absolute_env(kind.override_var()) {
        return Some(forced);
    }
    if let Some(xdg) = absolute_env(kind.xdg_var()) {
        return Some(format!("{xdg}/torogao"));
    }
    let home = std::env::var("HOME").ok()?;
    if !home.starts_with('/') {
        return None;
    }
    Some(format!("{}/{}/torogao", home, kind.fallback()))
}

/// The default tutorial progress file.
pub fn default_tutorial_progress_path() -> Option<String> {
    base_dir(PathKind::State).map(|dir| format!("{dir}/tutorial-progress.json"))
}

/// The playroom directory.
pub fn playroom_dir() -> Option<String> {
    base_dir(PathKind::Data).map(|dir| format!("{dir}/playroom"))
}

/// The playroom prefset directory.
pub fn playroom_sets_dir() -> Option<String> {
    playroom_dir().map(|dir| format!("{dir}/sets"))
}

/// The default playroom session file.
pub fn default_playroom_session_path() -> Option<String> {
    playroom_dir().map(|dir| format!("{dir}/session.json"))
}

/// Finds a data file by leaf directory and name, in the
/// reference search order: the environment override
/// directory, then exe-relative `../../../<leaf>`,
/// exe-relative `../<leaf>`, then `./<leaf>`. None when
/// no candidate contains the file.
pub fn find_data_file(env_var: &str, leaf: &str, name: &str) -> Option<PathBuf> {
    for dir in candidate_data_dirs(env_var, leaf) {
        let path = dir.join(name);
        if path.exists() && !path.is_dir() {
            return Some(path);
        }
    }
    None
}

/// Candidate directories for a data leaf, in search
/// order, with the reference's textual `..` segments
/// preserved.
pub fn candidate_data_dirs(env_var: &str, leaf: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = std::env::var(env_var) {
        if !dir.is_empty() {
            dirs.push(PathBuf::from(dir));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        // The reference builds its candidates from the
        // executable's full file path (the file-name
        // component counts as one level), not its parent
        // directory.
        dirs.push(exe.join("../../../").join(leaf));
        dirs.push(exe.join("../").join(leaf));
    }
    dirs.push(PathBuf::from(".").join(leaf));
    dirs
}

/// All existing candidate directories for a leaf.
pub fn existing_data_dirs(env_var: &str, leaf: &str) -> Vec<PathBuf> {
    candidate_data_dirs(env_var, leaf)
        .into_iter()
        .filter(|dir| dir.is_dir())
        .collect()
}

/// Normalizes a path for guard comparison: absolute,
/// `.`/`..` resolved textually, symlinks NOT resolved
/// (the reference guard's exact rule).
pub fn canonical_for_compare(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(path)
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{PathKind, canonical_for_compare};
    use std::path::{Path, PathBuf};

    #[test]
    fn canonical_compare_resolves_dots_without_symlinks() {
        let p = canonical_for_compare(Path::new("/a/b/../c/./d"));
        assert_eq!(p, PathBuf::from("/a/c/d"));
    }

    #[test]
    fn path_kinds_have_stable_variables() {
        assert_eq!(PathKind::Data.override_var(), "TOROGAO_DATA_PATH");
        assert_eq!(PathKind::State.xdg_var(), "XDG_STATE_HOME");
    }
}
