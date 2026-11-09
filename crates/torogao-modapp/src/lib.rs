//! torogao-modapp: the Modification App systems as Torogao
//! data + systems.
//!
//! Clean-room provenance. The systems were studied from the
//! estate's own copy of Modification App 0.12.3 (Godot 4.7.1)
//! under the fathom clean-room discipline: behavior and
//! tuning were measured, never copied. The record of record
//! is `games/modification-app-dynamics.md` in the Lane D
//! programme tree (`nsfw-remakes-20261108/`), extracted from
//! the build's plaintext tables and from 112 GDScript sources
//! decompiled out of its shipped bytecode (GDRE Tools,
//! 2026-11-08). No studied game's assets, art, text, names,
//! or code appear in this crate: what transfers is a phone-hub
//! navigation model over a level-gated story DAG, a per-node
//! state-table content resolver, a sessions-inside-a-day
//! clock, and the TP economy that binds them.
//!
//! The crate is a systems skeleton for the Torogao remake
//! programme (PIPELINE.md in `nsfw-remakes-20261108/`): pure,
//! deterministic functions over explicit inputs. No wall
//! clock, no RNG, no I/O beyond the RON loader in `dynamics`.
//! Content is estate-original placeholder data only; the
//! full content pass is an Owner-gated commission (S5).

#![forbid(unsafe_code)]

pub mod dynamics;
pub mod session;
pub mod world;

pub use dynamics::{
    ConsolePayout, DYNAMICS_RON, Dynamics, Item, ModappError, Node, ResolvedState, Reward,
    StateDelta, measured_dynamics,
};
pub use session::{Game, Menu, MenuOption, PlayOutcome, TeleportOutcome};
