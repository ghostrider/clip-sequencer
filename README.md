# Resolume Clip Sequencer

Plays multi-layer clip sequences in Resolume Arena, locked to the show's MIDI clock. Sequences are
edited in a browser UI and started from the UI or from Resolume itself (a `§` placeholder clip).
Clips are triggered over OSC on a 1/8-bar grid; Resolume's state is followed over its WebSocket.

Full concept: [`docs/SPEC.md`](docs/SPEC.md). Resolume API facts: [`docs/resolume-api-notes.md`](docs/resolume-api-notes.md).

## Requirements

- Windows 10/11, on the same machine as Resolume.
- Resolume Arena 7.26 or newer (tested with 7.28.1).
- The **MIDI Clock Shifter** (`../midi-clock-shifter`) running, providing its `Clock Shifter Out`
  virtual port. Resolume and the sequencer both follow that same (shifted) clock.
- To build: Rust (stable) and Node.js 20+.

## Resolume setup (once per composition)

1. **Preferences → Webserver:** enable *Webserver & REST API*, port `8080`.
2. **Preferences → OSC:** enable *OSC Input*, port `7000`.
3. **Composition → Clip Beat Snap: None.** Triggers are sent exactly on the tick; snapping would
   delay them.
4. **Name your layers** (e.g. `BG`, `MID`, `FX`). Lanes target layers by exact, unique name.
   Resolume *displays* unnamed layers as "Layer 1", "Layer 2", … but reports them all as
   `Layer #`, so they can't be used until renamed (double-click the layer name, or right-click →
   Rename). Renames are picked up live.
5. **Starting from Resolume (optional):** add a layer named `SEQ`. For each sequence, put a clip
   with the *Blank* source on it and name it `§ <sequence name>` (e.g. `§ Drop A`). Clicking the
   clip starts the sequence of that name in the live deck; disconnecting it (or clearing the
   layer) stops it. Set the SEQ layer's opacity to 0 if the placeholders should never be visible.

## Build

```powershell
powershell -ExecutionPolicy Bypass -File scripts\release.ps1
```

This builds the UI (`web/`) and the release binaries and copies them to `release\`:
`seq-app.exe` is the desktop app (no console window; its window arrives with M12 Phase D) and
`seq-app-cli.exe` the console program (headless, `--validate`, `--list-midi-ports`, `--clock-debug`).
Both contain the UI; no other files are needed to run them.

## Run

```powershell
release\seq-app-cli.exe --midi-port "Clock Shifter Out"
```

In the desktop app (`release\seq-app.exe`), pick the MIDI clock input under File → Preferences… (or click
the CLOCK chip, Ctrl+,); it is saved and used on every start. `--midi-port` overrides it for one run.

Then open **http://127.0.0.1:8090** (the console shows the actual address if 8090 was taken; Ctrl+C
stops the sequencer).

Useful options (`--help` lists all):

| Option | Default | |
|---|---|---|
| `--midi-port <name>` | — | MIDI clock input (exact name or a substring) |
| `--resolume-host`, `--rest-port`, `--osc-port` | `127.0.0.1`, `8080`, `7000` | Where Resolume listens |
| `--ui-port` | `8090` | Browser UI port |
| `--seq-layer`, `--seq-prefix` | `SEQ`, `"§ "` | Placeholder layer and clip-name prefix |
| `--data-dir` | `%APPDATA%\resolume-sequencer` | Sequence files, thumbnail and deck caches |
| `--list-midi-ports` | | Print MIDI input ports and exit |
| `--clock-debug` | | Show the incoming clock (bar.beat.tick, BPM) to check the port |
| `--validate <file>` | | Check a `.seq.json` file and exit |

Logging: set `RUST_LOG=debug` to see every OSC message with microsecond timestamps.

### Before a show

1. Start Resolume (composition open), then the Clock Shifter, then `seq-app`.
2. Check the chips in the UI's top bar: **CLOCK** shows the BPM and "locked", **REST**, **WS**,
   **OSC** are green.
3. Make every deck you will use live once: its clips are then cached, so its sequences show and
   resolve even while another deck is live (also after a restart).

## Using it

- **Deck tabs** show a deck's sequences; **LIVE** marks Resolume's active deck. Viewing a tab never
  switches Resolume. Starting a sequence of another deck asks first, stops what is running,
  switches the deck, then starts.
- **Sequences:** `+ New`, right-click for rename / duplicate / delete. Sequences of decks that no
  longer exist are listed under their old deck name; move them with right-click.
- **Editor:** drag clips from the browser onto a lane; drag a block's right edge to resize (1/8
  bar steps); drag to reorder, Alt-drag to duplicate; `Del` removes, `R` inserts a rest,
  `Space` starts/stops, `Esc` deselects, `Ctrl+Z` / `Ctrl+Shift+Z` undo/redo, `Ctrl+wheel` zooms.
- **Rests** clear the layer (it shows nothing; layers below show through). If the lane's clip
  lives on another layer (⚠ on the step) and is still playing, that layer is cleared too.
- **Lanes** can be reordered with ▲ / ▼ on the lane head. **Red steps** are unresolved (clip not found): they trigger nothing.
  The step inspector shows which of clip id / name / file still match and offers *Relink*.
- **Lane ends** (when a lane is shorter than the sequence): *Loop to end*, *Hold* (last clip keeps
  playing), *Clear*. **Repeat:** once, ×N or ∞; after the last pass the layers are cleared or held.
- **Manual override:** clicking a clip in Resolume on a layer a sequence owns makes that lane
  *yielded* — it keeps counting but stops triggering until *Resume lane*.
- **Edits while running** apply at the next start ("edited · applies at next start").
- **Push thumbnail to Resolume** sets the sequence's mosaic as its `§` clip's thumbnail.
- **Stop all** clears every layer the sequencer owns.

Sequences are saved automatically (about 1 s after an edit) to
`%APPDATA%\resolume-sequencer\<Composition>.seq.json`.

## Behaviour worth knowing

- No clock → sequences freeze in place (top bar shows "no clock") and continue when it returns.
  MIDI Start (e.g. the Clock Shifter's resync) does not restart sequences.
- If Resolume quits, triggers are not confirmed (red top edge on steps); when it comes back the
  sequencer reconnects by itself. A port that disappears (Clock Shifter restarted) is reconnected
  automatically too.
- Switching decks in Resolume by hand stops running sequences without clearing anything.
- A sequence started from its `§` clip clears the SEQ layer when it finishes, so the next click
  starts it again.

## Development

See [`CLAUDE.md`](CLAUDE.md) for the layout and commands. In short:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check.ps1      # fmt, clippy, tests
cargo run -p mock-resolume -- --rest-port 8080 --osc-port 7000   # fake Resolume
cd web; npm run dev                                              # UI with hot reload (app on :8090)
cargo run --release -p clock-in --example clock_gen -- "Default App Loopback (A)" 128   # test clock
cargo run --release -p seq-app --example soak -- "Default App Loopback (B)" 120          # long-run test
```
