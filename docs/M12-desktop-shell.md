# M12 — Desktop shell (Tauri v2, embedded UI, console tab)

Goal: the sequencer runs as **one desktop app**, a native window with the existing UI,
with no separate console window and no manual browser step. The engine, scheduler, server
and UI protocol stay **unchanged**. This milestone adds a shell around them.

**Order:** do M12 **before** M11 (open-source release). M12 changes the build and release
pipeline, so M11 Phase E (exe metadata) and Phase I (release workflow) have to build on it.
See "Impact on M11" at the end.

Work through the phases in order. Tick checkboxes as you go. Stop after each phase,
summarise, and wait for Uli. Items marked **[Uli]** need his decision or action.

---

## Target behaviour

- Double-clicking `seq-app.exe` opens **one window**. There's no console window.
- On startup the app starts the engine and the axum server in-process, picks a free UI port
  (Phase B), and loads the UI from that port in the window.
- At the top of the UI is a tab strip: **Sequencer | Console**.
- Closing the window shuts the server and engine down cleanly.
- `seq-app-cli.exe` (the console companion, decided 2026-10-27, see Decisions) behaves as the old headless
  exe: console output, no window, UI reachable in a
  normal browser.

## Phase A — Decisions [Uli]

- [x] **Distribution:** an installer (Tauri NSIS, which can bootstrap the WebView2 runtime on
      Windows 10) **and/or** a portable exe (requires WebView2 to already be installed). Default:
      both. The NSIS installer is the main download, and the portable zip is offered alongside it.
- [x] **Menu:** keep the HTML menu bar from M11 Phase F, or use a **native Tauri window menu**
      (File / View / Help). Default: native menu in the desktop app; the HTML menu appears only in
      browser/headless mode.
- [x] **Window behaviour:** remember the window size and position between runs (default: yes).
      Closing while a sequence runs asks for confirmation (default: yes).
- [x] **Icon:** the app icon (shared with M11 Phase E). Uli provides it, or Claude makes a placeholder.
- [x] **Splash artwork:** a logo or image for the splash screen (Phase D2). Uli provides it, or Claude
      uses the app name in the UI's typography, with the `§ SEQ` mark, on the dark background.

## Phase B — UI port selection and single instance

The UI server must never fail just because its port is taken, and two engines must never
drive the same Resolume at the same time.

- [x] **Bind-first, never check-then-bind.** Port selection binds the actual
      `tokio::net::TcpListener` and hands **that listener** to axum. Don't probe a port and bind
      it later (race condition).
- [x] **Selection order:**
  1. the configured port (`--ui-port` / `seq.toml`, default **8090**)
  2. if taken: `8091` … `8099`, in order
  3. if all taken: bind port `0` and let the OS assign a free port
- [x] **Bind address** stays `127.0.0.1` by default (the M11 security default). Port selection
      uses the configured bind address.
- [x] **`--strict-port` flag:** if set, don't fall back. Exit with a clear error instead
      ("Port 8090 is in use"). Without the flag, fall back as above and log a **warning** naming
      the requested port and the actual one.
- [x] **Single-instance check (before falling back):** if the configured port is taken, first
      request `GET http://127.0.0.1:<port>/api/version` with a short timeout (≈300 ms).
  - If it answers as **this app** (matching app name in the JSON), another sequencer instance
    is already running. *(Both done; desktop checked: the second instance shows "Open", focuses the running window and exits with 0.)* In desktop mode: show a native dialog, "The sequencer is already running
    (port 8090). Open it?", then focus that instance or open its URL, and **exit without starting
    a second engine**. In headless mode: log an error and exit with a non-zero code.
  - If it answers as something else, or doesn't answer, the port belongs to some other program
    and fallback proceeds normally.
- [x] **Publish the actual port:**
  - [x] the desktop window loads the actual URL (never a hard-coded 8090)
  - [x] the startup log line: `UI listening on http://127.0.0.1:<port>`
  - [x] headless mode prints the URL prominently on stdout
  - [x] `/api/version` includes `ui_port` (the About dialog is M11 Phase F; it reads `/api/version`)
  - [x] a runtime file `<data-dir>/runtime.json` holds `{ "pid", "ui_url", "started_at" }`,
        written on start and deleted on clean shutdown, so other tools can find the running instance
- [x] **Tests:** occupy 8090 with a dummy listener, then assert fallback to 8091. Occupy 8090–8099,
      then assert an OS-assigned port. Run a fake sequencer `/api/version` on 8090, then assert the
      app detects the second instance and exits. `--strict-port` with 8090 taken, then assert a clean
      error exit.

## Phase C — In-process runtime

- [x] Refactor `seq-app` so that startup (config, storage, engine, axum server) is a library
      function: `async fn run(config, shutdown: CancellationToken) -> Result<RunningApp>`, where
      `RunningApp` exposes the actual UI URL and a join handle. *(`seq_app::runner`; `RunningApp` also carries the
      engine command sender and the state watches, which the shell needs for close confirmation.)*
- [x] `main` decides the mode:
  - `--headless` → current behaviour: run on the tokio runtime, Ctrl+C triggers shutdown
  - default → desktop mode (Phase D) *(until Phase D, desktop mode runs headless; a build without the `desktop`
    feature is always headless)*
- [x] The engine never runs on the UI/main thread. *(`run()` is async on its own tokio runtime; the desktop
      thread split is done with the shell in Phase D: `desktop::run` builds a multi-thread runtime
      (`seq-runtime` threads) and hands it to Tauri.)* In desktop mode the tokio runtime runs on its own
      thread(s), and the Tauri event loop owns the main thread.
- [x] **Graceful shutdown** for both modes: stop all sequences (apply `after_last_pass` / clear owned
      layers per SPEC §7.4), flush the pending save, close the WebSocket and OSC, delete `runtime.json`,
      and exit within ~2 s. *(Tested: shutdown with a running sequence sends its clears; runner shutdown via the
      token < 2 s and removes `runtime.json`.)*
- [x] `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` hides the console window
      in release builds. In `--headless` mode on Windows, attach to the parent console
      (`AttachConsole(ATTACH_PARENT_PROCESS)`) so output still appears when started from a terminal.
      *(Superseded after Uli's test: a GUI exe started from a terminal runs in the background without Ctrl+C.
      Console use moved to `seq-app-cli.exe`; `seq-app.exe` attaches only to print `--help`, `--version` and a
      pointer to `seq-app-cli.exe`. Release check: `seq-app.exe` subsystem GUI, `seq-app-cli.exe` console.)*

## Phase D — Tauri shell

- [x] Add Tauri v2 (`tauri`, `tauri-build`) to `seq-app`, or as a thin new crate `seq-desktop` that
      depends on the `seq-app` library. Pick whichever keeps `seq-app` buildable without Tauri for
      headless use, and record the decision in `CLAUDE.md`.
- [x] The main window is created **hidden**. It navigates to the actual UI URL once `run()` reports
      ready, and is shown only after the UI has loaded (no white flash). Until then, the splash screen
      is visible (Phase D2).
- [x] Window title `<App name> <version>`; remembered size and position (Phase A); minimum size that
      fits the editor layout (≈1100×700).
- [x] **Close handling:** if a sequence is running, show a native confirmation ("Drop A is running.
      Quit and stop it?"). Then trigger graceful shutdown (Phase C) and close. *(Checked against the mock:
      Cancel keeps "Loop" running; Quit clears its layer and the process is gone 0.7 s later, `runtime.json`
      removed. Ctrl+Q, window close and the second-instance path all end in the same shutdown.)*
- [x] **Native menu** (if chosen in Phase A). Items call existing functions over a small Tauri command
      or by sending the existing `/ws` commands from the frontend: File (Reload composition, Save now,
      Open data folder, Install FFGL plugin… (see M11 Phase G3), Quit), View (Zoom in/out/reset, Toggle Console tab), Help (Keyboard shortcuts,
      Documentation, Report an issue, Licenses, About). Platform shortcuts: Ctrl+Q quit, Ctrl+= / Ctrl+- /
      Ctrl+0 zoom. *(Done: File, View, Help as decided in Phase A. Menu shortcuts never fire while the web content
      has focus (WebView2 handles keys in its own process), so the UI forwards Ctrl+Q and the zoom keys over `/ws`
      (`shell` command, SPEC §9.6); checked with real keystrokes and over DevTools. Menu clicks still to be tried
      by Uli.)*
- [x] **External links** (GitHub, docs, issues) open in the system default browser, never inside the
      app window. Navigation inside the window is restricted to the local UI URL. *(Checked: a `file:` navigation
      stays blocked, `window.open` is denied; other origins go to the system browser. The UI has no external
      links yet; they come with M11 F.)*
- [x] **Security:** Tauri capabilities/permissions limited to what the shell needs (window, dialog,
      opener/shell-open for external URLs). No filesystem or shell permissions for the web content.
      The CSP allows only the local UI origin. *(No capability files at all: the remote UI has no IPC. The axum
      server sends the CSP with every UI file; `tauri.conf.json` has one for the bundled shell page.)*
- [x] Keyboard shortcuts from SPEC §9.3 (Space, Del, R, Esc, Ctrl+Z…) work in the window exactly as
      in the browser. Check that the webview or the menu doesn't swallow any of them. *(Checked with real
      keystrokes: Space, Esc, Del, R, Ctrl+Z, Ctrl+Shift+Z all reach the UI.)*
- [ ] **Throttling note:** WebView2 throttles rendering while minimized. That's acceptable, because
      timing lives in the engine. Verify that a running sequence keeps triggering correctly while the
      window is minimized for several minutes. *(Open: needs a real MIDI clock; done in Phase G.)*

## Phase D2 — Splash screen

Shown from the moment the exe starts until the main window is ready. It reports what the app is
doing, and it shows startup problems before the main UI exists.

- [x] **Window:** a separate small Tauri window, ≈560×340, frameless, centred, not resizable, no
      taskbar entry, always on top only while it's visible. It's created **first**, before the tokio
      runtime starts, so it appears within ~200 ms of launch.
     *(Created in `setup` before the sequencer starts. Release build, second launch: window visible after 80 ms
      (dark background), page loaded at ~300 ms. The very first launch of a freshly built exe took 1.1 s: Windows
      Defender scans it.)*
- [x] **Content is bundled static HTML/CSS** in the Tauri shell (it can't be served by axum, because the
      server isn't up yet). It uses the same tokens as `web/src/tokens.css` and the bundled fonts (offline).
     *(`crates/seq-app/shell/`; build.rs copies `web/src/tokens.css` in. IBM Plex is bundled in M11 D;
      until then the token font stack falls back to system fonts.)*
- [x] **Layout:**
  - logo or artwork (Phase A) and the app name
  - version and build (`0.1.0 · build 42 · abc1234`)
  - one **status line** with the current step, plus a thin progress bar (determinate: steps done / total)
  - small footer: `Copyright (c) 2026 <holder> · MIT License · Not affiliated with Resolume BV`
- [x] **Startup steps reported** (from `run()` via a progress channel → Tauri event → splash):
  1. Loading configuration
  2. Starting UI server, showing the actual port (`on port 8091` if the fallback was used)
  3. Opening MIDI clock input, with the port name
  4. Connecting to Resolume (REST, WebSocket, OSC)
  5. Loading composition and sequences
  6. Fetching thumbnails (only the first batch; the rest continues in the background)
  7. Ready
     *(`runner::run` sends `StartupStep`s; the shell `eval`s them into the splash page. Build number and
      commit come from git via build.rs and are also in `/api/version`.)*
- [x] **Non-blocking failures don't stop startup.** If Resolume isn't reachable or the MIDI port is
      missing, the splash shows a warning line ("Resolume not reachable, will keep retrying"), waits
      ≈1.5 s so it can be read, and continues. The main UI then shows the red connection chips as usual.
      Startup must never hang on Resolume or MIDI: each step has a timeout of ≤3 s.
     *(Checked: no MIDI port and no Resolume give two warnings, each shown 1.5 s; the main
      window follows.)*
- [x] **Fatal failures** (e.g. `--strict-port` with the port taken, unreadable config, data dir not
      writable) turn the splash into an error panel: the message, then **Open log**, **Copy details**
      and **Quit** buttons (plus **Retry** where it makes sense). The splash doesn't close by itself in
      this case.
     *(Checked with `--strict-port` and 8090 taken: error panel, Retry after freeing the port starts normally,
      Copy details (real click) copies the message with version/build, Esc quits with exit code 1. Open log opens
      `<data-dir>/logs` once Phase E writes logs there, the data folder until then.)*
- [x] **Second instance** (Phase B): the "already running" dialog is shown from the splash, which then closes.
     *(Checked: dialog with the splash behind it; Cancel/Open close both, exit 0.)*
- [x] **Timing:** the splash stays visible at least ≈800 ms (no flicker on fast starts). When the main
      window has loaded the UI, show the main window first, then close the splash with a short fade
      (≤150 ms).
- [x] **Flags:** `--no-splash` skips it (the main window then appears when ready). `--headless` never
      shows it; the same steps are printed to stdout instead.
     *(Checked: `--no-splash` shows only the main window; `seq-app-cli` prints `[n/7] step` and `! warning`
      lines.)*
- [x] **Accessibility:** status changes go into an `aria-live="polite"` region; the error panel buttons
      are keyboard-focusable; Esc on the error panel = Quit.
- [x] Every startup step is also written to the log, so the Console tab shows the full startup sequence afterwards.

## Phase E — Log capture and Console tab

- [x] **Log capture:** replace or wrap the current logger (`log` + `env_logger`, or a `tracing`
      subscriber) with one that sends every record to three places:
  1. stderr (headless mode only)
  2. a **ring buffer** of the last 5,000 records in memory
  3. a **broadcast channel** for live subscribers
  - plus a **log file** in `<data-dir>/logs/`, rotated daily, keeping 7 days.
  The record shape is `{ ts, level, target, message }`. Logging must never block the engine; if the
  channel is full, drop records and count them (`dropped: N` shown in the UI).
  *(`seq_app::logging`, replacing env_logger; `RUST_LOG` still filters. Files `seq-app-YYYY-MM-DD.log`, UTC day.
  stderr in headless mode and debug builds. Found while testing at `RUST_LOG=trace`: the WebSocket crates log
  every frame, so forwarding their records to the Console logged the forwarding itself, endlessly; live
  subscribers now get DEBUG/TRACE only from the app's own crates. Checked: ~285 lines/s at trace level with the
  Console closed and open alike.)*
- [x] **Protocol:** extend the UI WebSocket (SPEC §9.6) with
  - engine → UI: `log_snapshot` (the ring buffer, on connect or on request) and `log` (new records,
    batched at most every 100 ms)
  - UI → engine: `log_subscribe` / `log_unsubscribe` (only send log traffic while the Console tab is
    open), `log_set_level` (runtime filter, optional)
  Document the new message types in `docs/SPEC.md` §9.6. *(Done; plus `log_stats` (error badge, dropped
  count) sent always, `GET /api/logs/current` for "Save log…". `log_set_level` was optional and is left out.)*
- [x] **Tab strip** in the Svelte app: **Sequencer | Console**, using the existing tokens. It's shown
      in both desktop and browser mode. The Sequencer tab is the current UI, unchanged.
      Ctrl+1 / Ctrl+2 switch tabs. The Console tab shows a red count badge when new errors arrive
      while it isn't visible. *(The desktop menu View → Toggle Console switches it too.)*
- [x] **Console tab:**
  - [x] a monospace list with timestamp, level (coloured: error = `--err`, warn = `--accent`,
        info = `--text-2`, debug/trace = `--muted`), target and message
  - [x] level filter toggles (Error / Warn / Info / Debug) and a text filter
  - [x] auto-scroll to the newest line; scrolling up pauses it ("⏸ paused · N new", click to resume)
  - [x] buttons: Clear view (client-side only), Copy visible, Save log… (downloads the current file
        or the buffer as `.log`), Open logs folder (desktop only)
  - [x] virtualised rendering, so 5,000 lines stay smooth
  - [x] a header line showing the app version, build, UI URL, and dropped-record count if any
  *(Checked in the desktop window against the mock: Ctrl+2, Debug toggle 41 → 15 rows, text filter, Clear
  view, scrolling up pauses with "⏸ paused · 10 new", resume follows again, 55 DOM rows for 3,803 records,
  level colours. Copy visible uses the same clipboard path as the splash's Copy details.)*

## Phase F — Build and packaging

- [x] `cargo tauri dev` for development (it uses the Vite dev server and proxies to the engine) and
      `cargo tauri build` for release. Document both in `CLAUDE.md` and the README. *(Done without the Tauri CLI:
      plain `cargo build` builds the shell, and with no installer there is nothing for `cargo tauri build` to bundle.
      Dev: `npm run dev` plus `SEQ_DEV_URL=http://localhost:5173` makes a debug `seq-app` window load the Vite dev
      server (hot reload); Vite now also proxies `/api`. Documented in `CLAUDE.md` and the README.)*
- [-] *(Dropped, Decisions: portable only.)* The NSIS installer has an optional component **"FFGL plugin for Resolume Arena"** (checked by default). It
      runs `seq-app --install-ffgl` after installation, so the installer and the in-app feature share one
      implementation (M11 Phase G3). The uninstaller removes the plugin only if the installer put it there.
- [x] The release build produces the **NSIS installer** (with the WebView2 bootstrapper, `downloadBootstrapper`
      mode) and a **portable exe** (Phase A). *(Portable only: `scripts\release.ps1` builds both exes and
      `release\clip-sequencer-<version>-windows-x64.zip` with a `.sha256`. Instead of the bootstrapper, `seq-app.exe`
      checks for WebView2 at start; if it is missing, a native message box explains it, offers the download page
      and points to `seq-app-cli.exe` (tested with `WEBVIEW2_BROWSER_EXECUTABLE_FOLDER` set to a missing folder).)*
- [x] Version, product name, copyright and icon come from the same single sources as M11 Phase E
      (`[workspace.package]`, the icon file). `tauri.conf.json` must not hold a second, hand-maintained
      version. *(Version: `[workspace.package]` only; build.rs fails if `tauri.conf.json` gets a `version`.
      Product name, publisher and copyright: `tauri.conf.json`, which build.rs exports to the code (`APP_NAME`,
      `COPYRIGHT`) and tauri-build writes into the exe's version resource. Icon: `icons/icon.ico`. Checked:
      both exes show ProductName "Clip Sequencer", 0.1.0, LegalCopyright and CompanyName.)*
- [x] The headless mode works from `seq-app-cli.exe` (shipped next to `seq-app.exe`; see Decisions).
- [x] Update `README.md`: desktop usage, headless usage, port fallback and `--strict-port`, where logs live,
      and the WebView2 requirement for the portable exe.

## Phase G — Verification

*Run 2026-10-27 against Arena 7.28 with the composition "show-b", the Clock Shifter's clock at
120 BPM on "Default App Loopback (B)", and the exes from the portable zip, unzipped into a new folder. A copy of
the data folder held the test sequence "G test": three 2-bar clips on `fw_background`, repeated endlessly.
The run found three bugs, all fixed (commits `94947a3`, `5659eeb`):*
- *A late confirmation (~700 ms, Resolume busy) counted as a manual trigger and yielded the lane.*
- *A failing first window panicked silently.*
- *`seq-app-cli` died without cleanup on Ctrl+Break or when its console window was closed.*

- [x] Fresh Windows 11 machine or Windows Sandbox: install via NSIS, start, and the window appears with
      no console. Repeat with the portable exe. *(Done 2026-11-01: the portable zip worked on a new machine
      (Uli). There is no NSIS installer (portable only, see Decisions). Earlier: Windows Sandbox needs Pro/Enterprise, and this machine
      runs Home. Done instead: the portable zip unzipped into a new folder with an empty data folder. The
      window appears without a console, and the first run works without settings ("No MIDI clock port
      selected" warning). The missing-WebView2 path was simulated in Phase F.)*
- [x] Port taken by another program: the app starts on 8091, and the splash, window, log, About and
      `runtime.json` all show 8091. *(A dummy listener held 8090. The startup step warned "port 8090 is in use",
      and the window URL, `/api/version` `ui_port` and `runtime.json` all showed 8091. There is no About
      dialog yet (M11 F).)*
- [x] Splash: appears within ~200 ms; steps progress in order; with Resolume closed it shows the warning and
      continues; with `--strict-port` and the port taken it shows the error panel with Open log / Copy / Quit;
      no white flash when switching to the main window; `--no-splash` and `--headless` behave as specified.
      *(Visible after 123 ms and 52 ms on warm starts. The very first start of the newly unzipped exe took
      1.3 s (Defender scan). With Resolume unreachable: the warning shows, and the window opens after 5.6 s
      (3 s step limit + 1.5 s warning pause). `--strict-port`: the error panel shows "Port 8090 is in use"
      with Retry / Open log / Copy details / Quit, and Esc quits. `--no-splash` and the white-flash check
      were done in D2.)*
- [x] Second launch while running: the dialog appears and no second engine starts (check the Resolume
      triggers: no doubles). *(The "already running" dialog appeared with Open / Cancel. Open brought the
      first window to the front, and the second process exited. The log has no second engine, and the
      first instance's `runtime.json` stayed.)*
- [x] Close the window during a running sequence: the confirmation appears, layers are cleared per SPEC,
      and the process exits within 2 s with `runtime.json` removed. *(Quit / Cancel dialog. After Quit:
      the layer clear was sent, the shutdown took 3 ms, and the process ended 257 ms after the click.
      Resolume then had no clip connected, and `runtime.json` was gone.)*
- [x] The Console tab shows live logs; the filters, pause, copy and save all work. A 30-minute run at
      128 BPM shows no UI slowdown. *(31 min at 120 BPM (the Clock Shifter's tempo) with
      `RUST_LOG=info,seq_app=debug` and the Console tab open the whole time.
      - UI at the start: eval 0.4 ms, frames 16.7 ms median, heap 3.3 MB, 787 nodes.
      - UI after 30 min: eval 0.7 ms, frames 17.8 ms median / 19.7 ms max, heap 2.9 MB, 1207 nodes (55 rows
        rendered); the app process used 42 MB.
      - Filters, pause, copy and save were tested in Phase E.)*
- [x] Minimised window for 5 minutes during a sequence: triggers stay in time (compare the WS confirmations).
      *(455 triggers, one every 4000 ms.
      - Visible: intervals within ±1 ms (one at −6 ms during a build); WS confirmation 4.6 ms median.
      - Minimised for 5 min: 75 intervals within ±2 ms; confirmation 3.5 ms median, 14.6 ms max.
      - An earlier minimise was undone after ~3 min by a click on the window (same result).
      - One trigger was confirmed after ~700 ms while the window was visible: the late-confirmation bug above.)*
- [x] `--headless` from a terminal: output appears in the terminal, and the UI works in Chrome.
      *(Done with `seq-app-cli.exe`:
      - It prints the steps `[1/7]`–`[7/7]` and the UI URL; the UI page and `/api/version` answer
        (`desktop: false`).
      - Each stop is clean and removes `runtime.json`: Ctrl+Break (exit 0 after 51 ms), closing its console
        window (391 ms, with "G test" running; the layer cleared) and Ctrl+C (Phase C).
      - The UI was used in a browser in earlier phases.)*

---

## Definition of done

- `seq-app.exe` plus the console companion `seq-app-cli.exe`; double-clicking `seq-app.exe` shows the splash screen immediately, then a native window with the UI. There's no
  console window in release builds.
- The splash reports each startup step, continues past a missing Resolume or MIDI port, and shows fatal
  errors with Open log / Copy / Quit.
- The UI port falls back automatically (8090 → 8091…8099 → OS-assigned), unless `--strict-port` is set.
  The actual port is used and shown everywhere.
- A second instance never starts a second engine.
- The Sequencer and Console tabs both work. Logs are also written to rotating files.
- Clean shutdown on window close, Ctrl+Q, menu Quit and Ctrl+C (headless).
- Headless mode still works.
- NSIS installer and portable exe build reproducibly from one version source.

## Impact on M11 (update `docs/M11-open-source-release.md` when M12 is done)

- **Phase D (third-party licenses):** Tauri, wry and WebView2-related crates now appear in the notices.
  Check that `cargo-deny` still passes. Mention the Microsoft WebView2 runtime and bootstrapper in the
  notices and the README (redistribution of the bootstrapper is permitted by Microsoft).
- **Phase E (version and exe metadata):** Tauri's bundler sets the exe/installer metadata and the icon.
  Use that instead of `embed-resource`/`winres`. Add `ui_port` to `/api/version` output.
- **Phase F (menu bar):** follow the Phase A menu decision from M12. If the menu is native, the HTML menu
  only appears in browser/headless mode.
- **Phase G (README, security):** add the desktop vs. headless usage, port fallback, `--strict-port`,
  and log locations.
- **Phase I2 (release workflow):** replace "zip the exe" with `cargo tauri build`. Release assets become the
  NSIS installer plus the portable zip (exe + LICENSE + THIRD_PARTY_LICENSES + OFL.txt + README + CHANGELOG),
  each with a SHA-256 checksum.

## Decisions

_(filled in during Phase A, 2026-10-27)_

- Distribution: **portable exe only** (no NSIS installer). Phase F's installer items, including the installer's
  "FFGL plugin" component, are dropped; `--install-ffgl` stays in M11 G3. WebView2 must be installed (it ships with
  Windows 11).
- Menu: **native Tauri menu** in the desktop app; the HTML menu bar (M11 F) appears only in browser/headless mode.
  M12 wires only functions that already exist: File (Reload composition, Save now, Open data folder, Quit),
  View (Zoom in/out/reset, Toggle Console), Help (Keyboard shortcuts). About, Licenses, Documentation, Report an
  issue and Install FFGL plugin are added in M11 F/G3.
- Window behaviour: remember size and position: **yes**; confirm closing while a sequence runs: **yes**.
- Icon: **placeholder made by Claude** (a `§` mark), replaceable; shared with M11 E.
- Splash artwork: **placeholder by Claude**: app name in the UI typography with the `§ SEQ` mark on the dark
  background. IBM Plex is bundled in M11 D; until then the splash uses the token font stack with system fallbacks.
- App name until M11 Phase A: **"Clip Sequencer"**, defined in one place.
- Copyright line (splash footer, same as the M11 LICENSE): **Copyright (c) 2026 Ulrich Staudinger**.
- Taken over from M11 because the shell needs it now: one version source in `[workspace.package]` (M11 E) and a
  minimal `GET /api/version` (`name`, `version`, `ui_port`); M11 E adds build number, commit, date and target.
- Tauri in `seq-app` or a separate `seq-desktop` crate (Phase D): **in `seq-app`, behind a default-on cargo feature
  `desktop`**. `cargo build -p seq-app --no-default-features` builds a
  headless-only binary without Tauri. A separate crate would produce a second exe.
- Console use (changed 2026-10-27, after testing Phase C): **two executables**. A GUI-subsystem exe started from a
  terminal runs in the background (the shell returns at once) and gets no Ctrl+C, so `--headless` from
  `seq-app.exe` did not work. `seq-app.exe` is the desktop app (GUI subsystem in release builds, no console flash);
  `seq-app-cli.exe` is a console program that is always headless and also runs `--validate`, `--list-midi-ports`
  and `--clock-debug`. Both are thin `main`s over `seq_app::cli`. `seq-app.exe` with a console-only flag prints a
  pointer to `seq-app-cli.exe` and exits with code 2. The portable zip ships both.
