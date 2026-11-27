# Recording from Resolume

Build a pattern by playing it: arm a sequence, start it, and click clips in Resolume in time. What you click lands
on the grid, and the next pass plays it back, so you can build a pattern up pass by pass: one layer in the first
pass, the next layer in the second while the first one plays back, and so on.

![A sequence recording: REC in the header and on the card, the Mid lane played by hand](images/recording.png)

## Recording

1. Select a sequence of the **live** deck and click **⏺** in the editor header. It shows `● armed`.
2. Start it (▶ or Space). It starts as usual, by its start mode: a bar start gives you a count-in. While it records
   it shows `● REC · pass 1` and loops pass after pass, whatever its repeat setting.
3. Click clips in Resolume, or press a layer's **Clear** button for a rest, in time with the music.
4. At the end of each pass, what you played in it is written into the sequence, and the next pass plays it.
5. **Stop** (or **⏺** again) ends the recording. What you played in the last, unfinished pass is kept up to that
   point.

Each pass is **one undo step**: **Ctrl+Z** removes the last pass you recorded.

## What gets recorded

- Every click lands on the **nearest sixteenth**. A click just before the end of a pass belongs to the start of
  the next one.
- A lane you click on is **overwritten from your first click** on its layer to the end of the pass. Before that
  click it keeps its steps; lanes you don't touch keep theirs. Each recorded step lasts until your next click on
  that layer.
- To fix part of a lane, record it again from that point.
- While you play a layer, its lane stops playing its own steps (it shows `rec`) and plays your take from the next
  pass on. The other lanes play as usual.
- A click on a layer that has **no lane** in the sequence adds a lane for it. The layer needs a unique name
  ([Setting up Resolume](resolume-setup.md)); the SEQ layer is never recorded.
- **Slot lanes** record the **column** you clicked, clip lanes the clip.
- **Transitions:** each step gets the transition Resolume used for your click: the layer's, or the clip's own if
  the clip isn't *Layer Determined*. The length is turned into beats at the current tempo. Change the layer's
  transition in Resolume while you record to play fades ([Transitions](transitions.md)). A lane you play without
  fades stays a lane without transitions.

## Limits

- Clips in **Piano** trigger style record as you play them: the clip from press to release, then a rest. A
  quick tap records at least a sixteenth. **Toggle** clips record a rest when you click them off.
- Clicking the clip that is already playing does nothing in Resolume, so it can't be recorded: the step just
  goes on.
- A layer's **Clear** with a fade is reported only when the fade has ended; the sequencer places it one fade
  earlier, so it lands where you pressed it.
- Only one sequence records at a time.
