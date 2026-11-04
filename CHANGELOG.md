# Changelog

All notable changes to Clip Sequencer for Resolume are listed here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Changes that are merged but not released yet go under
**Unreleased**; a release moves them into its own section (see `docs/RELEASING.md`).

## [Unreleased]

### Changed

- The release zip holds a single folder, `clip-sequencer`, instead of loose files.

## [0.2.0] - 2026-11-04

First public release.

### Added

- **Sequences:** lanes per Resolume layer, with clips and rests on a 1/16-bar grid (sequence file format v3).
  Repeat once, ×N or endlessly; lane ends *Loop to end*, *Hold* or *Clear*; after the last pass the layers are cleared or held.
- **Clock:** follows an incoming MIDI clock (any input port), with tempo estimate, clock-loss freeze and resume,
  and automatic reconnection when the port comes back.
- **Resolume:** triggers over OSC exactly on the tick, confirmed over the WebSocket; reads the composition and
  thumbnails over REST. Recovers from Resolume restarts, WebSocket drops and composition reloads.
- **Starting from Resolume:** `§ <name>` clips on a `SEQ` layer start and stop the sequence of that name in the
  live deck.
- **Manual override:** clicking a clip on a layer a sequence owns yields that lane until *Resume lane*.
- **Clip resolution:** clips are found by id, then name, then file, so renames and moved clips keep working;
  unresolved steps are shown in red and can be relinked.
- **Editor:** drag clips from the browser, resize (1/8, Shift for 1/16), reorder, Alt-drag to duplicate, rests,
  undo/redo, keyboard shortcuts; edits to a running sequence apply at its next start.
- **Decks:** sequences per deck; starting one of another deck from the app asks, stops what runs and switches
  the deck first. Clips of inactive decks are cached, so their sequences stay editable; sequences of decks that
  no longer exist are kept and can be moved.
- **Thumbnails:** cached clip thumbnails; a mosaic per sequence that can be pushed to its `§` clip.
- **Desktop app** (`seq-app.exe`, Tauri): one window with a splash screen that shows the startup steps, a
  Console tab with the live log, native menus, remembered window size, a confirmation before closing while a
  sequence runs, and single-instance detection.
- **Headless mode** (`seq-app-cli.exe`): console output, the UI in a browser with its own menu bar.
- **About, Licenses and Keyboard shortcuts** dialogs; version, build number and commit in `--version`,
  `/api/version`, the log and the About dialog.
- **FFGL plugin** "1000A Sequence": shows a sequence's timeline on its `§` clip in Resolume. It ships in the
  zip (static C++ runtime, no Visual C++ Redistributable needed) and is installed, updated or removed from the
  app (**File → Install FFGL plugin…**) or with `seq-app-cli --install-ffgl`.
- The UI server only answers on `127.0.0.1` and refuses requests from other web pages.
- Logs in daily files (kept 7 days); settings and sequences in `%APPDATA%\clip-sequencer`.
- **Slot lanes** (positional sequencing): a lane in *Slot* mode plays whatever clip sits in a column of its
  layer when each step starts, so clips can be swapped in Resolume while the sequence runs. Empty slots act as
  rests. Lanes switch between *Clip* and *Slot* in the lane header (steps are converted). The slot is read
  over REST just before its step, because drags in Resolume's own UI are not pushed.
- Column numbers on the clip browser thumbnails.

### Changed

- Sequence file format **v3** (lane `mode`, step `column`). Files from earlier builds (v1, v2) are migrated on
  load; the original is kept as `.v1.bak` / `.v2.bak` on the first save. Builds before 0.2.0 cannot read v3.
- The data folder is now `%APPDATA%\clip-sequencer`; the old `%APPDATA%\resolume-sequencer` of earlier builds is
  moved there on first start.

[Unreleased]: https://github.com/ghostrider/clip-sequencer/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/ghostrider/clip-sequencer/releases/tag/v0.2.0
