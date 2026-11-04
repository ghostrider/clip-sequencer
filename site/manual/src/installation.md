# Installation

The sequencer is a portable app: there is no installer. You unzip it and run it.

## Download

1. Open the [latest release](https://github.com/ghostrider/clip-sequencer/releases/latest) on GitHub.
2. Download `clip-sequencer-<version>-windows-x64.zip`.
3. Optional, but recommended: check the download against `SHA256SUMS.txt` from the same release. In PowerShell:

   ```powershell
   Get-FileHash .\clip-sequencer-<version>-windows-x64.zip -Algorithm SHA256
   ```

   The hash must match the line for the zip in `SHA256SUMS.txt`.
4. Unzip it into a folder of your choice, for example `C:\Tools`. You get a folder `clip-sequencer`.

The `clip-sequencer` folder contains:

| File | What it is |
|---|---|
| `seq-app.exe` | The desktop app: one window with the sequencer. This is the one you normally start. |
| `seq-app-cli.exe` | The console version: runs without a window, the UI opens in a web browser. Also has a few check commands ([Settings](settings.md)). |
| `ffgl\SeqView.dll`, `ffgl\README.txt` | The optional FFGL plugin for Resolume ([Installing the FFGL plugin](ffgl-plugin.md)). |
| `README.md`, `CHANGELOG.md` | Short overview and the list of changes per version. |
| `LICENSE`, `THIRD_PARTY_LICENSES.md`, `OFL.txt` | Licenses (also in the app under **Help → Licenses**). |

### If the browser blocks the download

The zip contains programs that aren't code-signed, so browsers may block it at first. Chrome says **"Dangerous
download blocked"** (in German: "Gefährlicher Download blockiert"). To keep it: open the downloads list
(`chrome://downloads`), find the file and choose the option to keep or download it anyway (the exact wording depends
on the Chrome version; in newer versions it's behind the ⋮ menu of the download). Edge and Firefox show similar
warnings.

Check the file before you run anything: the hash from step 3 must match `SHA256SUMS.txt` of the release. Then you
know it is the file GitHub built from the published source.

## First start

Double-click **`seq-app.exe`**.

- **Windows SmartScreen** may say "Windows protected your PC", because the app is not code-signed. Click
  **More info**, then **Run anyway**. Windows remembers this for that file.
- A **splash screen** shows the startup steps: loading settings, starting the UI, connecting to Resolume and the
  MIDI clock, loading the composition and its thumbnails. If Resolume isn't running or no MIDI port is set yet, you get a warning and
  the app opens anyway; it keeps trying in the background.
- The first time, choose the MIDI clock input under **File → Preferences…** ([Setting up the clock](clock.md)).

### WebView2

The desktop app draws its window with the **Microsoft Edge WebView2 Runtime**. It is part of Windows 11. On
Windows 10 it is usually installed already (with Edge or Office). If it is missing, the app says so and offers to
open Microsoft's [download page](https://developer.microsoft.com/microsoft-edge/webview2/): install the
"Evergreen" runtime and start the app again. `seq-app-cli.exe` doesn't need WebView2.

## Updating

1. Quit the sequencer (**File → Quit** or Ctrl+Q).
2. Unzip the new version, into a new place or over the old `clip-sequencer` folder.
3. Start the new `seq-app.exe`.

Your sequences, settings and caches are not in the app folder but in the data folder
(`%APPDATA%\clip-sequencer`, see [Files and data](files.md)), so they carry over. Read the
[changelog](https://github.com/ghostrider/clip-sequencer/blob/main/CHANGELOG.md) for what changed.

## Uninstalling

Delete the app folder. To remove your sequences and settings too, delete `%APPDATA%\clip-sequencer` as well;
make a copy of your `.seq.json` files first if you might need them again.
