# Troubleshooting

Start with the chips on the top bar and the **Console** tab: most problems show up there.

## CLOCK is red

- **"no port":** no MIDI input selected, or it isn't there. Choose it under **File → Preferences…**. If the list is
  empty, start the clock source or create the virtual port first, then **Refresh**.
- **"no clock":** the input is open but no clock arrives. Check that the source is running and sends MIDI clock
  (not only notes) to that port. `seq-app-cli.exe --clock-debug --midi-port "<name>"` shows what arrives.
- Sequences don't move without a clock: they wait at their position and continue when it arrives.

## REST, WS or OSC is red

- Resolume isn't running, or its webserver or OSC input is off: see [Setting up Resolume](resolume-setup.md).
- The ports differ: Resolume's webserver port must match `--rest-port` (8080), its OSC input `--osc-port` (7000).
- The sequencer retries by itself; the chips turn green once Resolume answers.

## Clips don't play, or play late

- **Steps are red ("unresolved"):** the clip isn't found in the deck (deleted, renamed and its file changed, or the
  deck was never live). Select the step: the inspector shows which of ID / name / file match. **Relink to clip…**
  fixes it. See [Files and data](files.md).
- **A lane head is red:** its layer name doesn't exist in the deck, is unnamed ("Layer #"), or is used by two
  layers. Name the layer in Resolume.
- **Steps get a red top edge ("not confirmed"):** Resolume didn't report the clip as playing within a quarter
  second. Usually Resolume was busy or the connection dropped; check the WS chip and the log.
- **Clips start late or on the next beat:** Beat Snap is on. Set **Composition → Clip Beat Snap: None**.
- **A lane stopped triggering:** it's **yielded** because a clip was clicked on its layer by hand. Click
  **Resume lane**.

## Starting from Resolume doesn't work

- The card says "no SEQ clip in deck": the placeholder's name must be `§ ` plus the sequence name exactly (with the
  space), on the layer named `SEQ`, in the same deck.
- The `§` clip starts the sequence of that name **in the live deck** only.
- You started it from the app while its `§` clip was still playing: see "one place at a time" in
  [Playing live](playing-live.md). Clear the SEQ layer, then start it again.

## "Port in use" or "already running"

- **The app is already running:** a second start doesn't open a second sequencer; it offers to bring the running
  one to the front. Look for it in the taskbar.
- **Another program uses port 8090:** the sequencer moves to the next free port by itself and logs it.
  With `--strict-port` it stops with an error instead; choose another `--ui-port`.

## Windows warnings

- **"Windows protected your PC" (SmartScreen):** the app isn't code-signed. **More info → Run anyway**.
- **"WebView2 Runtime not found":** install the Evergreen WebView2 Runtime from Microsoft, see
  [Installation](installation.md), or use `seq-app-cli.exe`.

## Reporting a bug

[Open an issue](https://github.com/ghostrider/clip-sequencer/issues/new/choose) and include:

- the version from **Help → About** (version, build and commit);
- your Windows and Resolume versions, and what sends the MIDI clock;
- what you did, what you expected, what happened;
- the log around the problem: **Console** tab → **Copy visible** or **Save log…**, or the files in
  `%APPDATA%\resolume-sequencer\logs`. Remove clip names or paths you don't want to share.

Security problems: please report them privately, see
[SECURITY.md](https://github.com/ghostrider/clip-sequencer/blob/main/SECURITY.md).
