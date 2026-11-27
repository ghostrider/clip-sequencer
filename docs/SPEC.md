# Resolume Clip Sequencer — Specification

Status: concept agreed 2026-10-25. This document is the source of truth.
Resolume API details were verified in Milestone 0 against Arena 7.28.1; endpoints, message shapes,
timings and samples are in `docs/resolume-api-notes.md` (cited below as *notes §n*).
Release 0.5 (decided 2026-11-23/24): file format v4 (§6), quantized start (§3.2), transitions per step (§7.7),
starting a running sequence (§7.1), in-app update (§15).

## 1. Purpose

A live tool and a playback engine for pre-composed clip sequences in Resolume Arena 7.26.

- A **sequence** plays clips on several Resolume layers in parallel, on a 1/16-bar grid,
  locked to the show's MIDI clock or to Resolume's tempo (§3.1).
- Sequences are stored, edited in a browser UI, and started either from the UI or
  from inside Resolume (by triggering a placeholder clip).
- Clips are **not** triggered via MIDI. MIDI is used only to receive the clock.

## 2. System context

```
BPM detection app ──MIDI clock──► MIDI Clock Shifter ──shifted clock──┬──► Resolume (MIDI clock sync)
                                   (separate app, virtual MIDI port)  └──► Sequencer (counts ticks)

Sequencer ──OSC (UDP)──────────► Resolume   triggers (timing-critical)
Sequencer ◄─WebSocket──────────► Resolume   state: connected clips, deck changes, composition updates
Sequencer ──REST (HTTP)────────► Resolume   composition dump, thumbnails, deck select, set clip source
Sequencer ──HTTP + WebSocket───► Browser UI
```

The sequencer receives the **same shifted clock as Resolume**, so projector-latency
compensation is already handled upstream by the Clock Shifter. The sequencer never
compensates latency itself.

## 3. Clock and timing

- **Input:** MIDI clock at 24 PPQN from a named MIDI input port (configurable), or ticks the app makes at
  Resolume's tempo (§3.1).
  Messages used: Timing Clock (0xF8). Start (0xFA) sets the **bar phase** for quantized starts
  (§3.2). Start, Stop (0xFC) and Continue (0xFB) are logged and **never** affect running sequences
  (the Clock Shifter's resync button sends Start; it must not restart sequences).
- **Grid:** 1/16 bar in 4/4 = one sixteenth note = **6 ticks**. All step lengths are
  integers in sixteenths (file format v2, 2026-11-01; v1 counted eighths, see §6). The editor
  snaps to eighths by default and to sixteenths with Shift (§9.3). Only 4/4 is supported.
- **Start:** a sequence starts on the tick its start mode gives (§3.2): the tick its start is
  processed (`immediate`), or the next beat or bar. Its internal position counts ticks from that
  tick. Step boundaries fire exactly on the tick where they fall.
- **Trigger timing:** the OSC message for a step is sent on the tick the step begins.
  No lookahead. Resolume's Beat Snap must be **off** for clip triggers.
- **Tempo:** derived from tick intervals (moving average) for display only.
- **Clock loss:** if no tick arrives for > 2 beats at the last known tempo, running
  sequences **freeze** (position stops, no triggers), the UI shows "no clock".
  When ticks resume, sequences continue from where they froze.

### 3.1 Resolume tempo as clock (M17, decided 2026-11-19)

For setups without a MIDI clock. The clock source is a setting: **MIDI clock** (as above, default) or
**Resolume tempo**. Resolume reports its tempo, transport, taps and resync over the WebSocket, but never its
beat phase (*notes §17*), so the app makes its own ticks.

- **Ticks:** the app generates 24 PPQN at Resolume's current tempo on its own thread (`clock_in::ResolumeClock`;
  short waits sleep in 1 ms slices, which Rust's `thread::sleep` times with a high-resolution timer on Windows).
  Each tick is stamped with the time it was due. Measured 2026-11-19: ticks arrive 0.4 ms after that (median),
  1.5 ms at most, none lost or added (`cargo run --release -p clock-in --example resolume_clock_jitter`). A tick
  more than 250 ms late is not caught up (the grid restarts from then). The engine sees the same ticks as from
  MIDI; scheduler and sequences don't change.
- **Tempo:** `tempo`, `play` and `resync` are subscribed by parameter id (ids from `tempocontroller` in each
  composition push) whatever the source, so switching needs no new subscriptions. A tempo change applies from
  the next tick (the rest of the current tick interval is scaled).
- **Transport:** **Pause** and **Stop** freeze the ticks (running sequences freeze as on clock loss; the
  message says "Resolume paused" / "Resolume stopped" instead of "no clock"). **Play** after Pause continues;
  Play after **Stop** counts as a resync. The state in a composition push or a subscription reply (on connect,
  after a reload) only starts or stops the ticks, without a resync: the phase is unknown then.
- **Resync:** a Resync press (the `resync` press update, not its release) restarts the tick phase at that
  moment **and jumps every playing sequence to its bar 1**: step 1 of each lane is triggered on the next
  tick, the pass counter is kept. It also sets the bar phase (§3.2). (Different from MIDI Start, which
  never restarts sequences, see above.)
- **Taps:** tempo changes from tapping arrive as tempo updates; a tap does not move the phase.
- **Phase:** until the first Resync (or Play after Stop) the app's beat is not aligned with Resolume's;
  the clock chip shows `RESOLUME 128.0 BPM · press Resync` until then, `· synced` after.
- **Without Resolume:** no ticks (as with no MIDI port); the chip shows `not connected`. After a reconnect
  the phase is unknown again.
- **Setting:** Preferences → Clock (`clock_source` in `settings.json`, `midi` by default); `--clock midi|resolume`
  wins for the run. The MIDI port stays saved while the source is Resolume. Switching takes effect at once.

### 3.2 Quantized start (release 0.5, decided 2026-11-23)

Each sequence has a **start mode** (`start` in the file, §6): `immediate` (as before), `beat` (the next beat) or
`bar` with `bars` N ∈ {1, 2, 4, 8, 16} (the next bar whose index is a multiple of N).

- **Bar phase:** bars are counted from the last **MIDI Start** (MIDI clock source) or the last **Resync** (Resolume
  tempo source, §3.1): that tick is the start of bar index 0. Neither restarts running sequences (§3, §3.1 is
  unchanged in that). Without either since the clock connected, the phase counts from the first tick received,
  and the clock chip adds `· bar phase not synced`.
- **Boundary:** a start processed exactly on a matching boundary tick starts on that tick; otherwise it waits for
  the next one. A Resync or MIDI Start while a start waits moves the phase, and the wait is computed again.
- **Waiting:** the card and the editor header show `starts in 3 beats` (`in 2 bars` for longer waits). **▶ again
  while waiting** starts it at once; **Stop** cancels the wait. A start from a `§` clip (§7.1) waits the same way
  and is linked to that clip as soon as it is pending. A deck switch (§7.2) happens first; the wait counts from the
  moment the new deck is confirmed.
- **Clock loss or pause** freezes a waiting start like a running sequence; it starts when the boundary is reached
  after the ticks resume.
- **Cue (G1):** §3.3.

### 3.3 Cue (release 0.5, decided 2026-11-25)

A **cue** says "play this next": the cued sequence starts at a cue point and **every other running sequence stops
there** (layers the cued one also uses are taken over as in §7.3; the others get their `after_last_pass`). Cues are
not saved.

- **How:** Shift+click on ▶ (card or editor header) or Shift+Space with the sequence selected cues it at its start
  mode (§3.2). The card's context menu **Cue at…** offers the cue points: *start mode* · *end of the current pass of
  <running sequence>* (one entry per running sequence) · *next 8 bars* · *next 16 bars*. Only sequences of the live
  deck can be cued (a cue never switches decks).
- **Cue points:** *start mode* and *next 8 / 16 bars* wait on the bar phase like a quantized start (§3.2; 8 and 16
  bars are the `bar` boundaries with N = 8 or 16). *End of the current pass* is the tick where that sequence's pass
  ends; if that sequence stops or finishes earlier, the cue starts then. With nothing running, a cue is a start.
- **One cue:** a new cue replaces the previous one. ▶ on the cued sequence starts it at once (replacing what plays);
  Stop on it, or × on its badge, cancels the cue. Stop all cancels it too.
- **Waiting:** the card shows `CUED · in 2 bars` (`· at end of pass` for that point), the editor header the same. A
  cue freezes with the clock like a quantized start, and a phase reset (MIDI Start, Resync) computes its wait again.
- A cued sequence that is already running restarts at the cue point (§7.1), the others stop.

## 4. Resolume interface

### 4.1 Channels
| Purpose | Channel | Notes |
|---|---|---|
| Trigger clip | OSC `/composition/layers/{L}/clips/{C}/connect` int 1 | UDP to Resolume OSC input (default 7000). Indices 1-based (same as REST). Send int 1 only — never a release (int 0), which would disconnect clips with Piano trigger style. *notes §6* |
| Clear layer | OSC `/composition/layers/{L}/clear` int 1 **then** int 0, back to back | for lane end `clear` and stop. Edge-triggered: without the int 0 the next clear is ignored. *notes §6* |
| State feedback | WebSocket `ws://host:8080/api/v1` | `{"action":"subscribe","parameter":"/parameter/by-id/<connected param id>"}` for clips on owned layers and the SEQ layer; updates arrive as `{"type":"parameter_update","id":…,"value":"Connected",…}`, matched by `id`. The full composition is pushed (no `type` field) on structural changes, clip connect changes and deck switches — **not** on renames. *notes §5* |
| Composition dump | REST `GET /api/v1/composition` | active deck only (layers are shared by all decks; columns and clips are per deck). *notes §2* |
| Thumbnails | REST `GET /api/v1/composition/clips/by-id/{id}/thumbnail` (PNG, `ETag` = `last_update`) | *notes §3* |
| Deck switch | REST `POST /api/v1/composition/decks/{n}/select` (OSC `/composition/decks/{n}/select` int 1 also works) | completion = the WS composition push with deck `n` selected (100–190 ms). REST reads during a switch return inconsistent data. *notes §4* |
| Set clip thumbnail | REST `POST /api/v1/composition/clips/by-id/{id}/thumbnail`, multipart field `file`, PNG | used to push generated sequence thumbnails; leaves the clip's source unchanged. *notes §3* |

### 4.2 Clip addressing
OSC addresses use layer and column indices. These shift when layers or columns are
inserted, so **indices are never stored**. They are resolved at runtime from the
composition index (rebuilt when a composition push changes structure: ids, names, files,
column count — pushes also arrive for plain connect changes, ~55 ms after each trigger, and are
250–380 KB, so they are parsed off the engine task).

Resolume only exposes the **live** deck's clips. The app keeps the last composition seen for each
deck (on disk, next to the thumbnail cache) and uses it to show and resolve sequences of decks
that are not live. Decks never seen live have no clip data until they are made live once.

Renames do not trigger a composition push, so the engine also subscribes (by id) to the `name`
parameters of the live deck's layers and clips and patches the index on `parameter_update`.

### 4.3 Clip resolution (per step)
A step stores up to three identifying features, resolved in order within the
sequence's deck:
1. `id` — Resolume clip ID (fast, valid within a session).
2. `name` — clip name, preferably on the lane's layer, otherwise anywhere in the deck.
3. `file` — media file name (fallback after renames).

If none matches, the step is **unresolved**: shown red in the UI, treated as a rest at
runtime (nothing triggered, nothing crashes). When resolution succeeds via 2 or 3, the
stored `id` is refreshed.

Details (from the clip-id probe, *notes §2.1*: an id belongs to a grid cell object that travels
with drag/swap/move but keeps its id when other media is loaded into it or it is cleared):
- An `id` match is ignored when the slot is empty, or when every stored content feature
  contradicts it (stored name ≠ clip name **and** stored file ≠ clip file). A reference without
  a file fits any clip without a file, so renamed generator clips stay resolved.
- Name compares exactly; file compares the media file name, case-insensitively.
- Among several candidates: the lane's layer first, then a candidate that also matches the other
  feature, then the lowest (layer, column). A pick from several equal candidates is flagged
  **ambiguous** (UI warning) but still resolved.
- A clip found on another layer than the lane's plays on its own layer (it is triggered at its
  own position); the UI flags it.

### 4.4 Layer references
Lanes reference layers **by name**. If a layer name is not found in the deck, the whole
lane is unresolved (red header, silent at runtime).
Unnamed Resolume layers all carry the raw name `Layer #` in the API (Resolume only *displays*
them as "Layer 1", "Layer 2", …), so they cannot be told apart. Lanes require real layer names
(decided 2026-10-26: no matching on the displayed "Layer N", which would move lanes to another
layer when layers are inserted or reordered). The UI shows such layers as "Layer N · unnamed,
rename in Resolume to use it"; a lane on an ambiguous name is unresolved.

### 4.5 Slot lanes (positional sequencing, M13, decided 2026-11-03)
A lane is in one of two modes, set per lane:
- **`clip`** (default, everything above): a step references a clip by identity (§4.3) and follows
  it when it is moved.
- **`slot`**: a step references a **column number** on the lane's layer. Whatever clip sits in
  that slot when the step starts is triggered. The sequence keeps its rhythm; the content is
  changed live by dragging other clips or media into those slots in Resolume (like a drum
  pattern playing notes whose samples are swapped on the pads).

Rules for slot lanes:
- The column is a plain 1-based index into the deck's columns, deliberately **not** stable:
  inserting or removing columns shifts what plays ("column 3" is always the third column). This is
  the one exception to "indices are never stored" (§4.2).
- The slot is always on the **lane's own layer**. Layer lookup by name is unchanged (§4.4).
- The slot is read **when the step starts**; the running sequence's snapshot (§7.1) holds the
  column, not a clip. The trigger is sent to the slot's position
  (`/composition/layers/{L}/clips/{column}/connect`, `L` from the current index), so it always plays
  what is in the slot now.
- The composition index can be stale after drags and drops in Resolume's UI (no push, *notes §15*).
  So a background task fetches the slot (`GET /composition/layers/{L}/clips/{column}`, ~11 ms) about
  150 ms before each slot step starts and reports it to the engine by message (id, empty or not,
  name, file). The engine uses the newest report it has at step start and the index if none
  arrived; it never waits for it. The same reports refresh the UI's slot thumbnails.
- An **empty slot**, or a column beyond the deck's column count, acts as a **rest**: it clears the
  lane's layer (unlike an unresolved clip step, which triggers nothing). There is no
  "unresolved" state for slot steps.
- A slot step whose slot holds the clip that is already playing re-triggers it, as with clip steps.
- Confirmation (§7.5) and manual-override detection (§7.3) work on the slot's clip id at
  trigger time.
- **Content changed in the playing slot** (new media dropped onto it): Resolume stops the playing
  clip and does not start the new media (*notes §15*), so the layer goes dark. The sequencer does
  not react: the lane is not yielded, keeps its timing and does not re-trigger. The new content
  plays when a step for that slot starts next, as the sequence says.
- Dragging the playing clip to another slot also stops it in Resolume (*notes §15*); the lane
  carries on the same way.
- Open (not probed): whether moving a stopped clip into, or dropping a file into, an **empty** slot
  produces a push. The pre-step fetch covers both.

## 5. Concepts

- **Composition** — the open `.avc`. Sequences are stored per composition.
- **Deck** — every sequence belongs to exactly one deck. Sequences never use clips from another deck.
- **Sequence** — name, deck, start mode (§3.2), repeat setting, end action, optional section markers, one or
  more lanes.
- **Lane** — targets one layer (by name), in `clip` or `slot` mode (§4.5). Has its own steps and its own length. Lanes can be
  reordered in the editor (order is display only). The lanes of one
  sequence target different layers. A lane without steps, or whose layer is not found, is silent.
- **Step** — a clip reference (or `null` = rest) and a length in sixteenths. A **rest clears the
  lane's layer** when it starts (silence). If the clip the lane triggered last lives on another
  layer (it plays there) and is still playing, that layer is cleared too: a rest stops what the
  lane started. The same applies to every clear of a lane (lane end `clear`, `after_last_pass`
  clear, stop all). An *unresolved* step triggers nothing, so the
  previous clip keeps playing through it.
  A step can also have (format v4, §6): a **condition** (`when`: plays only on some passes; a step that
  doesn't play sends nothing, so the previous clip keeps playing through it), **choices** (up to 3 more clips,
  columns or rests, one picked each time the step plays), and a **transition** (how its clip comes in, §7.7;
  a lane can set a default).
- **Section** — a named marker at a grid position of a sequence ("Intro", "Drop"); shown in the editor only.
- **Sequence length (one pass)** — the length of its longest lane.
- **Lane end behaviour** (when a lane is shorter than the sequence):
  - `loop_to_end` — restart the lane's steps until the pass ends.
  - `hold` — keep the last clip playing until the pass ends.
  - `clear` — clear the layer and stay silent until the pass ends.
- **Repeat** — `once` | `count` (N ≥ 2) | `infinite`. At the end of a pass, if passes remain,
  all lanes restart at step 1 in the same tick.
- **After last pass** — `clear` (clear all owned layers) | `hold` (leave the last clips playing).

## 6. File format

One file per composition: `<data-dir>/<CompositionName>.seq.json` (`--data-dir`, configurable).
The API does not expose the `.avc` path (*notes §2*), so the file cannot be placed next to it
automatically. Sequences are grouped by deck name.

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

- `start` (version 4): `{ "mode": "immediate" }` | `{ "mode": "beat" }` | `{ "mode": "bar", "bars": N }`,
  N ∈ {1, 2, 4, 8, 16}, default 1 (then it may be left out); `bars` only with `"bar"`. Meaning: §3.2.
- `repeat.mode`: `"once"` | `"count"` | `"infinite"`; `count` only with `"count"`.
- `length`: integer, in sixteenths (16 = one bar), ≥ 1.
- `mode` (lane, version 3): `"clip"` (default when omitted) | `"slot"` (§4.5). Steps of a clip
  lane have `clip` (object or `null` = rest); steps of a slot lane have `column` (integer ≥ 1,
  or `null` = rest). The other field is not allowed.
- `sections` (sequence, version 4, optional): `[{ "at", "name" }]`. `at` in sixteenths from the start of the
  sequence; sorted by `at`, no two at the same position, at most 64; `name` 1–64 characters. A section may lie
  beyond the sequence's current length (shown greyed out); changing lanes never moves or deletes sections.
  Guide lines every 8/16/32 bars are a view setting in `settings.json`, not part of this file.
- `transition` (lane default and step, version 4, optional): `{ "length", "blend"? }`. `length` in sixteenths,
  0–256 (`0` = cut); the UI shows it in beats. `blend`: a Resolume blend mode name, 1–64 characters; absent = keep
  the layer's original blend mode. A step's own `transition` wins over the lane's. Which transition a step gets
  and how it is applied: §7.7.
- `tie` (step, version 4, optional, decided 2026-11-27): `true` ties the step to the step **directly before** it
  in its lane (across the loop: the lane's last step of the pass before). When the step starts and its picked
  clip (or column) is the one the lane's layer still plays, nothing is sent: the clip plays on, no restart, no
  new transition. Otherwise (the first start, another clip, a rest or a mute in between, someone else changed the
  layer) it triggers as usual. Only written when `true`. Versions 1–3 have no ties.
- `when` (step, version 4, optional): plays the step only on some passes. The pass is the sequence's pass counter
  (1-based, `pass 2 / 4` on the card); all loops of a short lane within one pass count as that pass.
  - `{ "is": "first" }` / `{ "is": "not_first" }`: pass 1 / every pass but 1.
  - `{ "is": "last" }` / `{ "is": "not_last" }`: the last pass / every pass but the last. With
    `repeat: infinite` there is no last pass: `last` never plays, `not_last` always does (the step shows a warning).
  - `{ "is": "cycle", "pass": A, "of": B }`: pass `p` plays if `(p − 1) mod B == A − 1`; `2 ≤ B ≤ 16`,
    `1 ≤ A ≤ B`; shown as `A:B`. `pass` and `of` only with `"cycle"`.
  - A step that doesn't play sends nothing: the previous clip keeps playing through it (as with an unresolved
    step). For silence on some passes use a rest with a condition, or a rest among the `choices`.
- `choices` + `pick` (step, version 4, optional): the step's own `clip` / `column` is option 1, `choices` adds 1–3
  more of the same kind (clip references or `null` in clip lanes, columns ≥ 1 or `null` in slot lanes).
  `pick`: `"random"` (default, may be left out) or `"cycle"`; only together with `choices`.
  - `when` is checked first; an option is picked only when the step plays.
  - `cycle`: option 1 the first time the step plays in a run, then 2, 3 …, wrapping; every play counts, loops
    within a pass included; each start begins again at option 1.
  - `random`: any option each time, the same one twice in a row allowed. `seq-core` gets the random seed from the
    engine at start (no IO in `seq-core`, tests are repeatable).
  - Each option is resolved by itself (§4.3); a picked unresolved option triggers nothing. Relink, the red
    unresolved mark and the pre-show check cover every option.
- **Version 4** (release 0.5, decided 2026-11-24) changes `start` from a string to an object and adds `sections`,
  `transition`, `when`, `choices`, `pick` and (2026-11-27) `tie`. All new fields are optional, so a v4 file that uses none of them
  differs from v3 only in `version` and `start`.
- **Version 3** (M13, 2026-11-04) adds `mode` and `column`; lengths as in version 2.
- **Version 2** (2026-11-01) counts `length` in sixteenths.
- **Version 1** (until 2026-11-01) had the same shape with `length` in eighths. It is still
  read: lengths are doubled on load, so timing is unchanged.
- **Loading older versions:** v1–v3 load as before; their `start: "immediate"` becomes `{ "mode": "immediate" }`
  (in v4 the string form is an error). No other field is added. The first save writes the current version and
  keeps the original as `<file>.v<N>.bak`. Versions 1 and 2 cannot contain slot lanes. A file with a higher
  version than the app knows is refused, not overwritten.
- **Going back to 0.4:** 0.4.x cannot read version 4. The update rollback (§15) happens before the new version
  saves anything; a manual downgrade later needs the `.v3.bak` (the manual says so).
- Unknown fields are an error everywhere.
- Writes are atomic (write temp file, rename). Autosave after edits (debounced ~1 s).
- If a deck name in the file no longer exists in Resolume, its sequences are listed in the
  UI as **orphaned**, with an action "move to deck …".
- The composition name comes from the composition JSON field `name.value` (*notes §2*).

## 7. Runtime behaviour

### 7.1 Starting
- **Start from UI** (▶ on a sequence card, or Space with the sequence selected).
- **Start from Resolume:** a dedicated layer named `SEQ` (configurable) holds placeholder
  clips named `§ <sequence name>`. When such a clip becomes connected (WebSocket), the
  sequence of that name **in the active deck** starts. When it disconnects or the SEQ layer is
  cleared, the sequence stops. A `§` clip whose name matches no sequence is flagged in the UI.
  The placeholder clips should be invisible: use the `Blank` source (`source:///video/Blank`)
  and/or SEQ layer opacity 0. `connected` is still reported with layer opacity 0, master 0 or
  bypass on (*notes §7*).
- The start happens on the tick its start mode gives (§3.2); until then it is *pending*.
- On start, the engine takes a **snapshot** of the sequence. Edits made while it runs do
  **not** affect it; they apply at the next start (UI shows "edited · applies at next start").
  The next pass of a repeating sequence is not a new start.
- **Starting a running sequence** (release 0.5, decided 2026-11-23; replaces the earlier known limitation):
  - **▶ in the app** restarts it from step 1 with a fresh snapshot (layers it no longer uses get its
    `after_last_pass`), on the tick its start mode gives. Its link to a `§` clip is **kept**: clearing the SEQ
    layer still stops it, and that `§` clip stays the connected one.
  - **Its `§` clip connecting** while it runs (started from the app) **adopts the run**: no restart, no jump to
    step 1; from then on the run is linked to that `§` clip like a run started from it.
  - A `§` clip connecting while the sequence is pending (§3.2) links the pending start; it doesn't start a
    second one.
- When a sequence started from its `§` clip **finishes by itself**, the sequencer clears the SEQ
  layer (if that `§` clip is still the connected one), so the next click on it starts it again.

### 7.2 Decks
- The UI shows deck tabs. The tab marked **LIVE** is the active deck in Resolume.
- Selecting a deck tab or a sequence in another deck only **shows** it. It never switches
  Resolume.
- **Starting** a sequence whose deck is not live:
  1. If any sequence is running, show a confirmation ("Drop A is running. Switch to Act 2 and start?").
  2. Stop all running sequences.
  3. Send deck select to Resolume.
  4. Wait until the WebSocket confirms the new deck **and** the clip index is rebuilt.
     Confirmation = the first composition push whose selected deck is the target (observed
     100–190 ms after the select). Do not read REST meanwhile (it returns mixed states).
     The start button shows "switching…". Timeout 2 s → error, nothing starts.
  5. Start the sequence (its tick count starts now).
- If the deck changes in Resolume by other means, all running sequences stop **without clearing**
  (layers are shared by all decks, so a clear would hit the new deck) and the LIVE marker moves.
  The tab being viewed does not change.
- Resolume itself does not stop playing clips on a deck switch, so "stop" in step 2 means applying
  each sequence's `after_last_pass` (clears are sent before the deck select).

### 7.3 Layer ownership and conflicts
- While a lane runs, its sequence **owns** that layer.
- Starting a sequence that needs a layer owned by another running sequence stops the
  conflicting **lanes** of the other sequence (not the whole sequence) in the same tick.
- **Manual override:** if the WebSocket reports a clip connected on an owned layer that the
  sequencer did not trigger in the last ~100 ms, that lane becomes **yielded**: it stops
  triggering but keeps counting, so it stays in time. It resumes on "Resume lane" (UI)
  at its next step boundary, or at the next start of the sequence.

### 7.4 Stopping
- Stop sequence: apply `after_last_pass` to its owned layers, release ownership. Yielded lanes
  (manually overridden layers) are left alone.
- Stop all / panic: stop all sequences; `clear` all owned layers, yielded ones included.

### 7.5 Confirmation
Each trigger is expected to show up as `connected` on the WebSocket. If it does not within
~250 ms, the step is marked red in the UI ("not confirmed") and logged. No retry.
(Observed latency is 3–7 ms on localhost.) Re-triggering a clip that is already connected
produces no `connected` change, so such a step counts as confirmed immediately.
A confirmation that arrives after the timeout (seen once at ~700 ms with a busy Resolume) still
counts as the sequencer's own trigger, up to 5 s or the lane's next trigger: it clears the red mark
and is logged as late, and it does not yield the lane (§7.3).

### 7.6 Sync SEQ layer (M16, decided 2026-11-18)
A button sets up the SEQ layer in Resolume so that every sequence of the **live deck** has its `§` clip
(§7.1), instead of the user building the placeholders by hand.
- **Where:** `Sync SEQ` in the sequence list header (§9.2), shown for the live deck only (Resolume's
  API reads and writes only the live deck, *notes §2*). It is highlighted when the layer or a `§` clip
  is missing. It opens a dialog with the plan; nothing changes in Resolume before **Sync** is pressed.
- **Plan** (`seq_core::seq_sync`, pure): the layer (exists / missing / several layers with the name),
  the sequences without a `§` clip, the `§` clips whose name matches no sequence ("orphans"), and the
  empty slots of the layer. The engine shows it for the live deck; the worker computes it again from
  a fresh REST read before acting, because the pushed composition can be stale (*notes §15*).
- **Steps** (worker task, never on the timing path; each call in *notes §16*):
  1. Layer missing: add a layer (it goes on top), name it like the SEQ layer setting, set its opacity
     to 0 so the placeholders don't show on the output. Several layers with the name: stop with a
     message (rename all but one).
  2. Only with **Remove `§` clips without a sequence** ticked (off by default): clear the orphans that
     are not playing. Playing ones are kept and reported. Their slots are reused.
  3. Not enough empty slots: add the missing columns at the end.
  4. Fill empty slots, left to right, in sequence list order: load the "1000A Sequence" source if
     Resolume lists it (§12), else `Blank`; then name each clip `<prefix><sequence name>`. The
     engine then binds the plugin's `Sequence` parameter as usual (§12).
- Other clips on the SEQ layer, other layers and other decks are never touched. A second sync with
  nothing missing changes nothing. Running sequences keep running; a sync is refused during a deck
  switch, and it stops (with a message) if another deck becomes live while it runs.
- **Result:** one line in the status bar, e.g. `Sync SEQ: created layer SEQ; added 2 clips (1000A
  Sequence)`, or `Sync SEQ layer failed: …`. One sync at a time; the button reads `Syncing…` meanwhile.
- Not in scope: syncing decks that are not live (that needs a deck switch), renaming `§` clips when a
  sequence is renamed (the old clip becomes an orphan and a new one is added), reordering clips.

### 7.7 Transitions per step (release 0.5, decided 2026-11-23)

A step can say **how its clip comes in**: a fade length and optionally a blend mode (`transition`, §6), with an
optional default per lane. Resolume's **layer transition** is the means: the app sets it on the layer just before
the step triggers. Clip-level transitions in the composition are never changed. Nothing is cleaned up by hand:
when the sequence lets go of the layer, the layer gets back the transition it had before.

*Resolume side (probed 2026-11-27, notes §19):* each layer has `transition.duration` (ParamRange, seconds, 0–10,
clamped) and `transition.blend_mode` (ParamChoice, set by option name), each with a parameter id; they are set over
the WebSocket **by id only**, and every change costs a full composition push. Ids and values belong to the layer and
survive deck switches. A set sent in the same instant as the OSC `connect` (WebSocket first) already counts. A
change of duration **during a running fade** rescales the rest of it (0 ends it at once). A layer clear fades too.
While a fade runs, both clips of the layer are `Connected`; the outgoing one reports `Disconnected` when it ends. A
clip with `layer_determined: false` uses its own transition and ignores the layer's.

**Which transition a step gets**
1. its own `transition`, else
2. the lane's `transition`, else
3. if any step of the lane or the lane itself has a transition: the layer's **original** transition (the one it had
   before the sequence took it), so a step without a value always looks the same, whatever ran before it;
4. otherwise **Resolume's own**: the app never touches this layer's transition.

**Sending**
- The value goes out **ahead of the step**, separately from the trigger, and only if it differs from what the
  layer has. It goes out one sixteenth before the step starts, unless the layer is still fading then (the fade
  of the step before): a new duration would rescale that fade, so the value waits until the fade has ended. If
  the fade lasts until the step starts, the value goes out with the trigger, the WebSocket set just before the OSC
  message. For the first step of a run it goes out with the start, before the first trigger. The trigger still
  goes out exactly on its tick (§3). The scheduler (pure) decides the next step's pass condition and pick at that
  moment, so the transition matches the clip that will play; a step that doesn't play sets nothing.
- The length is converted to seconds at the current tempo and sent again (throttled) when the tempo changes while
  the value is set; a fade that is running then is rescaled with it, so it still ends on its beat. Resolume's
  maximum is 10 s: a longer fade is capped and the step shows a warning.
- While a fade runs, the outgoing clip's `Disconnected` at its end is no user action, and triggering the outgoing
  clip again counts as confirmed at once (it never disconnected, §7.5).
- `blend` is checked against the layer's option list from the composition. Unknown name: only the length is set,
  warning on the step and in the pre-show check.
- Clips whose own transition doesn't follow the layer (`layer_determined: false`) ignore the layer value: warning
  on the step and in the pre-show check.
- Set and restore are WebSocket messages from the engine's send path; the engine never waits for their answer
  (CLAUDE.md: nothing on the timing path blocks).

**Original and restore**
- When a lane that uses transitions (rule 3) takes its layer, the app reads the layer's current transition (from the
  composition it holds, kept up to date by a by-id subscription) and remembers it as the **original**, once per
  layer.
- A layer handed from one sequence to another (§7.3) keeps the first original, not the value in between.
- When no running lane holds the layer any more (stop, finish, Stop all, a deck change, or a takeover by a lane
  without transitions), the original is set back. **The clear uses the original** (decided 2026-11-27): a clear
  fades with the layer's transition, so the original goes back first and the clear follows in the same instant;
  stopping looks as it did before transitions (usually a cut). A lane end *Clear* does the same.
- **The user wins:** if the layer's transition changes in Resolume (a `parameter_update` with a value other than
  the one the app sent last; updates carry no origin), the
  original is dropped and nothing is restored on that layer for this run: the user's value stays. The lane keeps
  setting its steps' transitions (as with a manual override, the log says so).
- **Crash or quit while holding a layer:** originals are also written to `<data-dir>/transitions-pending.json`
  (composition name, layer id, original values; param ids are looked up again from the composition, they may
  change when it is reloaded). On the next connection to that composition the app restores them
  and deletes the file. Entries whose layer is gone are dropped with a log line.
- The log shows `transition on BG restored (0.0 s, Alpha)`.

### 7.8 Mute and solo (release 0.5, decided 2026-11-25)

Live switches per lane of a **running** sequence (G2). They belong to that run: not saved, and a restart starts
with every lane unmuted.

- **Mute:** the lane's layer is cleared at once (if the lane was playing); the lane keeps counting but sends nothing
  (no triggers, no clears). **Unmute:** it plays again from its next step boundary, in time, like "Resume lane".
- **Solo:** while any lane of the sequence is soloed, every lane of it that is not soloed is muted (several solos
  are allowed). Removing the last solo brings the other lanes back, except those muted by hand.
- A muted lane keeps its layer (§7.3): another sequence starting on that layer takes it over as usual, and a clip
  started by hand on it yields it (§7.3; unmuting then also resumes it). Muting a yielded lane clears the layer.
- Stopping and finishing apply as usual; a muted lane's layer is already dark.
- The log says `BG muted at 3.2` / `BG unmuted at 3.4 (plays from its next step)`.

### 7.9 Record from Resolume (release 0.5, decided 2026-11-27)

Build a pattern by playing it (G6): arm a sequence, start it, and click clips (or clear layers) in Resolume in
time. What you click lands on the grid, and the next pass plays it back, so a pattern can be built up pass by pass.

- **Arm:** the ⏺ button in the editor header arms the selected sequence of the **live** deck (one at a time). Armed,
  it records from its next start; the start waits for its start mode as usual (a bar start is the count-in).
  Arming a running sequence records from that moment, and it loops from its next pass. Disarm or Stop ends the
  recording (Stop also disarms; a disarmed run goes on looping until stopped). A sequence **without steps** asks
  for a **pattern length** (1–16 bars) when armed and loops over that length (changed 2026-11-27 after the live
  check; no placeholder step, which would clear a layer).
- **Takes:** each pass is one **take**. While recording, the sequence loops pass after pass, whatever its repeat
  setting. Each click is written into the sequence **at once** (it shows in the editor right away); the clicks of
  one pass are **one undoable edit** (Ctrl+Z removes that pass's take so far), and the next pass plays them.
- **What counts as a click:** a clip that becomes connected on a layer (WebSocket `connected`, §7.5) that the app
  did not trigger itself (the window of §7.3), and a layer that is cleared without the app clearing it (records a
  rest). A clear is the `Disconnected` of the layer's last connected clip that no `Connected` on that layer
  follows within 20 ms (a cut sends both in the same millisecond); it arrives at the end of the clear's fade, so
  it counts at that time minus the layer's duration. A `Disconnected` while another clip of the layer is
  connected is the end of a fade and records nothing (notes §20). Clicks on the SEQ layer are not recorded (they start sequences as usual).
  Clicking the clip that already plays reports nothing in Resolume (notes §13), so it can't be recorded: the step
  just goes on.
- **Piano trigger style** (checked 2026-11-27, notes §20): pressing a Piano clip is a click, releasing it a clear,
  so a held clip records as the clip until the release and a rest from there. A tap shorter than a sixteenth
  still records a sixteenth: a rest that snaps onto the sixteenth of the clip before it moves one sixteenth on.
  *Toggle* works the same way (the second click disconnects: a rest).
- **Snap:** each click goes to the nearest sixteenth of the pass (or eighth, or beat: **quantize**), from its
  arrival time minus a **latency** offset (ms, ±500, positive = clicks arrive late) and the tempo. Both are set in
  Preferences → Recording (saved in `settings.json` as `record_quantize`, `record_latency_ms`); quantize also
  next to the REC tag; changes apply at once. A click in the last half grid step of a pass belongs to the start of
  the next one. Two clicks on one layer that snap to the same place: the later one counts.
- **Punch in** (changed 2026-11-27 after the live check, was "overwrite to the end of the pass"): a click replaces
  its lane from its position up to the **next step that was already there**, or your next click on that layer, or
  the end of the pass. Everything else stays, so a clip clicked between two others keeps the later ones. Steps
  cut by a click keep their conditions, choices and transitions, shortened; a step replaced whole loses them.
- **Across the loop** (decided 2026-11-27): a clip of the take that still plays when the pass ends (nothing comes
  after it on its lane, or it is held) goes on into the next pass: it is recorded there at the start as a
  **tied** step (§6 `tie`), up to the next step that is there (or its release), and the next pass doesn't restart
  it.
- **What is recorded** (decided 2026-11-27, kept for the session, not in the file): while armed each lane head
  has a ⏺ toggle: a lane switched off doesn't record, and a click on its layer is a manual override (§7.3) as
  without recording. **+ lanes** on/off: whether a click on a layer without a lane adds one. Mode **punch in**
  (the default, above) or **replace**: a click runs to the next click on its layer or the end of the pass over
  the steps there, and the lane is quiet from that click until the pass ends (a clear too). The controls sit in
  one row of the editor header while armed (REC tag, quantize, mode, + lanes, sync chip).
- **Held clips** (Piano trigger style, the clip's own or the composition's `cliptriggerstyle`; requested
  2026-11-27): a clip pressed and held records **from press to release** and overwrites what lies in between.
  While it is held its lane triggers nothing (it yields); after the release the lane plays again from its next
  step. A hold that runs over the end of a pass goes on in the next: that lane begins the pass quiet, and the
  clip is recorded there from the start until its release. (Add where you click, overwrite where you hold.)
- A lane shorter than the pass is first written out to the pass length as it plays (*Loop to end*: its steps
  repeated; *Hold*: the last step longer; *Clear*: a rest), then overwritten. Its end mode stays.
- **While recording:** every lane plays on as usual, also the one you click on (its next step comes in where the
  punch-in ends); the new take plays from the next pass on. Manual override (§7.3) doesn't apply to the recorded
  sequence while it records.
- **The grid:** clicks are snapped to the sequence's grid. When it isn't lined up with the music (no Resync with
  Resolume's tempo, no MIDI Start, or a start mode `now`), clicks land a sixteenth early or late; a notice says
  so when the recording starts.
- **New layers:** a click on a layer without a lane in the sequence adds a lane for it (Clip mode, *Loop to end*,
  a rest up to the first click). Layers whose name is not unique can't get a lane: notice, not recorded.
- **Transitions** (§7.7, requested 2026-11-27): each recorded step gets the transition Resolume used for that
  click: the layer's (`transition.duration` / `blend_mode` at that moment, as the app follows it) for a clip that
  is *Layer Determined*, else the clip's own. The duration becomes sixteenths at the current tempo (nearest
  sixteenth, at most 256), the blend mode its name. A cut (0 s) is recorded as `{ "length": 0 }` only if another
  step of the lane has a transition; otherwise the step gets none, so a lane played without fades stays a lane
  without transitions (§7.7 rule 4). A recorded rest (layer clear) gets no transition. A lane with transitions
  keeps setting them as it plays, so a click on its layer uses the value of the step playing then.
- **Slot lanes** (§4.5) record the **column** that was clicked; Clip lanes record the clip (by id, name and file,
  as a drop from the clip browser does).
- **Stop** while recording keeps the take so far (it is already written).
- The UI shows `REC` on the sequence and the editor while it records, `armed` while it waits for its start and
  `REC in N` (beats) while the start counts in. Next to it a chip says whether the grid is lined up with the beat:
  `in sync`, `⚠ not synced` (with what to do) or `⚠ starts now` (a start mode `now`). In the grid, the steps
  recorded in the current pass have a red outline, the one of the newest click flashes once, and a clip held down
  shows hatched from the playhead on (it ends at the release). The log says
  `recorded pass 3 of Drop: BG, FG (2 steps)`.

## 8. Thumbnails

- **Clip thumbnails** are fetched via REST for the active deck and cached on disk, keyed by clip
  ID plus `thumbnail.last_update` from the clip JSON (also sent per clip in WS `thumbnail_update`
  messages). Empty slots and `thumbnail.is_default == true` are skipped (placeholder shown).
  `last_update` also changes on structural edits, so it may trigger harmless re-fetches.
- The UI loads thumbnails from the sequencer's own HTTP server, never directly from Resolume.
- Inactive decks show cached thumbnails from the last time each deck was active. Decks never
  seen active show placeholders.
- **Sequence thumbnails:** the sequencer renders a PNG mosaic per sequence (one row per lane,
  member clip thumbnails in order, sequence name on top) and can set it as the **custom thumbnail**
  of the sequence's `§` placeholder clip ("Push thumbnail to Resolume"), via multipart
  `POST /composition/clips/by-id/{id}/thumbnail`. The placeholder keeps its transparent source.

## 9. UI

Reference mockup: **`docs/ui/editor-mockup.html`** (open it in a browser). Its layout, design
tokens (`:root` CSS variables) and visual states are binding; sample data and exact pixel values
are not. Copy the tokens to `web/src/tokens.css`. The tokens are the default theme (Rack); other
themes change token values only, never the layout (§9.7).

Single screen, everything controlled from here (no separate live view).

### 9.1 Top bar
- The Clip Sequencer logo mark (`logo/cs_logo.png`, decided 2026-11-26), composition name.
- **View tabs:** Sequencer (Ctrl+1), LUTs (Ctrl+2, §14), Console (Ctrl+3).
- **Deck tabs** (see 7.2): selected tab = what is shown; `LIVE` badge = active deck in Resolume.
- **Stop all** (always visible).

### 9.2 Left: sequence list (for the selected deck tab)
- Cards: mosaic thumbnail, name, RUN badge when running, meta line
  (`<lanes> lanes · <bars> bars · once|×N|∞`), binding line (`§ Drop A · col 2`, red if no
  SEQ clip exists in the deck).
- Per card: ▶ / ■ button. On a non-live deck, the button shows the deck name (`▶ Act 2`)
  and switching follows 7.2.
- Click selects the sequence for editing. `+ New` creates one in the selected deck.
- `Sync SEQ` (live deck only): creates the SEQ layer and the missing `§` clips in Resolume (§7.6).
- Context menu: rename, duplicate, delete (with confirmation).

### 9.3 Center: sequence editor
- Header: name (editable), binding tag, start mode (`Start: now / beat / bar / 2 · 4 · 8 · 16 bars`, §3.2),
  `starts in …` while pending, start/length/end info, "edited · applies at next start"
  badge (only when running and edited), **Repeat** control (Once / ×N with number input / ∞),
  pass counter while running (`pass 2 / 4`), `+ Lane`, `Push thumbnail to Resolume`, Start/Stop.
- Lane grid:
  - Left column per lane, the **lane head** (changed 2026-11-27: it had run out of room): the layer (choose from
    the deck's layers), **M** and **S** next to it while the sequence runs (§7.8), a one-line summary
    (`clip · loop · ↗ 1 beat`; mode, end, default transition, `↗ steps` when only steps have one) and the state
    pill (`idle` / `playing` / `yielded ↺` / `muted` / `rec`; the yielded pill is the "Resume lane" action).
    Clicking the head selects the lane: its settings are in the **lane inspector** (§9.4).
  - Ruler: bars numbered, eighth subdivisions, sixteenths when zoomed in; zoom with Ctrl+wheel.
  - Steps as blocks, width proportional to length. States: normal (clip colour/thumbnail),
    rest (dashed), unresolved (red), selected (accent outline), playing (accent top edge),
    not confirmed (red top edge).
  - Playhead with `bar.beat` label while running.
  - **Sections** (§6): named flags in the ruler at their position, with a thin line down through the lanes;
    sections beyond the sequence's length greyed out. Add from the ruler's context menu or with `M` at the
    playhead / selected step's start; drag to move (on the grid), double-click to rename, Del to delete.
    Guide lines every 8/16/32 bars (or off) are a view setting.
  - **Step badges** (format v4): a condition as a small chip in the block's top right (`2:4`, `1st`, `!1st`,
    `last`, `!last`); choices as `1/3` with the pick (`⤨` random, `↻` cycle). The block shows option 1.
    A **tied** step has an arc `⌒` over the gap to the step before (dimmed when that step surely plays another
    clip, so the tie has no effect) and `⌒` in its length line; **T** ties or unties the selected step.
  - **Transition display** (§7.7; colour token `--fade` in `tokens.css`, `themes.ts` and the mockup; sketch
    `docs/images/transition-fade-sketch.png`):
    - a **fade wedge** at the left edge, rising from bottom left to top right, on the grid scale: its width is
      the fade length (a 1-beat fade covers the first beat of the block); `--fade`, translucent, under the name;
    - the label in the length line: `↗ 1 beat · Alpha` (blend only when set), in narrow blocks `↗ 1`;
    - a step's **own** transition solid, one **inherited** from the lane default as a dashed outline only;
    - a **cut** (`0`) set on a step over a lane default: a short vertical bar at the left edge;
    - a **fade longer than the step**: the wedge runs to the right edge and ends in `›`;
    - **warnings** (capped at 10 s, unknown blend mode, clip with its own transition): the label in
      `--err-text` with ⚠, the reason as tooltip and in the inspector;
    - steps getting the original or Resolume's own transition show nothing.
- Interactions:
  - Drag a clip from the browser onto a lane → appends or inserts a step (default length 16 = one bar).
  - Drag a block's right edge → resize, snapping to eighths (min 1/8); hold Shift to snap to
    sixteenths (min 1/16). Drag its **left edge** (not the first step's) → move its start: the step before gets
    longer or shorter by as much, nothing after it moves (edit `move_step_start`, one undo step). While an edge is
    dragged, a marker like the playhead runs through the grid at the edge, labelled `bar.beat.sixteenth` and the
    step's new length (added 2026-11-27).
  - Drag a block → reorder within its lane. Alt-drag → duplicate.
  - Del → remove step. `R` → insert rest after selected step.
  - Space → start/stop selected sequence; Shift+Space → cue it (§3.3). Esc → deselect.
  - Undo/redo (Ctrl+Z / Ctrl+Shift+Z) for all edits in the session.
- Slot lanes (§4.5):
  - Lane header: mode switch `Clip` / `Slot`. Switching converts the steps (undoable):
    clip → slot uses each resolved clip's column (unresolved → rest); slot → clip uses the clip
    currently in each slot (empty → rest).
  - Step blocks show `col N` and the **current** thumbnail of that slot, updated live when the slot's
    content changes in Resolume. An empty slot is drawn like a rest, labelled `col N · empty`.
  - Dropping a clip from the browser onto a slot lane inserts a step with that clip's column.

### 9.4 Right: clip browser + step inspector
- Clip browser for the selected deck tab: search field, one row per layer (top layer first,
  as in Resolume), clip thumbnails with names; draggable onto lanes. Each thumbnail shows its
  **column number** (1-based, the number a slot lane uses, §4.5) as a small overlay in its top-left
  corner, in every lane mode.
- **Lane inspector** for a selected lane (click its head; a selected step wins): Clips | Slots (§4.5), the end
  behaviour (`Loop to end` / `Hold` / `Clear`, explained), the default transition (length and blend mode, or none,
  §7.7), move up / down, delete lane.
- Step inspector for the selected step: thumbnail, name, resolution status per method
  (ID / name in deck / file, with ✓ or ✕), `Relink to clip…` (pick from browser), length stepper
  in sixteenths with bars shown. For a slot step instead: column stepper (1 … column count) and the
  clip currently in that slot (or "empty → rest").
  Format v4 (§6): **Tie** to the step before (checkbox), **Transition** (inherit from lane / own: length in beats,
  blend mode from the layer's list, warnings), **Play on** (always / first / not first / last / not last / A of B), **Choices** (the options with
  thumbnails and resolution status, add, remove, reorder, relink each; pick random / cycle).

### 9.5 Status bar
Current sequence and position (`bar.beat`, tick), yield and error notices. On the right, small connection chips:
CLOCK (BPM, locked / no clock; click opens Preferences → Clock), REST, WS, OSC — green when OK, red when not (moved
here from the top bar, 2026-11-22). OSC is UDP, so a send that works says nothing about Resolume hearing it: OSC is
also red when 3 triggers in a row time out (§7.5) while the WebSocket is up and subscribed (OSC input off, wrong
port), with a notice; the next confirmed trigger turns it green (2026-11-26).
The last OSC message and the last confirmation are no longer shown here (2026-11-26): they are DEBUG log lines,
in the Console tab and the log file when Preferences → Log → **Log debug messages** is on (`debug_log` in
`settings.json`, off by default; applies at once; adds the app's own DEBUG records to what `RUST_LOG` allows).

Menu bar (File, View, Help) above the top bar, in the browser UI and in the desktop app (no native menu since
2026-11-22, so it follows the theme); desktop actions go to the shell as `shell` commands (§9.6).

### 9.6 UI ↔ engine protocol
- UI talks to the app over one WebSocket (`/ws`) with JSON messages.
- Engine → UI: full `state` snapshot on connect, then `state` deltas at most 30 Hz while running,
  and immediately on structural changes. Includes connection status, decks, live deck,
  sequences (from file), runtime state (running, pass, position, lane states, confirmations).
- UI → engine: commands (`start`, `stop`, `stop_all`, `resume_lane`, `select_deck_view`,
  `edit` operations (format v4 adds: set start mode, set/clear lane transition, set/clear step transition,
  set/clear step condition, add/remove/reorder/relink a choice, set pick, add/move/rename/delete a section;
  all with undo; the step's flat form gains the optional `when`, `choices`, `pick`, `transition`), `push_thumbnail`, `sync_seq_layer` with `remove_orphans` (§7.6), `cue` with `seq` and
  `at` (`start_mode`, `end_of_pass` with the running sequence's id, `bars_8`, `bars_16`) and `cancel_cue` (§3.3),
  `set_lane_mute` / `set_lane_solo` with `seq`, `lane`, `on` (§7.8), `set_guide_lines` with `every` (8, 16, 32 or
  `null` = off; saved as `guide_lines` in `settings.json`, sent back in `appearance`, §9.3), and for the
  browser UI's File menu `reload_composition`, `save_now`). The live deck's `seq_sync` plan comes with
  the decks, `seq_sync_busy` with the runtime state. Edits are sent as operations, the engine applies them to the stored model, saves,
  and echoes the new state.
- UI → desktop shell: `{"cmd": "shell", "action": "quit" | "zoom_in" | "zoom_out" | "zoom_reset"}`.
  Keys typed into the web content never reach the native menu's shortcuts (WebView2 handles input in
  its own process), so in the desktop app the UI sends Ctrl+Q and Ctrl+= / + / - / 0 this way; the
  server forwards them to the shell. Without a shell (browser) they are ignored and the UI leaves
  these keys to the browser, except `quit`: in the headless app it stops the app like Ctrl+C (the
  browser UI's File → Quit server, after a confirmation).
- Menus (M11 Phase F): the desktop app has the native menu (File / View / Help); the browser UI has
  an HTML menu bar with the same items where they make sense in a browser (no Open data folder; View
  zooms the page). Help → Keyboard shortcuts, Licenses and About are dialogs in the page in both
  (the native menu opens them through `window.__seqOpenDialog`); the shortcut list has one source,
  `web/src/lib/shortcuts.ts`. Licenses come from `GET /api/licenses` (MIT text plus the third-party
  notices embedded at build time).
- UI → desktop shell also: `"open_logs"` (Console tab, opens `<data-dir>/logs`).
- Console tab (M12 Phase E):
  - UI → engine: `{"cmd": "log_subscribe"}` when the Console tab opens, `{"cmd": "log_unsubscribe"}` when it
    closes. Log records only flow while subscribed.
  - Engine → UI: `log_snapshot` `{ records, stats }` (the ring buffer of the last 5,000 records) on subscribe,
    then `log` with an array of new records, batched at most every 100 ms. A record is
    `{ seq, ts, level, target, message }`; `seq` increases by one, so the UI can merge snapshot and batches.
    DEBUG/TRACE records of dependencies (WebSocket, HTTP crates) are not sent live: sending them would log
    the sending itself. They are in the ring and the log file.
  - Engine → UI, always: `log_stats` `{ errors, dropped }` on connect and when it changes (red error badge on
    the Console tab; records that did not reach the file or a subscriber).
  - `GET /api/logs/current`: today's log file as a download (the in-memory buffer if there is no file).
  - Log files: `<data-dir>/logs/seq-app-YYYY-MM-DD.log` (UTC day), kept 7 days.
- Preferences (MIDI clock input): `GET /api/midi/ports` → `{ ports, selected }`; UI → engine
  `{"cmd": "set_midi_port", "port": "<name>" | null}` switches the clock input at once and saves it in
  `<data-dir>/settings.json` (`{ "midi_port": ... }`). At startup `--midi-port` wins over the saved port.
- Preferences (clock source, §3.1): UI → engine `{"cmd": "set_clock_source", "source": "midi" | "resolume"}`,
  saved as `clock_source`; `--clock` wins at startup. The runtime state's `clock` is
  `{ source, port, connected, running, bpm, transport, synced }`: with the Resolume source `connected` means
  the WebSocket is up with a tempo controller, `bpm` is Resolume's tempo, `transport` its `Play`/`Pause`/`Stop`
  and `synced` true after a resync (`transport` `null` and `synced` false with MIDI).
- Preferences (theme, §9.7): engine → UI `appearance` `{ theme, custom_themes, guide_lines, debug_log }` on connect and after every
  change (to every connected UI). UI → engine `{"cmd": "set_theme", "id": "<id>" | null}` (`null` = default),
  `{"cmd": "save_theme", "theme": {...}, "select": bool}` (adds, or replaces the one with the same id),
  `{"cmd": "delete_theme", "id": "<id>"}`, `{"cmd": "set_debug_log", "on": bool}` (§9.5). Commands of one UI are applied in the order sent; refused ones
  (bad id, name or colour, more than 50 saved themes) leave the settings unchanged and show a notice.
- `GET /api/version`: `{ app, name, version, build, commit, ui_port, desktop }` (`desktop`: running in the
  shell's window). Also used by a second instance to detect a running one (M12 Phase B).
- Thumbnails: `GET /thumbs/clip/{id}.png`, `GET /thumbs/seq/{uuid}.png`.
- Every UI page is served with a Content-Security-Policy that allows only the server's own origin.

### 9.7 Themes (decided 2026-11-18)
- Preferences has two tabs: **MIDI clock** and **Appearance**. Appearance lists the themes as cards (name,
  sample, colour strip). Clicking one shows it at once on the whole app; **Save** keeps it, **Cancel** (or Esc)
  goes back to the saved one. The clock chip opens the Clock tab.
- Five built-in themes, defined in `web/src/lib/themes.ts`: **Rack** (default: cool slate, cyan accent, square
  corners, Barlow / Barlow Condensed / JetBrains Mono), **Ink & Signal**, **Graphite & Lime**, **Classic Amber**
  (the look before 2026-11-18) and **Daylight** (light, for bright rooms). Built-in themes cannot be changed.
- A theme = all colour tokens (the `:root` colours of the mockup) + four fonts (text, uppercase labels, sequence
  title, numbers/names) + a look (corner radii, title case, label spacing). The meaning of the colour groups
  stays fixed: accent = running/selected/playhead, info = confirmed by Resolume, ok = connections,
  err = unresolved/yielded/disconnected.
- **Change colours and fonts…** opens an editor: every colour token (picker + hex, `#RRGGBB` or `#RRGGBBAA`)
  grouped as above, and a font per role from the bundled fonts. Changes preview at once. **Save as new theme**
  (name required) stores it and selects it; for a saved theme also **Save changes** (same id) and
  **Delete theme…**. A saved theme keeps the look of the theme it was made from (`base`). While there are
  unsaved changes, the dialog's Save is disabled (save them as a theme, or discard them).
- Stored by the app in `<data-dir>/settings.json`: `theme` (id; absent = default) and `custom_themes`
  (`[{ id, name, base, colors: {token: hex}, fonts: {role: font id} }]`, ids start with `custom-`). The app
  checks only the shape, so a newer UI can add tokens; missing colours or fonts, an unknown `base` or font
  come from the base theme (or the default). One setting for the desktop window and every browser.
- The browser remembers the last theme shown (localStorage) and applies it before connecting, so there is no
  flash of the default theme. The splash screen and the desktop window's first paint use the default theme.
- Fonts are bundled (no network at the venue): all SIL OFL 1.1, Latin and Latin Extended subsets, with their
  copyright and licence in the third-party notices (Help → Licenses). The logo is an image (white on
  transparent) shown as a mask in the theme's text colour: the mark alone in the top bar, the whole logo with its
  lettering on the splash screen.

## 10. Architecture

Cargo workspace (see `CLAUDE.md` for layout).

- **`seq-core`** (pure): data model + serde, clip resolution against a `CompositionIndex`
  value, and the **scheduler state machine**: `fn on_tick(&mut self) -> Vec<Action>` where
  `Action` = `Trigger { clip_id, layer_id, … }`, `Clear { layer_id, … }` and state notifications
  (`Started`, `PassStarted`, `StepStarted`, `LaneStateChanged`, `Ended`). Actions carry Resolume
  **ids**, not positions, because ids follow clips and layers when columns or layers are inserted
  mid-run; the engine maps them to the current OSC address at send time. Handles passes, lane end
  behaviours, ownership, yielding. Exhaustively unit-tested with simulated ticks.
- **`clock-in`**: opens the MIDI port with `midir`, converts bytes to `ClockEvent`, sends them on
  a channel. Reconnects if the port disappears. `ResolumeClock` makes the ticks itself at Resolume's tempo
  instead (§3.1), on its own thread, into the same channel; the engine feeds it tempo, transport and resync.
- **`resolume-client`**: REST client, WebSocket client (reconnect with backoff, resubscribe),
  OSC sender (one UDP socket), and the composition index builder.
- **`seq-app`** (binary):
  - **Engine task** owns the scheduler and the composition index. It receives clock events,
    WebSocket events and UI commands through channels, sends OSC directly, and publishes state
    on a broadcast channel. It never awaits HTTP.
  - Worker tasks: REST refresh, thumbnail fetch/cache, mosaic rendering, file save.
  - axum server: static UI (rust-embed), `/ws`, `/thumbs/...`.
- **`tools/mock-resolume`**: fake REST/WS/OSC endpoints with a scriptable composition, used in
  integration tests and for UI development without Resolume.

## 11. Configuration

CLI flags (clap) with optional `seq.toml`:
`--clock midi|resolume`, `--midi-port`, `--resolume-host`, `--rest-port 8080`, `--osc-port 7000`, `--ui-port 8090`,
`--seq-layer SEQ`, `--seq-prefix "§ "`, `--data-dir`. Logging via `RUST_LOG`.

## 12. "Sequence" FFGL source plugin (M11, decided 2026-10-26)

Replaces the *Blank* placeholder on `§` clips. Starting/stopping is unchanged (the `§` clip's
`connected` state, §7.1); the plugin only **shows** the sequence.

- **Content:** a live timeline: sequence name, one row per lane (layer name + steps as blocks
  with clip thumbnails, rests and unresolved steps marked like in the browser UI), and while the
  sequence runs a moving playhead, `bar.beat` and the pass counter. Static layout when idle.
- **Binding:** automatic. The sequencer finds the plugin on each `§` clip and sets its
  `Sequence` text parameter to the clip's sequence name (`§ ` stripped). The clip name stays the
  key; the parameter can also be typed by hand (fallback).
- **Visibility:** may appear in the preview monitor and on the output; the user controls it with
  the SEQ layer (opacity etc.).
- **Data:** the plugin polls the sequencer over localhost HTTP (`/plugin/v1/...`, UI port) from a
  worker thread; it never blocks the render thread. Without a sequencer it shows "no sequencer".
- **Verify first (M11a probe):** whether Resolume renders a source plugin when its clip is only
  previewed / connected on a layer with opacity 0, whether plugin parameters appear in the
  composition JSON (`video.sourceparams`) and can be set over the WebSocket, and what happens to
  the custom grid thumbnail.

### 12.1 Installing the plugin (M11 Phase G3, decided 2026-11-04)

The release zip ships the plugin as `ffgl/SeqView.dll` next to the executables. The app installs, updates or
removes it in Resolume's plugin folder, so users don't copy files by hand.

- **Target folder:** `<Documents>\Resolume Arena\Extra Effects`, with `<Documents>` from the Windows Known Folder
  API (it may be redirected, e.g. to OneDrive). Order: `--ffgl-dir` for the run, else the folder chosen in the
  dialog (saved as `ffgl_dir` in `settings.json`), else that default. A missing folder is created on request.
  Never `C:\Program Files\…` (no admin rights).
- **Versions:** the DLL's Windows version resource (`FILEVERSION` = major, minor, patch, build number; Phase E).
  Bundled vs. installed: newer → **Update**, equal → **Reinstall**, older → **Downgrade** (needs confirmation),
  nothing there → **Install**. An installed `SeqView.dll` without a version resource (an early personal build) counts
  as older.
- **Only our file:** an existing `SeqView.dll` is replaced or removed only if it is this plugin (its version
  resource names "Clip Sequencer for Resolume", or, for early builds, it contains the plugin id `VJSQ` and the name
  `SeqView`). Anything else is left alone and reported.
- **Resolume running:** Resolume locks loaded plugin DLLs, so before any change the app checks for a running
  `Arena.exe` / `Avenue.exe` and refuses with "Quit Resolume Arena to install the plugin". It never ends the process.
- **Safe copy:** copy to `SeqView.dll.tmp` in the target folder; rename the existing DLL to `SeqView.dll.bak`;
  rename the temp file to `SeqView.dll`; delete the `.bak`. If a step fails, the `.bak` is renamed back and the
  temp file removed, so the previous version stays installed. The result says "Restart Resolume Arena to load the
  plugin".
- **Remove:** deletes `SeqView.dll` from the target folder (only if it is ours, Resolume not running).
- **UI:** File → Install FFGL plugin… (native menu and browser menu) opens a dialog with the bundled version, the
  target folder (editable, Open folder in the desktop app), the installed version or "not installed", and one
  primary button (Install / Update / Reinstall / Downgrade…) plus Remove.
- **API** (UI server, JSON): `GET /api/ffgl` status; `POST /api/ffgl/install` `{ "allow_downgrade": bool }`;
  `POST /api/ffgl/remove`; `POST /api/ffgl/dir` `{ "dir": "<path>" | null }` (null = default).
- **CLI:** `seq-app-cli --install-ffgl [--ffgl-dir <path>]` does the same without the UI (creates the folder if
  needed, never downgrades); exit code 0 = installed, 1 = error with a message.
- Every install, update and removal is logged with the versions, the path and the result.
- **Status-bar hint** (decided 2026-11-04): in a build that bundles the plugin, the status bar shows
  "FFGL plugin missing · Install…" when there is no `SeqView.dll` in the target folder, or "FFGL plugin update
  available · Update…" when ours is older. Clicking it opens the dialog. Checked when the UI connects and when the
  dialog closes; nothing is installed automatically.

### 12.2 Requests from other web pages

The UI server answers only on `127.0.0.1`, but a web page open in the user's browser could still send requests to
it (WebSockets and simple POSTs are not limited by the same-origin policy). Every request that carries an
`Origin` header whose host and port differ from the request's `Host` is refused (403). The app's own pages are
same-origin; the FFGL plugin and command-line tools send no `Origin`.

## 13. Out of scope (for now)

Time signatures other than 4/4, per-step parameter automation (a step's transition, §7.7, is the one exception),
cross-deck sequences, multi-user editing.

## 14. LUTs (M18, decided 2026-11-22)

Colour-grade Resolume's output from the app: a library of LUTs edited in the **LUTs** tab and applied by Resolume's
own **LUT** effect, which reads a `.cube` file and reloads it by itself when the file changes (notes §18). No own
plugin.

### 14.1 Library and files

- Folder `<data-dir>/luts/`. Each LUT is `<name>.lut.json` (the controls, below) and `<name>.cube` (the baked
  table, 33³, 0..1 domain, six decimals, `TITLE` = the name). The name is the file name: at most 64 characters,
  none of `\ / : * ? " < > |`, no leading or trailing dot or space, no Windows device name; names differ in more
  than case. At most 200 LUTs.
- `.lut.json`: `{ "version": 1, "grade": {…}, "curves": {…}, "base"?: { "size", "data" } }`. Missing fields take
  their defaults (which change nothing); a newer `version` is refused (the file is skipped with a log line).
  `.seq.json` (§6) is unchanged.
- Every change bakes the table at once (in memory) and writes both files through a temp file renamed over the old
  one, so Resolume never reads a half-written file. While a control is dragged, a LUT's files are written at most
  every 100 ms; the last value is always written. A `.cube` that already holds the same text is not rewritten (a
  restart doesn't make Resolume reload).
- At start the library is read from the folder; files that don't load are skipped.

### 14.2 The transform (crates/lut-core)

Display-referred RGB 0..1, in this order: the imported **base** table (trilinear), **exposure** (stops, ×2^e),
**white balance** (temperature −1..1: R ×(1+0.25t), B ×(1−0.25t); tint −1..1: G ×(1−0.25n)), per channel
**gain** (master × channel), **lift** (`v + lift·(1−v)`, master + channel), **gamma** (`v^(1/γ)`, master ×
channel), **contrast** around 0.5, **saturation** around Rec. 709 luma, clamp to 0..1, then the **curves**: master,
then the channel's own. Curves are monotone cubic splines (Fritsch–Carlson) through their points; left of the
first point the first y, right of the last the last y.

Import: `.cube` with `LUT_3D_SIZE` (any 2..256) or `LUT_1D_SIZE`, optional `DOMAIN_MIN`/`DOMAIN_MAX`, `#`
comments; other keywords are ignored. It becomes the base table, resampled to 33³. Export writes the baked table.

### 14.3 Resolume's LUT effects

- The app reads every LUT effect (`name` `LUT` with a `ParamFile` `LUT 3D`) from each composition push: on the
  composition, the layers and the live deck's clips (layer groups are not read; their JSON is not probed). It
  subscribes to their `LUT 3D` parameters by id and follows changes.
- **Binding by reference:** an effect uses a library LUT when its `LUT 3D` path is that LUT's `.cube` (compared
  without case and slash direction). Resolume saves the path with the composition, so the binding survives
  reloads and restarts, and it works without the sequencer.
- **Apply** ("Use <LUT>" on an effect): the app writes the file if pending, then sets `LUT 3D` to its full path over
  the WebSocket. **Rename** points the effects using the old file at the new one, and removes the old files 2 s
  later. **Delete** removes the files; the UI asks first and says how many effects use it.

### 14.4 LUTs tab (Ctrl+2)

- Left: the library (name, "used by N LUT effects", "imported"), **+ New** (neutral), **Import…** (several
  `.cube` files; names from the file names, made unique).
- Middle: name (double-click renames), Rename, Duplicate, Export .cube, Delete…; the preview: two panels side by side
  (decided 2026-11-25, replaces the draggable split), the original on the left and the same image with the LUT on
  the right, each scaled to fit its panel with its aspect kept. Images: drawn test images (ramps, colour chart,
  skin tones, neon), the 24 photos of the Kodak Lossless True Color Image Suite (`web/public/kodak`, released by
  Kodak for unrestricted use; source and note in its README and the licence list) and the live deck's clip
  thumbnails. WebGL2 with the table the app baked, so it matches Resolume. Below:
  Resolume's LUT effects (where, which file or LUT) with **Use <LUT>**.
- Right: Basic (exposure, contrast, saturation, temperature, tint), Lift/Gamma/Gain (all, R, G, B), Curves (Master,
  R, G, B). Double-click on a slider resets it. Changes go to the app every 40 ms while dragging; a control being
  dragged isn't moved by the app's echo.

### 14.5 Protocol (§9.6)

- App → UI: `luts` `{ dir, luts: [{ name, rev, grade, curves, has_base, path }] }` on connect and on every change;
  `structure.lut_effects: [{ effect_id, file_param, path, place: { kind: composition | layer | clip, … } }]`.
- UI → app: `lut_create { name, from? }`, `lut_rename { from, to }`, `lut_delete { name }`,
  `lut_update { name, grade, curves }`, `lut_clear_base { name }`, `lut_apply { name, file_param }`. Errors show
  as a notice.
- HTTP: `GET /api/luts/table?name=` (the baked table: size³ little-endian f32 RGB, red fastest; headers
  `x-lut-rev`, `x-lut-size`), `GET /api/luts/cube?name=` (export), `POST /api/luts/import?name=` (body: the
  `.cube` text; `{ ok, name }` or 400 `{ ok: false, error }`).

## 15. In-app update (M14, release 0.5; replacement decided 2026-11-23, details 2026-11-25)

The app checks once at start-up whether a newer release exists (changed 2026-11-25: before, only by hand), and the
user can check by hand any time; an update is installed from the app. The first version that can update itself is
0.5.0 (0.4.x has no such feature).

**Check**
- **At start-up, once** (decided 2026-11-25): after the start-up is done ("Ready"), in the background. If a newer
  version is found that the user hasn't skipped, the UI shows *Version 0.5.1 is available (you have 0.5.0)* with the
  release notes and **Download and install**, **Skip this version**, **Later**. *Skip* is saved (`update_skipped` in
  `settings.json`) and that version is not offered at start-up again (a newer one is); *Later* asks again at the
  next start. A failed start-up check (no network, GitHub unreachable) shows nothing; it is only logged. Off with
  Preferences → **Check for updates at start-up** (`update_check: false` in `settings.json`, on by default) or
  `--no-update-check` for the run. The headless app does the same; its browser UI shows the question when it connects.
- **By hand:** Help → **Check for updates…** (desktop app and the browser UI's menu bar), or
  `seq-app-cli --check-update`; also offers a skipped version.
- `GET https://api.github.com/repos/ghostrider/clip-sequencer/releases/latest` (no login, `User-Agent:
  clip-sequencer/<version>`, 10 s timeout), in a worker task, never on the timing path. That endpoint never returns
  drafts or pre-releases. Its `tag_name` (`v0.5.1`) is compared by semver with the running version.
- The release must carry `clip-sequencer-<version>-windows-x64.zip` and `SHA256SUMS.txt` (the release workflow
  makes both); otherwise the result is an error ("the release has no Windows zip").
- **Result** in a dialog: *Checking…*; *Clip Sequencer 0.5.0 is up to date*; *Version 0.5.1 is available (you
  have 0.5.0)* with the release notes (the release body, as plain text) and **Install and restart**, **Release page**,
  **Close**; or the error with **Release page**. `--check-update` prints the same and exits 0 (up to date), 2
  (update available) or 1 (error).
- **Test source** (hidden, for testing only, also in release builds): `--update-source <url>` replaces
  `https://api.github.com` for this run; downloads then come from the asset URLs that source returns. Documented in
  `docs/RELEASING.md`, not in the manual.

**Install**
- **Install and restart** is disabled while a sequence runs, waits or is cued ("Stop all sequences first"), and
  when the program folder is not writable (checked by creating and deleting a file in it): the dialog says why and
  offers the release page.
- Steps shown in the dialog: *Downloading 3.1 / 13.2 MB*, *Verifying*, *Restarting*. The zip and `SHA256SUMS.txt`
  are downloaded to `%TEMP%\clip-sequencer-update-<version>\`; the zip's SHA-256 must match its line in
  `SHA256SUMS.txt`, it is extracted there, and every extracted file that has a line (the two exes, the plugin DLL)
  must match too. Once releases are signed (SignPath), the new exes' Authenticode signatures are checked as well:
  the hashes come from the same place as the zip and prove an intact download, not its origin.
- **Program folder:** the folder of the running exe (the `clip-sequencer` folder of the portable zip). Only it is
  changed; never the data folder, never Resolume's plugin folder.

**Helper** (decided 2026-11-25: the **new** `seq-app-cli.exe` in a hidden helper mode, no separate updater)
- The app starts `%TEMP%\clip-sequencer-update-<version>\clip-sequencer\seq-app-cli.exe --apply-update` with the
  program folder, its own process id, the exe to start again (`seq-app.exe` or `seq-app-cli.exe`) and that exe's
  arguments, then quits like File → Quit. Running from the temp folder, the helper locks nothing in the program
  folder.
- The helper waits for the old process to end (at most 30 s; else it stops and changes nothing), moves the entries
  of the program folder that the new release also has (the exes, `ffgl\`, the texts; anything else the user keeps
  there stays) into `.update-backup\<old version>\` inside it, copies the new ones in and starts the new exe
  with the old arguments plus `--after-update <ok file>`. A folder without `seq-app-cli.exe` is not touched.
- The new version writes the ok file when its start-up is done ("Ready") and before it saves any sequence file (so a
  rolled-back older version still reads them, §6). It shows *Updated to 0.5.1* and deletes leftover
  `clip-sequencer-update-*` temp folders.
- **Rollback:** the new version quits before writing the ok file (at once), no ok file within 30 s (a hang;
  60 s until 2026-11-26), or a step failing (a file in use, disk full) → the helper stops the new
  process if it runs, removes the new files, moves the backup back and starts the old exe with
  `--update-failed <reason>`; the old version shows *Update to 0.5.1 failed: <reason>. Still 0.5.0.*
- With the ok file the helper deletes `.update-backup`. Its own log: `.update-backup\update.log` while it runs,
  copied to `<data-dir>/logs/update-<date>.log` by the new (or the restored old) version.

**Other**
- **FFGL plugin:** the copy in Resolume's plugin folder is not replaced by the update (Resolume may hold it open);
  the existing "FFGL plugin update available" hint takes over after the restart (§12.1).
- **Privacy:** the privacy note, the code signing policy and SECURITY.md say that the app asks GitHub for the latest
  release once at each start (unless turned off) and when the user chooses Check for updates, sends nothing else,
  and downloads only when the user chooses to install.
