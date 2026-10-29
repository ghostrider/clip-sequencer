//! Probe: what Resolume reports over WebSocket when a clip that is already playing is triggered
//! again, and whether an effect's `ParamEvent` (FFGL event button) can be pressed over WebSocket.
//!
//! Subscribes by id to the clip's `connected`, `selected`, transport `position` and every effect
//! `ParamEvent`, then logs every message (composition pushes: which fields of this clip changed).
//! Modes: `rest` retriggers the clip itself twice (REST connect), `press` sets each event param to
//! true and back to false, `watch` only listens (click the clip in Resolume meanwhile).
//!
//! `cargo run -p resolume-client --example retrigger_probe -- <clip-id> rest|press|watch [seconds]`

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

const BASE: &str = "http://127.0.0.1:8080/api/v1";

fn find_clip(v: &Value, id: u64) -> Option<&Value> {
    match v {
        Value::Object(m) => {
            if m.get("id").and_then(Value::as_u64) == Some(id) && m.contains_key("connected") {
                return Some(v);
            }
            m.values().find_map(|c| find_clip(c, id))
        }
        Value::Array(a) => a.iter().find_map(|c| find_clip(c, id)),
        _ => None,
    }
}

/// Paths of leaves that differ between two JSON values (position ignored: it always moves).
fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for (k, va) in x {
                match y.get(k) {
                    Some(vb) => diff(va, vb, &format!("{path}/{k}"), out),
                    None => out.push(format!("{path}/{k} removed")),
                }
            }
            for k in y.keys().filter(|k| !x.contains_key(*k)) {
                out.push(format!("{path}/{k} added"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (va, vb)) in x.iter().zip(y).enumerate() {
                diff(va, vb, &format!("{path}/{i}"), out);
            }
        }
        _ if a != b => out.push(format!("{path}: {a} -> {b}")),
        _ => {}
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let clip_id: u64 = args.first().expect("clip id").parse()?;
    let mode = args.get(1).cloned().unwrap_or_else(|| "watch".into());
    let secs: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12);

    let (ws, _) = tokio_tungstenite::connect_async("ws://127.0.0.1:8080/api/v1").await?;
    let (mut tx, mut rx) = ws.split();
    let t0 = Instant::now();
    let t = || t0.elapsed().as_secs_f64();

    let comp: Value = loop {
        if let Some(Ok(Message::Text(s))) = rx.next().await {
            let v: Value = serde_json::from_str(&s)?;
            if v.get("type").is_none() {
                break v;
            }
        }
    };
    let clip = find_clip(&comp, clip_id)
        .expect("clip not in composition")
        .clone();
    let mut subs = vec![
        ("connected", clip["connected"]["id"].as_u64()),
        ("selected", clip["selected"]["id"].as_u64()),
        ("position", clip["transport"]["position"]["id"].as_u64()),
    ];
    let mut events = Vec::new();
    for fx in clip["video"]["effects"].as_array().into_iter().flatten() {
        for (name, p) in fx["params"].as_object().into_iter().flatten() {
            if p["valuetype"] == "ParamEvent" {
                let id = p["id"].as_u64().unwrap();
                println!("event param {} / {name}: id {id}", fx["name"]);
                events.push(id);
                subs.push(("event", Some(id)));
            }
        }
    }
    for (what, id) in &subs {
        let id = id.expect("param id");
        println!("subscribe {what} {id}");
        let msg = json!({"action":"subscribe","parameter":format!("/parameter/by-id/{id}")});
        tx.send(Message::Text(msg.to_string())).await?;
    }
    let pos_id = subs[2].1.unwrap();

    let mut last_clip = clip;
    let mut last_pos = f64::NAN;
    let mut pos_updates = 0u64;
    let mut step = 0u32;
    let mut tick = tokio::time::interval(Duration::from_secs(3));
    let http = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        tokio::select! {
            m = rx.next() => {
                let Some(Ok(Message::Text(s))) = m else { continue };
                let v: Value = serde_json::from_str(&s)?;
                match v.get("type").and_then(Value::as_str) {
                    None => {
                        let Some(c) = find_clip(&v, clip_id) else { continue };
                        let mut d = Vec::new();
                        diff(&last_clip, c, "", &mut d);
                        d.retain(|p| !p.starts_with("/transport/position/value"));
                        println!("[{:6.3}] composition push, clip changes: {d:?}", t());
                        last_clip = c.clone();
                    }
                    Some("parameter_update") if v["id"].as_u64() == Some(pos_id) => {
                        pos_updates += 1;
                        let p = v["value"].as_f64().unwrap_or(f64::NAN);
                        if p < last_pos {
                            println!("[{:6.3}] position went back {last_pos:.1} -> {p:.1}", t());
                        }
                        last_pos = p;
                    }
                    Some("thumbnail_update") => {}
                    Some(_) => println!("[{:6.3}] {}", t(), s.chars().take(300).collect::<String>()),
                }
            }
            _ = tick.tick() => {
                step += 1;
                if step == 1 { continue; }
                match mode.as_str() {
                    "rest" => {
                        println!("[{:6.3}] >>> REST connect (retrigger)", t());
                        http.post(format!("{BASE}/composition/clips/by-id/{clip_id}/connect")).send().await?;
                    }
                    "press" => {
                        for id in &events {
                            println!("[{:6.3}] >>> WS set event {id} true, 100 ms, false", t());
                            for val in [true, false] {
                                let msg = json!({"action":"set","parameter":format!("/parameter/by-id/{id}"),"value":val});
                                tx.send(Message::Text(msg.to_string())).await?;
                                tokio::time::sleep(Duration::from_millis(100)).await;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    println!("{pos_updates} position updates in {:.1} s", t());
    let _ = tx.send(Message::Close(None)).await;
    Ok(())
}
