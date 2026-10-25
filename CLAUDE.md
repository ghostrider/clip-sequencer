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
- All checks (fmt, clippy, test): `powershell -ExecutionPolicy Bypass -File scripts\check.ps1` (`-Fix` to format)
- UI: `cd web && npm install && npm run build` (dev: `npm run dev`, proxies to the app)
- Validate a sequence file: `cargo run -p seq-app -- --validate <file.seq.json>`
- Run: `cargo run -p seq-app -- --midi-port "Clock Shifter" --resolume-host 127.0.0.1`

Run tests, clippy and fmt before declaring a milestone done.
