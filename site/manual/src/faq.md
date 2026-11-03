# FAQ

**Does the sequencer send MIDI to Resolume?**
No. It only *receives* the MIDI clock. Clips are triggered over OSC, and Resolume is watched over its webserver.

**Can a sequence start on the next bar?**
Not yet: a sequence starts on the next clock tick after you start it. Start it on the beat.

**Which time signatures work?**
4/4 only.

**Can one sequence use clips from two decks?**
No. A sequence belongs to one deck, like the clips. Use one sequence per deck.

**Can two lanes of one sequence play on the same layer?**
No. Each lane of a sequence has its own layer.

**Does it work with Resolume Avenue?**
It is built and tested with Resolume Arena. Avenue has the same webserver and OSC, but it isn't tested.

**Does it work on macOS?**
No, the app is for Windows only.

**Can I open the UI on a tablet or another computer?**
No. The UI only listens on the Resolume computer itself, on purpose ([Settings](settings.md)).

**I renamed a clip in Resolume. Do I have to fix my sequences?**
No. Steps find clips by ID first, then name, then file name ([Files and data](files.md)).

**I renamed a deck. Where did its sequences go?**
They are listed under the old name as orphaned; right-click → **Move to deck** ([Working with decks](decks.md)).

**Why is a layer shown as "unnamed, rename in Resolume to use it"?**
Resolume reports all unnamed layers with the same name, so the sequencer can't tell them apart. Give the layer a
name in Resolume.

**Is it free?**
The source code is free under the MIT License; ready-made downloads are on the
[releases page](https://github.com/ghostrider/clip-sequencer/releases).
