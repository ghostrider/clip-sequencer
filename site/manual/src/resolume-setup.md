# Setting up Resolume

The sequencer talks to Resolume over its **webserver** (to read the composition and follow what happens) and its
**OSC input** (to trigger clips). Both are off by default.

## Once per computer

1. **Preferences → Webserver:** switch on *Webserver & REST API*. Keep the port at `8080` (or tell the sequencer
   another one, see [Settings](settings.md)).
2. **Preferences → OSC:** switch on *OSC Input*. Keep the port at `7000` (or tell the sequencer another one).
3. **Tempo:** let Resolume follow the same MIDI clock as the sequencer (MIDI clock sync in Resolume's MIDI
   settings), so the clips' own BPM-synced playback matches the sequencer's steps. See
   [Setting up the clock](clock.md).

The sequencer connects to `127.0.0.1`, the same computer. On the top bar of the app, the chips **REST**, **WS**
(WebSocket) and **OSC** turn green when the connection works.

## Once per composition

### Turn off Beat Snap for clip triggers

**Composition → Clip Beat Snap: None.** The sequencer sends each trigger exactly on the tick where the step
begins. With Beat Snap, Resolume would wait for the next beat or bar and the step would start late.

### Name your layers

Lanes find their layer **by name**, so every layer you want to use needs a real, unique name, for example `BG`,
`MID`, `FX`.

Resolume *shows* unnamed layers as "Layer 1", "Layer 2" and so on, but reports them all with the same internal name.
The sequencer can't tell them apart and lists them as "Layer 3 · unnamed, rename in Resolume to use it". Rename
the layer in Resolume (double-click its name, or right-click → Rename). The sequencer picks up renames at once.

Keep layer names unique: two layers with the same name are flagged, and a lane on such a name is silent.

### The SEQ layer: starting sequences from Resolume (optional)

To start sequences by clicking a clip in Resolume (or from a MIDI controller or the dashboard, like any other
clip):

1. Add a layer named **`SEQ`**.
2. For each sequence, put a clip on that layer and name it **`§ ` plus the sequence name**, for example
   `§ Drop A`. The `§` and the space are part of the name.
3. Use a clip that shows nothing: the *Blank* source, and/or set the SEQ layer's opacity to 0. The sequencer
   still sees the click.

Clicking `§ Drop A` starts the sequence "Drop A" of the deck that is live. Clicking another clip on the SEQ layer,
or clearing the layer, stops it. When a sequence started this way ends by itself, the sequencer clears the SEQ
layer, so the next click starts it again. The app shows each sequence's placeholder ("§ Drop A · col 1"), and in
red when there is none.

> **Tip:** The optional **"1000A Sequence"** FFGL source plugin can replace the *Blank* source on these clips. It
> shows the sequence's timeline (lanes, steps, playhead) right on the clip in Resolume. See
> [its README](https://github.com/ghostrider/clip-sequencer/blob/main/ffgl/README.md).

The layer name `SEQ` and the `§ ` prefix can be changed ([Settings](settings.md)).

## Decks

Sequences belong to a deck, like clips. A sequence can only use clips of its own deck. See
[Working with decks](decks.md).
