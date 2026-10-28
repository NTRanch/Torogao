//! Application state held outside any widget: the loaded
//! scene plus the node selection.
//!
//! The selection is deliberately a plain data structure —
//! one optional node index into the scene's arena — so the
//! panels read it directly today and a session host can
//! serve the same value later without untangling widget
//! internals.

use crate::scene::Scene;

/// The user's current node selection: an index into the
/// loaded puppet's node arena, or nothing selected.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Selection {
    /// The selected node index, when a node is selected.
    pub node_index: Option<usize>,
}

impl Selection {
    /// Selects the node at `index`.
    pub fn select(&mut self, index: usize) {
        self.node_index = Some(index);
    }

    /// Clears the selection.
    pub fn clear(&mut self) {
        self.node_index = None;
    }

    /// Whether `index` is the selected node.
    pub fn is_selected(&self, index: usize) -> bool {
        self.node_index == Some(index)
    }
}

/// Everything the shell holds between frames.
#[derive(Default)]
pub struct ShellState {
    /// The loaded scene, when a model is open.
    pub scene: Option<Scene>,
    /// The current node selection.
    pub selection: Selection,
    /// The last user-visible status or error line.
    pub status: String,
}

impl ShellState {
    /// A fresh shell with no model loaded.
    pub fn new() -> ShellState {
        ShellState {
            scene: None,
            selection: Selection::default(),
            status: "no model loaded".to_string(),
        }
    }

    /// Installs a freshly loaded scene: the selection resets
    /// (node indices belong to the old arena) and the status
    /// names what was loaded.
    pub fn install_scene(&mut self, scene: Scene) {
        self.status = format!(
            "loaded {} ({} nodes, {} parameters)",
            scene.model_name(),
            scene.puppet.nodes.nodes.len(),
            scene.puppet.params.len(),
        );
        self.scene = Some(scene);
        self.selection.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::Selection;

    #[test]
    fn selection_round_trips() {
        let mut selection = Selection::default();
        assert_eq!(selection.node_index, None);
        selection.select(7);
        assert!(selection.is_selected(7));
        assert!(!selection.is_selected(8));
        selection.clear();
        assert_eq!(selection.node_index, None);
    }
}
