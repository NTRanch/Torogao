//! The crate's one typed error. Every refusal this crate makes
//! is a named variant: no silent defaults, no stringly failures,
//! and no panics on untrusted input. Display strings are the
//! user-facing wording (the CLI prints them verbatim), so each
//! one names the field, bone, or value that failed.

use std::fmt;

/// A torogao-vrm failure, named by cause.
#[derive(Clone, Debug, PartialEq)]
pub enum VrmError {
    /// An export self-check or a bounded read disagreed with
    /// its input.
    CensusMismatch {
        /// What disagreed.
        detail: String,
    },
    /// A spring joint names a bone in the torogao-humanoid/1
    /// vocabulary that the model does not carry.
    DanglingSpringJoint {
        /// The bone the joint names.
        bone: String,
        /// The chain that carries the joint.
        chain: String,
    },
    /// Two source nodes resolve to the same torogao bone.
    DuplicateBone {
        /// The torogao bone claimed twice.
        torogao: String,
    },
    /// The expression map's format token is not
    /// `torogao-expression-map-3d/1`.
    ExpressionMapWrongFormat {
        /// The format token found.
        found: String,
    },
    /// A governed morph weight is non-finite or outside
    /// 0.0..=1.0.
    ExpressionWeightOutOfRange {
        /// The morph channel carrying the weight.
        channel: String,
        /// The expression carrying the weight.
        expression: String,
        /// The offending weight.
        weight: f64,
    },
    /// The input does not start with the GLB magic.
    GlbBadMagic,
    /// The GLB's JSON chunk does not parse.
    GlbJsonParse {
        /// The parse failure.
        detail: String,
    },
    /// The GLB carries no JSON chunk.
    GlbMissingJsonChunk,
    /// The input is shorter than the GLB header.
    GlbTooShort {
        /// The input length in bytes.
        actual_bytes: usize,
    },
    /// The GLB version is not 2.
    GlbUnsupportedVersion {
        /// The version found.
        found: u32,
    },
    /// A file's meta disagrees with the profile that should
    /// have written it.
    MetaMismatch {
        /// What the file carries.
        detail: String,
        /// What the profile expects.
        expected: String,
        /// The meta field (VRM JSON spelling).
        field: String,
    },
    /// Export was attempted with no profile at all.
    MissingProfile,
    /// The source model lacks required humanoid bones.
    MissingRequiredBones {
        /// The absent bones, VRM names, sorted.
        bones: Vec<String>,
    },
    /// A morph channel is declared by two meshes, so a bind
    /// cannot address it unambiguously.
    MorphTargetAmbiguous {
        /// The ambiguous channel.
        channel: String,
    },
    /// A governed expression references a morph channel the
    /// model does not carry.
    MorphTargetAbsent {
        /// The absent channel.
        channel: String,
    },
    /// A required profile text field is present but empty.
    ProfileEmpty {
        /// The empty field (RON spelling).
        field: String,
    },
    /// The profile document does not parse as RON, or exceeds
    /// its byte bound.
    ProfileParse {
        /// The parse failure.
        detail: String,
    },
    /// The profile leaves required fields unset; the refusal
    /// names every one of them.
    ProfileUnset {
        /// The unset fields (RON spellings), sorted.
        fields: Vec<String>,
    },
    /// The profile's `id` is not `vrm-profile/1`.
    ProfileWrongId {
        /// The id found.
        found: String,
    },
    /// A spring chain's joint has no ancestor earlier in the
    /// chain, violating the VRMC_springBone chain rule.
    SpringChainOrder {
        /// The joint's node name as written.
        bone: String,
        /// The chain carrying the joint.
        chain: String,
    },
    /// The spring document does not parse as RON, or exceeds
    /// its byte bound, or leaves a required joint field unset.
    SpringParse {
        /// The parse failure.
        detail: String,
    },
    /// A spring numeric parameter is non-finite or outside the
    /// range the VRM spec allows.
    SpringValueOutOfRange {
        /// The parameter (RON spelling).
        field: String,
        /// The joint or collider carrying it.
        owner: String,
        /// The offending value.
        value: f64,
    },
    /// A node name is spelled in VRM bone space, which source
    /// documents never use.
    UnknownBoneName {
        /// The name as written.
        name: String,
    },
    /// A node name matches no node in the model and is not in
    /// the bone vocabulary.
    UnresolvedNode {
        /// The name as written.
        name: String,
    },
    /// Read-back validation accumulated problems; each is
    /// named in the list.
    ValidationFailed {
        /// The problems found, in check order.
        problems: Vec<String>,
    },
}

/// The result type every fallible operation in the crate
/// returns.
pub type VrmResult<T> = Result<T, VrmError>;

impl fmt::Display for VrmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VrmError::CensusMismatch { detail } => {
                write!(f, "census mismatch: {detail}")
            }
            VrmError::DanglingSpringJoint { bone, chain } => write!(
                f,
                "spring chain `{chain}` joint names bone `{bone}`, which the model does not carry"
            ),
            VrmError::DuplicateBone { torogao } => {
                write!(f, "two source nodes resolve to torogao bone `{torogao}`")
            }
            VrmError::ExpressionMapWrongFormat { found } => write!(
                f,
                "expression map format is `{found}`, not `torogao-expression-map-3d/1`"
            ),
            VrmError::ExpressionWeightOutOfRange {
                channel,
                expression,
                weight,
            } => write!(
                f,
                "expression `{expression}` weight for `{channel}` is {weight}, outside 0.0..=1.0"
            ),
            VrmError::GlbBadMagic => write!(f, "input does not start with the GLB magic"),
            VrmError::GlbJsonParse { detail } => write!(f, "GLB JSON does not parse: {detail}"),
            VrmError::GlbMissingJsonChunk => write!(f, "GLB carries no JSON chunk"),
            VrmError::GlbTooShort { actual_bytes } => {
                write!(
                    f,
                    "input is {actual_bytes} bytes, shorter than the GLB header"
                )
            }
            VrmError::GlbUnsupportedVersion { found } => {
                write!(f, "GLB version is {found}, not 2")
            }
            VrmError::MetaMismatch {
                detail,
                expected,
                field,
            } => write!(
                f,
                "meta field `{field}` disagrees with the profile: expected {expected}; {detail}"
            ),
            VrmError::MissingProfile => write!(
                f,
                "export refused: no vrm-profile/1 supplied (meta permissions are never defaulted)"
            ),
            VrmError::MissingRequiredBones { bones } => write!(
                f,
                "source model is missing required humanoid bones: {}",
                bones.join(", ")
            ),
            VrmError::MorphTargetAmbiguous { channel } => write!(
                f,
                "morph channel `{channel}` is declared by more than one mesh"
            ),
            VrmError::MorphTargetAbsent { channel } => write!(
                f,
                "morph channel `{channel}` is not among the model's morph targets"
            ),
            VrmError::ProfileEmpty { field } => {
                write!(f, "profile field `{field}` is present but empty")
            }
            VrmError::ProfileParse { detail } => {
                write!(f, "profile does not parse as vrm-profile/1: {detail}")
            }
            VrmError::ProfileUnset { fields } => write!(
                f,
                "export refused: profile leaves permission fields unset: {}",
                fields.join(", ")
            ),
            VrmError::ProfileWrongId { found } => {
                write!(f, "profile id is `{found}`, not `vrm-profile/1`")
            }
            VrmError::SpringChainOrder { bone, chain } => write!(
                f,
                "spring chain `{chain}` joint `{bone}` has no ancestor earlier in the chain"
            ),
            VrmError::SpringParse { detail } => {
                write!(
                    f,
                    "spring record does not parse as torogao-vrm-spring/1: {detail}"
                )
            }
            VrmError::SpringValueOutOfRange {
                field,
                owner,
                value,
            } => write!(
                f,
                "spring {owner} field `{field}` is {value}, outside the range the VRM spec allows"
            ),
            VrmError::UnknownBoneName { name } => write!(
                f,
                "node name `{name}` is a VRM-space bone name; source documents use torogao-humanoid/1 or source-rig names"
            ),
            VrmError::UnresolvedNode { name } => {
                write!(f, "node name `{name}` matches no node in the model")
            }
            VrmError::ValidationFailed { problems } => {
                write!(f, "validation failed: {}", problems.join("; "))
            }
        }
    }
}

impl std::error::Error for VrmError {}
