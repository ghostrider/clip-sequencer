//! Client against the in-process mock Resolume: composition loads, an OSC trigger is confirmed
//! as `connected` over the WebSocket, deck switches, thumbnails, reconnect + resubscribe.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

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
async fn wait_for(h: &mut WsHandle, pred: impl FnMut(&WsEvent) -> bool) -> WsEvent {
    wait_for_within(h, Duration::from_secs(3), pred).await
}

async fn wait_for_within(
    h: &mut WsHandle,
    limit: Duration,
    mut pred: impl FnMut(&WsEvent) -> bool,
) -> WsEvent {
    tokio::time::timeout(limit, async {
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

#[tokio::test]
async fn after_resolume_closes_the_connection_the_client_waits_before_reconnecting() {
    // Resolume quits: it closes WebSocket connections but still accepts new ones until it is
    // gone, and waits for every client. Reconnecting at once would block its shutdown.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let at = |p: u16| MockConfig {
        rest_addr: format!("127.0.0.1:{p}").parse().unwrap(),
        ..MockConfig::default()
    };
    let m = start(at(port)).await.unwrap();
    let mut h = ws::spawn(format!("ws://127.0.0.1:{port}/api/v1"));
    wait_for(&mut h, |e| matches!(e, WsEvent::Composition { .. })).await;
    m.shutdown().await;
    let closed = Instant::now();
    wait_for(&mut h, |e| matches!(e, WsEvent::Disconnected { .. })).await;
    let _m2 = start(at(port)).await.unwrap(); // "Resolume" is back (or still accepting) at once
    wait_for_within(&mut h, Duration::from_secs(8), |e| {
        matches!(e, WsEvent::Connected)
    })
    .await;
    assert!(
        closed.elapsed() >= Duration::from_millis(4500),
        "reconnected after {:?}",
        closed.elapsed()
    );
}

#[tokio::test]
async fn a_connection_without_initial_composition_is_dropped() {
    let m = start(MockConfig {
        ws_silent: true,
        ..MockConfig::default()
    })
    .await
    .unwrap();
    let mut h = ws::spawn(format!("ws://{}/api/v1", m.rest_addr));
    wait_for(&mut h, |e| matches!(e, WsEvent::Connected)).await;
    let connected = Instant::now();
    let ev = wait_for_within(&mut h, Duration::from_secs(6), |e| {
        matches!(e, WsEvent::Disconnected { .. })
    })
    .await;
    assert!(
        matches!(&ev, WsEvent::Disconnected { reason } if reason.contains("no composition")),
        "{ev:?}"
    );
    let after = connected.elapsed();
    assert!(
        after >= Duration::from_millis(2900) && after < Duration::from_millis(4500),
        "dropped after {after:?}"
    );
}

#[tokio::test]
async fn rest_client_keeps_no_idle_connection_to_resolume() {
    // Resolume waits on quit until all its client connections are closed; an idle keep-alive
    // connection from the health check would block it forever (notes §12).
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 4096];
        let mut req = Vec::new();
        while !req.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = sock.read(&mut buf).await.unwrap();
            req.extend_from_slice(&buf[..n]);
        }
        let body = r#"{"name":"Arena"}"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{body}",
            body.len()
        );
        sock.write_all(resp.as_bytes()).await.unwrap();
        // The client must close its side by itself, without a second request.
        tokio::time::timeout(Duration::from_secs(2), sock.read(&mut buf)).await
    });
    let rest = RestClient::new(format!("http://{addr}/api/v1"));
    assert_eq!(rest.product().await.unwrap()["name"], "Arena");
    let after_response = server.await.unwrap();
    assert!(
        matches!(after_response, Ok(Ok(0))),
        "connection still open after the response: {after_response:?}"
    );
}
