# Release 0.5.0 — feature plan (draft, 2026-11-23)

Status: proposal for discussion. Nothing here is decided until it moves into `docs/PLAN.md` as a milestone.
Last release: 0.4.0 (2026-11-22, LUTs). Themes for 0.5: **smoother playback in Resolume** (transitions) and
**closing the loose ends** of earlier milestones.

## Overview

| # | Item | Size | Proposal |
|---|------|------|----------|
| A | Layer transitions, restored when the sequence stops (new M19) | L | **must**, headline feature |
| B | Open live and Windows checks from M6, M13, M15, M18, M10 soak | S–M | **must**, before tagging |
| C | Fix the "started twice" limitation (UI start + `§` clip, SPEC §7.1) | M | **must** (decided 2026-11-23) |
| D | In-app update check (M14, planned 2026-11-04) | L | **must** (decided 2026-11-23) |
| E | Start quantization (next bar / next beat; SPEC §3, §13) | M | **must** (decided 2026-11-23) |
| G | Cue, mute/solo, pass conditions, alternatives, record, sections, pre-show check | L | **must** (approved 2026-11-23) |
| F | Housekeeping: stale M11c boxes in PLAN, dependency PRs, LUT effects in layer groups | S | must (small) |

---

## A. Layer transitions (proposed M19)

**Goal:** a lane can set its layer's transition (duration and blend mode) while the sequence runs. When the
sequence stops, the layer gets back exactly the transition it had before. The user never has to clean up by hand,
so there is **no "remove all transitions" button**.

### What Resolume offers (from `docs/samples/composition.json`, not yet probed)
- Layer: `transition.duration` (ParamRange 0–10 s) and `transition.blend_mode` (ParamChoice, 51 options), each with
  a parameter id.
- Clip: the same two plus `layer_determined` (bool). The clips in the samples are all set to follow their layer.
- Setting parameters by id over the WebSocket works for other params (notes §5.2, §16). Not yet tried for these.

### Proposed behaviour
- **Per lane** (= per layer within a sequence), in the lane header: *Transition: Resolume's own* (default, the app
  touches nothing) or *duration + blend mode*. The duration is in grid units (1/16 to N bars) and converted to
  seconds at the current tempo, so a "1 beat" fade stays one beat when the BPM changes.
- **Take over:** when a lane takes its layer (start, or taking it from another sequence), the app sets the
  layer's transition. The value from before the **first** takeover is remembered once per layer ("original").
- **Hand back:** when the last running lane releases the layer (stop, finish, Stop all, deck change, lane taken
  over by a sequence whose lane says *Resolume's own*), the original comes back. A layer passed from one sequence to
  another keeps the first original, not the in-between value.
- **The user wins:** if the layer's transition changes in Resolume while we hold it, and the change isn't one we
  made (WS subscription by id), we drop the remembered original and restore nothing: the user's new value stays.
  Same idea as the manual override in §7.3.
- **Crash or quit while running:** the remembered originals are also written to a small file in the data folder
  (`transitions-pending.json`, keyed by composition name and layer id). On the next connection to the same
  composition they are restored and the file is deleted. A composition reload makes the ids invalid, so the
  entries are dropped with a log line.
- **Clips with their own transition** (`layer_determined: false`) ignore the layer value. The lane header shows a
  hint ("2 clips use their own transition"). The app doesn't change clip-level transitions, so the user's
  composition stays as it was.
- **Visible in the UI:** the lane header shows the active transition; the status bar or the log shows "restored
  transition on BG".

### Things the probe has to answer first (notes §19)
1. Do layer `transition.duration` / `blend_mode` take a WS `set` by id and by path, and what do the
   `parameter_set` / `parameter_update` messages look like? Units (seconds? 0–1 normalised?).
2. **Ordering:** if the `set` goes over the WebSocket and the `connect` over OSC in the same tick, does the trigger
   already use the new transition? If not, how far ahead must the set be sent? (The engine never waits, so the
   result decides whether the set goes out at start, or one step ahead of the first trigger.)
3. Does a layer **clear** (`/layers/{L}/clear`) also fade with the transition, or does it cut? (This matters for
   `after_last_pass: clear` and for the order of "clear, then restore".)
4. What does `layer_determined` on a clip mean exactly: does it cover the blend mode, the duration, or both?
5. Does a deck switch or a composition reload change the layer transition param ids? (Layer ids stay the same, notes §2.)

### Open decisions (for Uli)
- **File format:** a lane gets an optional `transition: { "length": <sixteenths>, "blend": "<name>" }`. That
  means **format version 4** with a v3→v4 migration (CLAUDE.md: don't change the format without asking).
  Alternative without a format change: a per-layer setting in `settings.json`, the same for every sequence on that
  layer. Proposal: in the lane, because different sequences want different fades.
- **Duration unit:** decided 2026-11-23: **beats**, converted to seconds at the current tempo.
- **Restore point:** at the release of the layer (proposed). Alternative: only when *all* sequences have stopped.

### Tasks (draft)
- [ ] Probe (`tools/resolume-probe`), record in notes §19, save samples.
- [ ] SPEC: new §7.7 Transitions; §6 format v4; §9.3 lane header; §9.6 protocol.
- [ ] `seq-core`: lane `transition`, v4 + migration, validation, tests; pure "transition owner" state (originals,
      takeover, hand-back, user change), driven by events, unit-tested.
- [ ] Engine and `resolume-client`: subscribe by id, set/restore off the timing path (WS is async already);
      pending file; mock-resolume transition params; integration tests (stop, finish, Stop all, deck change,
      lane takeover, user change, crash restore).
- [ ] UI: transition control in the lane header, clip hint, restored message.
- [ ] Live check in Resolume; manual, changelog, README.

---

## B. Open checks from earlier milestones (before tagging 0.5.0)
- [ ] M6: 2-lane sequence in time with the clock (Uli, Resolume). Probably long done in practice: tick it.
- [ ] M10: 2 h soak with the mock at 128 BPM, no drift (`examples/soak`).
- [ ] M13: slot lanes live: swap clips, drop media onto the playing slot, thumbnail after a drop.
- [ ] M15: Windows: window and splash colours, fonts in WebView2, zip size.
- [ ] M18: Resolume with a 33³ `.cube`, `DOMAIN_MIN/MAX`, reload speed during a fast drag (sets the write throttle).

## C. Started twice from UI and `§` clip (SPEC §7.1, known limitation; in 0.5, decided 2026-11-23)
Fix already sketched in the SPEC: starting a running sequence keeps the run and its `§` link instead of
restarting it as a UI start. Decide: should a second start still jump back to step 1 (explicit restart), or be
ignored? Proposal: from the UI, ▶ on a running sequence restarts it but keeps the `§` link; the `§` clip
connecting while the sequence runs only adopts the run.

## D. In-app update (M14; in 0.5, decided 2026-11-23)
Planned in detail in `docs/PLAN.md`. Still open: how the binary is replaced. If that decision doesn't come soon,
ship **check only** in 0.5 (menu entry, dialog with notes, "open release page") and the replacement in 0.6.
That way the check, the privacy texts and the CLI `--check-update` go out now.

## E. Start quantization (in 0.5, decided 2026-11-23)
The `start` field already exists (`"immediate"`). Add `"next_beat"` and `"next_bar"` (bar phase from the start of
the clock or from Resolume's Resync). It fits with transitions: a fade that starts on the bar sounds right.
Needs a format change too, so it goes into v4 together with A (one format bump).

## F. Housekeeping
- [ ] PLAN.md: M11c and the plugin docs are shipped since 0.2.0, but their boxes are still open; M10/M11 still say
      "⟵ next".
- [ ] Dependency PRs: cargo, npm and actions minor bumps (#15–#17) merge; **TypeScript 7 (#12)** is a major bump:
      check `npm run check` and the Svelte tooling first, or hold it.
- [ ] LUT effects inside layer groups are not found yet (M18 note): small, could ride along.

## G. Performance and editing features (approved 2026-11-23)

Approved for 0.5 from the suggestion list. Each still needs a SPEC section before it is built.

- [ ] **G1 Cue next sequence:** while one plays, cue another; it starts at the next bar, at the end of the current
      pass, or at the next 8/16 bars. Shares the quantization of E.
- [ ] **G2 Mute / solo lane live:** a muted lane keeps counting but sends no triggers and comes back in time at its
      next step.
- [ ] **G4 Pass conditions per step:** play a step only on some passes ("every 2nd", "last pass only", "not on
      the first"). Format v4.
- [ ] **G5 Alternatives per step:** 2–4 clips per step, picked at random or in rotation each pass. Format v4.
- [ ] **G6 Record from Resolume:** arm, click clips in Resolume in time, they land on the grid (snapped to 1/16).
- [ ] **G7 Section markers:** named sections ("Intro", "Drop") and 8/16/32-bar guide lines in the editor.

- [ ] **G16 Pre-show check:** one button that checks all sequences of the composition and lists what would go
      wrong live: unresolved clips, missing layers, Beat Snap on in Resolume, clips with their own transition
      (they ignore A), clips shorter than their step that don't loop. Uses data the app already has. Beat Snap
      is recorded (notes §2: `clipbeatsnap`); the clip play mode field (Loop / Play once …) still needs a probe.

Rejected 2026-11-23: performance view, rehearse without Resolume, copy sequences between decks/compositions.
Not for now (2026-11-23): MIDI controller mapping, fill button, half/double time, chain sequences, shift lane,
pattern fill.

**Format v4** therefore carries: lane `transition` (A), `start` values (E), step conditions (G4), step
alternatives (G5) and sections (G7). Design them together, then one bump and one migration.

## Proposed order
1. F (cleanup) and the probe for A → decide the format question.
2. A (M19), together with E if E is wanted (one format bump).
3. C.
4. D (check only, or full).
5. B checks, then release 0.5.0 (`docs/RELEASING.md`).
