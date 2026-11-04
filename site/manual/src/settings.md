# Settings

Most settings are command-line options. The MIDI clock input is also chosen in the app
(**File → Preferences…**) and remembered. There is no settings file to edit.

## How to pass options

- **Desktop app:** create a Windows shortcut to `seq-app.exe` and add the options to its *Target*, for example
  `"C:\Tools\Clip Sequencer\seq-app.exe" --osc-port 7001`. Or run it from a terminal with the options.
- **Console:** `seq-app-cli.exe --midi-port loopMIDI`.

## Options

| Option | Default | What it does |
|---|---|---|
| `--midi-port <name>` | the one saved in Preferences | MIDI clock input for this run; part of the name is enough |
| `--resolume-host <host>` | `127.0.0.1` | where Resolume runs |
| `--rest-port <port>` | `8080` | Resolume's webserver port (Preferences → Webserver) |
| `--osc-port <port>` | `7000` | Resolume's OSC input port (Preferences → OSC) |
| `--ui-port <port>` | `8090` | port of the sequencer's own UI (see below) |
| `--strict-port` | | exit with an error instead of trying other ports when the UI port is taken |
| `--seq-layer <name>` | `SEQ` | the layer that holds the placeholder clips |
| `--seq-prefix <text>` | `"§ "` | how placeholder clip names start |
| `--data-dir <folder>` | `%APPDATA%\resolume-sequencer` | where sequences, settings, caches and logs are kept |
| `--no-splash` | | desktop app: no splash screen, the window appears when ready |
| `--headless` | | no window, UI in the browser (`seq-app-cli.exe` always runs like this) |
| `--ffgl-dir <folder>` | the one chosen in the app, else `Documents\Resolume Arena\Extra Effects` | Resolume's plugin folder for the FFGL plugin |

Commands of `seq-app-cli.exe` that print something and exit:

| Command | What it does |
|---|---|
| `--list-midi-ports` | lists the MIDI inputs |
| `--clock-debug` | shows the incoming clock (bar.beat.tick and BPM) on the port given with `--midi-port` |
| `--validate <file>` | checks a `.seq.json` file (exit code 0 = valid) |
| `--install-ffgl` | installs or updates the FFGL plugin ([Installing the FFGL plugin](ffgl-plugin.md)) |
| `--version` | prints the version, build number, commit and build date |
| `--help` | lists all options |

## Desktop app and headless mode

`seq-app.exe` is the desktop app: one window, no console. `seq-app-cli.exe` runs the same sequencer without a
window: it prints the startup steps and the log to the console, and you open the UI in a browser at the address
it prints (normally `http://127.0.0.1:8090`). Ctrl+C, closing the console, or **File → Quit server…** in the
browser stops it cleanly.

## The UI port

The sequencer's UI (and the FFGL plugin) talk to it on port `8090`. If another program uses that port, the
sequencer takes the next free one from 8091 to 8099, then any free port, and says so in the log. The window and
the console always use the actual address. With `--strict-port` it exits instead.

If the port belongs to a sequencer that is already running, no second one starts: the desktop app offers to bring
the running one to the front.

The FFGL plugin expects `127.0.0.1:8090`; if the sequencer runs on another port, set the plugin's *Server*
parameter to match.

## Network and security

The UI listens on `127.0.0.1` only: it can be opened on this computer, not from other devices. It has no
password; any program on the computer can use it, like Resolume's own webserver.

## Logging

Logs go to the Console tab and to daily files in `logs\` in the data folder (kept for 7 days). For more detail,
set the environment variable `RUST_LOG=debug` before starting; then every OSC message is logged with its exact
time.
