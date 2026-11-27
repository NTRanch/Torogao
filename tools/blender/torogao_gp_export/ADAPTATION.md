# Grease Pencil → drawn-clip/1 Adaptation

**Status:** built and gated 2026-11-27. Exporter:
`tools/blender/torogao_gp_export/export_gp_drawn.py` (branch
`pax/grease-pencil-20261127`, local commits only). Witness scene maker
and the pixel-diff verifier live beside it. Artefacts of record:
`~/workspace/grease-pencil-adaptation-20261127/` (proof `.blend`,
exported sequences, pixel-diff JSONs).

The pipeline position is the one the Krita and OpenToonz adaptations
already occupy: the pencil stays in the real authoring tool; the
adaptation exports a `torogao-drawn-seq/1` manifest plus PNGs, and the
existing PNG-sequence importer (f20cc7a) turns that into a
`drawn-clip/1` clip. No importer or player change was needed — no gap
in the importer was found.

## The two export modes

- **Layered (default).** One drawn-clip track per Grease Pencil
  layer, solo-rendered. This is the commissioned authoring shape:
  tracks stay editable and re-timeable downstream.
- **`--flatten`.** One `composite` track: full-scene renders at the
  union of every layer's keyframe boundaries. Blender's own composite
  is carried verbatim, so playback is pixel-exact by construction
  (witness: 0 differing pixels on all 6 frames). The price is the
  per-track structure. Blend modes and layer masks, which refuse in
  layered mode, flatten-with-warning here.

## Concept mapping

| Grease Pencil | drawn-clip/1 |
|---|---|
| Layer list, in list order | Tracks, **bottom-first in list order**. Blender's GP layer list is already bottom-first (first-created = stack bottom). Proven on the witness: reversing the list put the opaque face fill over the eyes in playback; list order composites correctly. |
| Keyframed drawing (GP frame) held until the next keyframe | One exposure entry `{file, hold}` per maximal run of one in-force drawing. A drawing held N scene frames is one PNG with `hold: N`. |
| No drawing in force yet (first keyframe later) | Leading exposures against a shared `blank.png` (all layers hidden render). |
| Last drawing in force at range end | Held out to the range end. Keyframes past the range end are ignored with `TG-GP-WARN-FRAME-CLIP`. |
| Two keyframes whose drawings render identically | One drawing in the clip: the importer dedupes by content identity (SHA-256 of decoded RGBA). The witness's frame-6 eyes drawing is a distinct GP drawing with identical pixels and lands as the frame-1 drawing id (`d-9449c34e…` at ticks 0 and 5). |
| Layer opacity | Track `opacity`, quantised to 1/255 steps; `TG-GP-WARN-OPACITY` when the quantisation is lossy. |
| Layer hidden at audit time (`layer.hide`) | Track `visible: false`. (Solo state during export is keyed by layer name and fully restored; authored hide is captured before any soloing.) |
| Multiple GP objects | Tracks in per-object layer order; cross-object stacking follows view-layer/render order, not a GP concept — `TG-GP-WARN-OBJECT-ORDER`. A render-hidden GP object emits no tracks (`TG-GP-WARN-HIDDEN-OBJECT`). |
| Scene fps (incl. `fps_base`) | Clip fps. Effective fps must be an integer in 1..=240 or the export refuses (`TG-GP-FPS`). |
| Scene frame range | Clip range; one tick = one scene frame. Spans over 100 000 frames refuse (`TG-GP-RANGE`). |
| Onion skinning | **Authoring aid only.** It exists only in the viewport; the render pipeline never draws it, so it can never be baked into an export. Nothing to map. |
| GP materials, strokes, fills | Baked into the solo render pixels. No stroke-level representation crosses into the format. |

Manifest law observed throughout: the manifest carries **only** the
fields the strict reader accepts (`format, name, fps, loop, matte,
range, tracks`). All provenance — scene file, exporter version, GP
frame numbers per segment, per-file canonicalisation counts — lives
in sidecars (`gp-provenance.json`, `export-result.json`) written
beside the manifest, never inside it.

## Render-settings pin

The exporter overrides every setting that could open the
colour-management / premultiplication / scale trap family, prints
each override as `TG-GP-PIN`, and restores the scene afterwards:

| Setting | Pinned value | Why |
|---|---|---|
| `render.engine` | `BLENDER_WORKBENCH` | Deterministic, light-independent GP shading; Eevee/Cycles add sampling and light variance the format cannot carry. |
| `render.resolution_percentage` | 100 | No silent DPI/scale resampling of the canvas. |
| `render.film_transparent` | True | The matte is transparency; a background colour would bake into every drawing. |
| `render.image_settings` | PNG / RGBA / 8-bit (`file_format` in Blender 5.3) | The importer's decode contract. |
| `render.use_compositing`, `render.use_sequencer` | False | Post chains would alter pixels after the GP render. |
| `render.dither_intensity` | 0.0 | Dither is per-render noise; it breaks pixel identity. |
| `view_settings.view_transform` / `look` | `Standard` / `None` | AgX/Filmic tone mapping is a non-linear remap the player does not apply; Standard is the identity the PNG bytes assume. |
| `view_settings.exposure` / `gamma` | 0.0 / 1.0 | Same reason. |
| `display_device` | `sRGB` | The encoded space of the format. |

Camera: the scene's active camera is required (`TG-GP-NO-CAMERA`
otherwise); canvas over 8192 px on either axis refuses
(`TG-GP-CANVAS`).

## Transparent-pixel canonicalisation (export law)

Blender's PNG writer leaves non-zero RGB under zero alpha
(premultiplied→straight conversion residue); torogao-paint
normalises transparent pixels to `(0,0,0,0)`. The exporter therefore
rewrites every PNG it emits — segment files and reference renders
alike — zeroing RGB wherever alpha is 0, and records the per-file
zeroed counts in `export-result.json`. Proven lossless: on the
witness frame exactly the 156 residue pixels changed, 0 pixels at
alpha > 0 changed, max delta 0. Without this step the first witness
run differed from Blender's own renders by exactly those residue
pixels and nothing else.

## The one named deviation: linear compositing (layered mode)

Blender's GP engine composites strokes and layers in **scene-linear
premultiplied float** and sRGB-encodes once, on save. drawn-clip/1
composites the encoded 8-bit bytes. Source-over is not invariant
under that encoding, so layered playback differs from Blender's
composite **only where semi-transparent layers overlap** — AA edges,
glazes, highlights over fills.

Mechanism proof (isolated witness): opaque red under blue at fill
opacity 0.5. Blender's full render: `(188, 0, 188, 255)` — exactly
`sRGB(0.5 linear)` in both channels. Source-over of the encoded solo
PNGs: `(128, 0, 127, 255)`. The gap is the working space, not
rounding (a float composite in encoded space reproduces the player's
diff count exactly).

Measured on the character witness (640×800, 512 000 px/frame):
open-eye frames differ on 724 px (0.14 %), max channel delta 17;
closed-eye frames on 442 px, max delta 55 (dark arc strokes over
bright skin — the highest-contrast overlap). All opaque and
non-overlapping regions are exact. Any production that needs the
Krita-standard 0-diff verdict uses `--flatten`, which scored
**0 differing pixels on all 6 frames**.

## Refusals (typed, no manifest written)

| Code | Condition |
|---|---|
| `TG-GP-NO-CAMERA` | Scene has no active camera. |
| `TG-GP-FPS` | Effective fps non-integer or outside 1..=240. |
| `TG-GP-RANGE` | Frame range spans more than 100 000 frames. |
| `TG-GP-CANVAS` | Render resolution over 8192 on an axis. |
| `TG-GP-NO-GP` | No Grease Pencil object in the scene. |
| `TG-GP-FOREIGN` | A visible, renderable non-GP object would appear in renders the tracks cannot represent. |
| `TG-GP-BLEND` | A layer's blend mode is not `REGULAR` (layered mode only). |
| `TG-GP-MASK` | A layer uses layer masks (layered mode only). |
| `TG-GP-TRACK-NAME` | Two layers would collide as track names, or a name is unusable. |
| `TG-GP-USAGE` | Bad command line. |
| `TG-GP-TIME-VARYING` | A held segment's pixels change inside its hold under modifiers or object animation. The exporter renders the segment's last frame solo and demands exact equality; a held exposure cannot carry per-frame variation, so the export refuses rather than mistime silently. |

## Warnings (export proceeds, named in `export-result.json`)

| Code | Meaning |
|---|---|
| `TG-GP-WARN-OPACITY` | Layer opacity quantised to the manifest's 1/255 step. |
| `TG-GP-WARN-MODIFIER` | Object carries modifiers; their static effect is flattened into the pixels (and stability is proven per segment, above). |
| `TG-GP-WARN-TINT` | Layer tint is flattened into the pixels. |
| `TG-GP-WARN-LIGHTS` | A layer has `use_lights` on; under Workbench with no light objects GP shades black, so the export was probably not what the author saw in a lit viewport. |
| `TG-GP-WARN-HIDDEN-OBJECT` | A GP object is render-hidden and emitted no tracks. |
| `TG-GP-WARN-OBJECT-ORDER` | More than one GP object; cross-object stacking is render order. |
| `TG-GP-WARN-FRAME-CLIP` | Keyframes past the range end were ignored. |
| `TG-GP-WARN-BLEND-FLATTENED` / `TG-GP-WARN-MASK-FLATTENED` | `--flatten` mode carried a blend mode / layer mask inside Blender's composite, with no per-track representation. |

## Authoring traps (learned on the witness, all firsthand)

- **The fill gate is `stroke.fill_id`, not the material.** In GPv3 a
  scripted stroke fills iff `fill_id != 0`; `add_strokes` leaves it
  0, so fills silently do not render whatever the material says.
  Fill colour is `stroke.fill_color`, strength `stroke.fill_opacity`.
  The material's `show_fill`/`fill_color` neither gate nor colour a
  scripted fill.
- **`use_lights` defaults on and shades black.** GP layers default to
  `use_lights = True`; under Workbench with no light objects every
  stroke renders black. Author with it off, or light the scene and
  accept the warning.
- **`grease_pencil_add` seeds a default layer** with an empty frame;
  programmatic scenes must remove pre-existing layers before
  authoring, or the seed becomes a phantom track.
- **GP layer wrappers are not identity-stable** across `obj.data`
  accesses in Blender 5.3 — key solo/visibility state by
  `(object name, layer name)`, never by wrapper object.

## Verification record (2026-11-27, official Blender 5.3.0 on primo)

Witness scene (`make_proof_scene.py`, 640×800 @ 12 fps, 6 ticks):
Body held 6; Eyes A held 3 / B held 2 / A 1; Sparkle leading blank 4
then held 2.

- Layered export → import: 6 files, 6 exposures, **5 drawings**
  (content-identity dedupe), 6 keys; Eyes keys at ticks 0/3/5 with
  drawing ids `[A, B, A]`. Freeze hash reproduced identically across
  independent export runs (`b5c89960…`).
- Layered playback vs Blender's own renders: 724 px/frame (max Δ17)
  on open-eye frames, 442 (max Δ55) on closed-eye frames — the named
  linear-compositing deviation above, and nothing else.
- `--flatten` playback vs Blender's own renders: **0 differing
  pixels, 6/6 frames.**
- Refusal battery: ADD-blend layer → `TG-GP-BLEND` refusal, no
  manifest; same scene under `--flatten` → warning + export. Noise
  modifier witness → `TG-GP-TIME-VARYING` refusal, verified honest:
  renders are deterministic per frame (frame 6 re-render identical)
  while frames 1 and 6 genuinely differ — the modifier evolves
  inside the hold.
- Importer untouched; no gap found.

## Usage

```sh
# layered
blender --background scene.blend --python export_gp_drawn.py -- \
    --out DIR [--name NAME] [--refs DIR] [--no-loop]
# flattened (pixel-exact composite)
blender --background scene.blend --python export_gp_drawn.py -- \
    --out DIR --flatten [--refs DIR]
```

`--refs DIR` additionally renders the full scene once per tick
(`frameNNNN.png`, tick-indexed) under the same pin — the reference
set playback verification compares against. Every run writes
`export-result.json` into the out dir (Blender swallows script exit
codes; the JSON is the machine-readable verdict).
