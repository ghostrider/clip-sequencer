//! WebSocket client task (notes §5).
//!
//! Connects to `ws://host:port/api/v1`, reconnects with backoff, and keeps a set of parameter
//! subscriptions (by parameter id) alive across reconnects. Incoming messages become
//! [`WsEvent`]s; composition pushes are parsed here, off the engine task.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;

use crate::composition::{parse_composition, CompositionSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamMsg {
    Subscribed,
    Update,
    Get,
    Set,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WsEvent {
    Connected,
    Disconnected {
        reason: String,
    },
    /// A composition push (on connect, structure change, clip connect change, deck switch).
    Composition {
        snapshot: Box<CompositionSnapshot>,
        at: Instant,
    },
    /// A parameter message. `value` is the parameter's `value` field.
    Param {
        kind: ParamMsg,
        id: u64,
        path: String,
        value: Value,
        at: Instant,
    },
    Thumbnail {
        clip_id: u64,
        last_update: String,
        is_default: bool,
    },
    /// `{"error": "..."}` replies.
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsCommand {
    /// Replace the subscription set (parameter ids). Diffs are sent while connected; the full
    /// set is re-sent after every reconnect.
    SetSubscriptions(BTreeSet<u64>),
    /// Send the subscribe request again for these ids (those still in the set; answers lost or
    /// refused while Resolume was loading a composition).
    Resubscribe(BTreeSet<u64>),
    /// Send a raw JSON message (e.g. a `set`).
    Raw(String),
}

pub struct WsHandle {
    pub commands: UnboundedSender<WsCommand>,
    pub events: UnboundedReceiver<WsEvent>,
    pub task: JoinHandle<()>,
}

/// Starts the task. It runs until `commands` is dropped... or the event receiver is dropped.
pub fn spawn(url: String) -> WsHandle {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (ev_tx, ev_rx) = mpsc::unbounded_channel();
    let task = tokio::spawn(run(url, cmd_rx, ev_tx));
    WsHandle {
        commands: cmd_tx,
        events: ev_rx,
        task,
    }
}

const BACKOFF_MIN: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// Resolume waits on quit until every WebSocket client is gone. It closes existing connections
/// itself, but still accepts new ones while quitting - a client that reconnects at once then
/// blocks the shutdown forever. So after an established session ends (closed, reset, silent),
/// wait this long before connecting again. Failed connection attempts use the short backoff.
const AFTER_SESSION: Duration = Duration::from_secs(5);
/// A live Resolume sends the composition right after connecting and answers pings.
const FIRST_MESSAGE_LIMIT: Duration = Duration::from_secs(3);
const HEARTBEAT: Duration = Duration::from_secs(1);
const SILENCE_LIMIT: Duration = Duration::from_secs(3);

async fn run(
    url: String,
    mut cmds: UnboundedReceiver<WsCommand>,
    events: UnboundedSender<WsEvent>,
) {
    let mut subs: BTreeSet<u64> = BTreeSet::new();
    let mut backoff = BACKOFF_MIN;
    loop {
        let ws = match tokio_tungstenite::connect_async(&url).await {
            Ok((ws, _)) => ws,
            Err(e) => {
                if events
                    .send(WsEvent::Disconnected {
                        reason: format!("connect {url}: {e}"),
                    })
                    .is_err()
                {
                    return;
                }
                // Keep accepting subscription changes while waiting.
                let deadline = tokio::time::sleep(backoff);
                tokio::pin!(deadline);
                loop {
                    tokio::select! {
                        () = &mut deadline => break,
                        cmd = cmds.recv() => match cmd {
                            Some(WsCommand::SetSubscriptions(s)) => subs = s,
                            Some(WsCommand::Resubscribe(_) | WsCommand::Raw(_)) => {}
                            None => return,
                        }
                    }
                }
                backoff = (backoff * 2).min(BACKOFF_MAX);
                continue;
            }
        };
        backoff = BACKOFF_MIN;
        log::info!("WebSocket connected to {url}");
        if events.send(WsEvent::Connected).is_err() {
            return;
        }
        let (mut tx, mut rx) = ws.split();
        let mut ok = true;
        for id in &subs {
            if tx.send(Message::Text(subscribe(*id, true))).await.is_err() {
                ok = false;
                break;
            }
        }
        let connected_at = Instant::now();
        let mut last_heard = Instant::now();
        let mut got_message = false;
        let mut beat = tokio::time::interval(HEARTBEAT);
        beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let reason = loop {
            if !ok {
                break "send failed".to_owned();
            }
            tokio::select! {
                msg = rx.next() => {
                    last_heard = Instant::now();
                    match msg {
                        Some(Ok(Message::Text(text))) => {
                            got_message = true;
                            if let Some(ev) = classify(&text) {
                                if events.send(ev).is_err() {
                                    let _ = tx.close().await;
                                    return;
                                }
                            }
                        }
                        Some(Ok(Message::Close(c))) => break format!("Resolume closed the connection ({c:?})"),
                        Some(Ok(_)) => {}
                        Some(Err(e)) => break e.to_string(),
                        None => break "stream ended".to_owned(),
                    }
                },
                _ = beat.tick() => {
                    if !got_message && connected_at.elapsed() > FIRST_MESSAGE_LIMIT {
                        break "no composition after connecting (Resolume quitting?)".to_owned();
                    }
                    if last_heard.elapsed() > SILENCE_LIMIT {
                        break format!("no answer for {} s", SILENCE_LIMIT.as_secs());
                    }
                    ok &= tx.send(Message::Ping(Vec::new())).await.is_ok();
                },
                cmd = cmds.recv() => match cmd {
                    Some(WsCommand::SetSubscriptions(new)) => {
                        for id in new.difference(&subs) {
                            ok &= tx.send(Message::Text(subscribe(*id, true))).await.is_ok();
                        }
                        for id in subs.difference(&new) {
                            ok &= tx.send(Message::Text(subscribe(*id, false))).await.is_ok();
                        }
                        subs = new;
                    }
                    Some(WsCommand::Resubscribe(ids)) => {
                        for id in ids.intersection(&subs) {
                            ok &= tx.send(Message::Text(subscribe(*id, true))).await.is_ok();
                        }
                    }
                    Some(WsCommand::Raw(text)) => {
                        ok &= tx.send(Message::Text(text)).await.is_ok();
                    }
                    None => {
                        let _ = tx.send(Message::Close(None)).await;
                        return;
                    }
                }
            }
        };
        // Answer a Close / close our side properly, so Resolume can finish quitting.
        let _ = tx.close().await;
        drop(rx);
        log::warn!("WebSocket disconnected: {reason}");
        if events.send(WsEvent::Disconnected { reason }).is_err() {
            return;
        }
        tokio::time::sleep(AFTER_SESSION).await;
    }
}

fn subscribe(param_id: u64, on: bool) -> String {
    json!({
        "action": if on { "subscribe" } else { "unsubscribe" },
        "parameter": format!("/parameter/by-id/{param_id}"),
    })
    .to_string()
}

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: Option<String>,
    id: Option<Value>,
    path: Option<String>,
    value: Option<Value>,
    error: Option<Value>,
}

/// Turns one text frame into an event. Unknown or uninteresting messages give `None`.
pub fn classify(text: &str) -> Option<WsEvent> {
    let at = Instant::now();
    let env: Envelope = serde_json::from_str(text).ok()?;
    match env.kind.as_deref() {
        None if env.error.is_some() => Some(WsEvent::Error {
            message: match env.error? {
                Value::String(s) => s,
                Value::Null => return None, // `{"id":..,"error":null}`: success of post/remove
                other => other.to_string(),
            },
        }),
        None => match parse_composition(text) {
            Ok(snapshot) => Some(WsEvent::Composition {
                snapshot: Box::new(snapshot),
                at,
            }),
            Err(e) => {
                log::debug!("ignoring untyped WS message: {e}");
                None
            }
        },
        Some(
            kind
            @ ("parameter_subscribed" | "parameter_update" | "parameter_get" | "parameter_set"),
        ) => Some(WsEvent::Param {
            kind: match kind {
                "parameter_subscribed" => ParamMsg::Subscribed,
                "parameter_update" => ParamMsg::Update,
                "parameter_get" => ParamMsg::Get,
                _ => ParamMsg::Set,
            },
            id: env.id.and_then(|v| v.as_u64())?,
            path: env.path.unwrap_or_default(),
            value: env.value.unwrap_or(Value::Null),
            at,
        }),
        Some("thumbnail_update") => {
            let v = env.value?;
            Some(WsEvent::Thumbnail {
                clip_id: v.get("id")?.as_u64()?,
                last_update: v.get("last_update")?.as_str()?.to_owned(),
                is_default: v.get("is_default").and_then(Value::as_bool).unwrap_or(true),
            })
        }
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_recorded_message_shapes() {
        // Shapes copied from docs/samples/ws-*.jsonl.
        let upd = r#"{"value":"Connected","valuetype":"ParamState","options":["Empty","Disconnected","Previewing","Connected","Connected & previewing"],"index":3,"id":1792956592480,"path":"/composition/layers/2/clips/1/connect","type":"parameter_update"}"#;
        match classify(upd) {
            Some(WsEvent::Param {
                kind,
                id,
                path,
                value,
                ..
            }) => {
                assert_eq!(kind, ParamMsg::Update);
                assert_eq!(id, 1792956592480);
                assert_eq!(path, "/composition/layers/2/clips/1/connect");
                assert_eq!(value, "Connected");
            }
            other => panic!("{other:?}"),
        }
        let thumb = r#"{"value":{"last_update":"17929572483133115","is_default":false,"id":1792956590653,"path":"/api/v1/composition/clips/by-id/1792956590653/thumbnail"},"type":"thumbnail_update"}"#;
        assert_eq!(
            classify(thumb),
            Some(WsEvent::Thumbnail {
                clip_id: 1792956590653,
                last_update: "17929572483133115".into(),
                is_default: false
            })
        );
        // Deck-switch burst entries have id 0 and empty last_update; still classified.
        let burst = r#"{"value":{"last_update":"","is_default":true,"id":0,"path":""},"type":"thumbnail_update"}"#;
        assert!(matches!(
            classify(burst),
            Some(WsEvent::Thumbnail { clip_id: 0, .. })
        ));
        assert_eq!(
            classify(r#"{"path":"/x","error":"Invalid parameter path"}"#),
            Some(WsEvent::Error {
                message: "Invalid parameter path".into()
            })
        );
        assert_eq!(classify(r#"{"id":"probe-1","error":null}"#), None);
        assert_eq!(classify(r#"{"type":"sources_update","value":{}}"#), None);
        assert_eq!(classify("not json"), None);
    }

    #[test]
    fn classifies_composition_push() {
        let text = include_str!("../../../docs/samples/composition.json");
        assert!(matches!(classify(text), Some(WsEvent::Composition { .. })));
    }
}
