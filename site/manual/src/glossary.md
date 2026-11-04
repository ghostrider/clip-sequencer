# Glossary

**After last pass**
: What happens when a sequence ends: *clear* its layers or *hold* the last clips.

**Deck**
: A Resolume deck. Every sequence belongs to one deck and uses only its clips.

**End behaviour**
: What a lane does when it is shorter than the sequence: *Loop to end*, *Hold* or *Clear*.

**Lane**
: One row of a sequence: steps on one Resolume layer.

**LIVE**
: The deck Resolume is playing now; marked on the deck tabs.

**Orphaned**
: Sequences of a deck name that no longer exists in Resolume.

**Pass**
: One play-through of a sequence, as long as its longest lane. Repeat sets how many.

**Placeholder, SEQ clip, `§` clip**
: A clip on the SEQ layer named `§ <sequence name>`; clicking it in Resolume starts that sequence.

**Rest**
: A step without a clip. It clears the lane's layer at its start.

**Sequence**
: Clips on one or more layers, played together on a 1/16-bar grid, locked to the MIDI clock.

**SEQ layer**
: The Resolume layer (named `SEQ`) that holds the placeholder clips.

**Step**
: One block in a lane: a clip (or a rest) and a length in sixteenths of a bar.

**Tick**
: One MIDI clock pulse: 24 per beat, 6 per sixteenth note, 96 per bar.

**Unresolved**
: A step whose clip, or a lane whose layer, can't be found; it stays silent.

**Yielded**
: A lane that stopped triggering because its layer was taken over by hand; *Resume lane* hands it back.
