# Contributing

Thanks for helping. Bug reports and small, focused pull requests are the most useful. For anything larger
(new behaviour, a change to the sequence file format, a new dependency), please open an issue first so we can
agree on the approach before you spend time on it.

## Setup

Windows 10/11 with [Rust](https://rustup.rs/) (stable, MSVC toolchain) and Node.js 20.19+ or 22.12+.
Clone with the FFGL SDK submodule:

```powershell
git clone --recurse-submodules https://github.com/ghostrider/clip-sequencer
# or, in an existing clone:
git submodule update --init
```

Optional tools: `cargo install --locked cargo-deny` (license and advisory checks) and
`cargo install --locked cargo-about --features cli` (third-party notices for a release).
The FFGL plugin needs Visual Studio 2022 with the C++ desktop workload.

No Resolume? `cargo run -p mock-resolume` is a fake Resolume (REST, WebSocket, OSC) that behaves like the real
one as recorded in `docs/resolume-api-notes.md`; `--composition docs/samples/demo-composition.json` gives it a
small demo show.

## Build and test

| What | Command |
|---|---|
| All checks (versions, fmt, clippy, tests, cargo deny) | `powershell -ExecutionPolicy Bypass -File scripts\check.ps1` (`-Fix` formats) |
| Rust tests | `cargo test --workspace` |
| Lint | `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all` |
| UI build / type check | `cd web; npm install; npm run build` / `npm run check` |
| UI with hot reload | `cd web; npm run dev` (proxies to the app on :8090) |
| Release zip | `powershell -ExecutionPolicy Bypass -File scripts\release.ps1` |
| FFGL plugin | `powershell -ExecutionPolicy Bypass -File ffgl\build_seqview.ps1` (builds and installs; Resolume closed) |
| FFGL plugin tests | build `ffgl\tests\SeqViewTests.vcxproj` (Release, x64), run `ffgl\tests\bin\Release\SeqViewTests.exe` |
| Version consistency | `node scripts/check-version.mjs` |

Build the UI before `cargo build --release`: release binaries embed `web/dist`. `CLAUDE.md` lists more
commands (test clock, soak test, headless runs).

## Rules of the code base

- **Don't guess the Resolume API.** Endpoints, OSC addresses and WebSocket message shapes come from
  `docs/resolume-api-notes.md`, which records what Resolume really does. If something isn't there, propose a
  probe (see `tools/resolume-probe`) instead of guessing, and add the result to the notes.
- **The sequence file format** (`docs/SPEC.md` §6) changes only with a `version` bump and a migration from the
  previous version, plus a fixture and a test for the migration (see the v1 → v2 migration in
  `crates/seq-core/src/file.rs`). Users' show files must keep loading.
- **`seq-core` stays pure:** no IO, no async, no clock reads; time comes in as ticks. Everything there is
  unit-tested.
- **Nothing on the timing path blocks:** the engine task never awaits HTTP; REST and thumbnail work run in
  separate tasks and report back by message.
- **UI colours only from `web/src/tokens.css`.** `docs/ui/editor-mockup.html` is the visual reference; where it
  and `docs/SPEC.md` §9 disagree, the spec wins.
- **FFGL plugin (`ffgl/`):** read [`ffgl/AGENTS.md`](ffgl/AGENTS.md) first. Verify SDK APIs in the headers,
  build and run the tests yourself, and check the plugin in Resolume before calling a change done.
- **Dependencies** must pass `cargo deny check licenses` (`deny.toml`): permissive licenses only (MIT,
  Apache-2.0, BSD, ISC, Zlib, Unicode-3.0, MPL-2.0). GPL/AGPL/LGPL can't be used.
- **One version source:** `[workspace.package]` in `Cargo.toml`; `web/package.json` must match
  (`scripts/check-version.mjs`). Only maintainers change it, as part of a release.

## Style

- Rust: `cargo fmt`; `cargo clippy -- -D warnings` clean. Svelte/TypeScript: `npm run check` clean.
- Comments explain why, not what. Keep functions small and names plain.
- Commit messages in English, imperative, with a scope when it helps: `fix(web): …`, `feat(seq-app): …`.
  One logical change per commit.

## Pull requests

- Branch from `main`; keep the PR to one topic.
- Tests for new behaviour and for bug fixes (a test that fails without the fix).
- `scripts\check.ps1` passes (CI runs the same checks).
- Add a line under **Unreleased** in [CHANGELOG.md](CHANGELOG.md) for anything a user would notice.
- **Changes to user-facing behaviour update the user manual in the same PR** (`site/manual/`, once it exists),
  so the manual never describes an older app.
- Screenshots in docs show neutral demo content only (the demo composition), never real show footage.
- PRs are squash-merged.

By contributing you agree that your contribution is licensed under the project's [MIT License](LICENSE).
