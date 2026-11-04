# Slot lanes

A lane plays in one of two modes, chosen in its head with **Clip | Slot**:

- **Clip** (the default): each step plays a particular clip. If you move or rename the clip in Resolume, the step
  follows it ([Files and data](files.md) explains how clips are found).
- **Slot**: each step plays a **column** of the lane's layer, whatever clip sits there when the step starts. The
  sequence keeps its rhythm, and you change the content live by dragging other clips or media into those slots in
  Resolume, like a drum pattern whose sounds you swap on the pads.

## Making a slot lane

1. Click **Slot** in the lane head. The steps are converted (and **Ctrl+Z** undoes it): each clip step becomes the
   column its clip is in now; a clip that can't be found becomes a rest.
2. Drag clips from the browser onto the lane as usual. A slot lane takes the clip's **column**, not the clip. The
   clip browser shows each thumbnail's column number in its top-left corner.

Switching back to **Clip** turns each step into the clip that is in its slot now; an empty slot becomes a rest.

## Slot steps

A slot step is labelled `col N` and shows the thumbnail of the clip that is in that slot **now**. It updates when
you change the slot in Resolume.

In the step inspector you can:

- change the **column** with − / + (from 1 to the number of columns in the deck);
- use **Pick column from clip…**, then click a clip in the browser to take its column;
- use **Make rest**.

## How it plays

- The column is a plain number: "column 3" is always the third column of the deck. If you insert or delete
  columns in Resolume, a different clip plays.
- The slot is always on the lane's own layer.
- What plays is decided when the step starts. Just before that, the sequencer asks Resolume what is in the slot,
  because Resolume doesn't report drags in its own interface.
- An **empty slot**, or a column past the last one, plays as a **rest**: it clears the layer. Slot steps are never
  "unresolved".
- If you drop new media onto the slot that is **playing**, or drag the playing clip to another slot, Resolume
  stops it and the layer goes dark. The lane keeps its timing; the new content plays when the next step for that
  slot starts.
- Clicking a clip on the lane's layer in Resolume yields the lane, as with clip lanes
  ([Playing live](playing-live.md)).

Slot lanes need sequence file format 3, which version 0.2.0 introduced ([Files and data](files.md)).
