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
- [x] Web UI: LUT tab (Ctrl+3), library, controls, curve editor, WebGL2 preview, import/export, "used by",
      "Use <LUT>". Checked against the mock with Playwright.
- [x] Docs: SPEC §14, manual page "Colour LUTs" with screenshots, interface, shortcuts, glossary, README,
      changelog, landing page.

## M14 — In-app update  (planned 2026-11-04, not started)
Goal: the user can check by hand whether a newer release exists, and install it from the app. Work out the
details in SPEC first (new section), then build it.

Decided (2026-11-04):
- **Only by hand:** a menu entry **Help → Check for updates…** (desktop app, and the browser UI's menu bar). No
  automatic or background checks, no check at start-up, no setting for it.
- **If a newer release is found:** show its version and release notes, and offer to install it. How the files are
  replaced is **to be decided later** (see open points).

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
- [ ] SPEC section: the menu entry, the dialog, the check, the replacement once decided, failure and rollback.
- [ ] Update check (GitHub API, version compare) with tests against a local fake release server.
- [ ] Menu entry Help → Check for updates… (native menu and browser UI menu bar) and the result dialog
      ("up to date" / new version with notes / error).
- [ ] Download, verification and binary replacement (after the decision).
- [ ] Privacy note, code signing policy, SECURITY.md, manual (installation → updating), README.
- [ ] Test on Windows: no update, update found, no network, read-only folder, Resolume running with the plugin.
