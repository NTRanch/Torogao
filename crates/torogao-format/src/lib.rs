//! Torogao's format layer.
//!
//! This crate owns the INP1 container and the vendor-extension
//! records: parsing untrusted files with bounded work, and
//! byte-stable serialization (the same record always
//! serializes to the same bytes, so exports can be compared
//! byte for byte). It also owns the rig-model format — the
//! `.model.json` document the rig exporter writes and the
//! `rig-check` validator reads — including the one match site
//! where the model's format token is classified under ruling
//! RP1-D1 (see [`validate::classify_format_token`]).
//!
//! Port phase: RP1 brings the container reader and writer,
//! the payload record layer, the JSON byte form, and the
//! model validator; RP2 completes the record set the engine
//! consumes.

#![forbid(unsafe_code)]

pub mod container;
pub mod error;
pub mod jsonfmt;
pub mod limits;
pub mod model;
pub mod parse_error;
pub mod payload;
pub mod raw;
pub mod validate;
pub mod value;

pub use container::{
    ContainerScan, ExtSection, InpFile, TextureSlot, build_inp, read_inp, scan_container,
    write_with_ext,
};
pub use error::{FormatResult, TorogaoError};
pub use model::{FormatWarning, LEGACY_MODEL_FORMAT_TOKEN, MODEL_FORMAT_TOKEN, Model, ParsedModel};
pub use parse_error::ParseError;
pub use payload::{NodeKind, NodeRecord, PayloadDoc, decode_node_record, decode_payload};
pub use validate::parse_model;

#[cfg(test)]
mod tests {
    /// The skeleton's one verifiable fact: the workspace member
    /// builds under its contracted name.
    #[test]
    fn workspace_member_builds_under_its_contracted_name() {
        assert_eq!(env!("CARGO_PKG_NAME"), "torogao-format");
    }
}
