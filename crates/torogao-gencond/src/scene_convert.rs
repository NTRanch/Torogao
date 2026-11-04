//! Typed scene record <-> RON tree conversion.
//!
//! The tree shapes are the record types' own names:
//! `SceneRecord(...)`, `RenderRecipe(...)`,
//! `MediumExtension(...)`, `Puppet2dRef(...)`,
//! `CustodyMap(...)`, `CustodyEntry(...)`, `Frozen(...)`,
//! `FrozenTrack(...)`, `Keyform(...)`, `PaletteSnap(...)`,
//! `PalettePair(...)`, `FreezeProvenance(...)`. Unit states
//! ride as bare identifiers (`NotYetPopulated`, `Open`,
//! `PinnedContextByteIdentical`), the tree's enum rule.
//!
//! Every field is mandatory in the tree: an absent field is
//! a refusal, never a silent fill — most pointedly the
//! medium blocks, where absence is exactly the ambiguity the
//! format exists to refuse (D-838).

use crate::custody::{CustodyEntry, CustodyMap, CustodyState};
use crate::error::GencondResult;
use crate::error::{CODE_MALFORMED, CODE_UNKNOWN_KIND, refuse};
use crate::freeze::{FrozenTrack, Keyform, PalettePair, PaletteSnap};
use crate::ron_tree::{Fields, Ron, open, record};
use crate::scene::{
    EpsilonOrigin, ExtensionState, FreezeOp, FreezeProvenance, MediumExtension, MediumKind,
    Puppet2dRef, RenderRecipe, RerenderGuarantee, SceneRecord,
};

impl SceneRecord {
    /// The record as a RON tree (emission order = schema
    /// order, alphabetical within each record).
    pub fn to_ron_tree(&self) -> Ron {
        record(
            "SceneRecord",
            vec![
                ("custody", self.custody.to_ron_tree()),
                (
                    "extensions",
                    Ron::List(
                        self.extensions
                            .iter()
                            .map(MediumExtension::to_ron_tree)
                            .collect(),
                    ),
                ),
                (
                    "frozen_tracks",
                    Ron::List(
                        self.frozen_tracks
                            .iter()
                            .map(FrozenTrack::to_ron_tree)
                            .collect(),
                    ),
                ),
                ("id", Ron::Str(self.id.clone())),
                (
                    "palette_snaps",
                    Ron::List(
                        self.palette_snaps
                            .iter()
                            .map(PaletteSnap::to_ron_tree)
                            .collect(),
                    ),
                ),
                (
                    "provenance",
                    Ron::List(
                        self.provenance
                            .iter()
                            .map(FreezeProvenance::to_ron_tree)
                            .collect(),
                    ),
                ),
                ("recipe", self.recipe.to_ron_tree()),
                ("schema", Ron::Int(i64::from(self.schema))),
            ],
        )
    }

    /// Parses a record from a RON tree, then validates it.
    /// Unknown medium kinds, unknown freeze operations, and
    /// unknown fields refuse by name (the load-time refusal
    /// contract).
    pub fn from_ron_tree(tree: Ron) -> GencondResult<SceneRecord> {
        let mut fields = open(tree, "SceneRecord")?;
        let custody = CustodyMap::from_ron_tree(fields.take("custody")?)?;
        let extension_values = fields.list("extensions")?;
        let track_values = fields.list("frozen_tracks")?;
        let id = fields.string("id")?;
        let snap_values = fields.list("palette_snaps")?;
        let provenance_values = fields.list("provenance")?;
        let recipe = RenderRecipe::from_ron_tree(fields.take("recipe")?)?;
        let schema = fields.int("schema")?;
        fields.done()?;
        let parsed = SceneRecord {
            schema: u32::try_from(schema).map_err(|_| {
                crate::error::GencondError::new(
                    CODE_MALFORMED,
                    "schema version is not a non-negative integer",
                )
            })?,
            id,
            recipe,
            extensions: extension_values
                .into_iter()
                .map(MediumExtension::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
            custody,
            provenance: provenance_values
                .into_iter()
                .map(FreezeProvenance::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
            frozen_tracks: track_values
                .into_iter()
                .map(FrozenTrack::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
            palette_snaps: snap_values
                .into_iter()
                .map(PaletteSnap::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
        };
        parsed.validate()?;
        Ok(parsed)
    }
}

impl RenderRecipe {
    fn to_ron_tree(&self) -> Ron {
        record(
            "RenderRecipe",
            vec![
                ("backend", Ron::Str(self.backend.clone())),
                (
                    "conditioning_sha256",
                    Ron::Str(self.conditioning_sha256.clone()),
                ),
                (
                    "execution_context",
                    Ron::Str(self.execution_context.clone()),
                ),
                ("graph_sha256", Ron::Str(self.graph_sha256.clone())),
                ("rerender", self.rerender.to_ron_tree()),
                ("seed", Ron::Int(self.seed)),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<RenderRecipe> {
        let mut fields = open(tree, "RenderRecipe")?;
        let backend = fields.string("backend")?;
        let conditioning_sha256 = fields.string("conditioning_sha256")?;
        let execution_context = fields.string("execution_context")?;
        let graph_sha256 = fields.string("graph_sha256")?;
        let rerender = RerenderGuarantee::from_ron_tree(fields.take("rerender")?)?;
        let seed = fields.int("seed")?;
        fields.done()?;
        Ok(RenderRecipe {
            conditioning_sha256,
            graph_sha256,
            seed,
            backend,
            execution_context,
            rerender,
        })
    }
}

impl RerenderGuarantee {
    fn to_ron_tree(&self) -> Ron {
        match self {
            RerenderGuarantee::PinnedContextByteIdentical => {
                Ron::Ident("PinnedContextByteIdentical".to_string())
            }
            RerenderGuarantee::BestEffort { reason } => {
                record("BestEffort", vec![("reason", Ron::Str(reason.clone()))])
            }
        }
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<RerenderGuarantee> {
        match &tree {
            Ron::Ident(name) if name == "PinnedContextByteIdentical" => {
                Ok(RerenderGuarantee::PinnedContextByteIdentical)
            }
            Ron::Ident(name) => refuse(
                CODE_UNKNOWN_KIND,
                format!("re-render guarantee '{name}' is not known"),
            ),
            Ron::Struct(name, _) if name == "BestEffort" => {
                let mut fields = open(tree, "BestEffort")?;
                let reason = fields.string("reason")?;
                fields.done()?;
                Ok(RerenderGuarantee::BestEffort { reason })
            }
            Ron::Struct(name, _) => refuse(
                CODE_UNKNOWN_KIND,
                format!("re-render guarantee kind '{name}' is not known"),
            ),
            _ => refuse(CODE_MALFORMED, "re-render guarantee is not a variant"),
        }
    }
}

impl MediumExtension {
    fn to_ron_tree(&self) -> Ron {
        record(
            "MediumExtension",
            vec![
                ("kind", Ron::Ident(self.kind.ident().to_string())),
                ("state", self.state.to_ron_tree()),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<MediumExtension> {
        let mut fields = open(tree, "MediumExtension")?;
        let kind = MediumKind::from_ident(&fields.ident("kind")?)?;
        let state = ExtensionState::from_ron_tree(fields.take("state")?)?;
        fields.done()?;
        Ok(MediumExtension { kind, state })
    }
}

impl ExtensionState {
    fn to_ron_tree(&self) -> Ron {
        match self {
            ExtensionState::NotYetPopulated => Ron::Ident("NotYetPopulated".to_string()),
            ExtensionState::Puppet2d(reference) => reference.to_ron_tree(),
        }
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<ExtensionState> {
        match &tree {
            Ron::Ident(name) if name == "NotYetPopulated" => Ok(ExtensionState::NotYetPopulated),
            Ron::Ident(name) => refuse(
                CODE_UNKNOWN_KIND,
                format!("medium extension state '{name}' is not known"),
            ),
            Ron::Struct(name, _) if name == "Puppet2dRef" => {
                Ok(ExtensionState::Puppet2d(Puppet2dRef::from_ron_tree(tree)?))
            }
            Ron::Struct(name, _) => refuse(
                CODE_UNKNOWN_KIND,
                format!(
                    "medium extension state kind '{name}' is not known in this schema version; \
                     kinds other than puppet2d have no populated state yet"
                ),
            ),
            _ => refuse(CODE_MALFORMED, "medium extension state is not a variant"),
        }
    }
}

impl Puppet2dRef {
    fn to_ron_tree(&self) -> Ron {
        record(
            "Puppet2dRef",
            vec![
                ("format", Ron::Str(self.format.clone())),
                ("format_version", Ron::Int(i64::from(self.format_version))),
                ("model_sha256", Ron::Str(self.model_sha256.clone())),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<Puppet2dRef> {
        let mut fields = open(tree, "Puppet2dRef")?;
        let format = fields.string("format")?;
        let format_version = fields.int("format_version")?;
        let model_sha256 = fields.string("model_sha256")?;
        fields.done()?;
        Ok(Puppet2dRef {
            format,
            format_version: u32::try_from(format_version).map_err(|_| {
                crate::error::GencondError::new(
                    CODE_MALFORMED,
                    "format_version is not a non-negative integer",
                )
            })?,
            model_sha256,
        })
    }
}

impl CustodyMap {
    fn to_ron_tree(&self) -> Ron {
        record(
            "CustodyMap",
            vec![(
                "entries",
                Ron::List(self.entries.iter().map(CustodyEntry::to_ron_tree).collect()),
            )],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<CustodyMap> {
        let mut fields = open(tree, "CustodyMap")?;
        let values = fields.list("entries")?;
        fields.done()?;
        Ok(CustodyMap {
            entries: values
                .into_iter()
                .map(CustodyEntry::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
        })
    }
}

impl CustodyEntry {
    fn to_ron_tree(&self) -> Ron {
        record(
            "CustodyEntry",
            vec![
                ("node", Ron::Str(self.node.clone())),
                ("state", self.state.to_ron_tree()),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<CustodyEntry> {
        let mut fields = open(tree, "CustodyEntry")?;
        let node = fields.string("node")?;
        let state = CustodyState::from_ron_tree(fields.take("state")?)?;
        fields.done()?;
        Ok(CustodyEntry { node, state })
    }
}

impl CustodyState {
    fn to_ron_tree(&self) -> Ron {
        match self {
            CustodyState::Open => Ron::Ident("Open".to_string()),
            CustodyState::Frozen { by, reason } => record(
                "Frozen",
                vec![
                    ("by", Ron::Str(by.clone())),
                    ("reason", Ron::Str(reason.clone())),
                ],
            ),
        }
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<CustodyState> {
        match &tree {
            Ron::Ident(name) if name == "Open" => Ok(CustodyState::Open),
            Ron::Ident(name) => refuse(
                CODE_UNKNOWN_KIND,
                format!("custody state '{name}' is not known"),
            ),
            Ron::Struct(name, _) if name == "Frozen" => {
                let mut fields: Fields = open(tree, "Frozen")?;
                let by = fields.string("by")?;
                let reason = fields.string("reason")?;
                fields.done()?;
                Ok(CustodyState::Frozen { by, reason })
            }
            Ron::Struct(name, _) => refuse(
                CODE_UNKNOWN_KIND,
                format!("custody state kind '{name}' is not known"),
            ),
            _ => refuse(CODE_MALFORMED, "custody state is not a variant"),
        }
    }
}

impl FreezeProvenance {
    fn to_ron_tree(&self) -> Ron {
        record(
            "FreezeProvenance",
            vec![
                (
                    "epsilon",
                    Ron::Opt(self.epsilon.map(|value| Box::new(Ron::Float(value)))),
                ),
                (
                    "epsilon_origin",
                    Ron::Opt(
                        self.epsilon_origin
                            .map(|origin| Box::new(Ron::Ident(origin.ident().to_string()))),
                    ),
                ),
                ("frozen_sha256", Ron::Str(self.frozen_sha256.clone())),
                ("op", Ron::Ident(self.op.ident().to_string())),
                ("source_sha256", Ron::Str(self.source_sha256.clone())),
                (
                    "tolerance",
                    Ron::Opt(
                        self.tolerance
                            .map(|value| Box::new(Ron::Int(i64::from(value)))),
                    ),
                ),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<FreezeProvenance> {
        let mut fields = open(tree, "FreezeProvenance")?;
        let epsilon = take_opt_float(&mut fields, "epsilon")?;
        let epsilon_origin = match take_opt(&mut fields, "epsilon_origin")? {
            Some(Ron::Ident(name)) => Some(EpsilonOrigin::from_ident(&name)?),
            Some(_) => {
                return refuse(
                    CODE_MALFORMED,
                    "field 'epsilon_origin' is not a variant name",
                );
            }
            None => None,
        };
        let frozen_sha256 = fields.string("frozen_sha256")?;
        let op = FreezeOp::from_ident(&fields.ident("op")?)?;
        let source_sha256 = fields.string("source_sha256")?;
        let tolerance = take_opt_int(&mut fields, "tolerance")?;
        fields.done()?;
        Ok(FreezeProvenance {
            op,
            source_sha256,
            frozen_sha256,
            epsilon,
            epsilon_origin,
            tolerance: tolerance.map(|value| u32::try_from(value).unwrap_or(u32::MAX)),
        })
    }
}

impl FrozenTrack {
    fn to_ron_tree(&self) -> Ron {
        record(
            "FrozenTrack",
            vec![
                (
                    "keyforms",
                    Ron::List(
                        self.keyforms
                            .iter()
                            .copied()
                            .map(Keyform::to_ron_tree)
                            .collect(),
                    ),
                ),
                ("max_deviation", Ron::Float(self.max_deviation)),
                ("parameter", Ron::Str(self.parameter.clone())),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<FrozenTrack> {
        let mut fields = open(tree, "FrozenTrack")?;
        let values = fields.list("keyforms")?;
        let max_deviation = fields.float("max_deviation")?;
        let parameter = fields.string("parameter")?;
        fields.done()?;
        Ok(FrozenTrack {
            parameter,
            keyforms: values
                .into_iter()
                .map(Keyform::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
            max_deviation,
        })
    }
}

impl Keyform {
    fn to_ron_tree(self) -> Ron {
        record(
            "Keyform",
            vec![
                ("tangent_in", Ron::Float(self.tangent_in)),
                ("tangent_out", Ron::Float(self.tangent_out)),
                ("time", Ron::Float(self.time)),
                ("value", Ron::Float(self.value)),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<Keyform> {
        let mut fields = open(tree, "Keyform")?;
        let tangent_in = fields.float("tangent_in")?;
        let tangent_out = fields.float("tangent_out")?;
        let time = fields.float("time")?;
        let value = fields.float("value")?;
        fields.done()?;
        Ok(Keyform {
            time,
            value,
            tangent_in,
            tangent_out,
        })
    }
}

impl PaletteSnap {
    fn to_ron_tree(&self) -> Ron {
        record(
            "PaletteSnap",
            vec![
                (
                    "pairs",
                    Ron::List(self.pairs.iter().map(PalettePair::to_ron_tree).collect()),
                ),
                ("palette_sha256", Ron::Str(self.palette_sha256.clone())),
                ("tolerance", Ron::Int(i64::from(self.tolerance))),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<PaletteSnap> {
        let mut fields = open(tree, "PaletteSnap")?;
        let values = fields.list("pairs")?;
        let palette_sha256 = fields.string("palette_sha256")?;
        let tolerance = fields.int("tolerance")?;
        fields.done()?;
        Ok(PaletteSnap {
            palette_sha256,
            tolerance: u32::try_from(tolerance).map_err(|_| {
                crate::error::GencondError::new(
                    CODE_MALFORMED,
                    "tolerance is not a non-negative integer",
                )
            })?,
            pairs: values
                .into_iter()
                .map(PalettePair::from_ron_tree)
                .collect::<GencondResult<Vec<_>>>()?,
        })
    }
}

impl PalettePair {
    fn to_ron_tree(&self) -> Ron {
        record(
            "PalettePair",
            vec![
                ("snapped", Ron::Str(self.snapped.clone())),
                ("source", Ron::Str(self.source.clone())),
            ],
        )
    }

    fn from_ron_tree(tree: Ron) -> GencondResult<PalettePair> {
        let mut fields = open(tree, "PalettePair")?;
        let snapped = fields.string("snapped")?;
        let source = fields.string("source")?;
        fields.done()?;
        Ok(PalettePair { source, snapped })
    }
}

/// Takes an `Option` field's inner value; the field itself is
/// mandatory in the tree (present as `None` or `Some(...)`) —
/// absence is a refusal, the record's no-silent-fill rule.
fn take_opt(fields: &mut Fields, key: &str) -> GencondResult<Option<Ron>> {
    match fields.take(key)? {
        Ron::Opt(inner) => Ok(inner.map(|value| *value)),
        _ => refuse(CODE_MALFORMED, format!("field '{key}' is not an option")),
    }
}

fn take_opt_float(fields: &mut Fields, key: &str) -> GencondResult<Option<f64>> {
    match take_opt(fields, key)? {
        Some(Ron::Float(value)) => Ok(Some(value)),
        Some(Ron::Int(value)) => Ok(Some(value as f64)),
        Some(_) => refuse(CODE_MALFORMED, format!("field '{key}' is not a float")),
        None => Ok(None),
    }
}

fn take_opt_int(fields: &mut Fields, key: &str) -> GencondResult<Option<i64>> {
    match take_opt(fields, key)? {
        Some(Ron::Int(value)) => Ok(Some(value)),
        Some(_) => refuse(CODE_MALFORMED, format!("field '{key}' is not an integer")),
        None => Ok(None),
    }
}
