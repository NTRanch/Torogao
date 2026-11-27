//! torogao-mobile-core — the shell-agnostic core of Torogao Mobile.
//!
//! Owns everything both shells share, exactly once:
//! - [`cast`]: the canonical cast table (Prima, Lana, Sirena), each
//!   record citing the lock/style sheet it was taken from.
//! - [`style`]: the canonical style record — the wai checkpoint,
//!   the estate's proven stills recipe (D-1525: wai is the single
//!   generator), and the bounded user-tunable [`style::RecipeSettings`].
//! - [`workflow`]: the pure builder from a cast member + prompt to
//!   the workflow document the facade accepts, plus its approval hash.
//! - [`screen`]: the content screen — the estate's absolute refusal
//!   rules over prompt text, enforced here and again seat-side.
//! - [`client`]: the blocking facade client (submit, poll, fetch,
//!   health, wake, interrupt) with the bounded contract.
//! - [`settings`]: persisted app state — the 18+ attestation, the
//!   saved seats, the recipe settings.
//! - [`gallery`]: the local gallery — generated images plus a JSON
//!   index, owned by whichever data directory the shell hands in.
//! - [`queue`]: the generation queue (status + cancel), where the
//!   age gate and the screen are enforced at enqueue and at run.
//! - [`session`]: the one object both shells drive — settings,
//!   gallery, and queue kept consistent.
//! - [`helper`]: the animated helpers' deterministic state machine
//!   (Prima: seat setup, Lana: generation, Sirena: gallery).
//!
//! The core has no UI assumptions and imports no shell crate; the
//! Slint phone shell and the GPUI desktop shell are thin consumers.
//! The seat behind the facade is torogao-gen (D-867/D-968); the
//! facade is engine-agnostic, so an on-device engine can register
//! behind the same contract without the core changing.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod cast;
pub mod client;
pub mod gallery;
pub mod helper;
pub mod queue;
pub mod screen;
pub mod session;
pub mod settings;
pub mod style;
pub mod workflow;

pub use cast::{CAST, Character, character};
pub use client::{GenError, SeatClient, SeatStatus};
pub use gallery::{Gallery, GalleryEntry};
pub use helper::{
    FlowId, GateContext, Helper, HelperArea, HelperEvent, HelperState, Pose, ProposedAction,
    helper_for,
};
pub use queue::{GenQueue, Job, JobContext, JobRequest, JobStatus};
pub use screen::{ScreenVerdict, screen_text};
pub use session::Session;
pub use settings::{SeatConfig, Settings};
pub use style::RecipeSettings;
pub use workflow::{build_workflow, compose_negative, compose_positive};
