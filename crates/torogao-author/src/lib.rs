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
//! ([`op::MANIFEST`]). The rest are refused by name.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod apply;
pub mod census;
pub mod codec;
pub mod decide;
pub mod diff;
pub mod import;
pub mod intake;
pub mod limits;
pub mod model;
pub mod op;
pub mod refusal;
pub mod ron;
pub mod session;
pub mod store;
pub mod validate;

pub use refusal::{Refusal, RefusalClass};
pub use session::{Answer, Status, parse_script, run_requests, run_verb, verbs_doc};
