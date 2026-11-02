//! The Torogao authoring core.
//!
//! A project is text: `project.ron` (one canonical serializer,
//! floats through `torogao-format`'s canonical writer) plus
//! `ops.jsonl` (an append-only log of every admitted op and every
//! denial). Every mutating op runs one pipeline:
//! load → [`decide::decide`] → [`apply::step`] → canonical write.
//! A denial is final: no verb, input, or face carries an override,
//! force, or skip-checks parameter.
//!
//! Faces (the `torogao author` CLI now, MCP in slice A4) call
//! [`session::run_verb`] / [`session::run_requests`] and hold no
//! validation logic of their own.
//!
//! Slice A1 implements the project, art intake, parts & meshes,
//! deformers, parameters (without `param_link_define`), and
//! expressions groups: 30 of the manifest's 47 verbs
//! ([`op::MANIFEST`]). The G-10 completion adds nine more — the
//! A2 physics records ([`physics`]), the A3 motion and audio
//! records ([`motion`], [`audio`]), and `export_manifest`
//! ([`export`]) — for 39 of 47; the remaining eight are
//! descoped by recorded ruling ([`op::DESCOPED`]), so every
//! manifest verb is implemented here or refused by name with
//! its ruling and its capability's home.
//!
//! Slice G1 (Amendment 2, AUTH-G) adds the game family on the
//! same pipeline: the five game-core verbs of
//! [`game::GAME_MANIFEST`] (22 verbs), the §G3.1 tick skeleton
//! with its per-tick hash chain, and the `TG-GAME-*` refusal
//! classes. The convergence step adds the six §G2.4/§G2.6 flow
//! verbs (`choice_define`, `ending_define`,
//! `gallery_entry_define`, `schedule_define`, `scene_bind_rig`,
//! `scene_exit_define`) for 11 of 22 live; the remaining
//! eleven are G2's and are refused by name.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod apply;
pub mod audio;
pub mod census;
pub mod codec;
pub mod decide;
pub mod diff;
pub mod export;
pub mod game;
pub mod import;
pub mod intake;
pub mod limits;
pub mod model;
pub mod motion;
pub mod op;
pub mod physics;
pub mod refusal;
pub mod ron;
pub mod session;
pub mod store;
pub mod validate;

pub use refusal::{Refusal, RefusalClass};
pub use session::{Answer, Status, parse_script, run_requests, run_verb, verbs_doc};
