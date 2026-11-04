//! The custody map: which scene nodes a human has taken
//! custody of, and are therefore frozen against regeneration
//! (D-838, the LIFT layer, Stage 1).
//!
//! Custody is the Strobe sweep's custody finding generalized
//! into data (DESIGN lineage: the conditioning seam's compile
//! report made omission explicit; custody makes *ownership*
//! explicit). A generation proposal may re-derive any node it
//! likes — except a node a person has touched. The map is part
//! of the scene record (`torogao-scene-record/1`), and this
//! module is the shared-semantics definition lumen's custody
//! implementation is conformance-checked against (see
//! `SCENE-RECORD-SPEC.md` in the lift workdir):
//!
//! - A node id is a path of segments joined by `/`; each
//!   segment is `[a-z0-9-]`, starting with a letter or digit.
//! - A target node T is custody-frozen iff the map carries a
//!   `Frozen` entry whose node N satisfies T == N or T begins
//!   with N followed by `/` — freezing a node freezes its
//!   whole subtree, because a hand edit to a part owns the
//!   part's children.
//! - An `Open` entry never freezes anything; it records that
//!   custody was considered and released.
//! - A node absent from the map is open.
//! - The check is fail-closed and total: a proposal naming
//!   any frozen target refuses whole, naming every frozen
//!   target — no partial application, ever.

use crate::error::GencondResult;
use crate::error::{CODE_CONFLICT, CODE_CUSTODY_FROZEN, CODE_MALFORMED, CODE_RANGE, refuse};
use crate::limits::{CUSTODY_ENTRIES_MAX, NODE_ID_BYTES_MAX, SCENE_TEXT_BYTES_MAX};
use serde::{Deserialize, Serialize};

/// One node's custody state, with the provenance of the
/// decision (who holds custody, and why) carried per entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyState {
    /// Custody considered and released: regeneration may
    /// target this node and its subtree (unless a descendant
    /// entry freezes part of it).
    Open,
    /// A human holds this node: regeneration proposals naming
    /// it or anything under it refuse whole.
    Frozen {
        /// Who froze it (an author id, `[a-z0-9-]`).
        by: String,
        /// Why custody was taken. May be empty; the `by` is
        /// the provenance that must exist.
        reason: String,
    },
}

/// One custody-map row: a node path and its state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyEntry {
    /// The node path this entry speaks for.
    pub node: String,
    /// The node's custody state.
    pub state: CustodyState,
}

/// The scene record's custody map: entries sorted by node,
/// at most one entry per node (validated).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustodyMap {
    /// Custody entries, in node order.
    pub entries: Vec<CustodyEntry>,
}

impl CustodyMap {
    /// Validates the map: bounds, node-id syntax, one entry
    /// per node, frozen entries carrying their provenance.
    pub fn validate(&self) -> GencondResult<()> {
        if self.entries.len() > CUSTODY_ENTRIES_MAX {
            return refuse(CODE_RANGE, "custody map exceeds the entry bound");
        }
        let mut nodes: Vec<&str> = Vec::new();
        for entry in &self.entries {
            check_node_id(&entry.node)?;
            if nodes.contains(&entry.node.as_str()) {
                return refuse(
                    CODE_CONFLICT,
                    format!("custody node '{}' is defined twice", entry.node),
                );
            }
            nodes.push(entry.node.as_str());
            if let CustodyState::Frozen { by, reason } = &entry.state {
                crate::schema::check_source_id(by).map_err(|_| {
                    crate::error::GencondError::new(
                        CODE_MALFORMED,
                        format!(
                            "custody entry '{}' was frozen by '{by}', which is not a valid author id",
                            entry.node
                        ),
                    )
                })?;
                if reason.len() > SCENE_TEXT_BYTES_MAX {
                    return refuse(CODE_RANGE, "custody reason exceeds the byte bound");
                }
            }
        }
        Ok(())
    }

    /// True when `target` is custody-frozen under the shared
    /// semantics: an exact match on a frozen entry, or a
    /// descendant of one.
    pub fn is_frozen(&self, target: &str) -> bool {
        self.entries.iter().any(|entry| {
            matches!(entry.state, CustodyState::Frozen { .. })
                && (target == entry.node
                    || target
                        .strip_prefix(entry.node.as_str())
                        .is_some_and(|rest| rest.starts_with('/')))
        })
    }

    /// The frozen members of `targets`, sorted and
    /// de-duplicated (the refusal's naming list).
    pub fn frozen_among(&self, targets: &[String]) -> Vec<String> {
        let mut frozen: Vec<String> = targets
            .iter()
            .filter(|target| self.is_frozen(target))
            .cloned()
            .collect();
        frozen.sort();
        frozen.dedup();
        frozen
    }

    /// The custody gate a regeneration proposal passes before
    /// compilation: any frozen target refuses the whole
    /// proposal with `TG-GENCOND-CUSTODY-FROZEN`, naming every
    /// frozen target. Unfrozen targets are not judged here.
    pub fn check_targets(&self, targets: &[String]) -> GencondResult<()> {
        for target in targets {
            check_node_id(target)?;
        }
        let frozen = self.frozen_among(targets);
        if frozen.is_empty() {
            return Ok(());
        }
        refuse(
            CODE_CUSTODY_FROZEN,
            format!(
                "regeneration proposal touches custody-frozen node(s): {}; a human holds \
                 custody of these nodes and their subtrees — the proposal refuses whole, \
                 nothing is applied",
                frozen.join(", ")
            ),
        )
    }
}

/// Node ids: `/`-joined segments, each `[a-z0-9-]` starting
/// with a letter or digit; bounded length, no empty segments.
pub fn check_node_id(id: &str) -> GencondResult<()> {
    let valid = !id.is_empty()
        && id.len() <= NODE_ID_BYTES_MAX
        && id.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        });
    if !valid {
        return refuse(
            CODE_MALFORMED,
            format!("node id '{id}' is not a valid node path ([a-z0-9-] segments joined by '/')"),
        );
    }
    Ok(())
}
