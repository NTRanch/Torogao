//! AUTH-G: the game-authoring layer of `torogao-author`
//! (Amendment 2). A `game` module inside this crate, beside the
//! rig modules: it depends on the rig modules' id space (a
//! dialogue line may hook a rig expression), never the reverse.
//!
//! One pipeline, shared with the rig verbs: every mutating game
//! verb runs load → `decide` → apply → canonical write and
//! appends exactly one op-log entry; `game_simulate` is
//! read-only and appends none.
//!
//! Delivery, per the Phase 2 plan: slice P2-1 lands the record
//! format, the `content.ron` sidecar, and the dialogue emission
//! discipline; P2-2 the state lifetime primitive and the
//! interpreter; P2-3 the deferral semantics on the same tick
//! contract (one format bump for the pair, already stamped in
//! [`model::GAME_FORMAT_VERSION`], with the v1 → v2 migration a
//! named, tested step). Slice P2-7 adds the message-thread
//! family (D-461): thread records and their verbs, plus the
//! interpreter's interlude semantics — instant batch delivery
//! at scene entry, an authored reply delay consumed on the
//! step clock, and thread choices through the ordinary choice
//! machinery. Slice P2-8 adds the direct-manipulation
//! interaction family: `interaction_define` authors a scene's
//! (tool × body-node) grid — response effect sets and camera
//! presets as records — and the interpreter's `interact` input
//! resolves a recorded cell or lands as a typed no-op. The
//! family's evidence is the Doujin Fever study's census
//! (cursor scripts, GUINode sprites, the TabAngle camera
//! family): census-grade, inventoried not measured, and the
//! records claim expressibility, never behavioural parity.
//! Slice P2-9 adds the fail-ceiling disturbance channel from
//! the same study (`shaking_level` / `girl_wakeUp`): a gauge
//! channel kind inside the tick — gains from authored input
//! events, per-tick decay, and exactly one adverse
//! scene-ending transaction on crossing its ceiling. Slice
//! P2-12 adds the unseen-content marker lifecycle (the
//! Changr study's marker method family): `marker_define`
//! records keyed by content category and save partition, the
//! `mark_new` effect atom, and start/finish clears bound to
//! the scene lifecycle — the records are the format anchor;
//! the app's browser surfacing is the app's half of the
//! slice.

pub mod apply;
pub mod codec;
pub mod decide;
pub mod expr;
pub mod model;
pub mod op;
pub mod save;
pub mod simulate;
pub mod state;
#[cfg(test)]
mod tests;
pub mod visual;

pub use model::Game;
pub use op::{GAME_MANIFEST, GameOp, GameSlice, GameVerbSpec, game_verb_spec};
