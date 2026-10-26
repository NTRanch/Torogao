# Ambient drivers — the named contract

Breath, automatic blinking, gaze saccades, and idle micro-sway
are **ambient drivers**: a named, data-defined driver set in
the rig contract. A driver set is data:

```json
{ "drivers": ["breath", "blink", "saccade", "sway"], "seed": 7 }
```

Each driver is a deterministic function of the step index, the
step rate, and the recorded seed — no wall clock, no ambient
randomness. The same (project, driver set, seed, steps) always
produces the same input values, in preview and in export alike,
because both run the same code over the deterministic solver
path.

## The drivers

| Driver | Drives | Definition (periods in seconds, converted to steps at the run's rate) |
|---|---|---|
| breath | `ParamBreath` | 4 s sine between 15% and 85% of the parameter's range |
| blink | `ParamEyeLOpen`, `ParamEyeROpen` | closed to range minimum for 3 steps every 4 s |
| saccade | `ParamEyeBallX`, `ParamEyeBallY` | holds a target, jumps to a new seeded target every 1–3 s (seeded splitmix64 schedule) |
| sway | `ParamAngleZ` | 7 s sine at ±8% of range around the default |

A puppet without a driver's parameter is simply not driven on
that channel (a rig with no eyeball parameters has no saccades;
that is a property of the rig, not an error).

## Precedence — specified, not emergent

```
input track  >  state / variant recipe  >  ambient drivers
             >  parameter defaults
```

A driver never overrides an authored value: driver output is
merged only into parameters that neither the track nor a
state/variant recipe drives. Two authored layers driving the
same parameter (a state and a variant together) is an error,
never a silent composition. A parameter a physics driver
drives cannot be a preview input from any layer.

Live tracking, when it arrives, overrides drivers while
active; a tracked take is simply another authored layer above
this one, and recorded performances bake the composed result
so preview and export stay identical.

## Gate

A 60-second idle preview with all four drivers active replays
byte-identically across runs, and each driver's contribution
is individually measurable as an on/off delta on its
parameters. The P3 harness runs exactly this gate.
