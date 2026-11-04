# Resolume API notes (Milestone 0 findings)

Probed 2026-10-25 against **Resolume Arena 7.28.1 (rev 26535)** on Windows 11, composition `dev`,
with `tools/resolume-probe`. Note: the target in `CLAUDE.md` says 7.26; the machine runs 7.28.1.
Re-run the probe after Resolume upgrades.

Sources, in order of authority:
1. **Observed** — what the probe recorded. Raw captures are in `docs/samples/` (see its README).
2. **Local OpenAPI spec** — `GET http://127.0.0.1:8080/api/docs/rest/swagger.yaml` (OpenAPI 3.1,
   served by Resolume itself; Swagger UI at `/api/docs/rest/`).
3. **Resolume WebSocket article** — https://resolume.com/support/en/websocket-api (the WebSocket
   protocol is not in the OpenAPI spec).

Everything below marked ✅ was observed. Anything only documented is marked 📄.

---

## 1. General

| Fact | Status |
|---|---|
| REST base: `http://host:8080/api/v1` | ✅ |
| `GET /api/v1/product` → `{"name":"Arena","major":7,"minor":28,"micro":1,"revision":26535}` | ✅ |
| All REST index parameters (layers, clips, columns, decks) are **1-based** | 📄 + ✅ (used throughout) |
| Every object has a numeric `id`; every parameter is an object with its own `id`, `valuetype`, `value` (choice params also `index`, `options`) | ✅ |
| `/composition/.../by-id/{id}` exists for layers, clips, columns, decks; `/parameter/by-id/{param-id}` for any parameter | 📄 + ✅ |

## 2. Composition structure (`GET /api/v1/composition`)

Sample: `docs/samples/composition.json` (deck `2132323` live), `docs/samples/deck1-composition.json`
(deck `1000A Text3D` live). `GET /composition` takes ~10 ms; the JSON is 250–380 KB compact.

| Question | Answer | Status |
|---|---|---|
| Composition name field | `name.value` (`"dev"`). This is the composition name, not the file path. | ✅ |
| `.avc` file path | **Not exposed** for reading. It only appears as an input to `POST /composition/open` and `/composition/save`. | ✅ (absent in dump) + 📄 |
| Decks | `decks[]`: `id`, `name.value`, `selected.value` (true = live deck), `closed`, `colorid`, `scrollx` | ✅ |
| Active deck only? | **Yes.** `layers[]` / `columns[]` / clips are those of the selected deck only. Other decks' clips cannot be read without switching to them. | ✅ + 📄 ("Only the layers and clips of the active deck can be retrieved") |
| Layers vs decks | **Layer IDs are shared across all decks** (same 4 layer ids in both deck dumps). Layer names are composition-wide. | ✅ |
| Columns vs decks | **Columns are per deck** (deck `2132323`: 10 columns, ids `17929565906xx`; deck `1000A Text3D`: 9 columns, ids `17916197269xx`). | ✅ |
| Clips | `layers[L-1].clips[C-1]`; `clips.length == columns.length`. Empty slots exist as clip objects with `connected.value == "Empty"` and `name.value == ""`. | ✅ |
| Layer order | `layers[0]` is layer 1 (OSC/REST index 1). In Resolume's UI layer 1 is the bottom row, so the clip browser must list `layers` reversed (top first). The UI order was not checked by the probe. | ✅ (index) / convention (UI) |
| Clip id | `clip.id` (int64). **Stable** across deck switches and across REST/WS reads (`/thumbnail` by id, by-id subscriptions kept working after deck round trips). OpenAPI: "IDs remain constant across sessions and reordering." | ✅ + 📄 |
| What a clip id identifies | See §2.1. |
| Clip name | `clip.name.value`. Defaults to the file name without extension (`probe.png` → `probe`) or the source name (`Blank`). Duplicates are common (deck 1 has two clips `Clip 02…` on layer 2). | ✅ |
| Media file | `clip.video.fileinfo.path` (Windows path, e.g. `D:\media\…\x.mov`), plus `fileinfo.exists`, `format`, `width`, `height`, `duration_ms`. `null` for sources (generators) and empty slots. `clip.video.description` = `"<file name>\n<codec…>\n<duration>"`. | ✅ |
| Connected state | `clip.connected` is a `ParamState` choice: `Empty`, `Disconnected`, `Previewing`, `Connected`, `Connected & previewing`. Treat `Connected` and `Connected & previewing` as "playing". | ✅ |
| Active clip per layer | `layer.active_clip` (full clip object, or `null`) | ✅ (`composition-clip-connected.json`) |
| Thumbnail change marker | `clip.thumbnail` = `{"last_update": "<ms timestamp as string>", "is_default": bool, "id": <clip id>, "path": "/api/v1/composition/clips/by-id/<id>/thumbnail"}`. `is_default: true` (and `last_update: "0"`, `path: "/api/v1/composition/thumbnail/dummy"`) means no own thumbnail. **`last_update` also changes on structural edits** (adding a column re-issued it for every clip), so it marks "maybe changed", not "content changed". | ✅ |
| Layer names | `layer.name.value`. New layers are called `"Layer #"`; the raw API value stays `"Layer #"` for every unnamed layer (all three non-SEQ layers in `dev`). `#` appears to be a display-time placeholder for the index. | ✅ (raw value) |
| Layer opacity / master / bypass | `layer.video.opacity`, `layer.master`, `layer.bypassed` | ✅ |
| Composition-wide beat snap | `clipbeatsnap` choice, currently `None` (SPEC §3 requires off) | ✅ |

### 2.1 Clip id identity (probed for M2, 2026-10-25)

Script: `tools/resolume-probe/scripts/clip_id_identity.py`; output: `docs/samples/clip-id-identity.txt`.
Each operation was undone afterwards; the final snapshot equals the baseline.

| Operation | Effect on clip ids | Status |
|---|---|---|
| `POST /composition/clips/swap` (two clips with content) | ids move with the clips | ✅ |
| same, clip with content ↔ empty slot | ids move with the clip objects; the empty slot's id moves too | ✅ |
| `POST /composition/columns/swap`, `…/columns/{i}/insert-move` | every clip id moves with its column; column ids move too | ✅ |
| `POST /composition/columns/add` before column 1 | existing ids keep their clip and shift right; the new column's slots get **new** ids | ✅ |
| `POST /composition/layers/add` before layer 1 | existing layer and clip ids keep and shift up; new layer and slots get new ids | ✅ |
| `POST /composition/layers/{i}/move` | layer id and its clip ids move together | ✅ |
| `POST /composition/clips/by-id/{id}/open` into an **empty** slot | the slot **keeps its id**: the new content gets the empty slot's id | ✅ |
| `POST /composition/clips/by-id/{id}/clear` | the id stays, now on an `Empty` clip object (M0 §7/§8) | ✅ |

So a clip id identifies a **grid cell object** that travels with drag/swap/move. Loading
content into it or clearing it does not change the id. Consequences for clip resolution:
- An id may point to an `Empty` slot (clip deleted): ignore that match.
- An id may point to **different media** (slot reloaded with another file): check content.
- Empty slots have `video: null`; generator/source clips have `video.fileinfo: null`.

## 3. Thumbnails

| Endpoint | Result | Status |
|---|---|---|
| `GET /composition/clips/by-id/{id}/thumbnail` | `200 image/png`, `ETag: "<last_update>"` | ✅ |
| `GET /composition/layers/{L}/clips/{C}/thumbnail` | same bytes, same ETag | ✅ |
| `GET /composition/clips/by-id/{id}/thumbnail/{last_update}` | 200 if it matches the current `last_update`, **404** otherwise | ✅ |
| Clip of a deck that is not live | **404** (by id; the clip is not in the composition until its deck is selected). Thumbnails of other decks can only be shown from a cache filled while they were live. | ✅ (2026-10-27) |
| Empty slot / no thumbnail | returns the 394-byte dummy PNG, same as `GET /composition/thumbnail/dummy` (cacheable forever per OpenAPI) | ✅ |
| Size | thumbnails are 320×240 PNG (~128 KB for video clips) | ✅ |
| **Set a custom thumbnail** | `POST /composition/clips/by-id/{id}/thumbnail` as `multipart/form-data`, field `file`, `image/png` → `204`. Leaves the clip's source unchanged. Resolume sends a WS `thumbnail_update` with the new `last_update`, and GET then returns exactly the uploaded bytes. Also accepts a `text/plain` `file:///…` URL (📄). `DELETE` on the same path reverts to the auto thumbnail (📄). | ✅ |

## 4. Deck switching

| Question | Answer | Status |
|---|---|---|
| REST | `POST /composition/decks/{n}/select` (1-based) → `204` in ~1 ms (returns before the switch completes) | ✅ |
| OSC | `/composition/decks/{n}/select` int 1 also works | ✅ |
| WS notification | If subscribed to the decks' `selected` params: `parameter_update` on path `/composition/decks/{n}/select` (`value` true for the new deck, false for the old one), 11–23 ms after the command. Then ~40 `thumbnail_update` messages (empty `id: 0` payloads first). Then **exactly one composition push in its final state** with the new deck `selected`: 98 ms (small deck) to 187 ms (deck 1) after the command. | ✅ (`ws-02`, `ws-03-*`) |
| **REST during a switch** | **Unreliable.** Polling `GET /composition` every 5 ms after a select returned mixed states for 40–130 ms: first the new `selected` flag with the old clips, then the new deck's clip **names with the old deck's clip ids**, then the final state. Never read REST during a switch; wait for the WS composition push whose live deck is the target. | ✅ |
| Do playing clips stop on deck switch? | **No.** A clip playing in deck A kept `Connected` through a switch to deck B and back. | ✅ |
| Subscriptions across a switch | **By-id** subscriptions stay bound to the same parameter (clip) and keep working after a round trip. **Logical-path** subscriptions (`/composition/layers/2/clips/3/connected`) follow the *slot*: after switching they report the state of whatever clip is in that slot in the new deck (`Empty` here). | ✅ |

## 5. WebSocket

Endpoint: `ws://host:8080/api/v1` (same port as REST). All frames are JSON text.

### 5.1 On connect (✅)
1. Full composition (same shape as `GET /composition`, **no `type` field**). Arrives ~4 ms after the handshake.
2. `{"type":"sources_update","value":{"audio":[…],"video":[…]}}` (`docs/samples/ws-sources_update.json`)
3. `{"type":"effects_update","value":{…}}` (`docs/samples/ws-effects_update.json`)

### 5.2 Requests and responses (✅ unless marked)

| Send | Response |
|---|---|
| `{"action":"subscribe","parameter":"/parameter/by-id/1792956592480"}` | `{"type":"parameter_subscribed", "id":…, "valuetype":…, "value":…, "path":"/composition/layers/2/clips/1/connect", …}` (current value) |
| `{"action":"subscribe","parameter":"/composition/layers/2/clips/1/connected"}` | same. **Note: the echoed path ends in `/connect`, not `/connected`.** Match responses by `id`, not by path. |
| `{"action":"unsubscribe","parameter":…}` | `{"type":"parameter_unsubscribed", …}` |
| `{"action":"get","parameter":…}` | `{"type":"parameter_get", …}` |
| `{"action":"set","parameter":…,"value":…}` | `{"type":"parameter_set", …}` carrying the **old** value; subscribers then get `parameter_update` with the new value. Works by logical path for `name`, `master`, `bypassed`; `/composition/layers/4/video/opacity` by path → error, by id → works. |
| `{"action":"trigger"\|"reset","parameter":…}` | no response (📄) |
| `{"action":"post","id":"probe-1","path":"/composition/columns/add"}` | `{"id":"probe-1","error":null}` |
| `{"action":"remove","id":"probe-2","path":"/composition/columns/11"}` | `{"id":"probe-2","error":null}` |
| bad path in `set` | `{"path":"/composition/layers/4/video/opacity","error":"Invalid parameter path"}` |
| bad path in `get` | `{"id":null,"error":"Requested path not found"}` (no path echoed) |

Duplicate subscriptions to the same parameter (by id and by path) are accepted.

### 5.3 Pushed messages (✅)

| Message | When |
|---|---|
| `{"type":"parameter_update","id":<param id>,"path":…,"value":…,…}` | Value of a **subscribed** parameter changed. Nothing is pushed for unsubscribed parameters. |
| Full composition (no `type`) | ~55 ms after: **every clip connect/disconnect** (even with no subscriptions), clip open/clear, layer param sets (`master`, `bypassed`), column add/remove, deck switch. Close-together changes are coalesced into one push. **Not** sent for clip renames or layer renames. Size 250–380 KB each. |
| `{"type":"thumbnail_update","value":{"last_update":"…","is_default":…,"id":<clip id>,"path":…}}` | One per clip when a thumbnail changes: clip opened, custom thumbnail set, and a burst (one per slot) on deck switch and column add/remove. During deck switches the burst has empty payloads (`id: 0`, `last_update: ""`). |

Messages carry **no origin information**: an OSC-triggered connect looks the same as any other
(SPEC §7.3's time-window heuristic for manual override is still needed).

**Loading another composition (2026-10-27, Arena 7.28, `dev` <-> `show-b`):** the composition push for
the new composition arrives first. Subscribe requests for its parameter ids sent right after that push are partly
lost: of ~350, about 100 got neither `parameter_subscribed` nor an error, and Resolume answered ~250 requests with
`{"error":"Invalid parameter path"}` (no id; mostly the unsubscribes of the old composition's ids). Sending the
missing subscribes again 1 s later is answered normally. The sequencer re-sends unanswered subscriptions every
second (`engine::RESUBSCRIBE_AFTER`).

Deck names need not be unique: `show-b` has two decks named `opening`. The selected one is the deck
whose `selected.value` is `true`.

### 5.4 Consequences for the design
- Subscribe **by parameter id**, taken from the composition, and match updates by `id`.
- Composition pushes are frequent and large: parse them off the engine task. Rebuild the clip
  index only when structure changed (ids, names, column count, file paths), not on every push.
- Renames are not pushed: subscribe to the `name` params of the live deck's clips and layers, or
  refresh via REST when the UI opens a deck.
- After every rebuild, resubscribe by id (new deck → new clip param ids) and unsubscribe the old ones.

## 6. OSC (input port 7000, UDP)

| Address | Args | Behaviour | Status |
|---|---|---|---|
| `/composition/layers/{L}/clips/{C}/connect` | int 1 | Connects the clip. Indices are **1-based**, same as REST. WS `parameter_update` `Connected` arrives **3–6 ms** after sending. Works every time; no release needed. | ✅ |
| same | int 0 | "Release". With trigger style `Normal` it does **not** disconnect. (With `Piano` trigger style a release would disconnect, so **send only int 1**.) | ✅ (Normal) |
| same, clip already connected | int 1 | Re-triggers the clip, but **no `connected` change is reported** (still `Connected`); only a composition push follows. | ✅ |
| `/composition/layers/{L}/clear` | int 1 **then** int 0 | Disconnects the layer's clip; `Disconnected` arrives 3–7 ms later. **Edge-triggered:** a second int 1 without an int 0 in between is ignored, and so is a no-argument message once the button is "held". Always send `1` and `0` back to back. | ✅ |
| `/composition/layers/{L}/clear` | no args | Acts like a press (int 1) without a release. Don't use. | ✅ |
| `/composition/decks/{n}/select` | int 1 | Switches deck (see §4) | ✅ |

OSC has no acknowledgement; confirmation must come from the WebSocket.

## 7. SEQ placeholder clips

| Question | Answer | Status |
|---|---|---|
| Transparent source | `source:///video/Blank` exists (`GET /sources`); `POST /composition/clips/by-id/{id}/open` with body `source:///video/Blank` (text/plain) → 204; the clip is named `Blank` | ✅ |
| Rename to `§ <name>` | WS `set` on `.../clips/{C}/name` with `"§ probe"`. `§` survives in REST and WS. | ✅ |
| Still reports `connected` with layer **opacity 0**? | **Yes** (connect and clear both reported) | ✅ |
| … with layer **master 0**? | **Yes** | ✅ |
| … with layer **bypassed**? | **Yes** | ✅ |

## 8. Replacing a clip's source

| Endpoint | Result | Status |
|---|---|---|
| `POST /composition/clips/by-id/{id}/open`, `text/plain` body `file:///C:/…/probe.png` (forward slashes, three slashes, percent-encode spaces) | 204; the clip now plays the PNG (`fileinfo.format: "PNG"`, 320×240), name becomes `probe`, `thumbnail_update` follows | ✅ |
| `…/openfile` | deprecated alias of `open` for files | 📄 |
| `POST /composition/clips/by-id/{id}/clear` | removes the clip's content (slot becomes `Empty`) | ✅ |

For sequence thumbnails, the **custom thumbnail** endpoint (§3) is a better fit than replacing
the source: the placeholder keeps its transparent `Blank` source and only its grid thumbnail changes.

## 9. Timing summary (localhost)

| Path | Observed |
|---|---|
| OSC connect → WS `parameter_update` | 3–6 ms |
| OSC clear → WS `parameter_update` | 3–7 ms |
| Clip change → composition push | ~55 ms |
| Deck select → `decks/{n}/select` update | 11–23 ms |
| Deck select → final composition push | 98–187 ms |
| REST `GET /composition` | ~10 ms |
| REST `POST …/select`, `…/clear`, `…/open` | 1–40 ms |

## 10. Not probed / open

- Manual clicks in Resolume's UI were not generated by the M0 probe (OSC and REST only). Message
  shapes should be identical (no origin field exists); confirmed for clip retriggers in §13.
- How the `#` in `"Layer #"` is displayed in the UI (presumably as the layer number).
- Whether custom thumbnails survive saving and reopening the `.avc`.
- Layer groups (`dev` has none). Layers inside a group are listed under `layergroups[].layers`,
  per OpenAPI, which may change layer indexing. Check before relying on groups.

## 11. FFGL source plugins on clips (probed for M11, 2026-10-26)

Probe: `ffgl/` (then `ffgl-plugins/seq_view`) stage M11a ("1000A Sequence", ID `VJSQ`), which logs its lifecycle and frames per
second to Resolume's log (`%LOCALAPPDATA%\Resolume Arena\Resolume Arena log.txt`, lines `FFGL: SeqView#n ...`).
Composition: `dev`, two `§` clips on the SEQ layer holding the plugin. Capture: `docs/samples/ws-08-plugin-param-set.jsonl`.

| Question | Answer | Status |
|---|---|---|
| Plugin parameters in the composition JSON | Yes: `clip.video.sourceparams` maps the parameter **display name** to a parameter object, e.g. `"Sequence": {"valuetype": "ParamString", "id": 1793025852850, "value": ""}`. `clip.video.description` is the plugin name (`"1000A Sequence"`); `fileinfo` is absent. | ✅ |
| Set a text parameter over the WebSocket | Yes: `{"action":"set","parameter":"/parameter/by-id/<id>","value":"a1"}` → `parameter_set` (old value), `parameter_update` (new value) for subscribers; the plugin's `SetTextParameter` receives it immediately. Logical path: `/composition/layers/{L}/clips/{C}/video/source/1000asequence/sequence` (plugin name lower-cased without spaces). | ✅ |
| Rendered while connected on a layer with opacity 0 | **Yes**, continuously (~105-118 fps at 1920x1080). | ✅ |
| Rendered while only shown in the preview monitor | **Yes**: an unconnected clip's instance renders while the clip is selected (REST `POST .../clips/by-id/{id}/select` is enough) and pauses when it is not. | ✅ |
| `Connect` / `Disconnect` callbacks | Mark when the host starts/stops rendering the instance (selection/preview, connection) - not the clip's `connected` state. | ✅ |
| Instances | One per clip; Resolume also creates and destroys a short-lived instance about every 5 s while the plugin's source browser entry exists (no InitGL, no render). | ✅ |
| Grid thumbnail | Resolume generates it from the plugin output when the source is loaded (`is_default: false`). A custom thumbnail (multipart POST, §3) overrides it and is kept while the plugin renders; `DELETE` reverts to Resolume's own. | ✅ |

## 12. Quitting Resolume with WebSocket clients connected (2026-10-26)

| Observation | Status |
|---|---|
| On quit, Resolume **waits until every WebSocket client has disconnected** before it exits. Log: `Stop Composition Server` ... `Clean up Compositon`, then nothing until the last client is gone; `Deleting myself` follows within seconds after that. | ✅ (hung twice for minutes with the sequencer connected; finished 3 s after the sequencer closed) |
| A connected client gets a WebSocket **Close** frame (`code: Normal`) when the user quits; pings are answered and REST `/product` answers until then. | ✅ `docs/samples/ws-09-resolume-quit.log` (`shutdown_probe` example) |
| While quitting, Resolume still **accepts new WebSocket connections**; a client that reconnects at once holds such a connection and the shutdown waits forever. | ✅ (the hung process kept one established connection from the sequencer's new client port) |

| An idle HTTP keep-alive connection (e.g. a periodic REST health check through a pooling client) blocks the shutdown the same way. | ✅ (hung with only the sequencer's REST connection open after the WebSocket fix) |

Client rules (implemented in `resolume-client::ws` and `::rest`): REST without keep-alive (`pool_max_idle_per_host(0)`); WebSocket: answer/close properly, wait **5 s** after any established session ends before reconnecting, drop a connection that delivers no composition within 3 s, ping every second and drop the connection after 3 s of silence.


## 13. Clip retrigger and FFGL effect buttons (probed 2026-10-26)

Probe: `cargo run -p resolume-client --example retrigger_probe -- <clip-id> rest|press|watch`, clip with a video
source and the "1000A Shake" effect (`ffgl-plugins/shake`), 5 s timeline transport, play mode Loop, forward.

| Question | Answer | Status |
|---|---|---|
| What an FFGL effect sees when its clip is triggered again while playing | **Nothing**: no `Connect`, no parameter call, `SetTime` keeps the global host time. `Connect` comes only when the host starts rendering the instance (trigger from stopped, replacing another clip on the layer, selecting it for the preview). | ✅ (plugin log) |
| What the WebSocket shows on a retrigger (REST `connect` or a mouse click in the UI) | Subscribed `transport/position` (`clip.transport.position.id`, ms, `ParamRange` 0..duration) jumps to **exactly `0.0`** within ~10 ms. `connected` and `selected` do not change. The composition push that follows has no changed field for the clip. | ✅ (REST and UI clicks identical) |
| Loop wrap | Position goes from near the end to a small **non-zero** value (2.2, 4.3, 6.3, 7.2, 10.1 ms seen), never exactly 0. | ✅ |
| Clip stops (disconnect, other clip on the layer) | Position also jumps to `0.0`, ~2 ms **before** the `Disconnected` update. | ✅ |
| Position update rate | ~100 `parameter_update`s per second for one playing clip. | ✅ |
| Effect params in the composition JSON | `clip.video.effects[]`: `{"id", "name": "1000A Shake", "display_name", "bypassed", "mixer", "params": {<display name>: param}}`. An FFGL `FF_TYPE_EVENT` param is `{"id", "valuetype": "ParamEvent"}` (no value); a boolean is `ParamBoolean` with `value`. Logical path: `/composition/layers/{L}/clips/{C}/video/effects/1000ashake/effect/trigger`. | ✅ |
| Press a `ParamEvent` over the WebSocket | `set` with `value: true`, then `false` **100 ms later**: the effect fires (seen on output). `true` and `false` sent back to back: accepted (`parameter_set` + `parameter_update` each) but the effect does **not** fire. | ✅ |

## 14. User plugin folder (FFGL)

*2026-11-04, from Uli's setup (Arena 7.28):* Resolume loads user FFGL plugins from
`<Documents>\Resolume Arena\Extra Effects` at startup (the folder `ffgl/build_seqview.ps1` has always installed
`SeqView.dll` to). Plugins are loaded only at startup, and a loaded DLL is locked while Resolume runs (it can't
be replaced or renamed). Further folders can be added in Resolume's preferences; the installer lets the user
choose one (SPEC §12.1).

