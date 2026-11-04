# Files and data

## The sequence file

Sequences are saved per composition, in one file named after it: `<Composition>.seq.json`, in the data folder.
For the composition "Main Show" that is `%APPDATA%\clip-sequencer\Main Show.seq.json`. The app can't save it
next to the `.avc` file, because Resolume doesn't tell it where that is.

- Every change is saved automatically about a second later (**File → Save now** saves at once).
- Writes are safe: the file is written to a temporary file first and then swapped in, so a crash can't leave half
  a file.
- Sequences are grouped by deck name inside the file.

### Backups

The app doesn't keep old versions. Copy the `.seq.json` file before a show, and after a session of changes you
want to keep. To go back, quit the app, put the copy back, start the app.

### Files from older versions

Version 0.2.0 uses file format 3 (1/16-bar grid, [slot lanes](slot-lanes.md)). Files from earlier test builds are
converted when loaded, with the same timing: format 1 (eighths) and format 2 (no slot lanes). The first save keeps
the old file as `<file>.v1.bak` or `<file>.v2.bak`. Older builds can't open format 3 files.

Earlier test builds kept the data folder in `%APPDATA%\resolume-sequencer`. The first start of 0.2.0 moves it to
`%APPDATA%\clip-sequencer` (unless that already exists). If the move fails, for example because an old build is
still running, the app keeps using the old folder and logs a warning.

## Moving a show to another computer

1. Quit the sequencer on both computers.
2. Copy `<Composition>.seq.json` into the data folder on the other computer. Optionally also the `decks` folder
   (cached clips of inactive decks) and `thumbs` (cached thumbnails); without them, make each deck live once.
3. Start the sequencer with the same composition open in Resolume.

The composition must have the same **name** on the other computer, and the same **deck** and **layer names**.

## How clips are found again

A step remembers its clip in three ways and tries them in this order:

1. **Clip ID**, the number Resolume gives each clip slot. Fast, and it follows a clip when you drag it to
   another column.
2. **Name**, on the lane's layer first, then anywhere in the deck.
3. **Media file** name (case doesn't matter), in case you renamed the clip.

If the ID still points at a clip slot but both the name and the file no longer match, the ID is ignored, so a
slot that now holds something else isn't used by mistake. When a step is found by name or file, its ID is
updated.

If none of the three matches, the step is **unresolved**: red in the editor, and silent while the sequence plays
(the previous clip keeps playing through it). The step inspector shows which of the three still match; use
**Relink to clip…** to point it at the right clip.

If a clip is found on **another layer** than the lane's, it plays on its own layer, and the step shows ⚠.

## The data folder

`%APPDATA%\clip-sequencer` (change it with `--data-dir`, [Settings](settings.md)). The desktop app opens it
with **File → Open data folder**.

| In it | What |
|---|---|
| `<Composition>.seq.json` | your sequences, per composition |
| `<Composition>.seq.json.v1.bak`, `.v2.bak` | the original of a converted format-1 or format-2 file |
| `settings.json` | the chosen MIDI input and FFGL plugin folder |
| `logs\` | daily log files, kept for 7 days |
| `thumbs\` | cached clip thumbnails |
| `decks\` | cached clips of each deck, for decks that aren't live |
| `runtime.json` | only while the app runs: its process and UI address |

Everything except the `.seq.json` files can be deleted while the app is closed; it is rebuilt.
