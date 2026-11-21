# torogao-scene

Deterministic two-bone reach and surface anchors plus the NTRanch scene
instance module. Unsafe code is forbidden.

`schema` re-exports `lumen-scene-script` through the existing machine-local
path dependency at
`/home/phaedrus/.local/share/pax/lumen-scene-mvp-wt-20261112/crates/lumen-scene-script`.
Reviewed Lane A revision: `c7f04df` (NSFW label vocabulary added). Cargo path
dependencies do not enforce that revision; this seam is not portable yet.

`instance::Instance::seal` accepts a shared scene script, channel ranges, and
explicit scene/cast metadata. It admits only NSFW provenance from `ntranch`
with nonblank evidence, one of four candidate styles, and exactly the scene's
cast drawn from prima, alma, lux, piper, sloane and yui. All three consent
assertions must be explicitly true on the scene and each cast entry; a label
is never consent evidence. Duplicate references are rejected, never merged.
Missing labels/evidence fail shared typed deserialization; false or missing
consent assertions fail admission. Typed ingress is bounded with the shared
canonical byte cap before local traversal.

The resolver hashes compile-time snapshots of the existing local
`docs/cast-records/<id>.md` files with the shared SHA-256 helper and includes
their paths in provenance evidence. These snapshots update on rebuild, not
at runtime. Assertions are supplied by the caller; this module validates
explicit metadata, not the truth of human authorization.

Shared `freeze` seals the script and resolved registry first. A versioned,
private, owned instance envelope then binds that Frozen value, scene/cast
metadata, and the candidate style record using shared canonical serialization
and SHA-256. Read-only accessors and no deserialization/mutation API preserve
admitted instances. `verify` checks integrity; the identity is not a signature.
Changing scene metadata evidence changes the envelope hash even when the
shared frozen scene is byte-identical. No rendering is performed here.

Instance records remain in this lane; no local cast/style data or NSFW instance
is written into the shared schema crate or a qompassai surface.
