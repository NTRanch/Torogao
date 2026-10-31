# Port-back — what Handfast validated, for torogao / lumen-rig

Written at the v1.0 build (2026-10-30). Source of record for the maths:
fathom RE of reference 5447477 (`docs/RECIPE-5447477.md`) and 12369020.

## 1. Coupled-anchor contact data model

A contact is stored as **`(figure_id, anchor_id)` on the receiving
figure — never as a world position.** The planted hand's world position
is *derived every tick* from the receiver's anchor transform (breathing
lift and sway included). In Handfast this is `carry_hand()`: the guide
hand is recomputed from Sloane's origin each step, so drift is
impossible by construction, not by correction.

For lumen-rig: anchors are named points on a figure with a local
offset; a contact record references the anchor, and every consumer
(graphics, audio sync, scoring) reads the derived position. World-space
contact storage is the bug class this eliminates.

## 2. Contact-zone acceptance metric

The reference's final shot measured contact-zone activity in 564/581
frames (97%). Handfast adopts **≥95% of frames within an intimate shot
showing contact in the active zone** as the torogao acceptance metric
for any staged beat, and its demo playthroughs hold contact for the
whole contact/guidance/appraisal span by construction (the invariant
fuzz test asserts the planted hand stays within tear distance of its
anchor-or-path under random inputs for 20 000 steps).

## 3. Beat grammar

`approach → contact → guidance → held appraisal`, with the held beat
the longest element (the reference's 104 s single take is 88% of its
runtime). In rig terms: a beat is a small state machine whose terminal
state is a *hold with a filling meter*, not a cut. Torogao scenes
should author holds as first-class segments with their own duration
budgets, not as gaps between actions.

## 4. Expression ramps, never snaps

Every expressive channel (lid, mouth, brow, blush, gaze target, iris
glow) is a value that **ramps toward a target at a bounded rate** —
Handfast moves gaze/expression in fixed steps per tick (e.g. 90 milli
per tick), so any state change passes through all intermediates. The
receiving figure carries higher activity (Sloane ×1.30 activity,
matching the reference's measured 2.64 vs 2.08). For the rig: parameter
curves with rate limits, not keyframe pops; a "snap" is a data error.

## 5. Breathing is a layer, not an animation

Breath is an additive offset applied to the whole figure (and to
everything coupled to it — that is what makes carried contact read as
alive), period ~4.3 s (Prima) / ~4.9 s (Sloane), always on, never
gated by beat or screen. In lumen-rig this belongs in the base pose
composition, underneath every authored layer, with amplitude scaled
per beat (Handfast scales it ×1.0/1.15/1.30 by level).

## 6. Glowing iris is a layer, not a property (Owner ruling)

The iris glow is a boolean face/material layer switched by scene state
(beat + regard threshold) and a user setting — it is never baked into a
model or a character's identity record. Handfast implements it exactly
so (`glow` flag on the figure, set from beat state ∧ setting ∧ meter),
and the rig should carry it as a named, toggleable layer likewise.
