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

## M10 — Hardening  (soak test open, see Release 0.5.0)
- [ ] Long-run test (2 h with mock, simulated 128 BPM): no drift between tick count and triggers.
- [x] Resolume restart, WS drop, MIDI port loss, composition reload — all recover without restart.
      (tests/recovery.rs with the mock; port loss via the injectable clock supervisor.)
- [x] Release build, Windows run instructions in `README.md` (`scripts/release.ps1`).

## M11 — "Sequence" FFGL source plugin (SPEC §12)  (shipped in 0.2.0; boxes below to tidy, see Release 0.5.0)
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

## 1/16 grid  ✓ done 2026-11-01
- [x] File format v2: step lengths in sixteenths (6 ticks); v1 files migrated on load (×2),
      original kept as `<file>.v1.bak` on the first save (SPEC §3, §6).
- [x] Editor: resize snaps to 1/8, Shift snaps to 1/16; sixteenth ruler ticks when zoomed in.
- [x] Plugin protocol `SEQVIEW 2` (lengths in sixteenths); the plugin still reads `SEQVIEW 1`.

## M12 — Desktop shell (Tauri v2)  ✓ done 2026-11-01
Plan and checkboxes: `docs/M12-desktop-shell.md` (phases A–G). Precedes the open-source release in
`docs/M11-open-source-release.md` (not the "M11" Sequence plugin above).
- [x] Phase A decisions (recorded in the M12 doc)
- [x] Phase B UI port selection and single instance
- [x] Phase C in-process runtime (desktop thread split done with Phase D)
- [x] Phase D Tauri shell (minimised-window timing check moves to Phase G)
- [x] Phase D2 splash screen
- [x] Phase E log capture and Console tab
- [x] Phase F build and packaging (portable exe; zip + sha256 via scripts\release.ps1, WebView2 check)
- [x] Phase G verification (live with Arena + Clock Shifter; three bugs found and fixed; Windows Sandbox
      not possible on Home, the portable zip was tested from a new folder instead; fresh-machine check
      passed 2026-11-01)

## M13 — Slot lanes (positional sequencing, SPEC §4.5)  ✓ done 2026-11-03 (live check pending)
- [x] M13a probe (2026-11-03): UI drag/drop and the playing-slot case, `GET /layers/{L}` and
      `/layers/{L}/clips/{C}`. Recorded in `resolume-api-notes.md` §15, SPEC §4.5 adjusted.
- [x] `seq-core` model: lane `mode`, slot steps with `column`; file format version 3 (after merging main's
      v2 = sixteenths); v1/v2 load (v1 lengths doubled), slot lanes need v3, `.v<N>.bak` on first save;
      validation; round-trip + migration tests.
- [x] New media dropped onto the playing slot: no yield, no re-trigger; played at its next step (SPEC §4.5).
- [x] Scheduler: `StepTarget::Slot { column }` in the run snapshot; engine triggers the slot by position at send
      time; empty/out of range → clear. Pre-step slot fetch task (SPEC §4.5) feeding the engine by message. Tests incl.
      content swapped while running (silent swap) and empty slot = rest. (Not tested separately: columns
      inserted while running; that is a pushed structural change, handled like any composition push.)
- [x] Edit operations: set lane mode with step conversion (both directions), set step column; undo.
- [x] UI: mode switch in the lane header, `col N` blocks with live slot thumbnails, empty-slot state,
      browser drop onto slot lanes, inspector column stepper,
      column-number overlay (top left) on every clip browser thumbnail.
- [x] Sequence mosaic, `/plugin/v1/view.txt` and the FFGL view show the current slot contents.
- [x] Mock-resolume: slot GET route, silent slot swap and media drop; engine tests for both cases.
- [ ] Live check in Resolume (Uli): swap clips in slots and drop media onto the playing slot while a
      slot sequence runs; also whether the slot's thumbnail updates after a media drop (Resolume kept
      its `last_update`, notes §15; the UI keys thumbnails by content as well).

## M15 — Themes (SPEC §9.7)  ✓ done 2026-11-18 (Windows check pending)
Asked for on 2026-11-18, before M14: a look of its own instead of the typical generated-app look.
- [x] Direction chosen from a comparison of four looks: Rack (cool slate, cyan accent, square corners,
      condensed labels, no glow) becomes the default; the mockup's tokens and `web/src/tokens.css` updated.
- [x] All UI colours through tokens (the last hard-coded ones replaced), corner radii through
      `--radius-sm/-lg/-pill`, fonts through `--font/--font-label/--font-head/--mono`.
- [x] Five built-in themes, Preferences → Appearance with live preview, Save / Cancel.
- [x] Colour and font editor; save as new theme, save changes, delete; stored in `settings.json`,
      sent to every UI (`appearance`); validated in the app; integration test.
- [x] Fonts bundled (OFL 1.1) with their notices in the third-party licences; `§ SEQ` logo in Unbounded.
- [ ] Windows check: desktop window and splash colours, fonts in WebView2, release zip size.

## M16 — Sync SEQ layer (SPEC §7.6)  ✓ done 2026-11-19
Asked for on 2026-11-18: a button that creates the SEQ layer if needed and adds a `§` clip (with the FFGL
plugin) for every sequence of the deck.
- [x] Probe: add layer / column without a body, rename layer and clip, opacity by id (notes §16).
- [x] Probe: load the "1000A Sequence" source by URL (needs the plugin installed, Resolume restarted).
- [x] `seq_core::seq_sync::plan` (pure, unit tests): layer state, missing clips, orphans, empty slots.
- [x] REST: `add_layer`, `add_column`, `clear_clip`, `layer_json`, `video_sources`.
- [x] Worker `seq_app::seq_sync` (fresh REST read, re-plan, act, verify; reports by `Aux::SeqSynced`);
      engine command `SyncSeqLayer`, plan in the live `DeckView`, `seq_sync_busy` in the runtime state.
- [x] UI: `Sync SEQ` button in the sequence list header (live deck), dialog with the plan and the
      "remove `§` clips without a sequence" option.
- [x] Live check with Blank (plugin not installed): create layer, add clips, orphan removal, no-op re-run.
- [x] Live check with the plugin: clips hold "1000A Sequence" and their `Sequence` parameter is bound.
- [x] Docs: SPEC §7.6, §9.2, §9.6; notes §16; manual (Setting up Resolume, Playing live); README.

## M17 — Resolume tempo as clock (SPEC §3.1)  ✓ done 2026-11-19
Asked for on 2026-11-19: follow Resolume's tempo when no MIDI clock is used. Probed: notes §17.
- [x] Probe: tempo, play state, tap and resync over WebSocket and OSC (notes §17); Resolume has no beat phase.
- [x] Setting `clock_source` (`midi` | `resolume`) in settings.json, Preferences → Clock, CLI `--clock resolume`.
- [x] Tick generator (`clock_in::ResolumeClock`, own thread; `thread::sleep` in 1 ms slices instead of a
      waitable timer, Rust's sleep already uses a high-resolution one): 24 PPQN at a given tempo; set tempo,
      pause, resume, resync. Unit tests on the pure `TickPlanner`; jitter example (0.4 ms median, 1.5 ms max).
- [x] Resolume side: tempo, play, resync subscribed by id from each composition push (also with the MIDI
      source), fed to the generator; reconnect stops the ticks until Resolume reports again. Mock Resolume has
      the tempo controller.
- [x] Engine: resync jumps every playing sequence to bar 1 (`Scheduler::rewind_all`: position 0, pass counter
      kept; unit tests); the release update of a press is ignored; Pause/Stop freeze with their own message.
- [x] UI: clock source in Preferences → Clock; chip `RESOLUME 128.0 BPM · press Resync` / `· synced` /
      `· paused` / `· stopped` / `RESOLUME TEMPO · not connected`.
- [x] Live check (2026-11-19, silent test sequence): tempo changes, 3 resyncs (one jump each), Pause/Play
      (no jump), Stop/Play (jump). Release build tested with real clips by the user ("works"). A Resolume
      restart while following its tempo was not checked separately.
- [x] Docs: SPEC §3, §3.1, §9.6, §10, §11; manual (Setting up the clock, Settings, Playing live,
      Troubleshooting, Glossary, FAQ, Interface, Introduction, Installation, Quick start); README, changelog,
      landing page.

## M18 — LUT editor, applied through Resolume's LUT effect  (built 2026-11-22, live check pending)
Asked for on 2026-11-22: build LUTs in the app and apply them in Resolume live. Probed: notes §18. Resolume's
own **LUT** effect reloads its `.cube` file by itself when the file changes, and its path can be set over the
WebSocket, so the user chose it over an own FFGL plugin (2026-11-22; the plugin idea is dropped).
- **Applying:** the app writes each library LUT as `<data-dir>/luts/<name>.cube`, always as a temp file renamed over
  the old one. Resolume's LUT effect on a clip, layer, group or the composition points at that file (`LUT 3D`,
  `ParamFile`); its `Opacity` is the mix. It keeps working without the sequencer (the file stays on disk).
- **Live edits:** dragging a control rebakes and rewrites the file, throttled (about 10 writes per second at most,
  the last value always written); off the engine task. Every LUT effect using that file updates.
- **LUT library** in a new LUT tab:
  - library actions: new, duplicate, rename, delete, import `.cube` (1D/3D, resampled to 33³), export;
  - grading controls: exposure, contrast, temperature/tint, saturation, and lift/gamma/gain per channel;
  - **RGB curves** (master + R, G, B, monotone spline) from the start;
  - an imported table can be the base under the controls;
  - before/after preview (WebGL2, same baked table) on bundled test images and clip thumbnails.
- **Binding by reference:** the LUT effect's `LUT 3D` path is the link, saved by Resolume with the composition.
  - The app finds LUT effects in the composition JSON whose path is in its luts folder ("used by").
  - "Apply to…" lists all LUT effects and sets the chosen one's `LUT 3D` over the WebSocket.
  - Renaming a LUT renames the file and rewrites the path of the effects using it; deleting warns when in use.
- **Code and storage:** colour math only in a new pure crate `lut-core`; `<data-dir>/luts/<name>.lut.json`
  (versioned: params, curves, optional imported base table) next to the baked `<name>.cube`.

Tasks:
- [x] Probe (2026-11-22, notes §18): FFGL effect params at composition, layer and clip level; a 3D texture in an
      FFGL effect works; Resolume's LUT effect: `LUT 3D` path, reload on overwrite and on rename, path set over WS.
- [ ] Check before relying on them: Resolume with a 33³ `.cube` (probed only 17³), `DOMAIN_MIN/MAX`, and how fast
      it reloads during a fast drag (pick the write throttle from it).
- [x] `lut-core`: params, curves, bake to 33³, `.cube` read/write (1D/3D, domain), tests.
- [x] App: library storage, throttled atomic `.cube` writes, WS commands, LUT effect discovery (composition,
      layers, live deck's clips; not layer groups) and "apply to", mock-resolume effect parameters. Tests.
- [x] Web UI: LUT tab (Ctrl+2), library, controls, curve editor, WebGL2 preview, import/export, "used by",
      "Use <LUT>". Checked against the mock with Playwright.
- [x] Docs: SPEC §14, manual page "Colour LUTs" with screenshots, interface, shortcuts, glossary, README,
      changelog, landing page.

## M14 — In-app update  (planned 2026-11-04; built 2026-11-25; Windows test passed 2026-11-26; SPEC §15)
Goal: the user can check by hand whether a newer release exists, and install it from the app. Work out the
details in SPEC first (new section), then build it.

Decided (2026-11-04):
- ~~**Only by hand**~~ Changed 2026-11-25: a check **once at start-up** (Download and install / Skip this version /
  Later; off in Preferences or with `--no-update-check`), and **Help → Check for updates…** by hand (SPEC §15).
- **If a newer release is found:** show its version and release notes, and offer to install it.
- **Binary replacement (decided 2026-11-23): full replacement** by a helper after the app quits, with rollback
  (outline in `docs/release-0.5-plan.md` §D). Part of release 0.5.0.

Open points for the SPEC section:
- **Privacy texts:** the privacy note and the code signing policy (`site/landing/privacy.html`,
  `code-signing.html`: "no update check") and SECURITY.md say the app contacts nothing but Resolume. Reword them
  to: no automatic contact; **Check for updates** asks GitHub only when the user chooses it.
- **Source:** `GET https://api.github.com/repos/ghostrider/clip-sequencer/releases/latest` (no login). Compare
  semver with our version; ignore drafts and pre-releases. Network work in its own task, never on the timing path.
- **Binary replacement (to be decided):** e.g. rename the running exe and move the new files in, then restart; or
  a small helper that swaps the files after the app quits; or only download and open the folder. Whatever is
  chosen: verify the download first (`SHA256SUMS.txt`, and the signature once releases are signed through
  SignPath; the hash alone comes from the same place as the zip), touch only the `clip-sequencer` folder and never
  the data folder, keep the old version until the new one started, not while a sequence runs, and fall back to
  opening the release page if the folder isn't writable.
- **FFGL plugin:** after an update, the existing "FFGL plugin update available · Update…" hint takes over.
- **Console program:** `seq-app-cli --check-update` (and possibly `--update`).

Tasks (to refine when it starts):
- [x] SPEC section: the menu entry, the dialog, the check, the replacement once decided, failure and rollback.
- [x] Update check (GitHub API, version compare) with tests against a local fake release server.
- [x] Menu entry Help → Check for updates… (one menu bar for the desktop app and the browser UI since 0.4) and the
      result dialog ("up to date" / new version with notes / error); the check at start-up with Download and install /
      Skip this version / Later; Preferences → Updates.
- [x] Download, verification and binary replacement (helper = the new seq-app-cli, decided 2026-11-25), with
      rollback; `scripts/fake-update-source.mjs` and `--update-source` for testing (docs/RELEASING.md).
- [x] Privacy note, code signing policy, SECURITY.md, manual (installation → updating), README.
- [x] Test on Windows: no update, update found, no network, read-only folder, Resolume running with the plugin
      (with Uli 2026-11-26, 0.4.0 → 0.4.1, all passed), and the rollback (a broken 0.4.2: the old version came
      back). Found: the helper waited the full 60 s for a version that had already quit; now it rolls back at once
      when the new version quits, and waits 30 s for one that hangs.

---

# Release 0.5.0  (planned 2026-11-23/24)

Decided with Uli on 2026-11-23/24 (background and rejected ideas: `docs/release-0.5-plan.md`). Spec: format v4
(SPEC §6), quantized start (§3.2), starting a running sequence (§7.1), transitions per step (§7.7), editor (§9.3,
§9.4), in-app update (§15). Order: M19 first (everything else builds on the format); M21's probe needs Uli and
Resolume and can run whenever he has time; the rest in the order below. One milestone at a time, as always.

## M19 — Sequence file format v4 (SPEC §6)  ✓ done 2026-11-24
- [x] `seq-core` model: `start` object (`immediate` / `beat` / `bar` + `bars`), `sections`, lane and step
      `transition`, step `when`, `choices` + `pick`; strict serde forms (`deny_unknown_fields`, the repr pattern of
      `repeat`).
- [x] Load v1–v3 (`start: "immediate"` → object), v4; refuse > 4; first save writes v4 and keeps `.v<N>.bak`.
- [x] Validation for every new field (ranges, "only with" rules, kinds matching the lane mode, sorted sections).
- [x] Clip resolution (§4.3) and relink for every choice; unresolved marks per option.
- [x] Edit operations with undo (SPEC §9.6 list) and their protocol; round-trip, migration and validation tests.
- [x] The app keeps playing as before: new fields are carried but not acted on yet (the scheduler ignores them
      until their milestone); `--validate` reports v4 errors.
- [x] Samples: update `docs/samples/demo.seq.json` to v4 (keep a v3 copy for the migration test).

## M20 — Quantized start, cue, mute/solo (SPEC §3.2, §3.3, §7.8; G1, G2)  ✓ done 2026-11-25
- [x] Bar phase in the engine: MIDI Start, Resync (and Play after Stop), else first tick; `bar phase not synced`.
- [x] Scheduler (pure): pending starts with a target tick, ▶ again = now, Stop cancels, recompute on a phase reset,
      freeze with the clock; `§` starts wait too; deck switch first. Unit tests on boundaries.
- [x] UI: start mode in the editor header, `starts in …` on card and header.
- [x] SPEC section for **cue** (G1): how to cue (card action, key), cue points (start mode, end of the current pass
      of a running sequence, next 8/16 bars), one cue or a queue, cancelling; then build it.
- [x] SPEC section for **mute/solo** (G2): lane header buttons, a muted lane counts but doesn't trigger and comes
      back at its next step, solo = mute the others; not saved; what a mute does to the layer (clear or hold);
      then build it.
- [x] Docs: manual (Playing live), shortcuts, changelog.

## M21 — Transitions per step (SPEC §7.7; A)  ✓ done 2026-11-27
- [x] **Probe** with Uli in Resolume (`tools/resolume-probe`), recorded in notes §19 with samples: set layer
      `transition.duration` / `blend_mode` over WS (by id, by path; units); a set just before an OSC `connect` (same
      tick, 1 ms, 1/16 ahead); changing it during a running fade; does a layer clear fade; what `layer_determined`
      covers; do the param ids survive a deck switch / reload. Adjust SPEC §7.7 to the findings.
      Done 2026-11-27 (notes §19, Arena 7.29.0; reload not probed). Decided: the stop's clear uses the original.
- [x] Scheduler (pure): the transition each step gets (rules 1–4), decided one sixteenth ahead together with the
      condition and pick; "original" bookkeeping per layer (take over, hand over, release, user change). Tests.
      Done 2026-11-27: `scheduler` (`Fade`, `SetTransition`, `TransitionReleased`; waits for a running fade) and
      `seq_core::transition` (originals, seconds, own vs. user changes).
- [x] Engine + `resolume-client`: subscribe layer transition params by id, send ahead, tempo-change resend
      (throttled), 10 s cap, restore after clear, user-change detection, `transitions-pending.json` and restore on
      reconnect. Mock-resolume layer transition params. Integration tests: stop, finish, Stop all, deck change,
      takeover, user change, crash restore.
      Done 2026-11-27 (`engine/fades.rs`; the restore goes before the clear, decided 2026-11-27). Deck change
      and finish are covered by the scheduler tests and the finish test; no engine test of its own for a deck change.
- [x] UI: new token `--fade` (tokens.css, themes.ts, mockup); fade wedge, labels, own vs. inherited, cut, `›`,
      warnings; lane default chip; inspector Transition. Checked with Playwright against the mock.
      Done 2026-11-27 (`lib/fade.ts`, `FadePicker.svelte`; the lane chip edits in a small popover so the lane
      header keeps its height). Warnings use the clock's BPM, 120 without a clock.
- [x] Live check with Uli; docs: manual page "Transitions", changelog, README.
      Docs written 2026-11-27 (manual `transitions.md` with shots from the mock; `site/screenshots.mjs` doesn't
      make them yet). Live check passed 2026-11-27: fades, set ahead, other lane untouched, restore on stop (a cut),
      the user's value kept, restore after the app was killed.

## M22 — Pass conditions and choices (SPEC §6; G4, G5)  ✓ done 2026-11-25 (UI not yet checked in a browser)
- [x] Scheduler (pure): evaluate `when` per pass (loops of a short lane share the pass), `last` with count/once,
      infinite rules; pick (`cycle` counter per run, `random` from the engine's seed). Unit tests incl. a condition
      false (nothing sent) and a picked rest / unresolved option.
- [x] FFGL view and sequence mosaic: show option 1 (or the option playing). Both show option 1.
- [x] UI: condition chip and `1/3` badge on blocks, inspector Play on and Choices (add from the browser by drag onto
      the inspector, reorder, relink), warning for `last` with infinite repeat.
- [x] Docs: manual (Editing), changelog.

## M23 — Section markers (SPEC §6, §9.3; G7)  ✓ done 2026-11-25 (UI not yet checked in a browser)
- [x] UI: flags in the ruler, lines through the lanes, add (`M`, context menu), move, rename, delete, greyed beyond
      the end; guide lines 8/16/32/off as a view setting in `settings.json`.
- [x] Docs: manual, shortcuts, changelog.

## M24 — Starting a running sequence (SPEC §7.1; C)  ✓ done 2026-11-25
- [x] Engine: ▶ on a running sequence restarts but keeps the `§` link; the `§` clip connecting adopts the run;
      `§` connecting while pending links the pending start. Tests for every order.
- [x] Remove the "one place at a time" limitation from README and manual (Playing live); changelog.

## M25 — Record from Resolume (SPEC §7.9; G6)  ⟵ next
- [x] SPEC section: arm per sequence (which lanes, overwrite or insert, count-in), snap to 1/16, what a click on a
      layer without a lane does (new lane?), stop recording, undo as one step; slot lanes record columns.
      Done 2026-11-27 (§7.9): loops until Stop, one take per pass written at its end (one undo each) and played
      back the next pass; overwrite from the first click to the pass end; new layers get a lane.
- [ ] Probe with Uli: WS messages for clip clicks and the layer Clear button made by hand (shape, timing; a clear vs.
      switching clips), notes §20.
- [ ] Pure part (`seq-core`): a take (clicks per layer, snapped) applied to a sequence (write out short lanes,
      overwrite, new lanes, slot columns, stop position); tested.
- [ ] Engine: arm/disarm, loop while recording, clicks → take (own triggers and the SEQ layer excluded), yield from
      the first click, write the take at the pass end as one undo step and play it the next pass. Mock tests.
- [ ] UI: ⏺ arm in the editor header, `armed` / `REC` on the card and the editor. Checked with Playwright.
- [ ] Docs (manual, changelog, README) and a live check with Uli.

## M26 — Pre-show check (G16)
- [ ] Probe: the clip play mode field (Loop / Play once …) and the clip duration, notes §19 or §20.
- [ ] SPEC section: what is checked (unresolved clips and choices, missing layers, Beat Snap on (`clipbeatsnap`,
      notes §2), clips with their own transition, unknown blend modes, clips shorter than their step that don't
      loop, `last` with infinite repeat), where it shows (button, dialog, jump to the step).
- [ ] Build (pure checks in `seq-core` where possible), UI, docs.

## M27 — LUT preview side by side (requested by Uli 2026-11-25, SPEC §14.4)  ✓ done 2026-11-25 (not yet checked in a browser)
- [x] LUT tab preview: instead of the movable split, two panels next to each other: left the sample image as it is,
      right the same image with the LUT applied. Both scale the image to fit their panel (aspect kept, no cropping).
- [x] Keep the current sample images (and the "your clips" source); SPEC §14.4 and the manual (LUTs) updated.
- [x] Test images: the Kodak Lossless True Color Image Suite (decided with Uli 2026-11-25; 24 JPEGs in
      `web/public/kodak`, README and licence list entry). Not "Lenna": it is not free of copyright (a 1972 Playboy
      photo) and many journals no longer accept it.
- [x] New screenshot `site/manual/src/images/luts-tab.png` (`site/screenshots.mjs`, needs Playwright); also the update
      dialog, Preferences → Updates, and sections and badges in the editor and overview (2026-11-26).

## M28 — Remote Resolume (requested by Uli 2026-11-26)
Resolume runs on a computer next to the projector (short HDMI cable), the sequencer on another one further away,
connected over Ethernet. Today everything assumes one machine (`127.0.0.1`), although `--resolume-host` exists.
- [ ] Probe with Uli on two machines: REST, WebSocket and OSC to Resolume over the LAN (Resolume's web server and
      OSC input bound to the network, firewall), latency and jitter of OSC triggers, thumbnails; notes new §.
- [ ] SPEC section: what works remotely and what needs the Resolume machine — files Resolume must read from its own
      disk (LUT `.cube` files, M18; media dropped onto slots, M13), the FFGL plugin, which polls the sequencer on
      localhost (§12: needs the sequencer's address and the UI server bound beyond `127.0.0.1`, see its security note),
      "Resolume running" detection and FFGL install/update (local only), latency compensation per host.
- [ ] Build, docs (manual: two-machine setup), live check with Uli.

## M29 — UI access from other computers (requested by Uli 2026-11-26)
Today the UI server answers only on `127.0.0.1` (SPEC §12.2). Let another computer on the network open the UI.
- [ ] SPEC section: a setting "Allow access from other computers", **off by default**, in the Preferences dialog
      (and a command-line flag); when on, the server also listens on the network (all interfaces or a chosen
      one); what it means for security (anyone on the network can control the show: access key or PIN?, the
      Origin check of §12.2 with other hosts, a warning in the dialog, Windows firewall prompt); does a change
      apply at once or after a restart; the address to open shown in Preferences.
- [ ] Build: setting in the preferences file, bind address in `seq-app`, Preferences UI, tests (off: no answer
      on the LAN address; on: answers, foreign Origin still refused).
- [ ] Docs: manual (Preferences, privacy/security), README, SECURITY.md, changelog. Shares the network part with M28.

## M14 — In-app update (SPEC §15) — see above, now part of 0.5 with full replacement

## Release 0.5.0 — before tagging
- [ ] Housekeeping: M11c and the plugin docs boxes (shipped since 0.2.0; the "⟵ next" markers are already gone); dependency PRs
      (cargo/npm/actions minor bumps; TypeScript 7 only after `npm run check` passes); LUT effects in layer groups
      (optional).
- [ ] Open checks: M6 (2 lanes in time, live), M10 2 h soak, M13 slot lanes live, M15 Windows look and zip size,
      M18 33³ `.cube`, `DOMAIN_MIN/MAX`, reload speed.
- [ ] Changelog, manual, landing page; `docs/RELEASING.md`.
