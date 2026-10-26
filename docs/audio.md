# Procedural audio

Torogao's audio core renders deterministic procedural
audio from recipes and bakes audio envelopes into parameter
curves. It is a headless core: the CLI modes below and the
registry tools (`audio-render`, `audio-validate`,
`audio-bake`) run the same code.

```
torogao-cli audio-render <recipe.json> --out <dir>
torogao-cli audio-validate <recipe.json>
torogao-cli audio-bake <file.wav> --recipe <bake.json> --out <curves.json>
```

## Recipes — `torogao-audio-recipe/1`

```json
{
  "format": "torogao-audio-recipe/1",
  "name": "binaural-bed",
  "seed": 42,
  "durationSeconds": 4.0,
  "targetLufs": -16.0,
  "truePeakDb": -1.0,
  "stems": [
    {"type": "binaural", "baseHz": 200.0, "beatHz": 10.0,
     "gain": 0.5,
     "envelope": {"attackSeconds": 0.25, "releaseSeconds": 0.25}}
  ]
}
```

Stem types:

- `sine` / `tone` — a sine at `freqHz` (both channels).
- `binaural` — two sines at `baseHz − beatHz/2` (left) and
  `baseHz + beatHz/2` (right): an exact frequency-offset
  pair, the binaural-beat primitive.
- `noise` — white noise through a one-pole lowpass at
  `cutoffHz`, generated per channel from the recipe seed.

Every stem takes `gain` (0..1), optional `startSeconds` /
`durationSeconds`, and an optional linear `envelope`
(attack/release seconds). A recipe must carry an integer
`seed`; rendering is a pure function of the recipe.

## The master standard

Renders are 48 kHz, 24-bit PCM WAV, normalized to the
recipe's loudness target (default −16 LUFS integrated,
ITU-R BS.1770-4 gated measurement) with true peak at or
under the ceiling (default −1 dBTP, measured at 4×
oversampling). When both cannot hold, the ceiling wins and
the manifest reports the loudness actually achieved —
loudness is never faked by peak normalization.

Noise comes from xoshiro256** seeded via splitmix64 (the
algorithm is named in code and in every manifest), integer
math only, so noise renders identically on any platform.
Oscillators and filters run in double precision; the same
recipe rendered twice on one machine is byte-identical.

## Manifests

Each render writes `<name>.manifest.json` next to the WAV:
the recipe file's SHA-256, the seed, the engine id, the
PRNG name, sample rate/channels/bit depth, measured
loudness and true peak, the targets, the gain applied, and
a license field (procedural synthesis is the studio's own
original work; the field exists on every manifest so
downstream consumers never guess).

## Baking — `torogao-audio-bake/1`

`audio-bake` follows a WAV's amplitude envelope with the
house one-pole follower — α = 1 − exp(−dt/τ), τ = 10 ms
attack / 80 ms release, the same constants the rig
runtime's live follower uses — and emits a
`torogao-curves/1` document (see the curves format) whose
series (named by the bake recipe's `target`, default
`AudioLevel`) can drive any parameter: mouth opening,
breath, impact accents. Bake rates are the standard set
{15, 30, 60, 120} fps, default 60.

## Scope

This is the procedural core: synthesis, normalization,
manifests, envelope baking. Voice and SFX authoring
workflows, and the hypnosis-track builder (scripted voice
over a beat bed with timed induction segments), build on
this core in later phases; heavy generative audio still
routes through the studio's audio factory tooling.
