# Thumbnails

## Clip thumbnails

The clip browser and the steps show Resolume's own clip thumbnails. The sequencer fetches them from Resolume and
keeps a copy in the data folder (`thumbs\`), so decks that aren't live still show the thumbnails from the last
time they were. A clip without a thumbnail, or of a deck never seen live, shows a plain placeholder.

When a clip's thumbnail changes in Resolume, the sequencer picks up the new one by itself.

## Sequence mosaics

Each sequence gets a **mosaic**: its clips in miniature, one row per lane, in order. You see it on the sequence's
card.

## Push thumbnail to Resolume

**Push thumbnail to Resolume** (in the editor's header) sets the mosaic as the thumbnail of the sequence's `§`
placeholder clip, so you can recognise sequences on the SEQ layer at a glance. The clip itself doesn't change.

The button works for sequences of the live deck that have a placeholder; push again after changing the sequence.
