# Resolume Clip Sequencer — Specification

Status: concept agreed 2026-10-25. This document is the source of truth.
Resolume API details were verified in Milestone 0 against Arena 7.28.1; endpoints, message shapes,
timings and samples are in `docs/resolume-api-notes.md` (cited below as *notes §n*).

## 1. Purpose

A live tool and a playback engine for pre-composed clip sequences in Resolume Arena 7.26.

- A **sequence** plays clips on several Resolume layers in parallel, on a 1/8-bar grid,
  locked to the show's MIDI clock.
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

- **Input:** MIDI clock at 24 PPQN from a named MIDI input port (configurable).
  Messages used: Timing Clock (0xF8). Start (0xFA), Stop (0xFC) and Continue (0xFB)
  are logged but **do not** affect running sequences (the Clock Shifter's resync
  button sends Start; it must not restart sequences).
- **Grid:** 1/8 bar in 4/4 = one eighth note = **12 ticks**. All step lengths are
  integers in eighths. Only 4/4 is supported.
- **No quantization.** A sequence starts at the moment its start is processed. Its
  internal position counts ticks from that moment. Step boundaries fire exactly
  on the tick where they fall.
- **Trigger timing:** the OSC message for a step is sent on the tick the step begins.
  No lookahead. Resolume's Beat Snap must be **off** for clip triggers.
- **Tempo:** derived from tick intervals (moving average) for display only.
- **Clock loss:** if no tick arrives for > 2 beats at the last known tempo, running
  sequences **freeze** (position stops, no triggers), the UI shows "no clock".
  When ticks resume, sequences continue from where they froze.
- **Data model is ready for quantization later:** `start` field, currently only `"immediate"`.

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

## 5. Concepts

- **Composition** — the open `.avc`. Sequences are stored per composition.
- **Deck** — every sequence belongs to exactly one deck. Sequences never use clips from another deck.
- **Sequence** — name, deck, repeat setting, end action, one or more lanes.
- **Lane** — targets one layer (by name). Has its own steps and its own length. Lanes can be
  reordered in the editor (order is display only). The lanes of one
  sequence target different layers. A lane without steps, or whose layer is not found, is silent.
- **Step** — a clip reference (or `null` = rest) and a length in eighths. A **rest clears the
  lane's layer** when it starts (silence). If the clip the lane triggered last lives on another
  layer (it plays there) and is still playing, that layer is cleared too: a rest stops what the
  lane started. The same applies to every clear of a lane (lane end `clear`, `after_last_pass`
  clear, stop all). An *unresolved* step triggers nothing, so the
  previous clip keeps playing through it.
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
  "version": 1,
  "composition": "MyShow",
  "decks": {
    "Act 1": {
      "sequences": [
        {
          "id": "6f1c2a9e-0d5b-4c1e-9a0f-2f7d4c1b8e11",
          "name": "Drop A",
          "start": "immediate",
          "repeat": { "mode": "count", "count": 4 },
          "after_last_pass": "clear",
          "lanes": [
            {
              "layer": "BG",
              "end": "loop_to_end",
              "steps": [
                { "clip": { "id": 1712345, "name": "Tunnel Red", "file": "tunnel_red.mov" }, "length": 16 },
                { "clip": null, "length": 8 }
              ]
            }
          ]
        }
      ]
    }
  }
}
```

- `repeat.mode`: `"once"` | `"count"` | `"infinite"`; `count` only with `"count"`.
- `length`: integer, in eighths (8 = one bar), ≥ 1.
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
- On start, the engine takes a **snapshot** of the sequence. Edits made while it runs do
  **not** affect it; they apply at the next start (UI shows "edited · applies at next start").
  The next pass of a repeating sequence is not a new start. Starting a running sequence again
  restarts it with a fresh snapshot (layers it no longer uses get its `after_last_pass`).
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
are not. Copy the tokens to `web/src/tokens.css`.

Single screen, dark theme, everything controlled from here (no separate live view).

### 9.1 Top bar
- `§ SEQ` logo, composition name.
- **Deck tabs** (see 7.2): selected tab = what is shown; `LIVE` badge = active deck in Resolume.
- Connection chips: CLOCK (BPM, locked / no clock), REST, WS, OSC — green when OK, red when not.
- **Stop all** (always visible).

### 9.2 Left: sequence list (for the selected deck tab)
- Cards: mosaic thumbnail, name, RUN badge when running, meta line
  (`<lanes> lanes · <bars> bars · once|×N|∞`), binding line (`§ Drop A · col 2`, red if no
  SEQ clip exists in the deck).
- Per card: ▶ / ■ button. On a non-live deck, the button shows the deck name (`▶ Act 2`)
  and switching follows 7.2.
- Click selects the sequence for editing. `+ New` creates one in the selected deck.
- Context menu: rename, duplicate, delete (with confirmation).

### 9.3 Center: sequence editor
- Header: name (editable), binding tag, start/length/end info, "edited · applies at next start"
  badge (only when running and edited), **Repeat** control (Once / ×N with number input / ∞),
  pass counter while running (`pass 2 / 4`), `+ Lane`, `Push thumbnail to Resolume`, Start/Stop.
- Lane grid:
  - Left column per lane: layer name (choose from the deck's layers), end behaviour
    (`Loop to end` / `Hold` / `Clear`), state pill (`idle` / `playing` / `yielded`; yielded has a
    "Resume lane" action), delete lane.
  - Ruler: bars numbered, eighth subdivisions; zoom with Ctrl+wheel.
  - Steps as blocks, width proportional to length. States: normal (clip colour/thumbnail),
    rest (dashed), unresolved (red), selected (accent outline), playing (accent top edge),
    not confirmed (red top edge).
  - Playhead with `bar.beat` label while running.
- Interactions:
  - Drag a clip from the browser onto a lane → appends or inserts a step (default length 8).
  - Drag a block's right edge → resize, snapping to eighths (min 1).
  - Drag a block → reorder within its lane. Alt-drag → duplicate.
  - Del → remove step. `R` → insert rest after selected step.
  - Space → start/stop selected sequence. Esc → deselect.
  - Undo/redo (Ctrl+Z / Ctrl+Shift+Z) for all edits in the session.

### 9.4 Right: clip browser + step inspector
- Clip browser for the selected deck tab: search field, one row per layer (top layer first,
  as in Resolume), clip thumbnails with names; draggable onto lanes.
- Step inspector for the selected step: thumbnail, name, resolution status per method
  (ID / name in deck / file, with ✓ or ✕), `Relink to clip…` (pick from browser), length stepper
  in eighths with bars shown.

### 9.5 Status bar
Current sequence and position (`bar.beat`, tick), last OSC message sent and its send time,
last WebSocket confirmation, yield and error notices.

### 9.6 UI ↔ engine protocol
- UI talks to the app over one WebSocket (`/ws`) with JSON messages.
- Engine → UI: full `state` snapshot on connect, then `state` deltas at most 30 Hz while running,
  and immediately on structural changes. Includes connection status, decks, live deck,
  sequences (from file), runtime state (running, pass, position, lane states, confirmations).
- UI → engine: commands (`start`, `stop`, `stop_all`, `resume_lane`, `select_deck_view`,
  `edit` operations, `push_thumbnail`). Edits are sent as operations, the engine applies them
  to the stored model, saves, and echoes the new state.
- UI → desktop shell: `{"cmd": "shell", "action": "quit" | "zoom_in" | "zoom_out" | "zoom_reset"}`.
  Keys typed into the web content never reach the native menu's shortcuts (WebView2 handles input in
  its own process), so in the desktop app the UI sends Ctrl+Q and Ctrl+= / + / - / 0 this way; the
  server forwards them to the shell. Without a shell (browser) they are ignored and the UI leaves
  these keys to the browser.
- `GET /api/version`: `{ app, name, version, ui_port, desktop }` (`desktop`: running in the shell's
  window). Also used by a second instance to detect a running one (M12 Phase B).
- Thumbnails: `GET /thumbs/clip/{id}.png`, `GET /thumbs/seq/{uuid}.png`.
- Every UI page is served with a Content-Security-Policy that allows only the server's own origin.

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
  a channel. Reconnects if the port disappears.
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
`--midi-port`, `--resolume-host`, `--rest-port 8080`, `--osc-port 7000`, `--ui-port 8090`,
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

## 13. Out of scope (for now)

Start quantization, time signatures other than 4/4, per-step parameter automation,
cross-deck sequences, multi-user editing.
