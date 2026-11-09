//! Probe: what a connected WebSocket client can observe while Resolume quits (Resolume waits on
//! shutdown until WebSocket clients disconnect). Every 250 ms: send a WS Ping, GET /product.
//! Logs pongs, REST results and incoming frames with timestamps. Quit Resolume while it runs;
//! the probe disconnects after `--hold` seconds without any answer so Resolume can finish.
//!
//! `cargo run -p resolume-client --example shutdown_probe -- [host:port] [hold_seconds]`

use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let addr = args
        .first()
        .cloned()
        .unwrap_or_else(|| "127.0.0.1:8080".into());
    let hold: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(20);
    let http = reqwest::Client::builder()
        .timeout(Duration::from_millis(500))
        .build()?;
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/api/v1")).await?;
    let (mut tx, mut rx) = ws.split();
    let t0 = Instant::now();
    let t = || t0.elapsed().as_secs_f64();
    println!("[{:7.2}] connected; quit Resolume now", t());
    let mut last_pong = Instant::now();
    let mut last_any = Instant::now();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let mut rest_ok = None;
    let mut n: u64 = 0;
    loop {
        tokio::select! {
            msg = rx.next() => match msg {
                Some(Ok(Message::Pong(_))) => { last_pong = Instant::now(); last_any = last_pong; }
                Some(Ok(Message::Close(c))) => { println!("[{:7.2}] server sent Close {c:?}", t()); break; }
                Some(Ok(m)) => { last_any = Instant::now(); if !matches!(m, Message::Text(_)) { println!("[{:7.2}] frame {m:?}", t()); } }
                Some(Err(e)) => { println!("[{:7.2}] ws error: {e}", t()); break; }
                None => { println!("[{:7.2}] ws stream ended", t()); break; }
            },
            _ = tick.tick() => {
                n += 1;
                if tx.send(Message::Ping(n.to_be_bytes().to_vec().into())).await.is_err() {
                    println!("[{:7.2}] ping send failed", t());
                    break;
                }
                let ok = http.get(format!("http://{addr}/api/v1/product")).send().await.map(|r| r.status().is_success()).unwrap_or(false);
                if rest_ok != Some(ok) {
                    println!("[{:7.2}] REST /product {}", t(), if ok { "answers" } else { "FAILS" });
                    rest_ok = Some(ok);
                }
                let since_pong = last_pong.elapsed();
                if n.is_multiple_of(4) {
                    println!("[{:7.2}] last pong {:.2} s ago, last frame {:.2} s ago", t(), since_pong.as_secs_f64(), last_any.elapsed().as_secs_f64());
                }
                if last_any.elapsed() > Duration::from_secs(hold) {
                    println!("[{:7.2}] nothing for {hold} s: closing so Resolume can finish", t());
                    let _ = tx.send(Message::Close(None)).await;
                    break;
                }
            }
        }
    }
    Ok(())
}
