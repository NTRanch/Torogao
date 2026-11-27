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
