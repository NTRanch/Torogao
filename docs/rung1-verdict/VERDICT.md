# Torogao renderer Rung 1 — verdict pack (Prima arc)

**Stakes:** this pack decides whether the new Vulkan-native 3D path is a
sound foundation to build the cast on — the pipeline evidence is exact;
two plate-parity items diverge for measured, named reasons the Owner
rules on.

## Expected vs seen

SHOULD be seen: Prima performing her gated 360-frame arc on the couch
set — same camera, same set pieces on the same schedule (hypno screen +
rings frames 0–89, spreader bar 90–269, toy throughout), her canonical
paint (white/grey outfit, then the red dress from frame 90), face morphs
tracking the clip's channels, lit by the Sun 3.0 / Fill 1.1 pair over the
dark world, at the plates' framing (512×640 — measured; the gap
analysis's 1024×1280 was the gate-rig number, not the arc's).

SEEN: all of the above except limb articulation and overall light
level — items 5 and 6 below. Everything else matches.

## Items (Owner rules by number)

1. **Framing, camera, set, schedule — MATCH.** Pair frames and the
   contact sheet show the same composition; set-region mean |Δ| is
   (42.4, 43.3, 41.5)/255, dominated by the light-level item (6), not
   placement. Screen/rings appear and vanish on schedule; the spreader
   bar and toy sit where the plates put them. (stats.json, pair frames)
2. **Paint — MATCH.** The carried COLOR_1 REVIEW is resolved by
   measurement: COLOR_1 is the paint (COLOR_0 is constant white; the
   Hair mesh's COLOR_1 mean equals its material baseColorFactor to
   2e-3). The red dress reads red at PEAK (pair_f0180); outfit tones
   track the plates. (proving test `color1_evidence`)
3. **Face and morphs — MATCH.** Face crops (face_f0000, face_f0225)
   show the clip's channels on her face — eyes closing, mouth shapes —
   and the CPU reference reproduces Blender's own evaluated vertices
   at witness points to 2.0e-7 … 5.6e-7 m. (strain-witness.json,
   proving test `blender_strain_witnesses`)
4. **Joint placement — MATCH.** At frame 0, joint world positions from
   this pipeline equal Blender's pose-bone positions exactly (0.0000 m)
   for hips, spine chain, neck, head, shoulders, arm and leg origins.
   (probe7/probe8 measurements)
5. **Limb articulation vs plates — DIVERGES.** FAIL exemplar:
   pair_f0180 (plate: body extended along the couch; render: limbs
   folded differently) and the arms in pair_f0000. Cause, measured:
   the clip's rotation tracks were authored in Blender bone frames
   (gen_clip converts show eulers with no bone-frame conversion),
   while the GLB rig's bone frames — carried in its inverse bind
   matrices — differ for limb bones: world deformation-operator
   residuals vs Blender are ~180° about X for the leg chain, 135.6°
   Arm, 115.8° ForeArm/Hand, 108–137° fingers, while hips/spine/
   shoulders are 0.0000. The renderer consumes the canonical
   clip + def + GLB trio faithfully (items 3, 4, 8, 9 are exact);
   baking a per-bone correction table into the renderer was rejected
   — it would not generalize to the cast and contaminates the format
   boundary. The estate fix belongs in gen_clip (bake GLB-frame
   locals at export) or a def v2 carrying bone frames. Carried as
   REVIEW for the format owners with the per-bone evidence.
6. **Light level vs plates — BRIGHTER (parity not claimed).** Mean
   signed Δ over 60 sampled frames is (+42.5, +43.5, +41.7)/255
   full-frame; the R5 model (wrapped diffuse + single GGX lobe + AgX
   fit) was implemented as the governing analysis specifies, and the
   delta decomposes into wrap-vs-plain diffuse (≈1.37× on the sun
   term, ≈1.96× on the fill term at the floor normal), EEVEE
   light-energy semantics, and a +9…+12/255 AgX-fit residual measured
   on the background and the emissive screen. No ad-hoc retune was
   applied in this rung; shading parity is rung 3's scoped work.
   (stats.json)
7. **Self-shadowing — PARTIAL, by named omission.** The carried G10
   REVIEW is resolved by probe: the plates DO self-shadow, from both
   lights. This rung implements the Sun's shadow (2048² map, 3×3 PCF);
   the Fill's shadow is a named omission, rung 3 owns closing it.
   (scene-probe.json)
8. **Determinism — EXACT.** Two full 360-frame renders on the A6000
   produce identical per-frame RGBA SHA-256 for 360/360 frames.
   (run-a/manifest.json, run-b/manifest.json)
9. **GPU skinning vs CPU reference — EXACT within the stated bound.**
   Bound stated before the run: positions ≤ 1e-4 m, normals ≤ 1e-3.
   Measured over all 13,920 Body vertices × 6 frames: max position
   delta 2.68e-7 m, max normal delta 2.38e-7. (crosscheck.json)

## Reading the pack

- `pair_fNNNN.png` — plate (left) vs this renderer (right), 2×.
- `diff_fNNNN.png` — plate vs |difference| ×4 heat, 2×.
- `face_fNNNN.png` — decision region (head/shoulders) at 3×.
- `contact_sheet.png` — every 30th frame, plate row over render row.

No Owner verdict is recorded in this pack; the Owner rules keeper/not.
