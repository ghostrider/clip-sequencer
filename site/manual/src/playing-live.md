# Playing live

## Before the show

1. Start Resolume with your composition, then your MIDI clock source (if you use one), then the sequencer.
2. Check the status bar at the bottom: **CLOCK** shows the BPM and "locked" (with Resolume's tempo: **RESOLUME** and the BPM;
   press **Resync** in Resolume until it says "synced"); **REST**, **WS** and **OSC** are green.
3. Make every deck you'll use live once, so its clips are cached ([Working with decks](decks.md)).
4. Look for red: steps or lanes in red can't find their clip or layer and will stay silent
   ([Troubleshooting](troubleshooting.md)).
5. Make a copy of your sequence file ([Files and data](files.md)).

## Starting and stopping

| From | Start | Stop |
|---|---|---|
| The app | ▶ on the card or in the editor, or **Space** | ■, **Space** again, or **Stop all** |
| The app, next up | **Shift**+▶ or **Shift+Space** (a cue, see below) | × on the cue, or **Stop all** |
| Resolume | click the `§ <name>` clip on the SEQ layer | click another SEQ clip, or clear the SEQ layer |

- **When it starts** is the sequence's **start mode**, in the editor header: **now** (on the next clock tick), on
  the next **beat**, on the next **bar**, or on the next bar that is a multiple of **2, 4, 8 or 16** bars. While it
  waits, the card and the header say `starts in 3 beats`; press ▶ again to start it at once, or **Cancel**.
- Bars are counted from the last **MIDI Start** of your clock (or the last **Resync** in Resolume when the app
  follows Resolume's tempo). Neither restarts a sequence that is playing. Until one arrives, bars count from the
  first clock tick and the clock chip says `bar phase not synced`.
- When it stops (or after its last pass), its layers are cleared, or keep their last clips if it is set to *hold*.
- Starting a sequence that is already running starts it again from the beginning (at its start mode). If it was
  started from its `§` clip, it stays linked to it: clearing the SEQ layer still stops it.
- Clicking a sequence's `§` clip while it runs (started from the app) doesn't restart it: from then on it is linked
  to that clip, so clearing the SEQ layer stops it, and when it finishes the SEQ layer is cleared for the next click.
  The same goes for a start that is still waiting for its beat or bar.
- **Stop all** stops everything and clears every layer the sequencer used, also lanes you took over by hand.

## Cueing the next sequence

A **cue** says "play this next": **Shift**+click ▶ (or **Shift+Space**) and the sequence starts at its start mode;
at that moment everything else that plays stops. Right-click a card and choose **Cue at…** for other points: the
**end of the current pass** of a playing sequence, or the next **8** or **16** bars. There is one cue at a time (a
new one replaces it); the card shows `CUED · in 2 bars`. Click × on it to cancel, or ▶ to start it now. Only
sequences of the live deck can be cued.

## Muting and soloing lanes

While a sequence runs, each lane has **M** (mute) and **S** (solo). A muted lane's layer goes dark at once; the lane
keeps counting and comes back in time at its next step when you unmute it. Solo mutes the other lanes of that
sequence (you can solo several). Mute and solo are not saved: the next start plays every lane.

## Two sequences at once

Sequences on different layers run side by side. If a sequence starts that needs a layer another running sequence
uses, the older sequence gives up **that lane** (the rest of it keeps running).

## Taking over by hand

Clicking a clip in Resolume on a layer a running sequence uses takes that layer over: the lane becomes
**yielded**. It stops triggering but keeps counting, so it stays in time. Click its **yielded ↺** pill in the lane
head (*Resume lane*) to hand it back: it comes back at its next step. The next start of the sequence resumes it too.

## Editing while it plays

You can edit a running sequence. The changes are saved, but the run keeps playing the version it started with;
the header shows **edited · applies at next start**. Start it again to hear the changes.

## When something goes wrong

- **The clock stops:** running sequences freeze where they are and continue when the clock comes back. Start
  messages from the clock source don't restart them.
- **With Resolume's tempo:** **Pause** or **Stop** in Resolume freezes running sequences too, and **Resync**
  (or Play after Stop) jumps every playing sequence back to bar 1 ([Setting up the clock](clock.md)).
- **Resolume quits or the connection drops:** triggers aren't confirmed (red top edge on the steps). The
  sequencer reconnects by itself when Resolume is back; then **File → Reload composition** if something looks
  stale.
- **A trigger isn't confirmed** within a quarter second: the step gets a red top edge and the log says so. The
  sequencer doesn't retry; the next step triggers normally.
- **Panic:** **Stop all**.
