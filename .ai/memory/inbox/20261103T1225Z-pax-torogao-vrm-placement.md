# Placement follows direction of travel and lane, not code proximity

Date: 2026-11-03. Agent: pax (torogao-vrm build, D-748/D-749).
origin_lane: ntranch
content_label: sfw
evidence: `~/workspace/night-20261026/DECISIONS.md` D-748/D-749;
`~/workspace/torogao-landscape-20261103/SURVEY.md` §I-D, §6 item 8;
branch `pax/torogao-vrm` in primo
`~/.local/share/pax/torogao-vrm-20261103/torogao` (crate
`crates/torogao-vrm`); gate logs primo
`~/.local/share/pax/detach/vrm-stage-*.log`, `vrm-pkgtest.log`,
`vrm-proving-cli.log`.

## Lesson

When a capability already exists somewhere in the estate, the
new home for a second instance is decided by the direction the
artefacts travel and the lane the work ships in — not by how
close the existing code sits. lumen-factory already had a
973-line VRM module, but it faces the other way (VRoid INPUT:
foreign skeletons in, factory assumptions baked in) and lives
in the other lane's dependency graph. The Torogao exporter
(own cast out, torogao-humanoid/1 names, VRM permission meta
that must never default) was therefore built as a focused new
crate with NO dependency on the lumen module — the factory
file was read once as a contract reference (its parse-back /
byte-preservation test pattern is the part worth matching) and
the implementation written Torogao-native. The boundary
between the lanes stays what it was: file formats, not code.

## Corollary found while proving it

Byte-preservation proofs over serde_json values are only as
strong as the float path: this workspace's serde_json build
parses floats on a fast path that can land 1 ulp off, so a
parse/write/parse round-trip silently changed a pilot model's
node translation (3.259629011154175e-09 →
3.2596290111541752e-9) and the exporter's own census proof
caught it. Any crate claiming byte preservation over JSON
must enable serde_json's `float_roundtrip` feature, and must
compare numbers by value after canonicalizing decimal
spellings (zero-padded exponents differ lexically under
string-backed number builds). Proven by the D-654 Vivienne
proving case in `crates/torogao-vrm/tests/proving_vivienne.rs`
and the regression test `number_spellings_canonicalize_equal`.
