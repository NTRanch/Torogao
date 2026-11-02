//! The companion's cast roster (Stage 1.2: all three cast
//! rigs selectable). The cast is a closed roster of the
//! estate's governed rigs, each with its repertoire fixture
//! file; members resolve as siblings in one cast directory
//! (the Phase F fixture layout). Selecting a member is a
//! scene install in the one session host — the companion
//! holds no per-character state anywhere (PROGRAM.md
//! §3.4), so this module is data plus path resolution,
//! nothing more.

use std::path::{Path, PathBuf};

use crate::error::{CompanionError, CompanionResult};

/// One cast member: her stable id, her display name, and
/// the repertoire fixture file that carries her.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastMember {
    /// Stable id (`prima` / `yui` / `alma`).
    pub id: &'static str,
    /// Display name for the menu header.
    pub name: &'static str,
    /// Repertoire fixture file name, resolved inside the
    /// cast directory.
    pub fixture_file: &'static str,
}

/// The cast, in menu order (Prima is the debut member,
/// PROGRAM.md §6.9).
pub const CAST: [CastMember; 3] = [
    CastMember {
        id: "prima",
        name: "Prima",
        fixture_file: "prima-repertoire.inx",
    },
    CastMember {
        id: "yui",
        name: "Yui",
        fixture_file: "yui-repertoire.inx",
    },
    CastMember {
        id: "alma",
        name: "Alma",
        fixture_file: "alma-repertoire.inx",
    },
];

/// Finds a cast member by id.
pub fn member(id: &str) -> Option<&'static CastMember> {
    CAST.iter().find(|member| member.id == id)
}

/// The cast member a repertoire model path belongs to,
/// by file name — the presentation's `--model` may name
/// any of the three fixtures, and the roster must know
/// who is on stage before any switch is offered.
pub fn member_for_path(path: &Path) -> Option<&'static CastMember> {
    let file = path.file_name()?.to_str()?;
    CAST.iter().find(|member| member.fixture_file == file)
}

/// Resolves a member's model path inside `cast_dir`,
/// refusing typed when the fixture is absent (a partial
/// cast directory is a deployment error, never a silent
/// substitution of another member).
pub fn model_path(cast_dir: &Path, member: &CastMember) -> CompanionResult<PathBuf> {
    let path = cast_dir.join(member.fixture_file);
    if path.is_file() {
        Ok(path)
    } else {
        Err(CompanionError::new(
            "TG-COMP-CAST",
            format!(
                "cast member '{}' has no repertoire fixture at {}",
                member.id,
                path.display()
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_roster_is_the_three_governed_rigs_in_menu_order() {
        let ids: Vec<&str> = CAST.iter().map(|member| member.id).collect();
        assert_eq!(ids, ["prima", "yui", "alma"]);
        assert_eq!(member("yui").expect("yui").name, "Yui");
        assert!(member("nobody").is_none());
    }

    #[test]
    fn members_resolve_by_fixture_file_name() {
        let path = Path::new("/cast/alma-repertoire.inx");
        assert_eq!(member_for_path(path).expect("alma").id, "alma");
        assert!(member_for_path(Path::new("/cast/other.inx")).is_none());
    }

    #[test]
    fn a_missing_fixture_refuses_typed() {
        let dir = std::env::temp_dir().join("torogao-companion-cast-empty");
        let err = model_path(&dir, &CAST[0]).expect_err("absent fixture refuses");
        assert_eq!(err.code, "TG-COMP-CAST");
    }
}
