# Resolume API samples

Captured 2026-10-25 from Arena 7.28.1 with `tools/resolume-probe` (composition `dev`).
Findings: `docs/resolume-api-notes.md`. These files double as fixtures for M2 (composition
index) and M5 (mock Resolume).

| File | What |
|---|---|
| `product.json` | `GET /api/v1/product` |
| `composition.json` | `GET /api/v1/composition`, deck `2132323` live, settled, nothing playing |
| `deck1-composition.json` | WS composition push after switching to deck `1000A Text3D` (final state; 14 clips, duplicate names) |
| `composition-clip-connected.json` | WS composition push ~55 ms after an OSC connect: layer 2 clip 1 `Connected`, `active_clip` set |
| `ws-sources_update.json`, `ws-effects_update.json` | the two typed messages sent on WS connect |
| `thumbnail-clip.png` | clip thumbnail (320×240) |
| `thumbnail-dummy.png` | dummy thumbnail for empty slots / clips without thumbnail |
| `ws-NN-*.jsonl` | session timelines: one JSON object per line with `t_ms`, `dir` (`recv`/`step`), and the message or step result. Large messages are logged as `{kind, len, keys}`; the `saved_as` files they point to were pruned except those listed above. |

Sessions:
- `ws-01` OSC connect, connect another clip on the same layer, OSC layer clear.
- `ws-02` press/release semantics of `/clear`, re-trigger, deck round trip (OSC out, REST back), by-id vs path subscriptions.
- `ws-03-deck1/2-push` REST deck select → single final composition push.
- `ws-04` SEQ placeholder: open `Blank` source, rename to `§ probe`, master 0 and bypass still report `connected` (also the opacity-by-path error).
- `ws-05` opacity 0 by param id still reports `connected`; custom thumbnail upload; PNG as clip source.
- `ws-06` which changes push the composition (renames don't; column add/remove do); WS `post`/`remove`.
- `ws-07` `get`, `unsubscribe`, error shape for an unknown path; cleanup.

## Demo data (screenshots)

- `demo-composition.json`: a neutral demo composition ("Demo", decks `Act 1` / `Act 2` (live) / `Encore`,
  layers `Background`, `Mid`, `Front`, `SEQ` with `§` placeholders). Generated from `composition.json` (same
  structure, every id renumbered, clips renamed; no real media). Run the mock with it:
  `cargo run -p mock-resolume -- --composition docs/samples/demo-composition.json`.
- `demo.seq.json`: sequences for it (`Drop A`, `Intro`, `Build B`, …), file format v4; copy it to
  `<data-dir>/Demo.seq.json`. `demo-v3.seq.json` is the same file as 0.4.x saves it (format v3), used by the
  migration test in `crates/seq-core/tests/roundtrip.rs`.
- The thumbnails in `docs/images/screenshot.png` are abstract images generated for the demo and uploaded to the
  mock (`POST /api/v1/composition/clips/by-id/<id>/thumbnail`); without them every clip shows the sample gradient.
- `m16-01-layer-add.jsonl` … `m16-05-column-remove.jsonl`: building the SEQ layer for "Sync SEQ layer" (notes §16): add a
  layer and a column without a body, rename a layer and a clip, set opacity by id, remove both again.
- `m17-01-tempo-subscribe.jsonl` … `m17-04-resync.jsonl`: the tempo controller (notes §17): subscribe by
  path (fails) and by id, then tempo changes, taps, Pause/Stop/Play, Resync made by hand in Resolume.
- `m21-01-set.jsonl`, `m21-s1.jsonl` … `m21-s11.jsonl`, `m21-deck3.jsonl`, `m21-deck4.jsonl`: layer transitions
  (notes §19): set duration and blend mode by id and by path, then the scenes of
  `tools/resolume-probe/scripts/m21_transition_scenes.sh` (set ahead of a connect, change during a fade, clear,
  `layer_determined`), and the value across a deck switch.
