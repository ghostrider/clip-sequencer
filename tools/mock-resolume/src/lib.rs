//! Fake Resolume for integration tests and UI work without Resolume.
//!
//! Reproduces the behaviour recorded in `docs/resolume-api-notes.md` (Arena 7.28.1):
//! - REST: product, composition (live deck only), deck select, clip thumbnail GET/POST
//!   (multipart), clip open/clear.
//! - WebSocket `/api/v1`: composition + `sources_update` + `effects_update` on connect;
//!   subscribe/unsubscribe/get/set by `/parameter/by-id/<id>` or logical path (the echoed path of
//!   `connected` is `.../connect`); `parameter_set` echoes the old value; `parameter_update` only
//!   for subscribed params; a composition push ~55 ms after connect changes, clip open/clear and
//!   deck switches (coalesced), never after renames; `thumbnail_update` messages.
//! - OSC (UDP): `/composition/layers/{L}/clips/{C}/connect` int 1 (int 0 is ignored, Normal
//!   trigger style), `/composition/layers/{L}/clear` edge-triggered (1 then 0; a held button
//!   ignores further presses; no-arg = press), `/composition/decks/{n}/select` int 1. 1-based.
//!
//! Test controls (not Resolume API): `POST /mock/connect/{layer}/{column}` simulates a manual
//! click, `GET /mock/osc-log` lists received OSC messages with receive times.

mod state;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Multipart, Path, State as AxState};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::Router;
use serde_json::{json, Value};
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::broadcast;

pub use state::{OscRecord, State};

const SAMPLE_LIVE: &str = include_str!("../../../docs/samples/composition.json");
const SAMPLE_DECK1: &str = include_str!("../../../docs/samples/deck1-composition.json");
const THUMB_CLIP: &[u8] = include_bytes!("../../../docs/samples/thumbnail-clip.png");
const THUMB_DUMMY: &[u8] = include_bytes!("../../../docs/samples/thumbnail-dummy.png");

/// What the WebSocket connections are told about.
#[derive(Debug, Clone)]
pub enum MockEvent {
    ParamChanged(u64),
    Composition,
    Thumbnail(Value),
    /// The mock is shutting down: close every WebSocket session.
    Shutdown,
}

pub struct MockConfig {
    pub rest_addr: SocketAddr,
    pub osc_addr: SocketAddr,
    /// Live composition JSON; `None` = the `dev` sample (deck `2132323` live).
    pub composition: Option<Value>,
    /// Extra deck contents (full composition JSONs with that deck selected); `None` = the deck 1
    /// sample.
    pub other_decks: Option<Vec<Value>>,
    /// Delay of the composition push after a change (Resolume: ~55 ms).
    pub push_delay: Duration,
    /// WebSocket sessions accept but never send anything (a Resolume in the middle of quitting).
    pub ws_silent: bool,
}

impl Default for MockConfig {
    fn default() -> Self {
        Self {
            rest_addr: "127.0.0.1:0".parse().unwrap(),
            osc_addr: "127.0.0.1:0".parse().unwrap(),
            composition: None,
            other_decks: None,
            push_delay: Duration::from_millis(55),
            ws_silent: false,
        }
    }
}

#[derive(Clone)]
pub struct Shared {
    pub state: Arc<Mutex<State>>,
    pub events: broadcast::Sender<MockEvent>,
    push_delay: Duration,
    push_pending: Arc<Mutex<bool>>,
    ws_silent: bool,
}

impl Shared {
    /// Applies a state change and publishes what it caused.
    fn apply(&self, f: impl FnOnce(&mut State) -> state::Changes) -> state::Changes {
        let changes = f(&mut self.state.lock().unwrap());
        for id in &changes.params {
            let _ = self.events.send(MockEvent::ParamChanged(*id));
        }
        for t in &changes.thumbnails {
            let _ = self.events.send(MockEvent::Thumbnail(t.clone()));
        }
        if changes.push {
            self.schedule_push();
        }
        changes
    }

    fn schedule_push(&self) {
        let mut pending = self.push_pending.lock().unwrap();
        if *pending {
            return;
        }
        *pending = true;
        let me = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(me.push_delay).await;
            *me.push_pending.lock().unwrap() = false;
            let _ = me.events.send(MockEvent::Composition);
        });
    }
}

/// A running mock. Dropping it does not stop the server tasks; they end with the runtime.
pub struct Mock {
    pub rest_addr: SocketAddr,
    pub osc_addr: SocketAddr,
    pub shared: Shared,
    stop: tokio::sync::watch::Sender<bool>,
    server: tokio::task::JoinHandle<()>,
    osc: tokio::task::JoinHandle<()>,
}

impl Mock {
    /// Stops the mock like a quitting Resolume: WebSocket sessions close, REST and OSC stop
    /// listening and the ports are free again.
    pub async fn shutdown(self) {
        let _ = self.shared.events.send(MockEvent::Shutdown);
        let _ = self.stop.send(true);
        self.osc.abort();
        let _ = self.osc.await;
        let _ = tokio::time::timeout(Duration::from_secs(5), self.server).await;
    }

    pub fn osc_log(&self) -> Vec<OscRecord> {
        self.shared.state.lock().unwrap().osc_log.clone()
    }

    pub fn clear_osc_log(&self) {
        self.shared.state.lock().unwrap().osc_log.clear();
    }

    /// Simulates a click on a clip in Resolume's UI.
    pub fn manual_connect(&self, layer: usize, column: usize) {
        self.shared.apply(|s| s.connect(layer, column));
    }

    /// Current `connected` value of a clip.
    pub fn clip_state(&self, layer: usize, column: usize) -> Option<String> {
        self.shared.state.lock().unwrap().clip_state(layer, column)
    }

    pub fn live_deck(&self) -> Option<String> {
        self.shared.state.lock().unwrap().live_deck()
    }

    /// Mutates the composition JSON directly (e.g. rename a layer for a test) and pushes it.
    pub fn edit(&self, f: impl FnOnce(&mut Value)) {
        self.shared.apply(|s| {
            f(&mut s.comp);
            state::Changes {
                push: true,
                ..Default::default()
            }
        });
    }
}

pub async fn start(cfg: MockConfig) -> anyhow::Result<Mock> {
    let live = match cfg.composition {
        Some(v) => v,
        None => serde_json::from_str(SAMPLE_LIVE)?,
    };
    let others = match cfg.other_decks {
        Some(v) => v,
        None => vec![serde_json::from_str(SAMPLE_DECK1)?],
    };
    let (events, _) = broadcast::channel(4096);
    let shared = Shared {
        state: Arc::new(Mutex::new(State::new(live, others))),
        events,
        push_delay: cfg.push_delay,
        push_pending: Arc::new(Mutex::new(false)),
        ws_silent: cfg.ws_silent,
    };

    let udp = UdpSocket::bind(cfg.osc_addr).await?;
    let osc_addr = udp.local_addr()?;
    let osc = tokio::spawn(osc_loop(udp, shared.clone()));

    let listener = TcpListener::bind(cfg.rest_addr).await?;
    let rest_addr = listener.local_addr()?;
    let app = Router::new()
        .route("/api/v1", get(ws_upgrade))
        .route("/api/v1/product", get(product))
        .route("/api/v1/composition", get(composition))
        .route("/api/v1/composition/decks/{n}/select", post(select_deck))
        .route(
            "/api/v1/composition/clips/by-id/{id}/thumbnail",
            get(thumbnail_get).post(thumbnail_post),
        )
        .route("/api/v1/composition/clips/by-id/{id}/open", post(clip_open))
        .route(
            "/api/v1/composition/clips/by-id/{id}/clear",
            post(clip_clear),
        )
        .route("/mock/connect/{layer}/{column}", post(mock_connect))
        .route(
            "/mock/osc-log",
            get(mock_osc_log).delete(mock_osc_log_clear),
        )
        .with_state(shared.clone());
    let (stop, mut stop_rx) = tokio::sync::watch::channel(false);
    let server = tokio::spawn(async move {
        let shutdown = async move {
            let _ = stop_rx.wait_for(|s| *s).await;
        };
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
        {
            log::error!("mock REST server: {e}");
        }
    });
    log::info!("mock Resolume: REST/WS on {rest_addr}, OSC on {osc_addr}");
    Ok(Mock {
        rest_addr,
        osc_addr,
        shared,
        stop,
        server,
        osc,
    })
}

// ---- REST ----

async fn product() -> impl IntoResponse {
    axum::Json(json!({"name":"Arena","major":7,"minor":28,"micro":1,"revision":0}))
}

async fn composition(AxState(sh): AxState<Shared>) -> Response {
    let text = sh.state.lock().unwrap().comp.to_string();
    ([(header::CONTENT_TYPE, "application/json")], text).into_response()
}

async fn select_deck(AxState(sh): AxState<Shared>, Path(n): Path<usize>) -> StatusCode {
    let changes = sh.apply(|s| s.select_deck(n));
    if changes.not_found {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::NO_CONTENT
    }
}

async fn thumbnail_get(AxState(sh): AxState<Shared>, Path(id): Path<u64>) -> Response {
    let st = sh.state.lock().unwrap();
    let Some(clip) = st.clip_by_id(id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let last_update = clip["thumbnail"]["last_update"]
        .as_str()
        .unwrap_or("0")
        .to_owned();
    let is_default = clip["thumbnail"]["is_default"].as_bool().unwrap_or(true);
    let png: Vec<u8> = match st.custom_thumbnails.get(&id) {
        Some(p) => p.clone(),
        None if is_default => THUMB_DUMMY.to_vec(),
        None => THUMB_CLIP.to_vec(),
    };
    (
        [
            (header::CONTENT_TYPE, "image/png".to_owned()),
            (header::ETAG, format!("\"{last_update}\"")),
        ],
        png,
    )
        .into_response()
}

async fn thumbnail_post(
    AxState(sh): AxState<Shared>,
    Path(id): Path<u64>,
    mut multipart: Multipart,
) -> StatusCode {
    let mut png = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            png = field.bytes().await.ok().map(|b| b.to_vec());
        }
    }
    let Some(png) = png else {
        return StatusCode::BAD_REQUEST;
    };
    let changes = sh.apply(|s| s.set_custom_thumbnail(id, png));
    if changes.not_found {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::NO_CONTENT
    }
}

async fn clip_open(AxState(sh): AxState<Shared>, Path(id): Path<u64>, body: Bytes) -> StatusCode {
    let url = String::from_utf8_lossy(&body).to_string();
    let changes = sh.apply(|s| s.open_clip(id, &url));
    if changes.not_found {
        StatusCode::NOT_FOUND
    } else if changes.bad_request {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::NO_CONTENT
    }
}

async fn clip_clear(AxState(sh): AxState<Shared>, Path(id): Path<u64>) -> StatusCode {
    let changes = sh.apply(|s| s.clear_clip(id));
    if changes.not_found {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::NO_CONTENT
    }
}

async fn mock_connect(
    AxState(sh): AxState<Shared>,
    Path((layer, column)): Path<(usize, usize)>,
) -> StatusCode {
    sh.apply(|s| s.connect(layer, column));
    StatusCode::NO_CONTENT
}

async fn mock_osc_log(AxState(sh): AxState<Shared>) -> impl IntoResponse {
    let st = sh.state.lock().unwrap();
    let start = st.started;
    let list: Vec<Value> = st
        .osc_log
        .iter()
        .map(|r| json!({"t_ms": r.at.duration_since(start).as_secs_f64() * 1000.0, "addr": r.addr, "args": r.args}))
        .collect();
    axum::Json(Value::Array(list))
}

async fn mock_osc_log_clear(AxState(sh): AxState<Shared>) -> StatusCode {
    sh.state.lock().unwrap().osc_log.clear();
    StatusCode::NO_CONTENT
}

// ---- OSC ----

async fn osc_loop(sock: UdpSocket, sh: Shared) {
    let mut buf = vec![0u8; 4096];
    loop {
        let Ok((n, _)) = sock.recv_from(&mut buf).await else {
            continue;
        };
        let at = Instant::now();
        let Ok((_, packet)) = rosc::decoder::decode_udp(&buf[..n]) else {
            continue;
        };
        let mut msgs = Vec::new();
        flatten(packet, &mut msgs);
        for m in msgs {
            let ints: Vec<i32> = m
                .args
                .iter()
                .filter_map(|a| match a {
                    rosc::OscType::Int(i) => Some(*i),
                    rosc::OscType::Float(f) => Some(*f as i32),
                    rosc::OscType::Bool(b) => Some(i32::from(*b)),
                    _ => None,
                })
                .collect();
            sh.apply(|s| s.osc(&m.addr, &ints, at));
        }
    }
}

fn flatten(p: rosc::OscPacket, out: &mut Vec<rosc::OscMessage>) {
    match p {
        rosc::OscPacket::Message(m) => out.push(m),
        rosc::OscPacket::Bundle(b) => b.content.into_iter().for_each(|p| flatten(p, out)),
    }
}

// ---- WebSocket ----

async fn ws_upgrade(ws: WebSocketUpgrade, AxState(sh): AxState<Shared>) -> Response {
    ws.on_upgrade(move |socket| ws_session(socket, sh))
}

async fn ws_session(mut socket: WebSocket, sh: Shared) {
    if sh.ws_silent {
        // Keep the connection open and read (pings are answered by the library), send nothing.
        while let Some(Ok(_)) = socket.recv().await {}
        return;
    }
    let mut events = sh.events.subscribe();
    let initial = sh.state.lock().unwrap().comp.to_string();
    for text in [
        initial,
        json!({"type":"sources_update","value":{"audio":[],"video":[]}}).to_string(),
        json!({"type":"effects_update","value":{"audio":[],"video":[]}}).to_string(),
    ] {
        if socket.send(Message::Text(text.into())).await.is_err() {
            return;
        }
    }
    let mut subs = std::collections::HashSet::new();
    loop {
        tokio::select! {
            msg = socket.recv() => {
                let Some(Ok(msg)) = msg else { return };
                let Message::Text(text) = msg else { continue };
                let Ok(req) = serde_json::from_str::<Value>(&text) else { continue };
                for reply in handle_ws_request(&sh, &req, &mut subs) {
                    if socket.send(Message::Text(reply.to_string().into())).await.is_err() {
                        return;
                    }
                }
            }
            ev = events.recv() => {
                let text = match ev {
                    Ok(MockEvent::ParamChanged(id)) if subs.contains(&id) => {
                        let st = sh.state.lock().unwrap();
                        match st.param_message(id, "parameter_update") {
                            Some(v) => v.to_string(),
                            None => continue,
                        }
                    }
                    Ok(MockEvent::ParamChanged(_)) => continue,
                    Ok(MockEvent::Composition) => sh.state.lock().unwrap().comp.to_string(),
                    Ok(MockEvent::Thumbnail(v)) => json!({"type":"thumbnail_update","value":v}).to_string(),
                    Ok(MockEvent::Shutdown) => {
                        let _ = socket.send(Message::Close(None)).await;
                        return;
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => return,
                };
                if socket.send(Message::Text(text.into())).await.is_err() {
                    return;
                }
            }
        }
    }
}

fn handle_ws_request(
    sh: &Shared,
    req: &Value,
    subs: &mut std::collections::HashSet<u64>,
) -> Vec<Value> {
    let action = req["action"].as_str().unwrap_or("");
    match action {
        "subscribe" | "unsubscribe" | "get" | "set" => {
            let param = req["parameter"].as_str().unwrap_or("");
            let id = sh.state.lock().unwrap().resolve_param(param);
            let Some(id) = id else {
                return vec![if action == "set" {
                    json!({"path": param, "error": "Invalid parameter path"})
                } else {
                    json!({"id": null, "error": "Requested path not found"})
                }];
            };
            match action {
                "subscribe" => {
                    subs.insert(id);
                    message(sh, id, "parameter_subscribed")
                }
                "unsubscribe" => {
                    subs.remove(&id);
                    message(sh, id, "parameter_unsubscribed")
                }
                "get" => message(sh, id, "parameter_get"),
                _ => {
                    // Resolume echoes the old value, then notifies subscribers.
                    let reply = message(sh, id, "parameter_set");
                    sh.apply(|s| s.set_param(id, req["value"].clone()));
                    reply
                }
            }
        }
        "post" | "remove" => vec![json!({"id": req["id"].clone(), "error": null})],
        _ => Vec::new(),
    }
}

fn message(sh: &Shared, id: u64, kind: &str) -> Vec<Value> {
    sh.state
        .lock()
        .unwrap()
        .param_message(id, kind)
        .into_iter()
        .collect()
}
