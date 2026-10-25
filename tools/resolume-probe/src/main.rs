//! Milestone 0 probe: talks to a running Resolume and records what its REST, WebSocket and
//! OSC interfaces actually do. Findings go to `docs/resolume-api-notes.md`, raw captures to
//! `docs/samples/`.
//!
//! `session` is the workhorse: it opens the WebSocket, logs every message with a timestamp,
//! and runs a list of steps (OSC sends, REST calls, WS messages, waits) in between, so cause
//! and effect show up in one timeline.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use clap::{Parser, Subcommand};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;

/// WebSocket messages larger than this are logged as a summary; the first few are saved whole.
const BIG_MESSAGE: usize = 16 * 1024;
const BIG_SAVES_PER_TYPE: usize = 2;

#[derive(Parser)]
#[command(about = "Explore a running Resolume's REST, WebSocket and OSC APIs")]
struct Cli {
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = 8080)]
    rest_port: u16,
    #[arg(long, default_value_t = 7000)]
    osc_port: u16,
    /// Directory for captured samples.
    #[arg(long, default_value = "docs/samples")]
    out: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Save GET /product and GET /composition and print a structural summary.
    Dump {
        /// File name prefix, e.g. "deck2-" to keep dumps of several decks apart.
        #[arg(long, default_value = "")]
        prefix: String,
    },
    /// Fetch a clip thumbnail by id or by layer/clip index and save it.
    Thumb {
        #[arg(long)]
        id: Option<u64>,
        #[arg(long)]
        layer: Option<usize>,
        #[arg(long)]
        clip: Option<usize>,
        /// Append the `last_update` value to the path (the `/thumbnail/{last-updated}` variant).
        #[arg(long)]
        last_update: Option<String>,
        #[arg(long, default_value = "thumb")]
        name: String,
    },
    /// Open the WebSocket, log everything, and run steps. Steps (one argument each):
    ///   "wait <ms>"                    sleep
    ///   "sub <path>"                   {"action":"subscribe","parameter":<path>}
    ///   "ws <json>"                    send raw JSON
    ///   "osc <addr> [i:1 f:0.5 s:x T F]..."
    ///   "rest <METHOD> <path> [text body]"   path relative to /api/v1
    ///   "restjson <METHOD> <path> <json>"
    ///   "thumbpost <clip-id> <png file>"     multipart custom thumbnail
    ///   "until <substring> <timeout-ms>"     wait for a small WS message containing substring
    ///   "mark <text>"                  write a marker into the log
    #[command(verbatim_doc_comment)]
    Session {
        /// Log file name (without extension) under --out.
        #[arg(long)]
        log: String,
        /// Keep listening this long after the last step.
        #[arg(long, default_value_t = 1500)]
        tail_ms: u64,
        steps: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    tokio::fs::create_dir_all(&cli.out).await?;
    let base = format!("http://{}:{}/api/v1", cli.host, cli.rest_port);
    let http = reqwest::Client::new();
    match &cli.cmd {
        Cmd::Dump { prefix } => dump(&http, &base, &cli.out, prefix).await,
        Cmd::Thumb {
            id,
            layer,
            clip,
            last_update,
            name,
        } => {
            let mut path = match (id, layer, clip) {
                (Some(id), _, _) => format!("/composition/clips/by-id/{id}/thumbnail"),
                (None, Some(l), Some(c)) => format!("/composition/layers/{l}/clips/{c}/thumbnail"),
                _ => bail!("give --id or --layer and --clip"),
            };
            if let Some(lu) = last_update {
                path.push('/');
                path.push_str(lu);
            }
            thumb(&http, &base, &path, &cli.out.join(format!("{name}.png"))).await
        }
        Cmd::Session {
            log,
            tail_ms,
            steps,
        } => session(&cli, &http, &base, log, *tail_ms, steps).await,
    }
}

async fn dump(http: &reqwest::Client, base: &str, out: &Path, prefix: &str) -> Result<()> {
    let product: Value = http
        .get(format!("{base}/product"))
        .send()
        .await?
        .json()
        .await?;
    write_json(&out.join(format!("{prefix}product.json")), &product).await?;
    let t0 = Instant::now();
    let comp: Value = http
        .get(format!("{base}/composition"))
        .send()
        .await?
        .json()
        .await?;
    let took = t0.elapsed();
    write_json(&out.join(format!("{prefix}composition.json")), &comp).await?;
    println!("product: {product}");
    println!("GET /composition took {took:?}");
    print!("{}", summarize(&comp));
    Ok(())
}

/// Human-readable outline of a composition: decks, columns, layers, clips.
fn summarize(comp: &Value) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    let v = |x: &Value| x.get("value").cloned().unwrap_or(Value::Null);
    let _ = writeln!(s, "composition name: {}", v(&comp["name"]));
    for d in comp["decks"].as_array().into_iter().flatten() {
        let _ = writeln!(
            s,
            "deck id={} name={} selected={} closed={}",
            d["id"],
            v(&d["name"]),
            v(&d["selected"]),
            d["closed"]
        );
    }
    let cols = comp["columns"].as_array().map_or(0, Vec::len);
    let _ = writeln!(s, "columns: {cols}");
    for (li, l) in comp["layers"].as_array().into_iter().flatten().enumerate() {
        let _ = writeln!(
            s,
            "layer {} id={} name={} master={} opacity={}",
            li + 1,
            l["id"],
            v(&l["name"]),
            v(&l["master"]),
            v(&l["video"]["opacity"])
        );
        for (ci, c) in l["clips"].as_array().into_iter().flatten().enumerate() {
            if v(&c["connected"]) == "Empty" {
                continue;
            }
            let _ = writeln!(
                s,
                "  clip {} id={} name={} connected={} thumb.last_update={} file={}",
                ci + 1,
                c["id"],
                v(&c["name"]),
                v(&c["connected"]),
                c["thumbnail"]["last_update"],
                c["video"]["fileinfo"]["path"]
            );
        }
    }
    s
}

async fn thumb(http: &reqwest::Client, base: &str, path: &str, dest: &Path) -> Result<()> {
    let resp = http.get(format!("{base}{path}")).send().await?;
    let status = resp.status();
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("-")
        .to_owned();
    let headers: Vec<String> = resp
        .headers()
        .iter()
        .map(|(k, v)| format!("{k}: {}", v.to_str().unwrap_or("?")))
        .collect();
    let body = resp.bytes().await?;
    println!("GET {path} -> {status} {ctype} {} bytes", body.len());
    println!("headers: {}", headers.join(" | "));
    if status.is_success() {
        tokio::fs::write(dest, &body).await?;
        println!("saved {}", dest.display());
    } else {
        println!("body: {}", String::from_utf8_lossy(&body));
    }
    Ok(())
}

/// A timestamped line for the session log.
struct LogLine(Value);

async fn session(
    cli: &Cli,
    http: &reqwest::Client,
    base: &str,
    log: &str,
    tail_ms: u64,
    steps: &[String],
) -> Result<()> {
    let start = Instant::now();
    let t = move || start.elapsed().as_secs_f64() * 1000.0;

    let url = format!("ws://{}:{}/api/v1", cli.host, cli.rest_port);
    let (ws, resp) = tokio_tungstenite::connect_async(&url)
        .await
        .with_context(|| format!("connecting {url}"))?;
    println!("[{:8.1}] ws connected {url} ({})", t(), resp.status());
    let (mut ws_tx, mut ws_rx) = ws.split();

    let (log_tx, mut log_rx) = mpsc::unbounded_channel::<LogLine>();
    let log_path = cli.out.join(format!("{log}.jsonl"));
    let writer = tokio::spawn(async move {
        let mut f = tokio::fs::File::create(&log_path).await?;
        while let Some(LogLine(v)) = log_rx.recv().await {
            f.write_all(format!("{v}\n").as_bytes()).await?;
        }
        f.flush().await?;
        anyhow::Ok(())
    });

    // Every received text frame is broadcast so `until` steps can wait for it.
    let (seen_tx, _) = broadcast::channel::<Arc<String>>(4096);
    let reader = {
        let log_tx = log_tx.clone();
        let seen_tx = seen_tx.clone();
        let out = cli.out.clone();
        let log = log.to_owned();
        tokio::spawn(async move {
            let mut big_saved: std::collections::HashMap<String, usize> = Default::default();
            while let Some(msg) = ws_rx.next().await {
                let now = t();
                let text = match msg {
                    Ok(Message::Text(s)) => s,
                    Ok(Message::Binary(b)) => {
                        println!("[{now:8.1}] recv binary {} bytes", b.len());
                        continue;
                    }
                    Ok(Message::Close(c)) => {
                        println!("[{now:8.1}] recv close {c:?}");
                        break;
                    }
                    Ok(_) => continue,
                    Err(e) => {
                        println!("[{now:8.1}] ws error {e}");
                        break;
                    }
                };
                let parsed: Option<Value> = serde_json::from_str(&text).ok();
                let kind = parsed
                    .as_ref()
                    .map(classify)
                    .unwrap_or_else(|| "non-json".into());
                if text.len() > BIG_MESSAGE {
                    let n = big_saved.entry(kind.clone()).or_default();
                    let mut saved = Value::Null;
                    if *n < BIG_SAVES_PER_TYPE {
                        *n += 1;
                        let p = out.join(format!("{log}.{kind}.{n}.json"));
                        if let Some(v) = &parsed {
                            let _ = write_json(&p, v).await;
                        }
                        saved = json!(p.file_name().map(|f| f.to_string_lossy().into_owned()));
                    }
                    let keys: Vec<String> = parsed
                        .as_ref()
                        .and_then(Value::as_object)
                        .map(|o| o.keys().take(12).cloned().collect())
                        .unwrap_or_default();
                    println!(
                        "[{now:8.1}] recv BIG {kind} {} bytes keys={keys:?}",
                        text.len()
                    );
                    let _ = log_tx.send(LogLine(json!({
                        "t_ms": now, "dir": "recv", "kind": kind, "len": text.len(),
                        "keys": keys, "saved_as": saved,
                    })));
                } else {
                    println!("[{now:8.1}] recv {}", short(&text, 400));
                    let _ = log_tx.send(LogLine(json!({
                        "t_ms": now, "dir": "recv", "kind": kind,
                        "msg": parsed.unwrap_or(Value::String(text.clone())),
                    })));
                }
                let _ = seen_tx.send(Arc::new(text));
            }
        })
    };

    let osc = tokio::net::UdpSocket::bind("0.0.0.0:0").await?;
    let osc_target = format!("{}:{}", cli.host, cli.osc_port);

    for step in steps {
        let (cmd, rest) = step.split_once(' ').unwrap_or((step.as_str(), ""));
        let rest = rest.trim();
        let mark = |what: Value| {
            let _ = log_tx.send(LogLine(
                json!({"t_ms": t(), "dir": "step", "step": step, "info": what}),
            ));
        };
        match cmd {
            "wait" => tokio::time::sleep(Duration::from_millis(rest.parse()?)).await,
            "mark" => {
                println!("[{:8.1}] ---- {rest}", t());
                mark(Value::Null);
            }
            "sub" | "ws" => {
                let text = if cmd == "sub" {
                    json!({"action": "subscribe", "parameter": rest}).to_string()
                } else {
                    rest.to_owned()
                };
                println!("[{:8.1}] send ws {text}", t());
                mark(json!({"sent": text}));
                ws_tx.send(Message::Text(text)).await?;
            }
            "osc" => {
                let mut parts = rest.split_whitespace();
                let addr = parts
                    .next()
                    .ok_or_else(|| anyhow!("osc needs an address"))?;
                let args = parts.map(osc_arg).collect::<Result<Vec<_>>>()?;
                let packet = rosc::OscPacket::Message(rosc::OscMessage {
                    addr: addr.to_owned(),
                    args: args.clone(),
                });
                let bytes = rosc::encoder::encode(&packet)?;
                osc.send_to(&bytes, &osc_target).await?;
                println!("[{:8.1}] send osc {addr} {args:?} -> {osc_target}", t());
                mark(json!({"osc": addr, "args": format!("{args:?}")}));
            }
            "rest" | "restjson" => {
                let mut parts = rest.splitn(3, ' ');
                let method = parts.next().unwrap_or("GET");
                let path = parts.next().ok_or_else(|| anyhow!("rest needs a path"))?;
                let body = parts.next();
                let mut req = http.request(method.parse()?, format!("{base}{path}"));
                if let Some(b) = body {
                    let ctype = if cmd == "restjson" {
                        "application/json"
                    } else {
                        "text/plain"
                    };
                    req = req
                        .header(reqwest::header::CONTENT_TYPE, ctype)
                        .body(b.to_owned());
                }
                let sent = t();
                let resp = req.send().await?;
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                let took = t() - sent;
                println!(
                    "[{:8.1}] rest {method} {path} -> {status} in {took:.1} ms {}",
                    t(),
                    short(&text, 300)
                );
                mark(
                    json!({"status": status.as_u16(), "took_ms": took, "body": short(&text, 2000)}),
                );
            }
            "thumbpost" => {
                let (id, file) = rest
                    .split_once(' ')
                    .ok_or_else(|| anyhow!("thumbpost <clip-id> <file>"))?;
                let bytes = tokio::fs::read(file).await?;
                let part = reqwest::multipart::Part::bytes(bytes)
                    .file_name("thumb.png")
                    .mime_str("image/png")?;
                let form = reqwest::multipart::Form::new().part("file", part);
                let resp = http
                    .post(format!("{base}/composition/clips/by-id/{id}/thumbnail"))
                    .multipart(form)
                    .send()
                    .await?;
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                println!(
                    "[{:8.1}] thumbpost {id} -> {status} {}",
                    t(),
                    short(&text, 300)
                );
                mark(json!({"status": status.as_u16(), "body": text}));
            }
            "until" => {
                let (needle, timeout) = rest
                    .rsplit_once(' ')
                    .ok_or_else(|| anyhow!("until <substring> <timeout-ms>"))?;
                let mut rx = seen_tx.subscribe();
                let from = t();
                let deadline = Duration::from_millis(timeout.parse()?);
                let found = tokio::time::timeout(deadline, async {
                    loop {
                        match rx.recv().await {
                            // Composition pushes contain every state string; never match them.
                            Ok(m) if m.len() <= BIG_MESSAGE && m.contains(needle) => return true,
                            Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                            Err(_) => return false,
                        }
                    }
                })
                .await
                .unwrap_or(false);
                let waited = t() - from;
                println!(
                    "[{:8.1}] until {needle:?}: {} after {waited:.1} ms",
                    t(),
                    if found { "seen" } else { "TIMEOUT" }
                );
                mark(json!({"found": found, "waited_ms": waited}));
            }
            other => bail!("unknown step {other:?}"),
        }
    }

    tokio::time::sleep(Duration::from_millis(tail_ms)).await;
    let _ = ws_tx.send(Message::Close(None)).await;
    reader.abort();
    drop(log_tx);
    writer.await??;
    Ok(())
}

/// Short label for a WebSocket message, used in log lines and sample file names.
fn classify(v: &Value) -> String {
    if let Some(t) = v.get("type").and_then(Value::as_str) {
        return t.to_owned();
    }
    if v.get("decks").is_some() && v.get("layers").is_some() {
        return "composition".into();
    }
    if v.get("error").is_some() {
        return "response".into();
    }
    "unknown".into()
}

fn osc_arg(s: &str) -> Result<rosc::OscType> {
    Ok(match s {
        "T" => rosc::OscType::Bool(true),
        "F" => rosc::OscType::Bool(false),
        _ => match s.split_once(':') {
            Some(("i", n)) => rosc::OscType::Int(n.parse()?),
            Some(("f", n)) => rosc::OscType::Float(n.parse()?),
            Some(("s", x)) => rosc::OscType::String(x.to_owned()),
            _ => bail!("bad osc arg {s:?} (use i:1 f:0.5 s:text T F)"),
        },
    })
}

fn short(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (+{} bytes)", &s[..end], s.len() - end)
}

async fn write_json(path: &Path, v: &Value) -> Result<()> {
    tokio::fs::write(path, serde_json::to_string_pretty(v)?).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_by_type_field_and_shape() {
        assert_eq!(
            classify(&json!({"type": "parameter_update"})),
            "parameter_update"
        );
        assert_eq!(classify(&json!({"decks": [], "layers": []})), "composition");
        assert_eq!(classify(&json!({"id": "x", "error": null})), "response");
    }

    #[test]
    fn osc_args_parse() {
        assert_eq!(osc_arg("i:1").unwrap(), rosc::OscType::Int(1));
        assert_eq!(osc_arg("f:0.5").unwrap(), rosc::OscType::Float(0.5));
        assert_eq!(osc_arg("T").unwrap(), rosc::OscType::Bool(true));
        assert!(osc_arg("1").is_err());
    }

    #[test]
    fn short_respects_char_boundaries() {
        assert_eq!(short("abc", 5), "abc");
        assert!(short("§§§§", 3).starts_with('§'));
    }

    #[test]
    fn summary_skips_empty_clips() {
        let comp = json!({
            "name": {"value": "dev"},
            "decks": [{"id": 1, "name": {"value": "A"}, "selected": {"value": true}, "closed": false}],
            "columns": [{}],
            "layers": [{"id": 2, "name": {"value": "BG"}, "master": {"value": 1.0},
                        "video": {"opacity": {"value": 1.0}},
                        "clips": [{"id": 3, "name": {"value": ""}, "connected": {"value": "Empty"}},
                                  {"id": 4, "name": {"value": "Tunnel"}, "connected": {"value": "Disconnected"},
                                   "thumbnail": {"last_update": "5"},
                                   "video": {"fileinfo": {"path": "t.mov"}}}]}]
        });
        let s = summarize(&comp);
        assert!(s.contains("clip 2 id=4"));
        assert!(!s.contains("clip 1 id=3"));
    }
}
