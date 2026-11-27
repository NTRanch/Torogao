# Versioned format registry — torogao-format

This file is the registry of Torogao's named, versioned formats:
the formats whose byte meaning must never drift silently, because
saves, runs, and replays are exchanged between builds, machines,
and architectures under the runtime's determinism contract
(runtime plan, 2026-11-26, §2). A format's entry here is its
specification of record. Changing a format's meaning requires a
new version name, a migration story, and a registry amendment —
never an in-place edit to an existing entry.

Companion discipline: golden artifacts (documents, vectors) pin
each format in the owning crate's test suite, so drift fails a
gate rather than corrupting a replay.

## estate-rng/1 — the one randomness stream format

Status: **SOLE stream format of the runtime**, declared
2026-11-26 (Owner decision, runtime plan Decision 3; closes
AUTH-G convergence conflict C-5 — see the retirement record
below).

Owner of record: `torogao-author` (`src/game/model.rs`:
`RNG_ALGORITHM`; `src/game/simulate.rs`: `stream_seed`,
`rng_next`). A project names the algorithm per stream
(`RngStreamDef.algorithm`); an unknown algorithm name refuses at
authoring time.

### Specification

- Algorithm: **SplitMix64** (integer-only; no floating point
  anywhere in the format, by the Tier 1 rule).
- Seed derivation: a stream's genesis state word is
  `u64::from_str_radix(&hex[..16], 16)` where `hex` is the
  lowercase hex SHA-256 digest of the UTF-8 string
  `estate-rng/1:<run_seed>:<stream_id>` — `run_seed` an `i64`
  in its ordinary decimal rendering (a negative seed renders
  with its minus sign), `stream_id` the stream's stable id.
  (The digest's first 16 hex digits are the first eight digest
  bytes read big-endian; the code is the contract where any
  prose summary and the code disagree.)
- Genesis: at run start, every declared stream's state word is
  set to its derived seed (`simulate::genesis`).
- Step: one draw advances the stream's word by the golden gamma
  `0x9E3779B97F4A7C15` (wrapping), then mixes the new word:
  `z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9`;
  `z = (z ^ (z >> 27)) * 0x94D049BB133111EB`;
  output `z ^ (z >> 31)` (all arithmetic wrapping, u64).
- Consumption: a draw consumes exactly one step regardless of
  the weights it is drawn against, so stream consumption is
  weight-independent. A draw with no eligible outcome consumes
  no step (there was no draw).
- An undeclared stream reached at draw time starts from word 0
  (the `or_insert(0)` path) — pinned by the raw-from-zero
  golden vector.

### Golden vectors

Executable form: `torogao-author` `src/game/simulate.rs`,
`rng_golden_tests` (runtime Rung R0, 2026-11-26) — genesis words
for five (run_seed, stream_id) pairs, the first eight draws of
four seeded streams, and the bare SplitMix64 outputs from word
0 (whose first value, `0xE220A8397B1DCDAF`, is the algorithm's
published first output for state 0). The vectors were computed
independently of the implementation, from this specification.

### Successors

A future `estate-rng/2` is a deliberate, gated act: new entry
here, new golden vectors, and a stated migration for saves and
runs recorded under `/1`. The `/1` meaning above never changes.

## torogao-rng/1 — RETIRED BY RECORD (never shipped)

Status: **retired 2026-11-26** (Owner decision, runtime plan
Decision 3), closing AUTH-G convergence conflict **C-5**.

The AUTH-G G-01 branch lineage implemented a rival stream
format under this name, with seed/step semantics incompatible
with `estate-rng/1`: saves and runs are not cross-compatible
between the two. The trunk's implementation — `estate-rng/1`,
above — governed at convergence and is what main has always
carried; no shipped title, save, or run uses `torogao-rng/1`,
and none is migrated.

Retirement terms:

- The name `torogao-rng/1` is **reserved**: it must never be
  reused for any other format, so an old artefact naming it can
  never be silently reinterpreted.
- No code on main implements or names it (verified by tree-wide
  search at retirement, runtime Rung R0, 2026-11-26).
- An artefact that names `torogao-rng/1` as its stream
  algorithm refuses at authoring time, as any unknown algorithm
  does — it is not converted.

## torogao-runtime-corpus/1 — the replay corpus document

Status: **adopted 2026-11-26** (runtime plan Rung R2), the
corpus format of the cross-architecture twin gate (plan
SS2.4/SS4.1).

A corpus is one hosted core's fixed command script, carried
as a file so the one cross-title gate harness consumes every
title's replay corpus in the same shape (plan SS1.2). The
document is canonical JSON, written through this crate's
canonical JSON writer (`jsonfmt::to_d_json`), so the same
corpus is byte-identical on every target:

- `format`: the string `torogao-runtime-corpus/1`.
- `core_id`: the `GameCore::core_id` the corpus drives; a
  harness refuses a corpus naming a different core.
- `save_at`: the step index at which the cross-target save
  gate checkpoints (the save is taken after this many steps).
- `steps`: one command list per step, in the runtime's
  unified command vocabulary (the `Command` enum's serde
  form). Commands are integer-only by construction (pointer
  drags and pad axes travel in milli-unit buckets), so a
  corpus carries no float and means the same run everywhere.

Implementation: `torogao-runtime`'s `corpus` module
(`CorpusDoc`), with the two R1 witness corpora committed in
that form under `crates/torogao-runtime/corpus/`. A future
`/2` is a deliberate, gated act, like every entry here.

## torogao-motion-clip/2 — the hand-authored motion clip

Status: **adopted 2026-11-27** (hand-animation programme Rung
H0; DESIGN.md sections 2–3, 7). This is the authoring format
of the D-1397 pipeline: a person (or a Seahawk-driven agent
session) poses a character and sets keys; the keys are the
content of a frozen, hashed clip the timeline compiler
evaluates. It supersedes no shipped format:
`torogao-motion-clip/1` (the runtime clip lineage) remains in
force for its own artefacts; `/2` is the hand-authoring
surface and carries what `/1` cannot — per-key tangents, key
types, pose provenance, shader modules, and an op log.

Owner of record: `torogao-format` (`src/motion_clip.rs`:
`validate_clip`, `freeze_document`; gate tool
`src/bin/motion-clip-check.rs`). Checks run in specification
order and stop at the first failure; invalid input is a typed
`TG-CLIP-*` refusal, never a repair.

### Specification

A clip is one canonical JSON document. Its canonical form is
this crate's canonical JSON writer (`jsonfmt::to_d_json`) over
the document with the freeze block removed; the freeze hash
is the lowercase hex SHA-256 of those bytes. Fields:

- `format`: the string `torogao-motion-clip/2` (matched at
  one site, `motion_clip::MOTION_CLIP_FORMAT`).
- `name`: the clip's name (at most 128 bytes).
- `rig`: the target rig's identity — the lowercase hex
  SHA-256 of the hierarchy definition the clip was authored
  against. A clip names a hierarchy hash, never a character.
- `rate`: `{num, den}`, the declared frame rate as a rational
  of positive integers.
- `range`: `{start, end}`, integer frames, `0 <= start <=
  end`.
- `loop`: `null`, or `{start, end}` naming a non-empty
  sub-range of `range`.
- `channels`: 1–1024 channels. A channel's `target` is
  `{kind, name, property}`: a joint's `translation`,
  `rotation`, or `scale`; a morph target's `weight` (the
  `Fac_` channels ride here); or a named scalar `parameter`'s
  `value`. No two channels address the same target.
- Keys: 1–65536 per channel, at strictly increasing integer
  frames inside `range`. A float frame — even an integral
  one — is refused (`TG-CLIP-FLOAT-FRAME`): time is integer
  frames, and seconds are an export conversion. Each key
  carries a `value` (vec3 for translation/scale, quaternion
  `[x, y, z, w]` for rotation, a scalar for weight/value; all
  components finite; the zero quaternion refuses), an
  `interp` naming the segment it opens (`step`, `linear`,
  `cubic` — glTF's three modes; the final key opens no
  segment and its `interp` is `step`), and optionally a
  `key_type` (`keyframe`, `breakdown`, `moving_hold`,
  `extreme`), an `easing` provenance tag, and a `pose_ref`
  (`{pose, blend}` with `blend` in `0..=1`) citing the pose
  library entry the key was placed from — provenance only;
  the key always carries its full value.
- Tangents are explicit Hermite data, derivatives with
  respect to frames, in the value's own shape. A key's
  `out_tangent` is present exactly when its segment is
  `cubic`; a key's `in_tangent` is present exactly when the
  previous key's segment is `cubic`. Presence anywhere else
  is stray data and refuses.
- Tangent modes are edit-time only. A working document may
  carry a key's `tangent_mode` (`explicit`, `free`,
  `aligned`, `vector`, `auto`, `auto_clamped`); the freeze
  resolves every mode to explicit tangents, and a frozen
  document carrying any other mode than `explicit` refuses
  (`TG-CLIP-UNRESOLVED-TANGENT`). Easing presets likewise
  survive only as the `easing` provenance tag on a key whose
  tangents are explicit — never as a behaviour.
- `shaders`: 0–32 modules (DESIGN.md section 7). A module is
  `{name, hook, uniforms, source}`: `hook` is one of the
  closed stage hooks (`skinning_assist`, `morph_assist`,
  `post_skin_displacement`); `source` is WGSL text. The
  uniform contract is closed: `tick` (type `u32` — the only
  time a module may read; a wall-clock-shaped input name
  under any renaming refuses, `TG-CLIP-WALLCLOCK-UNIFORM`),
  `joint_matrices` (`mat4[]`; skinning-assist and
  post-skin-displacement hooks), `morph_weights` (`f32[]`;
  morph-assist and post-skin-displacement), and
  `param:<name>` (`f32`) for a scalar parameter the clip
  itself declares as a channel. Anything else refuses
  (`TG-CLIP-UNDECLARED-UNIFORM`), as does a `var<uniform>`
  declaration in the WGSL that the contract does not list
  (a contract name `param:X` is declared in WGSL as
  `param_X`). SPIR-V compilation and its hash join at Rung
  H6; a `/2` document carries source only.
- `op_log`: the append-only authoring record, 1–100000
  entries, `seq` counting from 0 by ones; each entry names
  its `actor` (`human`, `converter`, or `agent`), a
  `session`, and an `op`.
- `freeze` (frozen documents only): `{sha256, compiler,
  band}` — the hash defined above, the freezing tool's stamp,
  and the recorded evaluation band class the clip froze
  under. A document with a freeze block is validated under
  the frozen rules (resolution complete, hash reproduces);
  one without, under the working rules. Editing a frozen
  clip means thawing it (removing the block) and re-freezing
  as a new revision with a new hash.

A pose record (DESIGN.md section 4) is a `/2` document whose
range is a single frame; the golden set carries one.

Bounds live in `torogao-format`'s `limits.rs` (the `CLIP_*`
constants), as everywhere in this crate.

### Golden vectors

Executable form: `torogao-format` `tests/motion_clip_golden.rs`
over `tests/golden/motion-clip-2/` — for each golden, the
source document, the frozen document, and its canonical form
are all committed, and the suite reproduces the frozen
document byte for byte from the source, re-hashes the
canonical file independently, and checks canonicalisation is
a fixed point. The goldens' rig identity is a test constant.
Freeze hashes (compiler `torogao-format motion_clip h0
(2026-11-27)`, band `tier2-1e-6`):

- `minimal` (one joint channel, step keys):
  `816dc894f59f69d0419157a5b07e6ffc2b4c018054ece4df4dab062571a436eb`
- `loop-hold` (a looping clip; a cubic rotation channel with
  a moving hold):
  `6c55d19dabdd1c999ea81025e375728172f103a2b28522b27ba06ee300e96696`
- `fac` (Fac_ morph channels, a scalar parameter, pose and
  easing provenance):
  `54ea6b4610f89d26f8aab67ef775b4a1e9c76e94a1cb2d8d5550bd4105d8b199`
- `shader` (a clip carrying one shader module):
  `c56665d1db41173391277ee0bbbe6e11d8c514bd83c8eec4b95607c4b3832272`
- `pose` (a one-frame pose record):
  `a18821cf5f464690310d9232d94433edb6536739230621d67f49567837d5f3d8`

The deliberately malformed set (same directory, `malformed/`)
refuses with its typed codes: a float frame
(`TG-CLIP-FLOAT-FRAME`), out-of-order frames, a stray
tangent, a missing cubic tangent, the zero quaternion, a
float rate, an undeclared uniform (contract side and WGSL
side), a wall-clock uniform, a float tick, an unresolved
automatic tangent in a frozen document, and a freeze-hash
mismatch.

### Successors

A future `torogao-motion-clip/3` is a deliberate, gated act,
like every entry here: new entry, new goldens, and a stated
migration for clips frozen under `/2`. The `/2` meaning
above never changes.
## torogao-drawn-clip/1 — the drawn (raster) clip [DRAFT — spike]

Status: **draft, spike-proven 2026-11-27** (drawn-animation
spike; scope study `~/workspace/torogao-drawn-animation-scope-20261127/SCOPE.md`,
record `~/workspace/torogao-drawn-spike-20261127/RECORD.md`).
This is the drawn-animation counterpart of
`torogao-motion-clip/2`: where a motion clip keys rig
channels, a drawn clip keys *drawings* — whole raster
frames substituted on an exposure table, never
interpolated. It carries the drawn grammar of the
hand-animation determination (SS3.4-3.5): a held base
pose, accent tracks on independent exposure, loop closure
by construction.

Owner of record: `torogao-format` (`src/drawn_clip.rs`:
`validate_clip`, `freeze_document`; gate tool
`src/bin/drawn-clip-check.rs`; importer source reader
`src/kra.rs`). Checks run in specification order and stop
at the first failure; invalid input is a typed
`TG-DRAW-*` refusal, never a repair.

### Specification

A clip is a directory: one canonical JSON document
(`clip.json`) plus its drawing payloads. The document's
canonical form is this crate's canonical JSON writer
(`jsonfmt::to_d_json`) over the document with the freeze
block removed; the freeze hash is the lowercase hex
SHA-256 of those bytes. Fields:

- `format`: the string `torogao-drawn-clip/1`.
- `name`: lowercase ASCII, digits, `-`, `_`; at most 64
  bytes.
- `canvas`: `{width, height}`, each 1..=8192.
- `fps`: integer 1..=240. `range`: `{from, to}` integer
  frames, `0 <= from <= to`, span at most 100000.
- `loop`: boolean — the exposure wraps at the range end.
- `matte`: `[r, g, b, a]` under the stack (the colour the
  source renderer composited onto; an imported Krita
  document's banked renders prove it per file).
- `drawings`: the payload manifest, 1..=4096 entries of
  `{id, sha256, width, height, file}`. A drawing is a
  full-canvas RGBA raster; its identity is the SHA-256 of
  its RGBA bytes, its `id` is `d-<sha256>`, and its `file`
  is the derived path `drawings/<id>.png` (the PNG is a
  container; the hash covers pixels, not PNG bytes).
- `tracks`: the layer stack, **bottom first**, 1..=256
  entries of `{name, blend, opacity, visible, keys}`.
  `/1` carries `blend: "normal"` only — other blends
  enter when their arithmetic is proven against the
  paint core. `keys` is an exposure table of
  `{time, drawing}`: strictly increasing integer times,
  every time inside the range, the first key at or
  before `range.from` (hold coverage). Playback at frame
  *t* shows the drawing of the latest key at or before
  *t*, composited source-over over the matte, tracks
  bottom-to-top, through `torogao-paint`'s integer
  composite — the same code the paint surface renders.
- `provenance` (optional): a free-shaped object, covered
  by the freeze hash (the importer records the source
  file's hash and read census here).
- `freeze`: `{band, compiler, sha256}` — as
  motion-clip/2.

### The .kra bridge (reader: `src/kra.rs`)

The importer reads `.kra` directly (zip + `maindoc.xml`
+ keyframes XML + Krita's tile-stream payloads); no
Krita process is involved. The reader maps: RGBA colour
space, `paintlayer` nodes, `compositeop: normal`,
zero layer/keyframe offsets, tile stream `VERSION 2`
(64x64 tiles, 1-byte `0x01` marker + liblzf records,
channel planes in B, G, R, A order). Anything else —
group/filter layers, other blend modes, offsets, other
tile versions — is a typed `TG-KRA-*` refusal naming the
feature, never an approximation. The decode law was
proven byte-exact against Krita 6.0.5's own rendered
frames of the witness document (all ten frames, max
delta 0) before the port was written.

