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
    is already running. *(Desktop dialog: done in Phase D; headless: done.)* In desktop mode: show a native dialog, "The sequencer is already running
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
- [ ] The engine never runs on the UI/main thread. *(`run()` is async on its own tokio runtime; the desktop
      thread split is done with the shell in Phase D.)* In desktop mode the tokio runtime runs on its own
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

- [ ] Add Tauri v2 (`tauri`, `tauri-build`) to `seq-app`, or as a thin new crate `seq-desktop` that
      depends on the `seq-app` library. Pick whichever keeps `seq-app` buildable without Tauri for
      headless use, and record the decision in `CLAUDE.md`.
- [ ] The main window is created **hidden**. It navigates to the actual UI URL once `run()` reports
      ready, and is shown only after the UI has loaded (no white flash). Until then, the splash screen
      is visible (Phase D2).
- [ ] Window title `<App name> <version>`; remembered size and position (Phase A); minimum size that
      fits the editor layout (≈1100×700).
- [ ] **Close handling:** if a sequence is running, show a native confirmation ("Drop A is running.
      Quit and stop it?"). Then trigger graceful shutdown (Phase C) and close.
- [ ] **Native menu** (if chosen in Phase A). Items call existing functions over a small Tauri command
      or by sending the existing `/ws` commands from the frontend: File (Reload composition, Save now,
      Open data folder, Install FFGL plugin… (see M11 Phase G3), Quit), View (Zoom in/out/reset, Toggle Console tab), Help (Keyboard shortcuts,
      Documentation, Report an issue, Licenses, About). Platform shortcuts: Ctrl+Q quit, Ctrl+= / Ctrl+- /
      Ctrl+0 zoom.
- [ ] **External links** (GitHub, docs, issues) open in the system default browser, never inside the
      app window. Navigation inside the window is restricted to the local UI URL.
- [ ] **Security:** Tauri capabilities/permissions limited to what the shell needs (window, dialog,
      opener/shell-open for external URLs). No filesystem or shell permissions for the web content.
      The CSP allows only the local UI origin.
- [ ] Keyboard shortcuts from SPEC §9.3 (Space, Del, R, Esc, Ctrl+Z…) work in the window exactly as
      in the browser. Check that the webview or the menu doesn't swallow any of them.
- [ ] **Throttling note:** WebView2 throttles rendering while minimized. That's acceptable, because
      timing lives in the engine. Verify that a running sequence keeps triggering correctly while the
      window is minimized for several minutes.

## Phase D2 — Splash screen

Shown from the moment the exe starts until the main window is ready. It reports what the app is
doing, and it shows startup problems before the main UI exists.

- [ ] **Window:** a separate small Tauri window, ≈560×340, frameless, centred, not resizable, no
      taskbar entry, always on top only while it's visible. It's created **first**, before the tokio
      runtime starts, so it appears within ~200 ms of launch.
- [ ] **Content is bundled static HTML/CSS** in the Tauri shell (it can't be served by axum, because the
      server isn't up yet). It uses the same tokens as `web/src/tokens.css` and the bundled fonts (offline).
- [ ] **Layout:**
  - logo or artwork (Phase A) and the app name
  - version and build (`0.1.0 · build 42 · abc1234`)
  - one **status line** with the current step, plus a thin progress bar (determinate: steps done / total)
  - small footer: `Copyright (c) 2026 <holder> · MIT License · Not affiliated with Resolume BV`
- [ ] **Startup steps reported** (from `run()` via a progress channel → Tauri event → splash):
  1. Loading configuration
  2. Starting UI server, showing the actual port (`on port 8091` if the fallback was used)
  3. Opening MIDI clock input, with the port name
  4. Connecting to Resolume (REST, WebSocket, OSC)
  5. Loading composition and sequences
  6. Fetching thumbnails (only the first batch; the rest continues in the background)
  7. Ready
- [ ] **Non-blocking failures don't stop startup.** If Resolume isn't reachable or the MIDI port is
      missing, the splash shows a warning line ("Resolume not reachable, will keep retrying"), waits
      ≈1.5 s so it can be read, and continues. The main UI then shows the red connection chips as usual.
      Startup must never hang on Resolume or MIDI: each step has a timeout of ≤3 s.
- [ ] **Fatal failures** (e.g. `--strict-port` with the port taken, unreadable config, data dir not
      writable) turn the splash into an error panel: the message, then **Open log**, **Copy details**
      and **Quit** buttons (plus **Retry** where it makes sense). The splash doesn't close by itself in
      this case.
- [ ] **Second instance** (Phase B): the "already running" dialog is shown from the splash, which then closes.
- [ ] **Timing:** the splash stays visible at least ≈800 ms (no flicker on fast starts). When the main
      window has loaded the UI, show the main window first, then close the splash with a short fade
      (≤150 ms).
- [ ] **Flags:** `--no-splash` skips it (the main window then appears when ready). `--headless` never
      shows it; the same steps are printed to stdout instead.
- [ ] **Accessibility:** status changes go into an `aria-live="polite"` region; the error panel buttons
      are keyboard-focusable; Esc on the error panel = Quit.
- [ ] Every startup step is also written to the log, so the Console tab shows the full startup sequence afterwards.

## Phase E — Log capture and Console tab

- [ ] **Log capture:** replace or wrap the current logger (`log` + `env_logger`, or a `tracing`
      subscriber) with one that sends every record to three places:
  1. stderr (headless mode only)
  2. a **ring buffer** of the last 5,000 records in memory
  3. a **broadcast channel** for live subscribers
  - plus a **log file** in `<data-dir>/logs/`, rotated daily, keeping 7 days.
  The record shape is `{ ts, level, target, message }`. Logging must never block the engine; if the
  channel is full, drop records and count them (`dropped: N` shown in the UI).
- [ ] **Protocol:** extend the UI WebSocket (SPEC §9.6) with
  - engine → UI: `log_snapshot` (the ring buffer, on connect or on request) and `log` (new records,
    batched at most every 100 ms)
  - UI → engine: `log_subscribe` / `log_unsubscribe` (only send log traffic while the Console tab is
    open), `log_set_level` (runtime filter, optional)
  Document the new message types in `docs/SPEC.md` §9.6.
- [ ] **Tab strip** in the Svelte app: **Sequencer | Console**, using the existing tokens. It's shown
      in both desktop and browser mode. The Sequencer tab is the current UI, unchanged.
      Ctrl+1 / Ctrl+2 switch tabs. The Console tab shows a red count badge when new errors arrive
      while it isn't visible.
- [ ] **Console tab:**
  - [ ] a monospace list with timestamp, level (coloured: error = `--err`, warn = `--accent`,
        info = `--text-2`, debug/trace = `--muted`), target and message
  - [ ] level filter toggles (Error / Warn / Info / Debug) and a text filter
  - [ ] auto-scroll to the newest line; scrolling up pauses it ("⏸ paused · N new", click to resume)
  - [ ] buttons: Clear view (client-side only), Copy visible, Save log… (downloads the current file
        or the buffer as `.log`), Open logs folder (desktop only)
  - [ ] virtualised rendering, so 5,000 lines stay smooth
  - [ ] a header line showing the app version, build, UI URL, and dropped-record count if any

## Phase F — Build and packaging

- [ ] `cargo tauri dev` for development (it uses the Vite dev server and proxies to the engine) and
      `cargo tauri build` for release. Document both in `CLAUDE.md` and the README.
- [ ] The NSIS installer has an optional component **"FFGL plugin for Resolume Arena"** (checked by default). It
      runs `seq-app --install-ffgl` after installation, so the installer and the in-app feature share one
      implementation (M11 Phase G3). The uninstaller removes the plugin only if the installer put it there.
- [ ] The release build produces the **NSIS installer** (with the WebView2 bootstrapper, `downloadBootstrapper`
      mode) and a **portable exe** (Phase A).
- [ ] Version, product name, copyright and icon come from the same single sources as M11 Phase E
      (`[workspace.package]`, the icon file). `tauri.conf.json` must not hold a second, hand-maintained
      version.
- [ ] The headless mode works from `seq-app-cli.exe` (shipped next to `seq-app.exe`; see Decisions).
- [ ] Update `README.md`: desktop usage, headless usage, port fallback and `--strict-port`, where logs live,
      and the WebView2 requirement for the portable exe.

## Phase G — Verification

- [ ] Fresh Windows 11 machine or Windows Sandbox: install via NSIS, start, and the window appears with
      no console. Repeat with the portable exe.
- [ ] Port taken by another program: the app starts on 8091, and the splash, window, log, About and
      `runtime.json` all show 8091.
- [ ] Splash: appears within ~200 ms; steps progress in order; with Resolume closed it shows the warning and
      continues; with `--strict-port` and the port taken it shows the error panel with Open log / Copy / Quit;
      no white flash when switching to the main window; `--no-splash` and `--headless` behave as specified.
- [ ] Second launch while running: the dialog appears and no second engine starts (check the Resolume
      triggers: no doubles).
- [ ] Close the window during a running sequence: the confirmation appears, layers are cleared per SPEC,
      and the process exits within 2 s with `runtime.json` removed.
- [ ] The Console tab shows live logs; the filters, pause, copy and save all work. A 30-minute run at
      128 BPM shows no UI slowdown.
- [ ] Minimised window for 5 minutes during a sequence: triggers stay in time (compare the WS confirmations).
- [ ] `--headless` from a terminal: output appears in the terminal, and the UI works in Chrome.

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
