# Setting up the clock

The sequencer moves its sequences forward on a clock of 24 pulses per beat. It takes that clock from one of two
sources:

- **MIDI clock** (the default): it counts the MIDI clock on a MIDI input. Use this when your show already has a
  clock (a DAW, a clock tool, hardware).
- **Resolume tempo**: no MIDI needed. The sequencer makes its own clock at Resolume's BPM and follows its Play,
  Pause, Stop and Resync. See [Following Resolume's tempo](#following-resolumes-tempo).

Without a clock, nothing advances.

Choose the source under **File → Preferences… → Clock** (or click the clock chip on the top bar, or press Ctrl+,),
then **Save**. It switches at once and is remembered. For one run, `--clock resolume` or `--clock midi` wins over
the saved choice ([Settings](settings.md)).

## MIDI clock

Anything that sends MIDI clock to a MIDI input on the Resolume computer works:

- a DAW or another app, through a **virtual MIDI port** (for example loopMIDI);
- a hardware clock or MIDI interface;
- a tool that derives the clock from the music (BPM detection) or shifts it.

On Windows the sequencer can't create a virtual port itself: it connects to an existing input by name. Start the
clock source (or create the virtual port) before choosing it.

Resolume and the sequencer must both receive **the same MIDI clock**, so Resolume's tempo and the sequencer's
steps agree.

#### Projector latency

If you delay the clock to make up for projector or LED processor latency, give the sequencer the **same
shifted clock** as Resolume. The sequencer never compensates latency itself; it triggers on the clock it gets.

### Choosing the MIDI input

1. Open **File → Preferences…** (or click the **CLOCK** chip on the top bar, or press Ctrl+,).
2. Under **Clock source**, choose **MIDI clock**.
3. Pick the input under *MIDI clock input*. **Refresh MIDI inputs** lists the ports again if you just started the
   source.
4. Click **Save**.

The sequencer switches at once and remembers the input for the next start. *No MIDI clock* switches the input off.

For one run you can also start the app with `--midi-port "<name>"`. A part of the name is enough
(`--midi-port loopMIDI`). It wins over the saved choice for that run ([Settings](settings.md)).

### Checking it

The **CLOCK** chip on the top bar shows the state:

| Chip | Meaning |
|---|---|
| `CLOCK 128.0 BPM · locked` (green) | Ticks arrive; the BPM is measured from them. |
| `CLOCK · no clock` (red) | The input is open, but no ticks arrive. Start the clock source. |
| `CLOCK · no port` (red) | No input selected, or the port isn't there (yet). The sequencer keeps trying. |

From the console, `seq-app-cli.exe --clock-debug --midi-port "<name>"` shows the incoming clock (bar.beat.tick and
BPM), and `seq-app-cli.exe --list-midi-ports` lists the inputs.

## Following Resolume's tempo

With **Resolume tempo** as the source, the sequencer needs no MIDI at all. It reads Resolume's BPM and transport
over the connection it already has, and makes its own clock at that tempo.

Resolume tells the sequencer its tempo, but **not where its beat is**. So the sequencer's beat runs at the right
speed, but at first it can sit up to half a beat off Resolume's. To line the two up, press **Resync** in
Resolume's tempo controls once: that moment becomes beat 1 for both.

| In Resolume | The sequencer |
|---|---|
| Change the BPM (type, drag, tap) | follows at once. |
| **Resync** | lines up with Resolume's beat, and **every playing sequence jumps back to bar 1**. |
| **Pause** | freezes running sequences where they are. |
| **Play** after Pause | continues from there. |
| **Stop** | freezes running sequences. |
| **Play** after Stop | counts as a Resync: lines up, playing sequences start again at bar 1. |

The clock chip shows:

| Chip | Meaning |
|---|---|
| `RESOLUME 128.0 BPM · synced` (green) | Running and lined up with Resolume's beat. |
| `RESOLUME 128.0 BPM · press Resync` (green) | Running at Resolume's tempo, not lined up yet. Press Resync in Resolume. |
| `RESOLUME 128.0 BPM · paused` / `· stopped` (red) | Resolume's transport is paused or stopped; sequences wait. |
| `RESOLUME TEMPO · not connected` (red) | No connection to Resolume; nothing advances. After it reconnects, press Resync again. |

A few things to know:

- A Resync restarts *every* playing sequence, also one you started a moment ago. Press it on a downbeat of the
  music, or before you start sequences.
- Tapping the tempo changes the speed but doesn't line anything up. Press Resync after tapping.
- The MIDI input you chose stays saved, so you can switch back to MIDI clock at any time.

## What the sequencer does with the clock

For both sources:

- Steps start **exactly on the tick** where they fall: a sixteenth note is 6 ticks, a bar 96 ticks (4/4 only).
- A sequence starts **at once** when you start it; it isn't quantized to the next bar. Start it on the beat if it
  should line up with the music.
- MIDI clock: **Start, Stop and Continue** messages are ignored: a clock source that sends Start (for example on a
  resync) doesn't restart running sequences. Only Resolume's own **Resync** does that, with the Resolume tempo
  source.
- If the clock **stops** for more than two beats, running sequences freeze where they are (no triggers) and the
  chip shows "no clock". When the clock comes back, they continue from that point.
- MIDI clock: if the port disappears (the source app was restarted), the sequencer reconnects by itself.
