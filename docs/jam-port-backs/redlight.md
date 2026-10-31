# Port-back — Redlight (W5) → Torogao

Filed at v1.0.0, 2026-10-30. All findings technique-shaped; content
labels per item (§9 lane law: label governs flow, not origin).

## 1. Reaction-set sizing law (content_label: sfw)

A reactive voice set of **6 lines per event class** with a hard
no-immediate-repeat rule sustains a full session without a repeated
line being *felt*: across the conformance and playtest runs, sessions
fired 7–16 events over 3 classes and surfaced 6–12 distinct lines of
18 with zero immediate repeats (test A06). Below ~4 lines per class the
repeat window collapses and repetition shows within one session; 6 is
the floor for a session-length sampler, and longer scenes should scale
lines-per-class with expected events-per-class, not with total lines.
Torogao voice production: budget **≥6 lines per (character × event
class)**, selection = uniform pick with last-line exclusion, coverage
measured as distinct lines heard per class — never as total lines
recorded.

## 2. Attention economy numbers (content_label: sfw)

The playable loop, in milli-units (0–1000): decay 0.30 / 0.38 / 0.46
per step across the three audience sizes; a pose is worth +55 on a
30-step cooldown; applause +70; a timed lens answer +95; a compliment
raises the floor by +50 (cap 500) plus +35 immediate. Decay never
crosses the floor. Comparison with Exposure's slot economy: Exposure
decays attention ×0.88 per slot (≈ −12% of current per decision point)
with praise worth ×3/2 while a recorded context is warm — a
multiplicative, decision-point economy. Redlight's is additive and
continuous: the floor mechanic is the transferable difference. A floor
that only praise can raise turns compliments from flavour into
structure — the player *farms the floor early* and spends it late,
which is the session arc. Torogao should adopt the floor as a named
parameter (`praise_floor_step`, `praise_floor_cap`) in any
attention/presence system, with decay forbidden below it.

## 3. C# / Unity verdict (content_label: sfw)

The pure-C# core pattern carried the whole design with zero engine
contact: a plain net8.0 class library + console test runner (24 checks,
warnings-as-errors, nullable on) built and gated in the sandbox in
seconds. The JS realization, transliterated branch-for-branch and
locked by C#-dumped conformance vectors (3/3 scenarios exact, checksum
over snapshots + final draw list), cost less than a day and removes all
behavioural risk from the deferred Unity leg: what remains on primo is
*packaging* (WebGL build, single-file inlining, tackle observation),
not behaviour. Engine-week cost verdict: **the deferred-packaging seam
works** — a Unity week can ship a gated, playable page on the
conformance seam while the editor leg waits, at the price of
maintaining the transliteration (small, and machine-checked).

## §9 transfer filing

- Reaction-set sizing law — sfw — transferable.
- Attention floor mechanic + numbers — sfw — transferable.
- C#/conformance seam verdict — sfw — transferable.
- Lux's line texts, audience presets, and the body-writing archive
  content — nsfw — stay in this lane.


## Adaptation filed 2026-10-31 — post 11924184 (content_label: nsfw)

Style A (Cinematic Recorded) adaptation of the fathom RE recipe for
post 11924184 (deep-throating performance; recipe at
`~/workspace/fathom-re-11924184-20261031/RECIPE.md`): the source's
two-shot grammar and phase-locked gaze land as the Lens Toy
programme "Deep Take" — gaze windows score as timed lens answers
(+95), deep beats in the applause class (+70), one new reactive
voice class (*gaze*, ≥6 lines per the sizing law), the cut as the
Glimpsed → Known revelation beat. Full spec:
`docs/ADAPT-11924184.md` in this repo. NSFW — stays in this lane.
