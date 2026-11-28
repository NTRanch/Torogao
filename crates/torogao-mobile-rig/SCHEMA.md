# `torogao-character/1` — the character record schema

Status: authored by the rig lane (Commission C) under
ARCHITECTURE.md §3, 2026-11-27. The `art` and `prompt_identity`
sections are the art lane's to review in before either lane
builds against them; after this authoring, changes are
**additive-only** (new optional fields, never renames or
removals) and announced the same day in the programme record.

One character is one record: a JSON document, content-hashed,
edited on the phone, solved on primo. Code is the source of
truth (the Owner's code-first aim): the record plus the
hash-pinned builder reproduces the gated GLB byte-for-byte.

## Document shape

```json
{
  "schema": "torogao-character/1",
  "id": "solenne",
  "display_name": "Solenne",
  "revision": 3,
  "updated_at": "2026-11-27T19:04:00Z",
  "identity_lock": { "...": "see below" },
  "prompt_identity": { "...": "art-lane section" },
  "body_params": { "...": "knobs" },
  "face_params": { "...": "knobs" },
  "expressions": [ { "name": "Fac_Mth_Smile", "weight": 1.0 } ],
  "outfit": { "spec_ref": "NunBlack garment spec of record" },
  "rig": { "...": "see below" },
  "art": { "...": "art-lane section" }
}
```

Schema evolution is additive-only: new fields land with
serde defaults so older records keep parsing, and a breaking
change is a new schema id, never an in-place edit.

## Sections

### `identity_lock` — the Step 0 gate (D-1410)

| field | meaning |
|---|---|
| `status` | `unlocked` \| `locked` |
| `refs[]` | lock artefacts: `{label, sha256}`. The hash is filled primo-side when the artefact lands there; empty means unhashed. |
| `proportions` | optional `{height_m, bust_circ_m, waist_circ_m, hip_circ_m}` — the locked Step 0 measurements a derivation is checked against (tolerance 1 cm). |
| `locked_by`, `locked_at` | the lock act: who held it, when (ISO-8601). |
| `source` | the lock's citation: source document, ruling id, verbatim facts. |

**Gate order is the design:** no lock → no derivation. A
derivation additionally requires every lock ref hashed, the
lock act recorded, and a derivation spec imported. Art
origination is never gated by the lock.

### `prompt_identity` — art-lane section (rig lane authored, art lane reviews)

| field | meaning |
|---|---|
| `identity` | the character's identity tags, verbatim from the lock (Illustrious tag style). |
| `signature` | signature items / the canonical outfit line, appended after the identity tags. |
| `negative_guards` | per-character negative additions (e.g. Sirena's `freckles` guard — the trait is Lana's lock, D-1482). |
| `source` | the lock / style sheet the identity was taken from. |

### `body_params` / `face_params` — the knobs

Absolute settings applied over the frozen base spec by
`params::derive_spec` — the one place knob application
lives. Applying is not cumulative: knobs re-apply over the
base every time.

`body_params`: `z_scale` (0.90–1.15), `radial_scale`
(0.85–1.15), `bust_amp` (0–0.09 m, the builder's `bust.amp`),
`waist_scale` (0.85–1.15, multiplies the waist-band torso
rows' radii: raw-frame z ≤ 1.00), `hip_scale` (0.85–1.15,
the hip-band pelvis rows: 0.80 ≤ z ≤ 0.92), `hair_style`
(one of the builder's styles: waves, hime, bob, bob_messy,
ponytail, ponytail_high, braid, scraped, curls).

`face_params` (the builder spec's `face` block): `eye_x`
(0.026–0.040), `eye_z` (1.54–1.62), `mouth_z` (1.47–1.55),
`brow_z` (1.57–1.64), `iris_r` (0.004–0.009), `pupil_r`
(0.0015–0.005) — metres, raw frame.

### `expressions`

`[{name, weight}]`, weight 0..=2. A name must be a morph
the fleet builder emits — the mechanism table
(`morphs.rs`, Rule 10: the moving mechanism, stated from
`compute_morphs`) is the authority; a name with no mechanism
cannot be set, posed, or verdicted.

### `outfit`

`{spec_ref}` — a reference to the outfit spec of record
(never a second copy of it). Outfit *content* stays in the
builder spec / garment library; the record names what the
character wears.

### `rig`

| field | meaning |
|---|---|
| `builder_family` | `moon-deep-party` \| `torogao` — selects the gated builder copy primo-side. |
| `base_ref` | the canonical base the derivation starts from (builder default when empty). |
| `derivation_spec` | the frozen base builder spec (the per-girl spec of record), imported primo-side. Absent = cannot derive. When the parametric body layer lands, derivation moves on-device and this field retires additively — the record format does not change. |
| `expected_joint_count` | optional structural expectation for A10.2 (54 for the fleet party armature). |
| `gated_glb_sha256` | the gated GLB of the current admission, when one exists. |
| `gates` | the last derivation's full gate results (A10.1 battery + lock check, A10.2 census, twin hashes, builder hash), or null. |
| `admission` | `{by, at}` — who admitted the gated GLB and when. Set primo-side at the gate, never typed on the phone. |

A derivation's label follows the gates: all pass +
deterministic twin = **GATED**; anything else =
**CONSTRUCTION**, in those words (ARCHITECTURE §8).

### `art` — art-lane section (rig lane authored, art lane reviews)

| field | meaning |
|---|---|
| `candidates[]` | `{sha256, route, seed, prompt_sha256, class}` — `route` names the producing route (`wai-txt2img`, `edit-local`, …); every candidate is hash-pinned to the exact prompt document that produced it; `class` is `candidate` by construction (Rule 9) until an Owner admission. |
| `admitted[]` | `{sha256, admitted_by, at}` — the admitted keepers and their admission acts. |

## Hash discipline

Canonical bytes = `serde_json` compact serialisation with
recursively sorted keys (the estate's workflow-hash
convention). `record.sha256()` is the SHA-256 of the whole
record's canonical bytes. Job documents embed the record,
its hash, and (for derives) the derived spec and its hash;
the facade and the adapter both recompute and compare
before any build starts.

## Versioning

`revision` increments on every accepted edit; `updated_at`
is the editor's ISO-8601 timestamp. The schema id is the
format version: a breaking change is a new id
(`torogao-character/2`), never an in-place edit.
