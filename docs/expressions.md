# Governed expressions

Every lane pack ships **one governed expression set**: named
expression states with semantics notes, as data
(`packs/<lane>.json`, field `expressions`). A governed name is
a promise a character keeps — any character implemented
against a pack implements **every** governed name as her own
parameter recipe.

## The sets

**Adult pack (10):** neutral, smile, joy, playful, longing,
desire, arousal, trance, overwhelmed, bliss.

**All-ages pack (8):** neutral, smile, joy, playful, surprise,
sad, angry, calm.

Semantics for each name live in the pack file beside its
default recipe — the pack is the authority, this page is the
map.

## The carrier: states

An expression state is a named parameter recipe
(parameter → value) stored with the project in its
vendor-extension section (`io.ntranch.torogao`, field
`states`). States apply in the deterministic preview as an
input layer (`physics-preview --state <name>`, or the
`apply-state` tool): application is an input track, so
preview/replay determinism is untouched, and applying a state
never mutates the stored puppet.

A recipe key is a parameter name or a **blend-shape name**
(blend shapes are additive keyform deltas held as extension
data; a state targeting one folds its value into the
blend shape's parameter — see the boundary note in
`torogao.cli.states`).

## The coverage rule

`expression-check` (CLI + registry tool) validates a project
against a pack: every governed name must be implemented as a
state. A character missing any name fails with
`TG-EXPR-MISSING` and the missing names listed. The Character
Creator satisfies the obligation at creation time by
installing the pack's default recipes as the character's
states; the artist then makes each recipe her own — the
obligation is that the name exists and drives the rig, not
that the default values survive.

Stage variants ride the same mechanism: a variant set keyed
by stage id swaps a parameter recipe plus named part
visibility/appearance data (both stored as extension data;
visibility and appearance are carried for scene and export
consumers, the recipe half is what preview applies). A variant
referencing an unknown parameter or part fails validation
with `TG-VARIANT-UNKNOWN-PARAM` / `TG-VARIANT-UNKNOWN-PART`.

## Ratings ride along

Projects carrying extension data carry a rating stamp
(pack, lane, rating, basis). Validation refuses an incomplete
stamp (`TG-RATING-STAMP-MISSING`) and a rating the lane
forbids (`TG-RATING-LANE-MISMATCH`); every export carries the
stamp in its report and in a `<name>.stamp.json` sidecar.
