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


## 15. Slot content changes from Resolume's own UI (probed for M13, 2026-11-03, Arena 7.28.1)

Manual actions by Uli on layer `TEST` (deck "1000A Text3D", composition `dev`) while
`resolume-probe session` logged the WebSocket with the `connect` and `transport/position` params of
columns 1–6 subscribed. Log (filtered: no thumbnail updates, position only at jumps, first 100 s): `docs/samples/m13-slot-probe.jsonl`; layer `TEST` before/after: `docs/samples/m13-test-layer-before-after.json`.
(At ~106 s and after, other changes, then a composition load, appear in the log. They are not part of this probe.)

| Action (mouse in Resolume) | Ids | WebSocket | Status |
|---|---|---|---|
| A: drag the **playing** clip from col 1 to an empty col 2 | cells swap: the clip's id moves to col 2, the empty slot's id moves to col 1 (as with REST, §2.1) | the moved clip goes `Disconnected` (moving a playing clip stops it); subscribed params report their new path; composition push after ~60 ms | ✅ |
| B: drag a stopped clip from col 2 onto an occupied col 6 | **swap**, both ids move with their content | **no composition push** and no `connect` update; only updates of params that change anyway (a running `transport/position`) show the new path | ✅ |
| C: drop a media file onto the **playing** col 6 | slot **keeps its id**, gets the new media | `Disconnected`, position to 0: the old clip stops and the new media does **not** play (Uli saw the same). **No composition push** | ✅ |
| D: drop a media file onto a stopped col 4 | slot keeps its id, gets the new media | `Previewing` then `Disconnected` within ~150 ms; composition push after ~70 ms | ✅ |
| E: click col 3, then col 6 | – | `Connected` as usual, composition push ~50 ms later | ✅ |

| Endpoint (read-only) | Result | Status |
|---|---|---|
| `GET /composition/layers/{L}` (1-based) | `200`, the layer with all its clips, 159 KB, ~3 ms | ✅ |
| `GET /composition/layers/by-id/{id}` | `404` | ✅ |
| `GET /composition/layers/{L}/clips/{C}` | `200`, one clip (id, name, `connected`, `video.fileinfo`), 4.6 KB, ~11 ms | ✅ |

Consequences for slot lanes (SPEC §4.5):
- The composition index can be **stale** after UI drags and drops (B, C: no push), so it cannot be
  trusted for what a slot holds at step time.
- The trigger itself does not need it: OSC addresses are layer/column positions (§6), so
  `/composition/layers/{L}/clips/{C}/connect` always hits what is in the slot now.
- A dropped file stops the playing clip in that slot (C). The lane stays silent until its next
  step. This is not a connect, so it does not look like a manual override (SPEC §7.3).
- Not probed: dragging a **stopped** clip into an empty slot, and dropping a file into an
  **empty** slot (push or not).


## 16. Building the SEQ layer (probed for M16, 2026-11-18, Arena 7.29.0)

For "Sync SEQ layer" (SPEC §7.6). Unsaved scratch composition, deck `Gradient`, 3 layers, 12 columns; every
change was undone afterwards. Logs: `docs/samples/m16-0*.jsonl` (`resolume-probe session`).

| Call | Result | Status |
|---|---|---|
| `POST /composition/layers/add`, **no body** | `204` in ~73 ms. The new layer is appended **on top** (highest index, here 4), named `"Layer #"`, one empty slot per column with new ids. Existing layer and clip ids keep their indexes. A composition push follows. | ✅ `m16-01` |
| WS `set` `/composition/layers/{L}/name` | renames the layer (`parameter_set` carries the old `"Layer #"`) | ✅ `m16-02` |
| `POST /composition/clips/by-id/{id}/open`, body `source:///video/Blank`, into an empty slot of the new layer | `204`; the clip is named `Blank` | ✅ (as §7) |
| WS `set` clip name, by path `/composition/layers/{L}/clips/{C}/name` **and** by id `/parameter/by-id/<clip name param id>` | both rename the clip; UTF-8 (`§`, `C2 A7`) is stored as sent | ✅ `m16-02` |
| WS `set` `/parameter/by-id/<layer video.opacity id>` value `0.0` | sets the new layer's opacity (id from `GET /composition/layers/{L}` → `video.opacity.id`, as §5.2: by id, not by path) | ✅ `m16-03` |
| WS `{"action":"remove","id":…,"path":"/composition/layers/{L}"}` | `{"id":…,"error":null}`, the layer is gone (probe clean-up; the app doesn't remove layers) | ✅ `m16-03` |
| `POST /composition/columns/add`, **no body** | `204` in ~70 ms; the column is appended **at the end**, existing clips keep their positions | ✅ `m16-04` |
| WS `remove` `/composition/columns/{n}` | removes the column (as §5.2) | ✅ `m16-05` |
| `GET /sources` → `video[].name` | lists generators and FFGL source plugins by display name (`Blank`; a plugin only when Resolume loaded it at start-up, §14) | ✅ |
| `GET /sources` entry of the plugin | `{"idstring":"VJSQ","name":"1000A Sequence","category":"Video Sources",…}` (probed 2026-11-19, plugin 0.2.0 build 133) | ✅ |
| `POST …/open` with `source:///video/1000A%20Sequence` (display name, space as `%20`), into an empty slot | `204` in ~165 ms; the clip is named `1000A Sequence`, `video.sourceparams` holds the plugin's `Sequence`, `Server`, `Build` parameters (`Sequence` empty until the engine binds it, §14). Opening the **last** column's slot made Resolume append an empty column. | ✅ |

End-to-end with the app (`seq-app-cli`, scratch data folder, 2 sequences in the live deck): Sync created
layer 4 `SEQ` at opacity 0 with `§ Intro`, `§ Drop` (Blank, plugin not installed); after renaming a
sequence and syncing with orphan removal, the old `§` clip was cleared and its slot (same id) reused; a
third sync changed nothing. With the plugin installed (2026-11-19, deck `Downloads`, existing empty SEQ
layer): Sync loaded "1000A Sequence" into `§ Verse` and `§ Chorus`, the engine set their `Sequence`
parameters to `Verse` and `Chorus`, and a second sync reported "already in sync".


## 17. Tempo controller (probed for a Resolume tempo clock, 2026-11-19, Arena 7.29.0)

Can the sequencer follow Resolume's own tempo instead of a MIDI clock? Read-only: the user made every
change in Resolume while `resolume-probe session` listened. Logs: `docs/samples/m17-0*.jsonl`.

| Question | Result | Status |
|---|---|---|
| `GET /composition` → `tempocontroller` | `tempo` (`ParamRange` 20–500, e.g. `152.389…`), `play_state` (`ParamChoice` `Play`/`Pause`/`Stop`), events `tempo_tap`, `resync`, `tempo_push`, `tempo_pull` (`ParamEvent`). **No beat or bar position** anywhere in it. | ✅ |
| WS `subscribe` by path `/composition/tempocontroller/tempo` | `{"error":"Invalid parameter path"}` | ❌ `m17-01` |
| WS `subscribe` `/parameter/by-id/<id>` (ids from REST) | `parameter_subscribed` with the value. The logical paths it reports differ from the REST keys: `…/tempocontroller/play` (REST `play_state`), `…/tempocontroller/tempotap` (REST `tempo_tap`). Silent while nothing changes. | ✅ `m17-02` |
| Tempo typed or dragged in Resolume | one `parameter_update` per change, full precision (e.g. `130.574…` → `131.574…` while dragging, ~150–300 ms apart) | ✅ `m17-03` |
| Tap tempo | **two** `parameter_update`s on `tempotap` per tap (press and release, ~90–120 ms apart; no value), and from the 3rd tap on a new `tempo` in the same millisecond as the press (127.9 → 126.4 → 128.0 → 129.6) | ✅ `m17-03`, `m17-04` |
| Pause / Stop / Play | `parameter_update` on `play` with `value` and `index` each time; **Stop** was followed by a full composition push | ✅ `m17-03` |
| Resync | **two** `parameter_update`s on `…/tempocontroller/resync` per press (press and release, ~100 ms apart; no value). In `m17-03` it was not pressed. | ✅ `m17-04` |
| OSC output (Preferences → OSC → Output, 127.0.0.1:7001; off by default here) | only changes, each message **sent twice**: `/composition/tempocontroller/tempo` **normalised 0–1** over 20–500 BPM (`0.22993` = 130.37), `…/tempotap` and `…/resync` as `1` (press) / `0` (release), and the playing clip's `…/transport/position` (also as `/composition/selectedclip/…`, ~frame rate). **No beat or bar message**, also not over 20 s of steady tempo (~40 beats). | ✅ (logs not kept: `osc-m17*.jsonl` in the session scratchpad) |
| Ableton Link state | not probed | ❓ |

Result: Resolume reports **tempo, transport, taps and resync**, over the WebSocket and over OSC, but never its
**beat phase**. A tempo clock built on this runs its own beat grid at Resolume's BPM (start on Play, freeze
on Pause, reset on Stop) and takes its phase from events: a **Resync** press marks bar 1, a tap press marks
a beat (both arrive within WebSocket latency). Until the first resync the grid is not in phase with
Resolume's beat. Still open: whether Resolume's own beat restarts at bar 1 on Resync (check by eye), and
Ableton Link.

## 18. LUTs: an FFGL effect with a 3D texture, and Resolume's own LUT effect (probed for M18, 2026-11-22, Arena 7.29.0)

Can the app colour-grade Resolume's output live? A throwaway FFGL effect "1000A LUT Probe" (`LUT` text param,
`Mix`; a procedural 3D LUT chosen by name, re-uploaded only when the name changes; not kept in the repo) on a
clip, a layer and the composition, then Resolume's built-in **LUT** effect on the same clip. The user made the
changes in Resolume; the app side only read the composition, except one WebSocket `set` (agreed with the user).

| Question | Result | Status |
|---|---|---|
| FFGL effect on composition, layer, clip | appears in `video.effects[]` of the composition, the layer and the clip; a text param is `{"valuetype":"ParamString","id":…,"value":"warm65"}`, the typed value is in the composition JSON; a standard float is `ParamRange` 0–1 | ✅ |
| 3D texture in Resolume's FFGL context | works: `GL_RGB16F` 33³ and 65³ via `glTexImage3D`/`glTexSubImage3D`, trilinear lookup; GL 4.1 (Quadro T2000), `GL_MAX_3D_TEXTURE_SIZE` 16384. Text param changes arrive by `SetTextParameter` on a host thread, not the render thread. | ✅ (by eye) |
| Upload time | not measured (the probe's log line after the upload was never written; cause not found) | ❓ |
| Resolume's **LUT** effect (`name` `LUT`, idstring `c_374f5d67-85db-4fa7-afd8-d86621b6926e`) | params `LUT 3D` (`ParamFile`, `value` = full path, default `C:\Program Files\Resolume Arena\luts\Elenno.cube`) and `Opacity` (`ParamRange` 0–1); the usual `bypassed` and `mixer` | ✅ |
| A `.cube` anywhere on disk | the file picker takes any path (a 17³ `.cube` with `TITLE` and `LUT_3D_SIZE`, R fastest, applied correctly) | ✅ |
| The file is **overwritten** while in use | Resolume **reloads it by itself**, within about a second | ✅ (by eye) |
| The file is **replaced by a rename** (temp file renamed over it) | reloaded by itself as well, "pretty quickly" | ✅ (by eye) |
| WS `set` `/parameter/by-id/<LUT 3D id>` with another path | accepted; the composition JSON shows the new path and the picture changes to that file | ✅ |
| 1D LUTs, sizes above 17³, `DOMAIN_MIN/MAX` | not probed | ❓ |

Result: Resolume's own LUT effect is enough for live grading. The app writes a `.cube` (temp file + rename), every
LUT effect pointing at that file reloads, and the app can point an effect at a file over the WebSocket. No own
plugin is needed (M18 decision, 2026-11-22).

## 19. Layer transitions (probed for M21, 2026-11-27, Arena 7.29.0)

Can the app set a layer's transition just before a step, and what does a fade look like on the WebSocket? Layer 2
of the probe composition, two generator clips (Gradient, Sine Wave). Every scene starts at 0 s / Alpha with clip 1
connected and puts the layer back to 0 s / Alpha. Script `tools/resolume-probe/scripts/m21_transition_scenes.sh`;
logs `docs/samples/m21-*.jsonl`. A fade's length is measured from the WebSocket: the outgoing clip stays
`Connected` until the fade is over, then reports `Disconnected`.

**Shape** (`GET /composition`, per layer):

```json
"transition": {
  "duration":   {"id": …, "valuetype": "ParamRange", "min": 0, "max": 10, "in": 0, "out": 10, "value": 0},
  "blend_mode": {"id": …, "valuetype": "ParamChoice", "index": 2, "value": "Alpha", "options": ["Add", …, "Zoom Out"]},
  "params": null
}
```

Each clip has the same two plus `layer_determined` (`ParamBoolean`), and its `blend_mode` options start with
`"Layer Determined"`. The layer's 52 options are the mixer list (`Add`, `Alpha`, `Cut`, `Dissolve`, `Wipe Ellipse`,
…, including installed FFGL mixers such as `Add Example`). Over the WebSocket the paths are
`/composition/layers/{L}/transition/duration`, `/composition/layers/{L}/video/transition/mixer/blendmode` and
`/composition/layers/{L}/clips/{C}/transition/layerdetermined` (not the REST keys).

| Question | Result | Status |
|---|---|---|
| `set` duration / blend mode **by id** | works: `parameter_set` (old value), then `parameter_update` to subscribers, within ~1 ms. Duration in **seconds** (2.5 → 2.5). Blend mode by **option name** (`"Dissolve"`). | ✅ `m21-01` |
| `set` by path (`…/layers/2/transition/duration`, `…/transition/blend_mode`) | `{"error":"Invalid parameter path"}`: by id only | ✅ `m21-01` |
| blend mode by `index` only | `"Cannot set parameter, missing value"` | ✅ `m21-01` |
| unknown blend name | `{"path":…,"error":"Invalid choice name for param"}`, value unchanged | ✅ `m21-01` |
| duration out of range (12, −1) | clamped to 10 and 0, no error | ✅ `m21-01` |
| Side effect of a set | a **full composition push** (~870 KB) ~85 ms later, for each changed value | ✅ `m21-01` |
| Set 1 s, 30 ms, or **0 ms** (same instant: WS `set`, then the OSC `connect`) before a connect | the fade has the new value every time (2.00 s, 2.00 s, 1.97 s) | ✅ `m21-s1`, `s2`, `s3` |
| Duration changed **during a running fade** | the fade keeps its progress and the rest is rescaled: at 25 % of a 4 s fade, 8 s → ends after 1 + 0.75 × 8 = 7 s, 2 s → 2.5 s, **0 → ends at once** | ✅ `m21-s4`, `s7`, `s8` |
| Blend mode changed during a fade | the length is unchanged (3 s); how the picture changes was not watched | ✅ / ❓ look `m21-s6` |
| Layer **clear** (OSC `clear` 1, 0) with 3 s | the clear **fades** too: the clip reports `Disconnected` after 3.0 s | ✅ `m21-s5` |
| During a fade | **two clips** of the layer are `Connected`; connecting the outgoing one again sends no update (it never disconnected) and the other one leaves after the rest of the fade | ✅ `m21-s9` |
| Clip `layer_determined: false` (clip's own duration 0) | the clip ignores the layer: cut although the layer has 2 s | ✅ `m21-s10` |
| Clip `layer_determined: true` and a `set` of the clip's own duration | answered (`parameter_set`) but **ignored**: no update, value stays; the layer's 3 s fade is used | ✅ `m21-s11` |
| Param ids across a **deck switch** | layer ids and the transition param ids stay the same | ✅ |
| Value across a deck switch | belongs to the layer: 1.5 s stays 1.5 s on the other deck and back | ✅ `m21-deck3`, `deck4` |
| Param ids after **reloading** the composition | not probed. Param ids (1795…) are much newer than layer ids (1787…), so they are probably minted at load; look them up from the composition by layer id each time | ❓ |

Consequences (SPEC §7.7): send a value only when it differs (every set costs a composition push); a set sent in the
same instant as the trigger is in time, so the lead time only has to keep clear of a running fade, which any
change of duration rescales; the stop's clear fades, so the original goes back **before** the clear; while a fade
runs, the outgoing clip's late `Disconnected` is no user action.

## 20. Clicks and clears made by hand (probed for M25, 2026-11-27, Arena 7.29.0)

What the WebSocket reports when the user plays clips in Resolume's own UI, for recording (SPEC §7.9). Layer 1 had
a transition of 2.37 s (Displace), layer 2 none; every clip *Layer Determined*. The probe only listened
(`connected` of the clips of layers 1–2 and both layers' transition parameters, subscribed by id). Log
`docs/samples/m25-clicks.jsonl` (composition pushes left out).

| Action in Resolume | WebSocket | Status |
|---|---|---|
| Click a clip on an empty layer | `parameter_update` `Connected` for it, same shape as for an OSC trigger (§5.3) | ✅ |
| Click another clip, layer with a 2.37 s fade | the new clip `Connected` at once; the old one `Disconnected` **2.34 s later**, when the fade ends | ✅ |
| Click the clip that is playing | **nothing** (as for OSC, §13) | ✅ |
| Layer **Clear** button, layer with a 2.37 s fade | the playing clip `Disconnected` only at the **end** of the fade-out; nothing at the press | ✅ |
| Layer Clear, layer with no fade | `Disconnected` at once | ✅ |
| Switch clips with a cut (§19 scene 10, OSC) | old `Disconnected` and new `Connected` in the **same millisecond**, the `Disconnected` first | ✅ |
| Preview a clip | `Connected & previewing` (still connected), `Previewing` alone (not connected) | ✅ |
| Transition duration typed in the layer | one `parameter_update` (`2`, later `0`) | ✅ |

**Piano trigger style** (same day; per clip `triggerstyle` `Composition Determined` / `Normal` / `Piano` /
`Toggle`, WS path `…/clips/{C}/cliptriggerstyle`; the composition default is `cliptriggerstyle`). Logs
`m25-piano-osc.jsonl` (OSC, set by the probe) and `m25-piano-hand.jsonl` (by hand):

| Action | WebSocket | Status |
|---|---|---|
| OSC `connect` 1 on a Piano clip, no release | `Connected`, and it stays (1.5 s watched) | ✅ |
| OSC `connect` 0 afterwards | `Disconnected` at once | ✅ |
| OSC 1 and 0 back to back | `Connected`; the release is ignored | ✅ |
| Hold a Piano clip by hand ~1 s | `Connected` on press, `Disconnected` on release (1.08 s later) | ✅ |
| Tap by hand | connected for **91 ms** (less than a sixteenth at 128 BPM) | ✅ |
| Hold 1.8 s with a 1 s layer fade | `Disconnected` 2.84 s after the press: the release **fades out** like a clear | ✅ |

So a release is a clear: recorded as a rest one fade before its `Disconnected`. The app's own triggers (1 only)
keep a Piano clip playing until the next step or a rest replaces it.

Consequences (SPEC §7.9): a click is a `Connected` that the app didn't cause. A clear is a `Disconnected` of the
layer's last connected clip that no `Connected` on the same layer follows within a few milliseconds (a cut
switch sends both together); with a fade it arrives one fade later, so the press was at that time minus the
layer's duration. A `Disconnected` while another clip of the layer is connected is the end of a fade, nothing to
record.
