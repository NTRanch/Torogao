# Visemes: the mouth contract

Talking mouths are driven by one governed contract, not by
per-character improvisation. The contract
(`contracts/visemes.json`, format `torogao-visemes/1`)
names a standard viseme set and maps every viseme onto
three mouth parameters that every puppet implements.

## The set

The set is the **Oculus LipSync 15-viseme set** — `sil,
PP, FF, TH, DD, kk, CH, SS, nn, RR, aa, E, ih, oh, ou` —
the de-facto standard vocabulary for real-time lip sync,
also what voice-analysis tooling emits. Set membership is
the standard's; the target values are this contract's own
mapping:

| Parameter | Range | Meaning |
|---|---|---|
| `ParamMouthOpenY` | 0..1 | jaw/mouth opening |
| `ParamMouthForm` | −1..1 | −1 rounded … +1 wide |
| `ParamMouthX` | 0..1 | mouth width (0.5 neutral) |

Every viseme in the contract carries a target for all
three parameters, e.g. `aa` = open 0.9 / form 0.1 /
width 0.55; `ou` = open 0.35 / form −1.0 / width 0.25;
`sil` = closed rest.

## The gate: `viseme-check`

```
torogao-cli viseme-check <project.inx> [--contract <file>]
```

A project gate in the core tool namespace, alongside
`validate`:

- `TG-VISEME-PARAM-MISSING` (error) — a contract mouth
  parameter is absent; the puppet cannot answer voice.
- `TG-VISEME-PARAM-UNBOUND` (warning) — the parameter
  exists but has no bindings and no driver; viseme curves
  would move nothing.

The base humanoid template implements all three parameters
with bindings, so template-born puppets pass clean.

## The baker: `viseme-bake`

```
torogao-cli viseme-bake <track.json> --out <curves.json>
```

A track (`torogao-viseme-track/1`) is a timed event list —
`{viseme, start, end}` in seconds — at one of the standard
rates {15, 30, 60, 120} fps (default 60). Baking produces
a `torogao-curves/1` document with one series per mouth
parameter. The interpolation rule, exactly:

1. Step *i* samples time *t = i·dt*.
2. The held targets are those of the latest event with
   `start ≤ t` (ties: later track order wins). Before the
   first event, the held targets are `sil`.
3. For `blendSeconds` (default 0.06) after each event
   start, values interpolate **linearly** from the
   evaluated value at the event's start — so an event that
   interrupts a blend starts from where the mouth actually
   is — to the event's targets.
4. An event's `end` does **not** release the mouth: targets
   hold until the next event. Authored silence is an
   explicit `sil` event.

No other smoothing exists anywhere in the path, so a baked
track is byte-stable and identical in preview and export.

## Scope

P2 lands the contract, the gate, and the baker core.
Authoring viseme tracks inside the editor arrives with the
tutorial phase; tracking-driven visemes (a camera
performance becoming a track) arrive with the tracking
phase. The resting mouth — breath and micro-motion — lives
on the ambient layer, never on the viseme track.
