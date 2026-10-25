//! Client against the in-process mock Resolume: composition loads, an OSC trigger is confirmed
//! as `connected` over the WebSocket, deck switches, thumbnails, reconnect + resubscribe.

use std::collections::BTreeSet;
use std::time::Duration;

use mock_resolume::{start, Mock, MockConfig};
use resolume_client::osc::OscSender;
use resolume_client::rest::RestClient;
use resolume_client::ws::{self, ParamMsg, WsEvent, WsHandle};
use seq_core::index::ClipState;
use serde_json::Value;

const L2C1_CLIP: u64 = 1792956592496;
const L2C1_CONNECTED: u64 = 1792956592480;

async fn mock() -> Mock {
    start(MockConfig::default()).await.unwrap()
}

/// Waits for the first event matching `pred`.
async fn wait_for(h: &mut WsHandle, mut pred: impl FnMut(&WsEvent) -> bool) -> WsEvent {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let ev = h.events.recv().await.expect("ws task ended");
            if pred(&ev) {
                return ev;
            }
        }
    })
    .await
    .expect("timed out waiting for WS event")
}

#[tokio::test]
async fn rest_composition_and_thumbnails() {
    let m = mock().await;
    let rest = RestClient::new(format!("http://{}/api/v1", m.rest_addr));
    assert_eq!(rest.product().await.unwrap()["name"], "Arena");
    let snap = rest.composition().await.unwrap();
    assert_eq!(snap.index.composition, "dev");
    assert_eq!(snap.index.live_deck.as_deref(), Some("2132323"));

    let t = rest.thumbnail(L2C1_CLIP).await.unwrap();
    assert!(t.png.starts_with(b"\x89PNG"));
    let last_update = snap
        .index
        .clip_by_id(L2C1_CLIP)
        .unwrap()
        .thumbnail
        .last_update
        .clone();
    assert_eq!(t.etag.as_deref(), Some(last_update.as_str()));

    // Custom thumbnail round trip.
    let png = b"\x89PNG custom".to_vec();
    rest.set_clip_thumbnail(1792956590653, png.clone())
        .await
        .unwrap();
    assert_eq!(rest.thumbnail(1792956590653).await.unwrap().png, png);
    assert!(rest.thumbnail(42).await.is_err());
}

#[tokio::test]
async fn osc_trigger_is_confirmed_over_websocket() {
    let m = mock().await;
    let mut h = ws::spawn(format!("ws://{}/api/v1", m.rest_addr));
    wait_for(&mut h, |e| matches!(e, WsEvent::Connected)).await;
    let first = wait_for(&mut h, |e| matches!(e, WsEvent::Composition { .. })).await;
    let WsEvent::Composition { snapshot, .. } = first else {
        unreachable!()
    };
    assert_eq!(snapshot.states[&L2C1_CLIP], ClipState::Disconnected);

    h.commands
        .send(ws::WsCommand::SetSubscriptions(BTreeSet::from([
            L2C1_CONNECTED,
        ])))
        .unwrap();
    let sub = wait_for(&mut h, |e| {
        matches!(
            e,
            WsEvent::Param {
                kind: ParamMsg::Subscribed,
                ..
            }
        )
    })
    .await;
    assert!(matches!(
        sub,
        WsEvent::Param {
            id: L2C1_CONNECTED,
            ..
        }
    ));

    let osc = OscSender::new(&m.osc_addr.to_string()).unwrap();
    let sent = std::time::Instant::now();
    osc.connect_clip(2, 1).unwrap();
    let upd = wait_for(&mut h, |e| {
        matches!(
            e,
            WsEvent::Param {
                kind: ParamMsg::Update,
                ..
            }
        )
    })
    .await;
    let WsEvent::Param {
        id,
        value,
        path,
        at,
        ..
    } = upd
    else {
        unreachable!()
    };
    assert_eq!(
        (id, value, path.as_str()),
        (
            L2C1_CONNECTED,
            Value::from("Connected"),
            "/composition/layers/2/clips/1/connect"
        )
    );
    assert!(
        at.duration_since(sent) < Duration::from_millis(250),
        "confirmation within the 250 ms window"
    );

    // ~55 ms later the composition is pushed with the clip connected.
    let push = wait_for(&mut h, |e| matches!(e, WsEvent::Composition { .. })).await;
    let WsEvent::Composition { snapshot, .. } = push else {
        unreachable!()
    };
    assert_eq!(snapshot.states[&L2C1_CLIP], ClipState::Connected);

    osc.clear_layer(2).unwrap();
    let upd = wait_for(&mut h, |e| {
        matches!(
            e,
            WsEvent::Param {
                kind: ParamMsg::Update,
                ..
            }
        )
    })
    .await;
    assert!(matches!(upd, WsEvent::Param { ref value, .. } if value == "Disconnected"));
    // The OSC log saw connect + clear press + clear release.
    let addrs: Vec<String> = m.osc_log().into_iter().map(|r| r.addr).collect();
    assert_eq!(addrs.len(), 3);
}

#[tokio::test]
async fn deck_switch_is_announced_by_a_composition_push() {
    let m = mock().await;
    let rest = RestClient::new(format!("http://{}/api/v1", m.rest_addr));
    let mut h = ws::spawn(format!("ws://{}/api/v1", m.rest_addr));
    wait_for(&mut h, |e| matches!(e, WsEvent::Composition { .. })).await;
    rest.select_deck(1).await.unwrap();
    let push = wait_for(&mut h, |e| matches!(e, WsEvent::Composition { .. })).await;
    let WsEvent::Composition { snapshot, .. } = push else {
        unreachable!()
    };
    assert_eq!(snapshot.index.live_deck.as_deref(), Some("1000A Text3D"));
    assert_eq!(snapshot.index.columns.len(), 9);
}

#[tokio::test]
async fn client_started_before_resolume_connects_and_subscribes_when_it_appears() {
    // Reserve a free port, release it, and start the mock there only later.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let mut h = ws::spawn(format!("ws://127.0.0.1:{port}/api/v1"));
    h.commands
        .send(ws::WsCommand::SetSubscriptions(BTreeSet::from([
            L2C1_CONNECTED,
        ])))
        .unwrap();
    wait_for(&mut h, |e| matches!(e, WsEvent::Disconnected { .. })).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let _m = start(MockConfig {
        rest_addr: format!("127.0.0.1:{port}").parse().unwrap(),
        ..MockConfig::default()
    })
    .await
    .unwrap();
    wait_for(&mut h, |e| matches!(e, WsEvent::Connected)).await;
    wait_for(&mut h, |e| {
        matches!(
            e,
            WsEvent::Param {
                kind: ParamMsg::Subscribed,
                id: L2C1_CONNECTED,
                ..
            }
        )
    })
    .await;
}
