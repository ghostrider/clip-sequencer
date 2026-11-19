# Setting up Resolume

The sequencer talks to Resolume over its **webserver** (to read the composition and follow what happens) and its
**OSC input** (to trigger clips). Both are off by default.

## Once per computer

1. **Preferences → Webserver:** switch on *Webserver & REST API*. Keep the port at `8080` (or tell the sequencer
   another one, see [Settings](settings.md)).
2. **Preferences → OSC:** switch on *OSC Input*. Keep the port at `7000` (or tell the sequencer another one).
3. **Tempo:** Resolume and the sequencer must both receive **the same MIDI clock**, so Resolume's tempo and the
   sequencer's steps agree. See [Setting up the clock](clock.md).

The sequencer connects to `127.0.0.1`, the same computer. On the top bar of the app, the chips **REST**, **WS**
(WebSocket) and **OSC** turn green when the connection works.

## Once per composition

### Turn off Beat Snap for clip triggers

**Composition → Clip Beat Snap: None.** The sequencer sends each trigger exactly on the tick where the step
begins. With Beat Snap, Resolume would wait for the next beat or bar and the step would start late.

### Clip transport mode

Any transport mode works (for example *Timeline* or *BPM Sync*): the sequencer only triggers when a clip starts;
how the clip plays after that is up to Resolume.

### Name your layers

Lanes find their layer **by name**, so every layer you want to use needs a real, unique name, for example `BG`,
`MID`, `FX`.

Resolume *shows* unnamed layers as "Layer 1", "Layer 2" and so on, but reports them all with the same internal name.
The sequencer can't tell them apart and lists them as "Layer 3 · unnamed, rename in Resolume to use it". Rename
the layer in Resolume (double-click its name, or right-click → Rename). The sequencer picks up renames at once.

Keep layer names unique: two layers with the same name are flagged, and a lane on such a name is silent.

### The SEQ layer: starting sequences from Resolume (optional)

To start sequences by clicking a clip in Resolume (or from a MIDI controller or the dashboard, like any other
clip), each sequence needs a placeholder clip on a layer named `SEQ`.

#### The quick way: Sync SEQ

In the app, with the deck live in Resolume, click **Sync SEQ** at the top of the sequence list. The button is
highlighted while something is missing. A dialog shows what will happen; click **Sync** to do it:

- If there is no `SEQ` layer, the app adds one on top of the other layers, with opacity 0.
- Every sequence of the deck without a placeholder gets one, in the next empty slot: a clip with the
  **"1000A Sequence"** plugin if it is installed ([Installing the FFGL plugin](ffgl-plugin.md)), else the
  *Blank* source, named `§ ` plus the sequence name. If the layer is full, columns are added at the end.
- Placeholders whose sequence no longer exists (for example after renaming a sequence) are listed. Tick
  **Remove § clips without a sequence** to clear them too; a playing one is left alone.

Nothing else is changed: other clips on the SEQ layer, other layers and other decks stay as they are. Sync works on
the live deck only, because Resolume lets the app read and change only that deck. For other decks, make each one
live in turn and sync again. Running sequences keep running. The result appears in the status bar.

Save the composition in Resolume afterwards, as after any change you make there.

#### By hand

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
> [Installing the FFGL plugin](ffgl-plugin.md).

The layer name `SEQ` and the `§ ` prefix can be changed ([Settings](settings.md)).

## Decks

Sequences belong to a deck, like clips. A sequence can only use clips of its own deck. See
[Working with decks](decks.md).
