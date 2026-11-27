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
- **Play on:** a step can play only on some passes (first, all but the first, last, all but the last, or pass A of
  every B); on the others it sends nothing.
- **Choices:** up to 3 more clips (or columns, or rests) per step, picked at random or in turn each time it plays.
- **Section markers** in the editor ("Intro", "Drop"): add with M or from the ruler, drag, rename, delete. View →
  Guide lines draws a line every 8, 16 or 32 bars.
- **LUT test photos:** the 24 images of the Kodak test suite in the LUT tab's preview.
- **Transitions:** a step can fade in over some beats, optionally with a blend mode; a lane can have a default.
  The app sets the layer's transition in Resolume just before the step and gives the layer its own back when the
  sequence lets go of it (also after a crash, at the next start). The editor draws the fade on each step and
  warns about fades over Resolume's 10 s, unknown blend modes and clips with their own transition.
- **Record from Resolume:** ⏺ arms a sequence; started, it loops and records the clips you click (and layers you
  clear) in Resolume, snapped to sixteenths and punched in up to the next step (the rest stays). They show at once,
  each pass is one undo step, and the next pass plays them, with the transition Resolume used for each click.
  Clicks on layers without a lane add one; a sequence without steps records over a pattern length chosen when
  arming. A Piano clip held down overwrites what lies under it until you let go, and a clip still playing at the
  loop goes on into the next pass, tied. Quantize (1/16, 1/8, 1/4) and a latency offset in Preferences → Recording;
  per lane ⏺, a switch for new lanes, and punch in or replace.
- **Ties:** a step can be tied to the step before (⌒ in the step inspector): if that clip still plays, it goes on
  instead of starting again, also across the loop (sequence file format v4, field `tie`).
- **Resizing steps:** the left edge of a step can be dragged too (it moves the boundary with the step before), and
  while an edge is dragged a marker through the grid shows where it is and the new length.
- **Updates from the app:** once at each start the app asks GitHub whether a newer version exists and offers it
  (Download and install, Skip this version, Later); Help → Check for updates… any time. Installing replaces the
  program files and restarts; if the new version doesn't start, the old one comes back. Turn the check at start-up
  off in Preferences → Updates. `seq-app-cli --check-update` prints the result.

### Changed

- **New logo**: in the top bar, on the splash screen, as the app and window icon, in the About dialog and on the
  website, replacing the "§ SEQ" mark. The Unbounded font (used only by the old mark) is no longer bundled.
- **Lane inspector:** a lane's mode, end, default transition, move and delete are in the right panel (click the
  lane head). The lane head keeps the layer, M/S while running, a one-line summary and the state; the yielded
  pill itself resumes the lane.
- **Starting a running sequence** works from either side: ▶ in the app restarts it and keeps its link to the `§`
  clip, and its `§` clip clicked while it runs takes it over without a restart (no more "start it from one place
  at a time").
- The LUT preview shows the original and the result **side by side**, each fitted to its panel, instead of one
  picture with a split to drag. It uses the whole width of the tab and is as high as the image needs.

- **Log debug messages** (Preferences → Log): every OSC message sent to Resolume and each trigger's confirmation
  go to the Console tab and the log file, with their times. Off by default; applies at once. They are no longer
  shown in the status bar.

### Fixed

- The **OSC** chip turns red when Resolume ignores the triggers (for example with its OSC input off): before, it
  stayed green because sending over OSC succeeds whether or not anyone listens.
- Step badges (Play on, choices) sit next to the length and no longer cover the clip name.
- A disabled main button (such as **Download and install** when the app can't update itself) looks disabled.
- The splash screen keeps its own size and stays centred instead of reopening at the main window's last size.
- Picking a clip for a choice or a relink no longer shifts the clip browser down.
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
