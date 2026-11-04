# Playing live

## Before the show

1. Start Resolume with your composition, then your MIDI clock source, then the sequencer.
2. Check the top bar: **CLOCK** shows the BPM and "locked"; **REST**, **WS** and **OSC** are green.
3. Make every deck you'll use live once, so its clips are cached ([Working with decks](decks.md)).
4. Look for red: steps or lanes in red can't find their clip or layer and will stay silent
   ([Troubleshooting](troubleshooting.md)).
5. Make a copy of your sequence file ([Files and data](files.md)).

## Starting and stopping

| From | Start | Stop |
|---|---|---|
| The app | ▶ on the card or in the editor, or **Space** | ■, **Space** again, or **Stop all** |
| Resolume | click the `§ <name>` clip on the SEQ layer | click another SEQ clip, or clear the SEQ layer |

- A sequence starts **at once**, on the next clock tick, not on the next bar. Start it on the beat.
- When it stops (or after its last pass), its layers are cleared, or keep their last clips if it is set to *hold*.
- Starting a sequence that is already running starts it again from the beginning.
- **Start a sequence from one place at a time.** If you start it in the app while its `§` clip in Resolume is still
  playing, the two get out of step: clearing the SEQ layer then doesn't stop it, and clicking the `§` clip does
  nothing until the layer is cleared. Stop it the same way you started it.
- **Stop all** stops everything and clears every layer the sequencer used, also lanes you took over by hand.

## Two sequences at once

Sequences on different layers run side by side. If a sequence starts that needs a layer another running sequence
uses, the older sequence gives up **that lane** (the rest of it keeps running).

## Taking over by hand

Clicking a clip in Resolume on a layer a running sequence uses takes that layer over: the lane becomes
**yielded**. It stops triggering but keeps counting, so it stays in time. Click **Resume lane** in the lane head
to hand it back: it comes back at its next step. The next start of the sequence resumes it too.

## Editing while it plays

You can edit a running sequence. The changes are saved, but the run keeps playing the version it started with;
the header shows **edited · applies at next start**. Start it again to hear the changes.

## When something goes wrong

- **The clock stops:** running sequences freeze where they are and continue when the clock comes back. Start
  messages from the clock source don't restart them.
- **Resolume quits or the connection drops:** triggers aren't confirmed (red top edge on the steps). The
  sequencer reconnects by itself when Resolume is back; then **File → Reload composition** if something looks
  stale.
- **A trigger isn't confirmed** within a quarter second: the step gets a red top edge and the log says so. The
  sequencer doesn't retry; the next step triggers normally.
- **Panic:** **Stop all**.
