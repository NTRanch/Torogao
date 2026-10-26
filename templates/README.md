# Torogao rig templates

Templates are data, not code. The headless CLI (and, through
the tool registry, the MCP and A2A surfaces) lists and
instantiates them:

```
torogao-cli list-templates
torogao-cli instantiate-template base-humanoid \
    --preset hair-sway-3step --preset skirt-sway --out puppet.inx
```

Instantiation is deterministic: the same template + presets
produce a byte-identical INP file on every run.

## Format: `torogao-template/1`

Two kinds of document share the format:

- **puppet** — a complete skeleton puppet. Its `parameters`
  and `nodes` are written in the payload's own JSON shapes
  (the same objects the puppet loader reads), so a template
  introduces no second binding language. It also carries
  `meta`, `physics` (payload physics settings), and an
  `anchors` name map declaring the attachment points presets
  use.
- **physics-chain** — a pendulum-chain preset: an `attachTo`
  anchor name, `inputs`, per-stage settings, and `outputs`.
  Every physics setting is stored as
  `{"chosen": <number>, "range": [lo, hi] | null, "provenance": ..., "note"?: ...}`
  so the data carries its own evidence: `range` is the
  attested band where one exists, `chosen` is the value
  instantiation uses, and `provenance` says where the number
  comes from (see below). Gaps are recorded as `unrecovered`
  notes, never filled with invented numbers.

## The v1 set

- `base-humanoid` — body/head angle parameters, breath,
  eye/brow parameters, and the three mouth parameters the
  viseme contract maps onto (see `contracts/`). Anchors:
  `HairAnchor`, `BreastAnchor`, `SkirtAnchor`. Geometry is
  nominal placeholder quads; rig real artwork over the same
  node names.
- `hair-sway-2step`, `hair-sway-3step` — sway chains after
  the reference editor's shipped "Hair 2-step / 3-step
  pendulum" physics presets.
- `hair-bounce` — the bounce component of the hair rig
  (up/down + fan/squeeze), rigged separately from sway.
- `breast-x`, `breast-y` — the two breast components; X is
  driven by body X + Z, Y by body Y alone, with outputs
  staggered across pendulum stages.
- `skirt-sway` — a hip-hung sway chain for skirts and hems.

The two hair-sway variants are alternatives for the same
slot: they define the same parameter names, so applying both
to one puppet is an instantiation error, by design.

## Provenance classes

Preset data is classified after the physics annex the values
were derived from:

- `DOC` — stated in official documentation.
- `EXPORT` — present in exported physics settings of shipped
  sample rigs.
- `TUTORIAL` — structure or inputs demonstrated in the
  reference rigging tutorial. The tutorial's concrete
  on-screen numeric settings were not recoverable from its
  caption track; presets record this in their `unrecovered`
  field and no preset number is attributed to the tutorial.
- `chosen` — our choice: interpolated inside an attested
  band, or a value where the sources attest none. Always
  marked, always with a note saying why.

Known gaps, stated plainly: `skirt-sway`'s group is attested
by documentation but no skirt-specific exported ranges were
captured, so its chain follows the exported sway class;
`hair-bounce` has no bounce-specific exported range either,
and its band fields are null with chosen values inside the
sway class.

## The instantiation conversion

Applying a preset to a puppet is a mechanical conversion:

1. The preset's `attachTo` node is located by name in the
   base puppet's tree (missing anchor = typed error).
2. Each preset input must name a parameter that exists in
   the base puppet (missing input = typed error).
3. Stage *i* becomes, nested under the previous stage:
   a segment part (a quad of the stage's `segmentLength`),
   a `SimplePhysics` node, and — except after the last
   stage — a rotation node whose child anchor sits one
   segment length further along +Y (the model's down axis).
   The last stage's segment part is the chain tip.
4. Field mapping from preset settings to the physics node:
   - `length` = the stage's `segmentLength` (template
     geometry, our proportion choice);
   - `angle_damping` = `1 − easeOfSwinging.chosen` — the
     pendulum's damping is the complement of the documented
     ease-of-swinging band;
   - `output_scale` = `output.scale.value / 90`, preserving
     the graduated 5-toward-90 output pattern relative to
     its attested maximum;
   - `gravity` = 1.0, `frequency` = 1.0,
     `length_damping` = 0.5, model `Pendulum`, map mode
     `AngleLength`.
   - Recorded but not consumed by v1's physics node:
     `mobility`, `delay`, `acceleration` (kept in the preset
     data for the native rig export's richer physics model).
5. Each output becomes a driven parameter (range −1..1)
   whose binding rotates its stage's rotation node — or the
   tip part, for the last stage — over `transform.r.z`
   (−0.6..0.6 rad).
6. All fresh uuids come from a deterministic counter in
   construction order; physics `param` references and
   binding `node` references are remapped through the same
   allocation (the GUID-remap step any template system over
   this format needs).
7. The payload is packed as a standard INP file with one
   texture slot holding a 1×1 placeholder PNG. The result
   goes through the ordinary `validate`, `export-rig`, and
   `physics-preview` gates like any other puppet.
