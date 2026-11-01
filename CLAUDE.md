# CLAUDE.md — Resolume Clip Sequencer

## What this is
A Rust app that plays multi-layer clip sequences in Resolume Arena 7.26.
It counts an incoming MIDI clock, triggers clips over OSC, follows Resolume's
state over WebSocket, reads the composition and thumbnails over REST, and serves
a browser UI. Full concept: `docs/SPEC.md`. Work order: `docs/PLAN.md`.

**Read `docs/SPEC.md` and `docs/PLAN.md` at the start of every session.**

## Ground rules
- Work on ONE milestone from `docs/PLAN.md` at a time. When it is done, tick its
  checkboxes in `docs/PLAN.md`, then stop and summarise. Don't start the next one unasked.
- Never guess Resolume endpoints, OSC addresses or WebSocket message shapes.
  Use only what is recorded in `docs/resolume-api-notes.md` (written in Milestone 0).
  If something is missing there, say so and propose a probe instead of guessing.
- Do not change the sequence file format (`docs/SPEC.md` §6) without asking.
  If a change is needed, bump `version` and write a migration.
- `seq-core` has no IO, no async, no clock reads. Everything time-related is
  driven by ticks passed in. It must stay fully unit-testable.
- Nothing on the timing path may block: the engine task never awaits HTTP.
  REST calls and thumbnail work run in separate tasks and report back by message.
- Prefer small, reviewable commits, one per logical step.

## Stack
- Rust (stable), Cargo workspace, tokio.
- Crates: `midir` (MIDI in), `rosc` (OSC), `reqwest` (REST), `tokio-tungstenite` (WS),
  `axum` (UI server + UI WebSocket), `serde`/`serde_json`, `image` (thumbnail mosaics),
  `rust-embed` (bundle the web UI), `clap`, `log` + `env_logger`, `anyhow`/`thiserror`, `uuid`.
- Frontend: Svelte + TypeScript + Vite in `web/`, built to `web/dist`, embedded into the binary.
- Target: Windows 11, same machine as Resolume. Resolume REST on `127.0.0.1:8080`,
  OSC input on `127.0.0.1:7000` (both configurable).
- The MIDI clock comes from the separate "MIDI Clock Shifter" app through its virtual
  MIDI port. On Windows `midir` cannot create virtual ports; it connects to an existing one by name.

## Layout
```
crates/seq-core/        model, serde, clip resolution, scheduler state machine (pure)
crates/clock-in/        midir input -> ClockEvent (Tick, Start, Stop, Continue)
crates/resolume-client/ REST, WebSocket, OSC, composition index
crates/seq-app/         binary: wiring, engine task, storage, thumbnails, axum UI server
tools/resolume-probe/   Milestone 0: explores the live API, writes findings
tools/mock-resolume/    fake REST/WS/OSC server for integration tests
web/                    Svelte UI
ffgl/                   "1000A Sequence" FFGL source plugin (C++/MSVC, own build: build_seqview.ps1, SPEC §12)
external/ffgl/          FFGL SDK (git submodule, pinned commit)
third_party/stb/        stb_image.h, stb_truetype.h (vendored; MIT / public domain)
docs/                   SPEC.md, PLAN.md, resolume-api-notes.md, samples/
docs/ui/                editor-mockup.html — the binding UI reference (layout, tokens, states)
```

## UI
Build the UI to match `docs/ui/editor-mockup.html` (open it in a browser; read its CSS).
Copy its `:root` tokens to `web/src/tokens.css` and use only those variables for colour.
Interaction rules are in `docs/SPEC.md` §9; where the mockup and the spec disagree, the spec wins.

## Commands
- Build: `cargo build --workspace`
- Test: `cargo test --workspace`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all`
- FFGL plugin (`ffgl/`, second build system: MSVC, not cargo): build + install
  `powershell -ExecutionPolicy Bypass -File ffgl\build_seqview.ps1` (Resolume closed); tests: build
  `ffgl\tests\SeqViewTests.vcxproj` (Release x64) and run `ffgl\tests\bin\Release\SeqViewTests.exe`.
  It uses the FFGL SDK as a pinned git submodule (`external/ffgl`; after cloning run
  `git submodule update --init`) and the vendored stb headers in `third_party/stb`.
  Follow `ffgl/AGENTS.md` for plugin work.
- All checks (fmt, clippy, test): `powershell -ExecutionPolicy Bypass -File scripts\check.ps1` (`-Fix` to format)
- UI: `cd web && npm install && npm run build` (dev: `npm run dev`, proxies `/ws` and `/thumbs` to the
  app on :8090). Build the UI before `cargo build --release`: release binaries embed `web/dist`.
  Type check: `npm run check`.
- Validate a sequence file: `cargo run -p seq-app -- --validate <file.seq.json>`
- MIDI: `cargo run -p seq-app -- --list-midi-ports`; live clock check `--clock-debug --midi-port <name>`.
  Test clock without the Clock Shifter (Windows MIDI loopback A→B):
  `cargo run --release -p clock-in --example clock_gen -- "Default App Loopback (A)" 128`
- Mock Resolume (no Resolume needed): `cargo run -p mock-resolume -- --rest-port 8080 --osc-port 7000`
- Smoke test vs. real Resolume (triggers + clears the first clip): `cargo run -p resolume-client --example live_check`
- Headless test run: `cargo run -p seq-app -- --midi-port "Default App Loopback (B)" --start "<sequence>" --exit-when-done`
- Release: `powershell -ExecutionPolicy Bypass -File scripts\release.ps1` → `release\seq-app.exe`, `seq-app-cli.exe` and the
  portable zip `clip-sequencer-<version>-windows-x64.zip` (+ `.sha256`). Portable only, no installer (M12).
- Long-run test: `cargo run --release -p seq-app --example soak -- "Default App Loopback (B)" <minutes>`
  (with clock_gen running)
- Run: `cargo run -p seq-app -- --midi-port "Clock Shifter" --resolume-host 127.0.0.1`
  (`seq-app-cli` is the console program: always headless, UI in a browser. `seq-app` is the desktop app, M12;
  release builds of it have no console. Run headless with `cargo run -p seq-app --bin seq-app-cli -- ...`. The UI port falls
  back from `--ui-port` (8090) to the next 9 ports, then an OS-assigned one; `--strict-port` disables that.)
- Headless-only build without the desktop shell: `cargo build -p seq-app --no-default-features`
- Desktop shell (M12): Tauri v2 lives in `seq-app` behind the default-on `desktop` feature (`src/desktop.rs`,
  `tauri.conf.json`, `icons/`, `shell/`); no separate crate, so one `cargo build` gives both exes. The web content
  gets no Tauri IPC; window actions from the UI go over `/ws` (`shell` command). WebView2 DevTools for debugging:
  set `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9223` before starting `seq-app`.
  Plain `cargo build`/`run`; the Tauri CLI (`cargo tauri dev/build`) is not used (no bundle, the window loads the
  app's own server). Hot reload in the window: `npm run dev` in `web/`, then `SEQ_DEV_URL=http://localhost:5173`
  for a debug `seq-app` (debug builds only). Product name and copyright: `tauri.conf.json` (build.rs exports
  them to the code and the exe resource); the version only in `[workspace.package]`.
- Logging: `seq_app::logging` (not env_logger) feeds the Console tab and `<data-dir>/logs/` (7 days);
  `RUST_LOG` filters as before. Use `log::` macros; never log from inside the logger's own send path.

Run tests, clippy and fmt before declaring a milestone done.

## Git
- Commit messages in English; small commits, one per logical step.
- Feature branches from AI sessions use the prefix `claude/`.
- After cloning: `git submodule update --init` (FFGL SDK in `external/ffgl`).
