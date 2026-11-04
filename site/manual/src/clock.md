# Setting up the clock

The sequencer doesn't keep time by itself. It counts an incoming **MIDI clock** (24 pulses per beat) and moves
its sequences forward with it. Without a clock, nothing advances.

## Which clock source

Anything that sends MIDI clock to a MIDI input on the Resolume computer works:

- a DAW or another app, through a **virtual MIDI port** (for example loopMIDI);
- a hardware clock or MIDI interface;
- a tool that derives the clock from the music (BPM detection) or shifts it.

On Windows the sequencer can't create a virtual port itself: it connects to an existing input by name. Start the
clock source (or create the virtual port) before choosing it.

Resolume and the sequencer must both receive **the same MIDI clock**, so Resolume's tempo and the sequencer's
steps agree.

### Projector latency

If you delay the clock to make up for projector or LED processor latency, give the sequencer the **same
shifted clock** as Resolume. The sequencer never compensates latency itself; it triggers on the clock it gets.

## Choosing the MIDI input

1. Open **File → Preferences…** (or click the **CLOCK** chip on the top bar, or press Ctrl+,).
2. Pick the input under *MIDI clock input*. **Refresh** lists the ports again if you just started the source.
3. Click **Save**.

The sequencer switches at once and remembers the input for the next start. *No MIDI clock* switches the input off.

For one run you can also start the app with `--midi-port "<name>"`. A part of the name is enough
(`--midi-port loopMIDI`). It wins over the saved choice for that run ([Settings](settings.md)).

## Checking it

The **CLOCK** chip on the top bar shows the state:

| Chip | Meaning |
|---|---|
| `CLOCK 128.0 BPM · locked` (green) | Ticks arrive; the BPM is measured from them. |
| `CLOCK · no clock` (red) | The input is open, but no ticks arrive. Start the clock source. |
| `CLOCK · no port` (red) | No input selected, or the port isn't there (yet). The sequencer keeps trying. |

From the console, `seq-app-cli.exe --clock-debug --midi-port "<name>"` shows the incoming clock (bar.beat.tick and
BPM), and `seq-app-cli.exe --list-midi-ports` lists the inputs.

## What the sequencer does with the clock

- Steps start **exactly on the tick** where they fall: a sixteenth note is 6 ticks, a bar 96 ticks (4/4 only).
- A sequence starts **at once** when you start it; it isn't quantized to the next bar. Start it on the beat if it
  should line up with the music.
- **Start, Stop and Continue** messages are ignored: a clock source that sends Start (for example on a resync)
  doesn't restart running sequences.
- If the clock **stops** for more than two beats, running sequences freeze where they are (no triggers) and the
  chip shows "no clock". When the clock comes back, they continue from that point.
- If the port disappears (the source app was restarted), the sequencer reconnects by itself.
