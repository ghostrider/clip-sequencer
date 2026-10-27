# Implementation plan

One milestone per Claude Code session. Each milestone ends with: tests green, clippy clean,
checkboxes ticked here, a short summary, and a commit. Don't start the next milestone unasked.

## M0 — Resolume API probe  ✓ done 2026-10-25 (Arena 7.28.1)
Goal: replace every **[verify]** in `docs/SPEC.md` with facts.
- [x] `tools/resolume-probe`: small CLI that talks to a running Resolume (Uli runs it).
- [x] Dump `GET /api/v1/composition` to `docs/samples/composition.json`; document: deck info,
      layer/column/clip structure, clip id, name, media file field, thumbnail change marker,
      composition name field.
- [x] Confirm thumbnail endpoint (by id and by layer/column).
- [x] Confirm deck select (REST and/or OSC) and how the deck change appears on the WebSocket.
- [x] WebSocket: subscribe to a clip's `connected`; record exact message shapes for subscribe,
      update, and composition-changed notifications. Save samples to `docs/samples/`.
- [x] OSC: confirm clip connect and layer clear addresses; confirm 1-based vs 0-based indices.
- [x] Check whether clips on a layer with opacity 0 still report `connected`.
- [x] Check whether a clip's source can be replaced with an image file via the API.
- [x] Write findings to `docs/resolume-api-notes.md` (endpoint, method, payload, sample response).
Done when: every [verify] item is answered in `resolume-api-notes.md`, and SPEC.md is updated.

## M1 — Workspace + data model  ✓ done 2026-10-25
- [x] Cargo workspace with the crates from `CLAUDE.md`, CI-style script for test/clippy/fmt.
- [x] `seq-core` model types + serde for the file format (SPEC §6), with round-trip tests
      against a fixture file.
- [x] Validation (lengths ≥ 1, count ≥ 2, unique names per deck) with clear errors.
- [x] Storage in `seq-app`: load/save `<Composition>.seq.json`, atomic write, debounce.

## M2 — Clip resolution + composition index  ✓ done 2026-10-25
- [x] `CompositionIndex` built from the composition JSON (use M0 samples as fixtures).
- [x] Resolution chain id → name (layer first) → file, per SPEC §4.3; layer lookup by name.
- [x] Tests: renamed clip, moved column, inserted layer, deleted clip, duplicate names.

## M3 — Scheduler (pure)  ✓ done 2026-10-25
- [x] `Scheduler` in `seq-core`: start/stop, ticks → actions, 12 ticks per eighth.
- [x] Lane end behaviours, repeat modes, after-last-pass, snapshot on start.
- [x] Layer ownership and partial takeover; yield/resume; clock-loss freeze/resume.
- [x] Unresolved steps behave as rests.
- [x] Tests with simulated tick streams covering every rule in SPEC §5 and §7.

## M4 — Clock input  ✓ done 2026-10-25
- [x] `clock-in` with midir: list ports, open by name, parse 0xF8/0xFA/0xFB/0xFC.
- [x] Tempo estimate; clock-loss detection; reconnect on port loss.
- [x] `seq-app --list-midi-ports`; a debug mode that prints bar.beat.tick live.

## M5 — Resolume client + mock  ✓ done 2026-10-25
- [x] `tools/mock-resolume` implementing the M0-confirmed REST/WS/OSC surface.
- [x] REST client (composition, thumbnails, deck select), WS client (reconnect, resubscribe),
      OSC sender.
- [x] Integration test: app + mock — composition loads, a trigger produces a confirmed `connected`.

## M6 — Engine wiring (headless)  ✓ done 2026-10-25 (visual check pending)
- [x] Engine task: clock + WS + commands → scheduler → OSC; state broadcast.
- [x] SEQ layer binding (§ clips start/stop sequences), deck-switch-on-start flow with timeout,
      deck change stops sequences, manual-override detection, trigger confirmation.
- [x] Temporary CLI commands to start/stop a sequence from a file, for testing against real Resolume.
- [ ] Manual test with Uli in Resolume: a 2-lane sequence plays in time with the clock.
      (Automated live run done 2026-10-25: real Arena 7.28.1 + 128 BPM loopback clock, bar
      boundaries 1.875 s ± 0.2 ms, all triggers confirmed. Visual check by Uli pending.)

## M7 — UI server + UI skeleton  ✓ done 2026-10-26
- [x] axum: static files (rust-embed), `/ws` protocol (SPEC §9.6), `/thumbs/...`.
- [x] Svelte app with tokens from `docs/ui/editor-mockup.html`; layout of all four regions.
- [x] Live state: deck tabs + LIVE badge, connection chips, sequence list, read-only lane grid
      with playhead, status bar.

## M8 — Editing  ✓ done 2026-10-26
- [x] Create/rename/duplicate/delete sequences; add/remove lanes; layer and end-behaviour pickers.
- [x] Clip browser with thumbnails and search; drag to lane; resize, reorder, Alt-duplicate, rests.
- [x] Step inspector with resolution status and relink; repeat control; keyboard shortcuts; undo/redo.
- [x] "edited · applies at next start" badge; orphaned-deck handling.

## M9 — Thumbnails  ✓ done 2026-10-26
- [x] Disk cache with change detection; cached thumbnails for inactive decks.
- [x] Sequence mosaic rendering (`image` crate) and "Push thumbnail to Resolume".

## M10 — Hardening  ⟵ next
- [ ] Long-run test (2 h with mock, simulated 128 BPM): no drift between tick count and triggers.
- [x] Resolume restart, WS drop, MIDI port loss, composition reload — all recover without restart.
      (tests/recovery.rs with the mock; port loss via the injectable clock supervisor.)
- [x] Release build, Windows run instructions in `README.md` (`scripts/release.ps1`).

## M11 — "Sequence" FFGL source plugin (SPEC §12)  ⟵ next
Follow `ffgl-plugins/AGENTS.md` (verify SDK headers, build and run every test, check the host log).
- [x] M11a probe: minimal source plugin with a `Sequence` text parameter that logs render calls;
      installed in Resolume (Uli restarts Resolume). Record in `resolume-api-notes.md`: render when
      previewed / connected at layer opacity 0, `video.sourceparams` shape, setting the text
      parameter over WS, effect on the custom thumbnail. (Done: notes §11 - all yes.)
- [x] M11b sequencer: `/plugin/v1/sequence?name=` (layout + thumbnail URLs + revision) and
      `/plugin/v1/runtime?name=` (running, pos, pass, bar.beat); auto-set the plugin's `Sequence`
      parameter on `§` clips; tests against the mock. (Done as one line-based endpoint,
      `/plugin/v1/view.txt`, format in crates/seq-app/src/plugin_view.rs.)
- [ ] M11c plugin `ffgl/` (moved from `ffgl-plugins/seq_view/` on 2026-10-27): FFGL 2.1 source, unique 4-char ID, worker thread
      (WinHTTP) polling the sequencer, thumbnails decoded off the render thread (stb_image),
      timeline rendering with playhead; test harness per repo convention (hidden WGL context, real
      FBO, PNG preview); confirmed loading and rendering in Resolume.
- [ ] Docs: plugin README, sequencer README (setup with the plugin instead of Blank).

## M12 — Desktop shell (Tauri v2)  ⟵ in progress
Plan and checkboxes: `docs/M12-desktop-shell.md` (phases A–G). Precedes the open-source release in
`docs/M11-open-source-release.md` (not the "M11" Sequence plugin above).
- [x] Phase A decisions (recorded in the M12 doc)
- [x] Phase B UI port selection and single instance
- [x] Phase C in-process runtime (desktop thread split done with Phase D)
- [x] Phase D Tauri shell (minimised-window timing check moves to Phase G)
- [x] Phase D2 splash screen
- [ ] Phase E log capture and Console tab
- [ ] Phase F build and packaging (portable exe)
- [ ] Phase G verification
