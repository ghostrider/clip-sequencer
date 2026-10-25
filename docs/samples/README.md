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
