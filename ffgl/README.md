# SeqView – "1000A Sequence" (FFGL source plugin)

Shows a sequence of the Resolume Clip Sequencer (`../../resolume-sequencer`, SPEC §12) on the `§` placeholder clips
of the SEQ layer, instead of a *Blank* source: sequence name, deck and length, one row per lane (layer name, lane
state, steps as blocks with clip thumbnails, rests, unresolved steps framed red), and while the sequence runs a
moving playhead, `bar.beat` and the pass counter. Plugin ID `VJSQ`, FFGL 2.1, type Source.

Starting and stopping is unchanged: triggering the `§` clip starts the sequence (the sequencer watches the clip's
`connected` state). The plugin only *shows* it.

## Setup

1. Build and install (Resolume closed: it locks loaded plugin DLLs):
   `powershell -ExecutionPolicy Bypass -File build_seqview.ps1`
2. Start Resolume and the sequencer (`seq-app`, UI port 8090).
3. On the SEQ layer, drag **1000A Sequence** (Sources) into an empty slot and name the clip `§ <sequence name>`.
   The sequencer fills in the plugin's `Sequence` parameter by itself; renaming the clip re-binds it.
4. The SEQ layer can stay at opacity 0: the plugin still renders when the clip is selected (preview monitor) or
   connected. Raise the opacity if you want the timeline on an output.

## Parameters

| Name | Type | |
|---|---|---|
| Sequence | text | Sequence name. Set automatically by the sequencer from the clip name; can be typed. |
| Server | text | Sequencer UI address, default `127.0.0.1:8090` (`seq-app --ui-port`). |
| Build | text | Build number and date (read-only). |

## How it works

- **Data:** a worker thread polls `GET http://<Server>/plugin/v1/view.txt?name=<Sequence>&rev=<layout revision>`
  every 50 ms (format: `resolume-sequencer/crates/seq-app/src/plugin_view.rs`). The layout is only resent when its
  revision changes. Clip thumbnails come from `/thumbs/clip/<id>.png?v=<version>` and are decoded (stb_image) on the
  worker. HTTP is WinHTTP (system DLL, nothing to deploy).
- **Only while rendered:** polling runs between the host's `Connect` and `Disconnect` calls, i.e. while Resolume
  renders this instance (selected or connected). Idle placeholders cost nothing. The short-lived instances Resolume
  creates every few seconds for its browser never start the worker (it starts in `InitGL`).
- **Rendering:** one small shader draws rectangles, thumbnails (cover-cropped) and text glyphs on the host's
  framebuffer; text uses Segoe UI from the Windows fonts folder (Arial as fallback), baked with stb_truetype into a
  1024x1024 atlas, Latin-1 (so `§` and umlauts work). Colours are the sequencer UI's design tokens. The playhead is
  advanced between polls at the reported tempo (at most 0.25 s ahead).
- **Threads:** see the class comment in `SeqView.h`. GL only on the render thread; the worker hands decoded pixels
  over by `Client::Take()`.

Messages instead of a timeline: "No sequence assigned" (empty `Sequence`), "Sequencer not reachable at …",
"No sequence named …", "Unexpected answer from …".

## Tests

`tests/SeqViewTests.exe` (build `tests/SeqViewTests.vcxproj`, Release x64; runs on the discrete GPU via
`NvOptimusEnablement`): parser cases; rendering with an injected view (thumbnail drawn, unresolved frame, playhead
moves by exactly 4/16 of the timeline, no playhead when stopped, messages, resize); the full network path against a
local test server (`tests/MiniHttp.*`: polling only after `Connect`, revision sent back, thumbnail fetched and drawn,
polling stops after `Disconnect`, unreachable server reported); GPU time at 1080p. PNG previews go to
`tests/test_output/`.
