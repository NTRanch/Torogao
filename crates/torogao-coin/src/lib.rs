//! torogao-coin: the coin dynamic as Torogao data + systems.
//!
//! Clean-room provenance. The dynamic was studied from the
//! estate's own copy of RJ318915 ("Changing Her Personality With
//! My Magic Coin", Unity Mono) under the fathom clean-room
//! discipline: behavior and tuning were measured, never copied.
//! The record of record is `COIN-DYNAMIC.md` (D-1045) in the TF
//! Jam programme tree; the tuning constants in
//! `data/coin-dynamics.ron` are the static-IL measurements
//! gate-verified 2026-11-08 (see that file's header). No studied
//! game's assets, art, text, names, or code appear in this crate:
//! what transfers is one timed amplitude verb, a three-stat
//! preparation economy, and level-scaled resistance.
//!
//! The crate is a systems skeleton for the Torogao remake
//! programme (PIPELINE.md in `nsfw-remakes-20261108/`): pure,
//! deterministic functions over explicit inputs. No wall clock,
//! no RNG, no I/O beyond the RON loader in `dynamics`.

#![forbid(unsafe_code)]

pub mod dynamics;
pub mod session;
pub mod swing;

pub use dynamics::{CoinError, DYNAMICS_RON, Dynamics, PrepAction, StatTriple, measured_dynamics};
pub use session::{AttemptOutcome, Session};
pub use swing::SwingAccumulator;
