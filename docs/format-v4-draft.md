# Sequence file format version 4 — draft (2026-11-23)

**Merged into `docs/SPEC.md` on 2026-11-24** (§3.2, §5, §6, §7.7, §9.3, §9.4, §9.6). The SPEC is the source of
truth; this draft is kept as the record of how the format was decided.

Status: **draft for approval**. Nothing is built. Once approved, this text replaces SPEC §6 (and adds to §3, §5,
§7), and `crates/seq-core` gets the model, validation, migration and tests. Release plan: `docs/release-0.5-plan.md`.

Version 4 carries everything in 0.5 that needs to be saved, in one bump:

| Feature | Where in the file | Plan |
|---|---|---|
| Quantized start | `sequence.start` (object instead of a string) | E |
| Transition per step | `step.transition`, default `lane.transition` (both optional) | A |
| Pass conditions | `step.when` (optional) | G4 |
| Alternatives per step | `step.choices` + `step.pick` (optional) | G5 |
| Section markers | `sequence.sections` (optional) | G7 |

Not in the file (runtime or settings only): cue (G1), mute/solo (G2, not saved: every start begins unmuted),
recording (G6 writes ordinary steps), pre-show check (G16), guide-line spacing (a view setting in `settings.json`),
the remembered "original" transitions (their own file, release plan §A).

## Example

```json
{
  "version": 4,
  "composition": "MyShow",
  "decks": {
    "Act 1": {
      "sequences": [
        {
          "id": "6f1c2a9e-0d5b-4c1e-9a0f-2f7d4c1b8e11",
          "name": "Drop A",
          "start": { "mode": "bar", "bars": 4 },
          "repeat": { "mode": "count", "count": 4 },
          "after_last_pass": "clear",
          "sections": [
            { "at": 0, "name": "Intro" },
            { "at": 128, "name": "Drop" }
          ],
          "lanes": [
            {
              "layer": "BG",
              "end": "loop_to_end",
              "transition": { "length": 4, "blend": "Alpha" },
              "steps": [
                { "clip": { "id": 1712345, "name": "Tunnel Red", "file": "tunnel_red.mov" }, "length": 32,
                  "choices": [ { "id": 1712399, "name": "Tunnel Blue", "file": "tunnel_blue.mov" }, null ],
                  "pick": "cycle" },
                { "clip": null, "length": 16 },
                { "clip": { "id": 1712400, "name": "Strobe" }, "length": 16,
                  "when": { "is": "last" }, "transition": { "length": 0 } }
              ]
            },
            {
              "layer": "FG",
              "mode": "slot",
              "end": "loop_to_end",
              "steps": [
                { "column": 1, "length": 16, "when": { "is": "cycle", "pass": 2, "of": 2 } },
                { "column": 2, "length": 16, "choices": [3, 4], "pick": "random" },
                { "column": null, "length": 16 }
              ]
            }
          ]
        }
      ]
    }
  }
}
```

Everything new is optional except `start`, which changes shape. A v4 file that uses none of the new features
differs from v3 only in `version` and the `start` object.

## Fields

### `sequence.start` (E) — changed

Same shape as `repeat`: an object with `mode`.

| Value | Meaning |
|---|---|
| `{ "mode": "immediate" }` | As today: starts on the tick the start is processed. Default for migrated files. |
| `{ "mode": "beat" }` | Starts on the next beat. |
| `{ "mode": "bar", "bars": N }` | Starts on the next bar whose number is a multiple of N. `bars` ∈ {1, 2, 4, 8, 16}; default 1, then it may be left out. |

- `bars` only with `mode: "bar"` (same rule as `count` in `repeat`).
- In v1–v3 files the string `"immediate"` is read as `{ "mode": "immediate" }`. In v4 the string form is an error.
- Runtime (goes into SPEC §3): the bar phase counts from the last **MIDI Start** (MIDI clock source) or the last
  **Resync** (Resolume tempo source). Today SPEC §3 says MIDI Start is only logged; it would now also set the bar
  phase, but it still never restarts running sequences. Without either, the phase counts from the first tick
  after the app connected to the clock, and the UI says "bar phase not synced".
- A start that waits shows as "starts in 3 beats" on the card; pressing ▶ again while it waits starts it
  immediately; Stop cancels it. A `§` clip start waits the same way.
- A cue (G1) uses this value by default; the cue menu can override it per cue, e.g. "end of the current pass" of
  another sequence. That choice is not saved.

### `step.transition` and `lane.transition` (A) — new, optional

Decided 2026-11-23: the transition is set **per step** ("how this clip comes in"), with an optional **default per
lane**. Resolume's layer transition is only the means: the app sets it just before each step triggers.

```json
"transition": { "length": 4, "blend": "Alpha" }
```

| Field | Meaning |
|---|---|
| `length` | Fade length in **sixteenths**, like step lengths (4 = 1 beat, 16 = 1 bar). `0` = cut. Required. The UI shows and edits it in beats (1/4, 1/2, 1, 2, 4 …). |
| `blend` | Resolume blend mode name, e.g. `"Alpha"`, `"Add"`. Optional: absent = keep the layer's blend mode as it was before the sequence started. |

Which transition a step gets:

1. its own `step.transition`, else
2. the lane's `lane.transition`, else
3. if **any** step of the lane (or the lane default) has a transition: the layer's **original** transition (the one
   it had before the sequence took it). So a step without a value always looks the same, whatever ran before it.
4. If neither the lane nor any of its steps has a transition: **Resolume's own**. The app never touches this
   layer's transition.

Runtime (goes into SPEC §7.7):
- The value is sent **ahead of the step**, not with the trigger: at the latest one sixteenth before the step,
  and only if it differs from what the layer has now. The trigger itself still goes out exactly on its tick.
  The scheduler (pure) already knows the next step, its pass condition and its pick, so this works with `when`
  and `choices`. A step at the very start of a run: the set goes out with the start, before the first trigger.
  *The probe must confirm that this order holds and that changing the value doesn't disturb a fade still
  running.*
- The length is converted to seconds at the current tempo. Resolume's limit is 10 s: a longer value is capped, and
  the step shows a warning.
- With `choices`, the step's transition applies to whichever option is picked. A step that doesn't play (`when`)
  sets nothing.
- When the sequence releases the layer, the original comes back (release plan §A).
- `blend` is checked against the layer's own option list at run time (the list comes from Resolume). Unknown name:
  only the length is set; warning on the step and in the pre-show check (G16).
- Clips with their own transition in Resolume (`layer_determined: false`) ignore the layer value: warning on the
  step and in the pre-show check.
- Validation: `length` 0–256 (16 bars); `blend` 1–64 characters.

Editor display (goes into SPEC §9.3; a new colour token `--fade` in `tokens.css`, `themes.ts` and the mockup;
sketch: `docs/images/transition-fade-sketch.png`):
- **Fade wedge:** a ramp at the left edge of the step block, rising from bottom left to top right, drawn on the
  grid scale, so its width **is** the fade length (a 1-beat fade covers the first beat of the block). Fill
  `--fade`, translucent, under the name.
- **Label** in the block's length line: `↗ 1 beat · Alpha` (blend only when set). In narrow blocks only `↗ 1`.
- **Own vs. lane default:** a step's own transition is drawn solid; one inherited from the lane default only as an
  outline (dashed), so you see at a glance which steps differ.
- **Cut (`0`) set on a step** over a lane default: a short vertical bar at the left edge instead of a wedge.
- **Fade longer than the step:** the wedge runs to the block's right edge and ends in a small `›` (the next step
  starts while this fade still runs).
- **Warnings** (capped at 10 s, unknown blend mode, clip with its own transition): the label in `--err-text` with ⚠, the
  reason as a tooltip and in the inspector.
- Steps with the original or Resolume's own transition show nothing.
- Lane header: the lane default as a small chip `↗ 1 beat`, click to edit; inspector: the step's transition
  (inherit / own: length in beats, blend mode list from the layer).

### `step.when` (G4) — new, optional

Plays the step only on some passes of the sequence. The pass is the sequence's pass counter (1-based), the same
number the card shows as `pass 2 / 4`.

| Value | Plays when |
|---|---|
| (field absent) | always |
| `{ "is": "first" }` / `{ "is": "not_first" }` | pass 1 / every pass except 1 |
| `{ "is": "last" }` / `{ "is": "not_last" }` | the last pass / every pass except the last |
| `{ "is": "cycle", "pass": A, "of": B }` | pass A of every B passes: pass `p` plays if `(p − 1) mod B == A − 1`. `2 ≤ B ≤ 16`, `1 ≤ A ≤ B`. Shown as `A:B`, e.g. `2:4`. |

- `pass` and `of` only with `"cycle"`.
- **Last pass** exists for `once` and `count`. With `repeat: infinite` there is no last pass: `last` never plays
  and `not_last` always does. Validation does not refuse it, but the step shows a warning.
- **A step that doesn't play** sends nothing, and the previous clip keeps playing through it (same as an
  unresolved step today). To get silence on some passes, use `choices` with a rest, or a rest step with a
  condition. *(Decision 1 below.)*
- Within one pass, a lane with `loop_to_end` repeats its steps; all repeats in that pass see the same pass number.
  *(Decision 2 below.)*

### `step.choices` + `step.pick` (G5) — new, optional

A step can hold up to 4 options: the step's own `clip` (or `column`) is option 1, and `choices` adds up to 3 more.

| Field | Meaning |
|---|---|
| `choices` | Clip lanes: a list of clip references or `null` (rest). Slot lanes: a list of column numbers or `null`. 1–3 entries. |
| `pick` | `"random"` (default, may be left out) or `"cycle"`. Only allowed together with `choices`. |

- `cycle`: option 1 the first time the step plays in a run, option 2 the second time, and so on, wrapping around.
  It counts every time the step plays, including loops within a pass. It starts again at each start of the sequence.
- `random`: picks one option each time the step plays. The same option twice in a row is allowed. *(Decision 3.)*
  `seq-core` stays free of IO: the engine passes in a seed at start, so tests are repeatable.
- `when` is checked first; `choices` only apply if the step plays.
- Each option is resolved by itself (SPEC §4.3); an unresolved option, when picked, triggers nothing (as today).
  Relinking, the red "unresolved" mark and the pre-show check cover every option.
- Clip lanes: `choices` holds clip references, slot lanes: columns. The other kind is an error (same rule as
  `clip` and `column`).
- The editor shows the step's block with a small `1/3` badge and the thumbnail of option 1; the inspector lists
  all options.

### `sequence.sections` (G7) — new, optional

```json
"sections": [ { "at": 0, "name": "Intro" }, { "at": 128, "name": "Drop" } ]
```

- `at`: position from the start of the sequence, in sixteenths (on the grid). `name`: 1–64 characters.
- Sorted by `at`, no two sections at the same position. At most 64 sections.
- A section may lie beyond the current length of the sequence (it is shown greyed out); shortening lanes never
  deletes or moves sections.
- For now only markers in the editor (and in the FFGL view later, if wanted). "Start from section" / "loop
  section" are not part of 0.5.
- Guide lines every 8/16/32 bars are a view setting in `settings.json`, not in this file.

## Validation (additions to `seq_core::validate`)

- `start.bars` ∈ {1, 2, 4, 8, 16} and only with `mode: "bar"`.
- `transition.length` (step and lane) 0–256; `blend` 1–64 characters.
- `when`: known `is`; `pass`/`of` only with `cycle`, in range.
- `choices`: 1–3 entries, matching the lane mode; columns ≥ 1; `pick` only with `choices`.
- `sections`: sorted, unique `at`, names 1–64 characters, at most 64.
- Unknown fields stay an error everywhere (`deny_unknown_fields`, as now).

## Migration

- v1, v2, v3 load as today; additionally `start: "immediate"` becomes `{ "mode": "immediate" }`. Nothing else
  changes: no new field is added to old files.
- The first save writes version 4 and keeps the original as `<file>.v3.bak` (or `.v1.bak`, `.v2.bak`), as before.
- A file with a version above 4 is refused, not overwritten (as now).
- **In-app update and rollback (D):** once a file is saved as v4, 0.4.x can no longer read it. A rollback during an
  update happens before the new version has saved anything, so it is safe. Going back to 0.4 by hand later
  needs the `.v3.bak`; the manual says so.

## Edit protocol (SPEC §9.6)

New edit operations (with undo): set sequence start, set/clear lane default transition, set/clear step transition, set/clear step condition,
add/remove/reorder/relink a step choice and set pick, add/move/rename/delete a section. The step's flat serde
form (`{"clip": …, "length": 8}`) gains the optional `when`, `choices`, `pick`, `transition`.

## Decisions (Uli, 2026-11-23)

1. **A step whose condition is false:** nothing is sent; the previous clip keeps playing.
2. **Pass number inside a looping lane:** all loops of a short lane in one pass count as that pass.
3. **Random pick:** the same option may come twice in a row.
4. **MIDI Start sets the bar phase** for quantized starts (it still never restarts sequences); Resync does the
   same with the Resolume tempo source.

5. **Transitions per step with a per-lane default**, not per clip; the editor draws the fade on the step.
