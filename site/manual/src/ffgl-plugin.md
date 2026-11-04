# Installing the FFGL plugin

**1000A Sequence** is an optional Resolume source plugin that comes with the sequencer. Put it on a sequence's
`§` placeholder clip, and the clip shows that sequence's timeline in Resolume: its name, one row per lane with the
clips' thumbnails, rests and unresolved steps, and, while it plays, a moving playhead with `bar.beat` and the pass
counter.

The plugin only **shows** the sequence. Starting and stopping works exactly the same with a *Blank* placeholder
([Setting up Resolume](resolume-setup.md)).

## Installing from the app

1. **Quit Resolume.** Resolume keeps loaded plugins locked, so they can't be replaced while it runs.
2. In the sequencer, choose **File → Install FFGL plugin…**.
3. Check the **plugin folder**. The default is Resolume's own folder for extra plugins,
   `Documents\Resolume Arena\Extra Effects` (also when your Documents folder lives in OneDrive). Click
   **Change…** to use another folder, for example one you added to Resolume's plugin folders in its preferences.
4. Click **Install**. The dialog confirms where the plugin went.
5. Start Resolume. It loads plugins at startup.

The button follows what is installed:

| Button | When |
|---|---|
| **Install** | there is no plugin in the folder yet |
| **Update** | the app has a newer version than the folder |
| **Reinstall** | both are the same version |
| **Downgrade…** | the folder has a newer version than the app; click again to confirm |

**Remove** deletes the plugin from the folder again (Resolume closed). The sequencer only ever replaces or removes
its own `SeqView.dll`; a different file with that name is left alone and the dialog says so.

If something goes wrong while copying, the previous version is put back, so a half-copied plugin is never left
behind.

When the plugin isn't installed yet, the status bar at the bottom of the window says
**FFGL plugin missing · Install…**; after a new version of the sequencer it says
**FFGL plugin update available · Update…**. Click it to open the dialog. The plugin is optional: if you don't use
it, you can ignore the hint.

## Installing from the console

```powershell
seq-app-cli.exe --install-ffgl
seq-app-cli.exe --install-ffgl --ffgl-dir "D:\Resolume\Plugins"
```

This installs or updates the plugin (it never downgrades) and creates the folder if needed. Exit code 0 means
done.

## Installing by hand

The release zip contains the plugin as `clip-sequencer\ffgl\SeqView.dll`. Copy it into Resolume's plugin folder
(normally `Documents\Resolume Arena\Extra Effects`, create it if it doesn't exist) while Resolume is closed, then
start Resolume.

## Using it in Resolume

1. In Resolume's **Sources**, find **1000A Sequence**.
2. Drag it onto the `SEQ` layer, into an empty slot.
3. Name the clip `§ ` plus the sequence name, for example `§ Drop A`.

The sequencer fills in the plugin's *Sequence* parameter by itself, from the clip name; rename the clip and it
follows. The SEQ layer can stay at opacity 0: the plugin still draws in the preview when the clip is selected.
Raise the opacity if you want the timeline on an output.

The plugin talks to the sequencer on this computer, at `127.0.0.1:8090`. If the sequencer runs on another port
([Settings](settings.md)), set the plugin's *Server* parameter to match, for example `127.0.0.1:8091`. Its
*Build* parameter shows the plugin's version.

## Updating

Each version of the sequencer comes with the matching plugin. After unzipping a new version, open
**File → Install FFGL plugin…** (with Resolume closed) and click **Update**.

## Troubleshooting

- **"Quit Resolume Arena to install the plugin":** Resolume is running. Quit it, then click **Check again**.
- **"… is in use (is Resolume running?)":** the file is locked, usually by Resolume or a second Resolume
  instance. Quit it and try again.
- **The plugin doesn't show up in Resolume:** restart Resolume after installing; check that the dialog's folder is
  one Resolume loads plugins from (its default folder, or one listed in its preferences).
- **The clip shows "Sequencer not reachable at 127.0.0.1:8090":** the sequencer isn't running, or runs on another
  port: set the *Server* parameter.
- **"No sequence named …":** the clip name (after `§ `) doesn't match a sequence of the live deck.
- **"No sequence assigned":** the clip isn't named `§ <sequence>` yet.
