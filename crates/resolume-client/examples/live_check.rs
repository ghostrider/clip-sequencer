//! Smoke test against a running Resolume: REST composition, WS subscribe by id, OSC trigger,
//! confirmation latency, OSC clear. Triggers the first non-empty clip of the live deck and
//! clears its layer afterwards.
//! `cargo run -p resolume-client --example live_check -- [host] [rest_port] [osc_port]`

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use resolume_client::osc::OscSender;
use resolume_client::rest::RestClient;
use resolume_client::ws::{self, ParamMsg, WsCommand, WsEvent};
use resolume_client::ResolumeAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let addr = ResolumeAddr {
        host: a.first().cloned().unwrap_or_else(|| "127.0.0.1".into()),
        rest_port: a.get(1).and_then(|s| s.parse().ok()).unwrap_or(8080),
        osc_port: a.get(2).and_then(|s| s.parse().ok()).unwrap_or(7000),
    };
    let rest = RestClient::new(addr.rest_base());
    println!("product: {}", rest.product().await?);
    let snap = rest.composition().await?;
    let idx = &snap.index;
    println!(
        "composition {:?}, live deck {:?}, {} layers, {} columns, {} clips with content",
        idx.composition,
        idx.live_deck,
        idx.layers.len(),
        idx.columns.len(),
        idx.filled_clips().count()
    );
    let clip = idx
        .filled_clips()
        .next()
        .ok_or_else(|| anyhow::anyhow!("live deck has no clips"))?
        .clone();
    println!(
        "test clip: layer {} column {} {:?}",
        clip.layer, clip.column, clip.name
    );

    let mut h = ws::spawn(addr.ws_url());
    h.commands
        .send(WsCommand::SetSubscriptions(BTreeSet::from([
            clip.connected_param
        ])))?;
    let osc = OscSender::new(&addr.osc_target())?;
    let mut sent = None;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        let Ok(Some(ev)) = tokio::time::timeout(Duration::from_millis(500), h.events.recv()).await
        else {
            continue;
        };
        match ev {
            WsEvent::Param {
                kind: ParamMsg::Subscribed,
                value,
                ..
            } => {
                println!("subscribed, state {value}");
                osc.connect_clip(clip.layer, clip.column)?;
                sent = Some(Instant::now());
            }
            WsEvent::Param {
                kind: ParamMsg::Update,
                value,
                at,
                ..
            } if value == "Connected" => {
                println!(
                    "confirmed Connected after {:.1} ms",
                    at.duration_since(sent.unwrap()).as_secs_f64() * 1000.0
                );
                osc.clear_layer(clip.layer)?;
                sent = Some(Instant::now());
            }
            WsEvent::Param {
                kind: ParamMsg::Update,
                value,
                at,
                ..
            } if value == "Disconnected" => {
                println!(
                    "cleared, Disconnected after {:.1} ms",
                    at.duration_since(sent.unwrap()).as_secs_f64() * 1000.0
                );
                return Ok(());
            }
            WsEvent::Composition { snapshot, .. } => {
                println!("composition push ({} clips)", snapshot.index.clips().len())
            }
            other => println!("{other:?}"),
        }
    }
    anyhow::bail!("timed out")
}
