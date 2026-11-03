# Resolume Clip Sequencer

Plays multi-layer clip sequences in Resolume Arena, locked to the show's MIDI clock. Sequences are
edited in a browser UI and started from the UI or from Resolume itself (a `§` placeholder clip).
Clips are triggered over OSC on a 1/16-bar grid; Resolume's state is followed over its WebSocket.

Full concept: [`docs/SPEC.md`](docs/SPEC.md). Resolume API facts: [`docs/resolume-api-notes.md`](docs/resolume-api-notes.md).

## Requirements

- Windows 10/11, on the same machine as Resolume.
- Resolume Arena 7.26 or newer (tested with 7.28.1).
- The **MIDI Clock Shifter** (`../midi-clock-shifter`) running, providing its `Clock Shifter Out`
  virtual port. Resolume and the sequencer both follow that same (shifted) clock.
- The desktop app (`seq-app.exe`) needs the **Microsoft Edge WebView2 Runtime**. It ships with
  Windows 11. On Windows 10 without it, the app says so and offers the
  [download page](https://developer.microsoft.com/microsoft-edge/webview2/) ("Evergreen" runtime).
  `seq-app-cli.exe` doesn't need it.
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

This builds the UI (`web/`) and the release binaries. It puts them in `release\`, together with the
portable zip `clip-sequencer-<version>-windows-x64.zip` and its `.sha256`:

- `seq-app.exe` is the desktop app: a native window, no console.
- `seq-app-cli.exe` is the console program: headless, `--validate`, `--list-midi-ports`, `--clock-debug`.

Both contain the UI. Nothing needs installing: unzip anywhere and run.

## Run

### Desktop app

Double-click **`seq-app.exe`**.
- A splash screen shows the startup steps. A missing Resolume or MIDI port only gives a warning; the
  sequencer keeps retrying in the background.
- Then the window opens with the UI.
- The first time, pick the MIDI clock input under **File → Preferences…** (or click the CLOCK chip,
  or press Ctrl+,). It's saved and used on every start.

Menu:
- **File:** Reload composition, Save now, Open data folder, Preferences…, Quit (Ctrl+Q).
- **View:** zoom (Ctrl+= / Ctrl+- / Ctrl+0) and Toggle Console.
- **Help:** Keyboard shortcuts, User manual, Report an issue (both open in the browser), Licenses, About.

Closing the window while a sequence runs asks first. Then the sequences end as configured (layers
cleared or held) before the app exits. The window size and position are remembered. Starting
`seq-app.exe` a second time doesn't start a second sequencer: it offers to bring the running one to
the front.

The same options as below work on the command line (e.g. `seq-app.exe --midi-port "Clock Shifter Out"`
overrides the saved port for that run). `--no-splash` opens the window without the splash.

### Headless (console, UI in a browser)

```powershell
release\seq-app-cli.exe --midi-port "Clock Shifter Out"
```

It prints the startup steps and then the UI address, normally **http://127.0.0.1:8090**; open it in
a browser. Ctrl+C (or closing the console window) stops the sequencer: running sequences end as
configured, then it exits. In the browser the UI has a menu bar (File, View, Help; Alt or F10 opens it), and
**File → Quit server…** stops the sequencer the same way.

### UI port

The UI uses port `8090` (`--ui-port`). If another program has it, the sequencer takes the next free
port from 8091 to 8099, then one the OS assigns. It logs a warning, and the desktop window and
console output use the actual address. `--strict-port` exits with an error instead of falling back.
If the port belongs to a running sequencer, no second one starts. The running instance's address is
written to `%APPDATA%\resolume-sequencer\runtime.json` (`pid`, `ui_url`, `started_at`) while it runs.

### Options

Useful options (`--help` lists all):

| Option | Default | |
|---|---|---|
| `--midi-port <name>` | saved in Preferences | MIDI clock input (exact name or a substring) |
| `--resolume-host`, `--rest-port`, `--osc-port` | `127.0.0.1`, `8080`, `7000` | Where Resolume listens |
| `--ui-port` | `8090` | UI port (falls back, see above) |
| `--strict-port` | | Don't fall back if the UI port is taken |
| `--no-splash` | | Desktop app: no splash screen |
| `--seq-layer`, `--seq-prefix` | `SEQ`, `"§ "` | Placeholder layer and clip-name prefix |
| `--data-dir` | `%APPDATA%\resolume-sequencer` | Sequence files, settings, logs, thumbnail and deck caches |
| `--list-midi-ports` | | Print MIDI input ports and exit (`seq-app-cli.exe`) |
| `--clock-debug` | | Show the incoming clock (bar.beat.tick, BPM) to check the port (`seq-app-cli.exe`) |
| `--validate <file>` | | Check a `.seq.json` file and exit (`seq-app-cli.exe`) |

### Logs

- **Files:** daily log files in `%APPDATA%\resolume-sequencer\logs\` (`seq-app-YYYY-MM-DD.log`). They
  are kept for 7 days.
- **Console tab:** shows the log live (Ctrl+2), with level filters, pause, copy and save. In the
  desktop app, it opens with View → Toggle Console.
- **Headless:** `seq-app-cli.exe` also prints the log to the console.
- **More detail:** set `RUST_LOG=debug` to see every OSC message with microsecond timestamps.

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
  bar steps, hold `Shift` for 1/16); drag to reorder, Alt-drag to duplicate; `Del` removes, `R` inserts a rest,
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
- Start a sequence from one place at a time. Starting it from the UI while its `§` clip is
  connected restarts it from step 1 as a UI start: clearing the SEQ layer then no longer stops it,
  and clicking the `§` clip does nothing until the layer is cleared (known limitation, SPEC §7.1).

## Development

See [`CLAUDE.md`](CLAUDE.md) for the layout and commands. In short:

```powershell
powershell -ExecutionPolicy Bypass -File scripts\check.ps1      # fmt, clippy, tests
cargo run -p mock-resolume -- --rest-port 8080 --osc-port 7000   # fake Resolume
cd web; npm run dev                                              # UI with hot reload (app on :8090)
$env:SEQ_DEV_URL='http://localhost:5173'; cargo run -p seq-app   # desktop window on the hot-reload UI
cargo run --release -p clock-in --example clock_gen -- "Default App Loopback (A)" 128   # test clock
cargo run --release -p seq-app --example soak -- "Default App Loopback (B)" 120          # long-run test
```

The desktop shell (Tauri v2) is built by plain `cargo build`. The Tauri CLI (`cargo tauri dev` /
`cargo tauri build`) isn't needed: there is no installer to bundle, and the window loads the UI
from the app's own server.
- **Debug builds:** they read `web/dist` from disk, so `npm run build` is enough to see UI changes
  after a reload.
- **Hot reload:** `SEQ_DEV_URL` (debug builds only) points the window at the Vite dev server instead.
  Vite proxies `/ws`, `/api` and `/thumbs` to the app; set `SEQ_APP` for Vite if the app isn't on :8090.
- **Release builds** embed `web/dist`.
- **WebView2 DevTools:** set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223`.
