# Changelog

All notable changes to Clip Sequencer for Resolume are listed here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Changes that are merged but not released yet go under
**Unreleased**; a release moves them into its own section (see `docs/RELEASING.md`).

## [Unreleased]

### Added

- **Start mode** per sequence (sequence file format **v4**): start now, on the next beat, on the next bar, or on the
  next 2, 4, 8 or 16 bars. Bars count from the clock's MIDI Start (or Resolume's Resync); the card and the editor
  show `starts in …` while a start waits.
- **Cue:** Shift+▶ or Shift+Space cues a sequence to play next; at its cue point (its start mode, the end of a
  playing sequence's pass, or the next 8 or 16 bars) it starts and everything else stops.
- **Mute and solo** per lane of a running sequence: a muted lane goes dark, keeps counting and comes back in time.

### Changed

- Sequence files are saved in format **v4**. Files of earlier builds are read as before; the first save keeps the
  original as `.v3.bak` (or `.v1.bak`, `.v2.bak`). 0.4.x cannot read v4 files.

## [0.4.0] - 2026-11-22

### Added

- **LUTs tab** (Ctrl+2): a library of colour LUTs with exposure, contrast, saturation, temperature, tint,
  lift/gamma/gain per channel and RGB curves, a before/after preview on test images or your clips, and `.cube`
  import and export. Resolume's own **LUT** effect applies them: **Use <LUT>** points an effect at a LUT's file,
  and Resolume reloads it on every change.
- **Resizable side panels** in the Sequencer tab: drag the edge next to the sequence list or the clip browser;
  double-click resets it. The widths are remembered.

### Changed

- The desktop app uses the app's own menu bar instead of the Windows menu, so it follows the theme. File gets
  **Open data folder**; View lists the three tabs.
- The connection indicators (clock, REST, WS, OSC) moved to the right end of the status bar and are smaller;
  the top bar is more compact. **Stop all** stays in the top bar.
- Tab order Sequencer, LUTs, Console: **Ctrl+2** opens LUTs, **Ctrl+3** the Console (was Ctrl+2).

## [0.3.0] - 2026-11-19

### Added

- **Sync SEQ:** a button above the sequence list creates the `SEQ` layer in Resolume if needed and adds a
  `§ <name>` clip for every sequence of the live deck, with the "1000A Sequence" plugin (or *Blank* without it).
  Optionally clears `§` clips whose sequence no longer exists.
- **Themes:** Preferences → Appearance with five built-in themes (new default *Rack*) and an editor for your own
  colours and fonts; all fonts are bundled.
- **Resolume tempo as clock:** no MIDI clock needed. Preferences → Clock (or `--clock resolume`) makes the app
  follow Resolume's BPM, Play, Pause and Stop; **Resync** in Resolume lines it up with Resolume's beat and jumps
  playing sequences back to bar 1.

### Changed

- The release zip holds a single folder, `clip-sequencer`, instead of loose files.

### Fixed

- Dialogs opened from the sequence list (Sync SEQ, delete) were drawn behind the thumbnails of a running sequence.

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

[Unreleased]: https://github.com/ghostrider/clip-sequencer/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/ghostrider/clip-sequencer/releases/tag/v0.4.0
[0.3.0]: https://github.com/ghostrider/clip-sequencer/releases/tag/v0.3.0
[0.2.0]: https://github.com/ghostrider/clip-sequencer/releases/tag/v0.2.0
