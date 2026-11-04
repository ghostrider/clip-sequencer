//! Engine against the mock Resolume with an injected clock (SPEC §7).

use clap::Parser;
use std::time::{Duration, Instant};

use clock_in::{ClockEvent, ClockMsg};
use mock_resolume::{start as start_mock, Mock, MockConfig};
use resolume_client::ResolumeAddr;
use seq_app::app::{self, App, AppConfig, ClockSource};
use seq_app::engine::view::{RuntimeState, StructureState};
use seq_app::engine::{EngineCommand, EngineConfig};
use seq_app::saver::DebounceConfig;
use seq_app::storage::Storage;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

const TIME_TRAVEL: &str = "Clip 07";
const TWISTED: &str = "Clip 13";
const LIQUID: &str = "Clip 08";
const DECK1_FLUID: &str = "Clip 01";

const SEQ_A: Uuid = Uuid::from_u128(0xA);
const SEQ_B: Uuid = Uuid::from_u128(0xB);
const SEQ_C: Uuid = Uuid::from_u128(0xC);

struct Harness {
    mock: Mock,
    app: App,
    clock: mpsc::UnboundedSender<ClockEvent>,
    dir: tempfile::TempDir,
}

fn step(name: &str, length: u32) -> serde_json::Value {
    json!({"clip": {"name": name}, "length": length})
}

fn sequence(
    id: Uuid,
    name: &str,
    repeat: serde_json::Value,
    lanes: serde_json::Value,
) -> serde_json::Value {
    json!({"id": id, "name": name, "start": "immediate", "repeat": repeat, "after_last_pass": "clear", "lanes": lanes})
}

async fn harness() -> Harness {
    let mock = start_mock(MockConfig::default()).await.unwrap();
    // Unique layer names (unnamed layers are all "Layer #"), and a § clip on SEQ column 1.
    mock.edit(|v| {
        for (i, n) in ["L1", "L2", "L3"].iter().enumerate() {
            v["layers"][i]["name"]["value"] = (*n).into();
        }
        let c = &mut v["layers"][3]["clips"][0];
        c["name"]["value"] = "§ Drop".into();
        c["connected"]["value"] = "Disconnected".into();
        c["video"] = json!({"fileinfo": null, "description": "Blank"});
    });
    let dir = tempfile::tempdir().unwrap();
    let file = json!({
        "version": 1,
        "composition": "dev",
        "decks": {
            "2132323": {"sequences": [
                sequence(SEQ_A, "Drop", json!({"mode": "once"}), json!([
                    {"layer": "L2", "end": "loop_to_end", "steps": [step(TWISTED, 8), step(LIQUID, 8)]},
                    {"layer": "L1", "end": "hold", "steps": [step(TIME_TRAVEL, 16)]}
                ])),
                sequence(SEQ_B, "Loop", json!({"mode": "infinite"}), json!([
                    {"layer": "L2", "end": "loop_to_end", "steps": [step(TWISTED, 1), step(LIQUID, 1)]}
                ]))
            ]},
            "1000A Text3D": {"sequences": [
                sequence(SEQ_C, "Other deck", json!({"mode": "once"}), json!([
                    {"layer": "L1", "end": "hold", "steps": [step(DECK1_FLUID, 8)]}
                ]))
            ]}
        }
    });
    std::fs::write(
        dir.path().join("dev.seq.json"),
        serde_json::to_string_pretty(&file).unwrap(),
    )
    .unwrap();

    let (app, clock) = start_app(&mock, dir.path());
    let h = Harness {
        mock,
        app,
        clock,
        dir,
    };
    h.structure(|s| s.decks.iter().any(|d| !d.sequences.is_empty()))
        .await;
    h.runtime(|r| r.ws_synced).await;
    h
}

fn start_app(mock: &Mock, dir: &std::path::Path) -> (App, mpsc::UnboundedSender<ClockEvent>) {
    start_app_ui(mock, dir, None)
}

fn start_app_ui(
    mock: &Mock,
    dir: &std::path::Path,
    ui: Option<std::net::SocketAddr>,
) -> (App, mpsc::UnboundedSender<ClockEvent>) {
    let (clock, clock_rx) = mpsc::unbounded_channel();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig {
            quiet: Duration::from_millis(50),
            max_wait: Duration::from_millis(200),
        },
        ui: ui.map(|addr| std::net::TcpListener::bind(addr).unwrap()),
        shell: None,
        quit: None,
        ffgl_dir: None,
    })
    .unwrap();
    (app, clock)
}

impl Harness {
    async fn structure(&self, pred: impl Fn(&StructureState) -> bool) -> StructureState {
        let mut rx = self.app.outputs.structure.clone();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                {
                    let s = rx.borrow_and_update();
                    if pred(&s) {
                        return s.clone();
                    }
                }
                rx.changed().await.unwrap();
            }
        })
        .await
        .expect("structure state never matched")
    }

    async fn runtime(&self, pred: impl Fn(&RuntimeState) -> bool) -> RuntimeState {
        let mut rx = self.app.outputs.runtime.clone();
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                {
                    let s = rx.borrow_and_update();
                    if pred(&s) {
                        return s.clone();
                    }
                }
                rx.changed().await.unwrap();
            }
        })
        .await
        .expect("runtime state never matched")
    }

    fn ticks(&self, n: u64) {
        for _ in 0..n {
            self.clock
                .send(ClockEvent::Msg {
                    msg: ClockMsg::Tick,
                    at: Instant::now(),
                })
                .unwrap();
        }
    }

    fn cmd(&self, c: EngineCommand) {
        self.app.commands.send(c).unwrap();
    }

    /// Waits until the mock's OSC log has at least `n` entries, returns (addr, args) pairs.
    async fn osc(&self, n: usize) -> Vec<(String, Vec<i32>)> {
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let log = self.mock.osc_log();
            if log.len() >= n || Instant::now() > deadline {
                return log.into_iter().map(|r| (r.addr, r.args)).collect();
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}

fn connect(l: usize, c: usize) -> (String, Vec<i32>) {
    (
        format!("/composition/layers/{l}/clips/{c}/connect"),
        vec![1],
    )
}

fn clear(l: usize) -> [(String, Vec<i32>); 2] {
    let a = format!("/composition/layers/{l}/clear");
    [(a.clone(), vec![1]), (a, vec![0])]
}

#[tokio::test]
async fn two_lane_sequence_plays_on_exact_ticks_and_is_confirmed() {
    let h = harness().await;
    h.cmd(EngineCommand::Start { seq: SEQ_A });
    h.runtime(|r| r.running.len() == 1).await;
    assert_eq!(h.osc(2).await, vec![connect(2, 1), connect(1, 2)]);

    h.ticks(95);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(h.mock.osc_log().len(), 2, "nothing before tick 96");
    h.ticks(1);
    assert_eq!(h.osc(3).await[2], connect(2, 3));

    h.ticks(96);
    let log = h.osc(7).await;
    let [c1, c2] = clear(2);
    let [c3, c4] = clear(1);
    assert_eq!(
        log[3..].to_vec(),
        vec![c1, c2, c3, c4],
        "after_last_pass clear on both lanes"
    );
    let r = h
        .runtime(|r| r.running.is_empty() && r.last_confirm.is_some())
        .await;
    assert!(r.notices.iter().all(|n| !n.contains("not confirmed")));
    assert_eq!(h.mock.clip_state(2, 3).as_deref(), Some("Disconnected"));

    // Ids found by name were written back to the file.
    h.runtime(|r| r.saved > 0).await;
    let text = std::fs::read_to_string(h.dir.path().join("dev.seq.json")).unwrap();
    let saved = seq_core::from_json(&text).unwrap();
    let (_, s) = saved.find_sequence(SEQ_A).unwrap();
    assert_eq!(
        s.lanes[0].steps[0].clip.as_ref().unwrap().id,
        Some(1792956592496)
    );
    h.app.shutdown().await;
}

#[tokio::test]
async fn seq_clip_starts_its_sequence_and_finishing_clears_the_seq_layer() {
    let h = harness().await;
    let s = h.structure(|s| s.decks.iter().any(|d| d.live)).await;
    let live = s.decks.iter().find(|d| d.live).unwrap();
    let drop = live
        .sequences
        .iter()
        .find(|v| v.sequence.name == "Drop")
        .unwrap();
    assert_eq!(
        drop.binding.map(|b| b.column),
        Some(1),
        "§ Drop sits in SEQ column 1"
    );
    assert!(drop.resolution.as_ref().unwrap().complete);

    h.mock.manual_connect(4, 1); // the user clicks "§ Drop" in Resolume
    let r = h.runtime(|r| r.running.len() == 1).await;
    assert!(r.running[0].from_seq_clip);
    h.ticks(192);
    h.runtime(|r| r.running.is_empty()).await;
    let log = h.osc(9).await;
    let [s1, s2] = clear(4);
    assert!(
        log.ends_with(&[s1, s2]),
        "SEQ layer cleared after finishing: {log:?}"
    );
    assert_eq!(h.mock.clip_state(4, 1).as_deref(), Some("Disconnected"));
    h.app.shutdown().await;
}

#[tokio::test]
async fn disconnecting_the_seq_clip_stops_the_sequence() {
    let h = harness().await;
    h.mock.manual_connect(4, 1);
    h.runtime(|r| r.running.len() == 1).await;
    h.ticks(10);
    h.mock.manual_connect(4, 2); // empty slot: nothing happens
                                 // Clearing the SEQ layer by hand disconnects § Drop.
    resolume_client::osc::OscSender::new(&h.mock.osc_addr.to_string())
        .unwrap()
        .clear_layer(4)
        .unwrap();
    h.runtime(|r| r.running.is_empty()).await;
    h.app.shutdown().await;
}

#[tokio::test]
async fn manual_trigger_on_an_owned_layer_yields_the_lane() {
    let h = harness().await;
    h.cmd(EngineCommand::Start { seq: SEQ_B });
    h.runtime(|r| r.running.len() == 1).await;
    tokio::time::sleep(Duration::from_millis(150)).await; // outside the 100 ms window
    h.mock.manual_connect(2, 3);
    let r = h
        .runtime(|r| {
            r.running
                .first()
                .is_some_and(|s| s.lanes[0].state == "yielded")
        })
        .await;
    assert!(
        r.notices.iter().any(|n| n.contains("L2 yielded")),
        "{:?}",
        r.notices
    );
    h.mock.clear_osc_log();
    h.ticks(48);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(h.mock.osc_log().is_empty(), "yielded lane must not trigger");

    h.cmd(EngineCommand::ResumeLane {
        seq: SEQ_B,
        lane: 0,
    });
    h.runtime(|r| r.running[0].lanes[0].state == "resume_pending")
        .await;
    // Position 48 + 12 = 60: step 2 of the 1-eighth loop (Liquid, column 3).
    h.ticks(12);
    assert_eq!(h.osc(1).await, vec![connect(2, 3)]);
    h.runtime(|r| r.running[0].lanes[0].state == "playing")
        .await;
    h.app.shutdown().await;
}

/// Seen live in M12 Phase G: Resolume confirmed a trigger after ~700 ms. The step is "not
/// confirmed" at 250 ms, but the late `connected` is still ours, not a manual trigger.
#[tokio::test]
async fn a_late_confirmation_does_not_yield_the_lane() {
    let h = harness().await;
    h.mock.set_osc_delay(Duration::from_millis(400));
    h.cmd(EngineCommand::Start { seq: SEQ_B });
    h.runtime(|r| {
        r.running
            .first()
            .is_some_and(|s| s.lanes[0].not_confirmed == vec![0])
    })
    .await;
    let r = h
        .runtime(|r| {
            r.last_confirm
                .as_deref()
                .is_some_and(|c| c.contains("after 4"))
        })
        .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let r = {
        let now = h.app.outputs.runtime.borrow().clone();
        assert_eq!(now.confirmations, r.confirmations);
        now
    };
    assert_eq!(r.running[0].lanes[0].state, "playing", "{:?}", r.notices);
    assert!(r.running[0].lanes[0].not_confirmed.is_empty());
    assert!(
        !r.notices.iter().any(|n| n.contains("yielded")),
        "{:?}",
        r.notices
    );

    // A real click on the layer still yields it.
    h.mock.set_osc_delay(Duration::ZERO);
    tokio::time::sleep(Duration::from_millis(150)).await;
    h.mock.manual_connect(2, 3);
    h.runtime(|r| r.running[0].lanes[0].state == "yielded")
        .await;
    h.app.shutdown().await;
}

#[tokio::test]
async fn starting_a_sequence_of_another_deck_switches_first() {
    let h = harness().await;
    h.cmd(EngineCommand::Start { seq: SEQ_A });
    h.runtime(|r| r.running.len() == 1).await;
    h.osc(2).await; // the start triggers have reached the mock
    h.mock.clear_osc_log();

    h.cmd(EngineCommand::Start { seq: SEQ_C });
    let r = h
        .runtime(|r| r.running.first().is_some_and(|s| s.seq == SEQ_C))
        .await;
    assert_eq!(r.running[0].deck.as_deref(), Some("1000A Text3D"));
    assert_eq!(h.mock.live_deck().as_deref(), Some("1000A Text3D"));
    let log = h.osc(6).await;
    // Drop A stopped with its after_last_pass clears, then deck select, then the new trigger.
    let [a, b] = clear(2);
    let [c, d] = clear(1);
    assert_eq!(
        log,
        vec![
            a,
            b,
            c,
            d,
            ("/composition/decks/1/select".into(), vec![1]),
            connect(1, 1)
        ]
    );
    h.app.shutdown().await;
}

#[tokio::test]
async fn deck_changed_in_resolume_stops_sequences_without_clearing() {
    let h = harness().await;
    h.cmd(EngineCommand::Start { seq: SEQ_B });
    h.runtime(|r| r.running.len() == 1).await;
    h.osc(1).await; // the start trigger has reached the mock
    h.mock.clear_osc_log();
    resolume_client::rest::RestClient::new(format!("http://{}/api/v1", h.mock.rest_addr))
        .select_deck(1)
        .await
        .unwrap();
    let r = h.runtime(|r| r.running.is_empty()).await;
    assert!(r.notices.iter().any(|n| n.contains("deck changed")));
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        h.mock.osc_log().is_empty(),
        "no clears on a foreign deck change"
    );
    let s = h
        .structure(|s| s.live_deck.as_deref() == Some("1000A Text3D"))
        .await;
    let cached = s.decks.iter().find(|d| d.name == "2132323").unwrap();
    assert_eq!(
        cached.clips_source,
        seq_app::engine::view::ClipsSource::Cached
    );
    h.app.shutdown().await;
}

#[tokio::test]
async fn edits_are_validated_saved_and_undoable() {
    use seq_core::edit::EditOp;
    let h = harness().await;
    let has = |s: &StructureState, name: &str| {
        s.decks
            .iter()
            .any(|d| d.sequences.iter().any(|v| v.sequence.name == name))
    };
    h.cmd(EngineCommand::Edit {
        op: EditOp::RenameSequence {
            seq: SEQ_A,
            name: "Drop Renamed".into(),
        },
    });
    h.structure(|s| s.can_undo && has(s, "Drop Renamed")).await;
    // Invalid: duplicate name in the deck.
    h.cmd(EngineCommand::Edit {
        op: EditOp::RenameSequence {
            seq: SEQ_B,
            name: "Drop Renamed".into(),
        },
    });
    h.runtime(|r| r.notices.iter().any(|n| n.contains("edit rejected")))
        .await;
    h.cmd(EngineCommand::Undo);
    h.structure(|s| s.can_redo && has(s, "Drop")).await;
    h.cmd(EngineCommand::Redo);
    h.structure(|s| !s.can_redo && has(s, "Drop Renamed")).await;
    h.runtime(|r| r.saved > 0).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let saved =
        seq_core::from_json(&std::fs::read_to_string(h.dir.path().join("dev.seq.json")).unwrap())
            .unwrap();
    assert_eq!(saved.find_sequence(SEQ_A).unwrap().1.name, "Drop Renamed");
    h.app.shutdown().await;
}

#[tokio::test]
async fn editing_a_running_sequence_marks_it_edited() {
    use seq_core::edit::EditOp;
    let h = harness().await;
    h.cmd(EngineCommand::Start { seq: SEQ_B });
    h.runtime(|r| r.running.len() == 1 && !r.running[0].edited)
        .await;
    h.cmd(EngineCommand::Edit {
        op: EditOp::SetStepLength {
            seq: SEQ_B,
            lane: 0,
            index: 0,
            length: 4,
        },
    });
    h.runtime(|r| r.running.first().is_some_and(|s| s.edited))
        .await;
    h.app.shutdown().await;
}

#[tokio::test]
async fn unmatched_seq_clip_is_reported() {
    let h = harness().await;
    h.mock.edit(|v| {
        let c = &mut v["layers"][3]["clips"][1];
        c["name"]["value"] = "§ Nope".into();
        c["connected"]["value"] = "Disconnected".into();
        c["video"] = json!({"fileinfo": null});
    });
    let s = h
        .structure(|s| {
            s.decks
                .iter()
                .any(|d| d.live && !d.unmatched_seq_clips.is_empty())
        })
        .await;
    assert_eq!(
        s.decks.iter().find(|d| d.live).unwrap().unmatched_seq_clips,
        vec!["Nope"]
    );
    h.mock.manual_connect(4, 2);
    h.runtime(|r| r.notices.iter().any(|n| n.contains("matches no sequence")))
        .await;
    h.app.shutdown().await;
}

#[tokio::test]
async fn deck_clip_cache_survives_a_restart() {
    use seq_app::engine::view::ClipsSource;
    let h = harness().await;
    // Make deck 1 live once, then go back: both decks are now cached on disk.
    let rest =
        resolume_client::rest::RestClient::new(format!("http://{}/api/v1", h.mock.rest_addr));
    rest.select_deck(1).await.unwrap();
    h.structure(|s| s.live_deck.as_deref() == Some("1000A Text3D"))
        .await;
    rest.select_deck(2).await.unwrap();
    h.structure(|s| s.live_deck.as_deref() == Some("2132323"))
        .await;
    let cache = h.dir.path().join("decks").join("dev");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !(cache.join("1000A Text3D.json").exists() && cache.join("2132323.json").exists()) {
        assert!(Instant::now() < deadline, "deck cache files not written");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let Harness { mock, app, dir, .. } = h;
    app.shutdown().await;

    // A fresh app knows deck 1's clips before it is ever live in this session.
    let (app2, _clock) = start_app(&mock, dir.path());
    let mut rx = app2.outputs.structure.clone();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let ok = rx.borrow_and_update().decks.iter().any(|d| {
                d.name == "1000A Text3D"
                    && d.clips_source == ClipsSource::Cached
                    && !d.layers.is_empty()
            });
            if ok {
                break;
            }
            rx.changed().await.unwrap();
        }
    })
    .await
    .expect("cached deck not offered after restart");
    let s = rx.borrow().clone();
    let deck1 = s.decks.iter().find(|d| d.name == "1000A Text3D").unwrap();
    let intro = &deck1.sequences[0];
    assert!(
        intro.resolution.as_ref().unwrap().complete,
        "resolved against the cached clips"
    );
    app2.shutdown().await;
}

#[tokio::test]
async fn ui_server_protocol_thumbnails_and_push() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    let h = harness().await;
    let Harness { mock, app, dir, .. } = h;
    app.shutdown().await;
    let (app, _clock) = start_app_ui(&mock, dir.path(), Some("127.0.0.1:0".parse().unwrap()));
    let ui = app.ui_addr.unwrap();

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{ui}/ws"))
        .await
        .unwrap();
    let mut types = Vec::new();
    while types.len() < 2 {
        match tokio::time::timeout(Duration::from_secs(3), ws.next())
            .await
            .expect("ws message")
        {
            Some(Ok(Message::Text(t))) => {
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                types.push(v["type"].as_str().unwrap().to_owned());
            }
            Some(Ok(_)) => continue,
            other => panic!("ws ended: {other:?}"),
        }
    }
    assert_eq!(types, ["structure", "runtime"], "full snapshots on connect");

    // Wait until the structure knows the § Drop binding, then push its thumbnail.
    let drop_clip = loop {
        let s = app.outputs.structure.borrow().clone();
        if let Some(b) = s
            .decks
            .iter()
            .find(|d| d.live)
            .and_then(|d| d.sequences.iter().find(|v| v.sequence.id == SEQ_A))
            .and_then(|v| v.binding)
        {
            break b.clip_id;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    ws.send(Message::Text(
        json!({"cmd":"push_thumbnail","seq": SEQ_A}).to_string(),
    ))
    .await
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let png = loop {
        if let Some(p) = mock
            .shared
            .state
            .lock()
            .unwrap()
            .custom_thumbnails
            .get(&drop_clip)
            .cloned()
        {
            break p;
        }
        assert!(Instant::now() < deadline, "thumbnail never pushed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    let img = image::load_from_memory(&png).unwrap();
    assert_eq!((img.width(), img.height()), (320, 240));

    // Commands over the socket reach the engine.
    ws.send(Message::Text(
        json!({"cmd":"start","seq": SEQ_B}).to_string(),
    ))
    .await
    .unwrap();
    let mut rt = app.outputs.runtime.clone();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !rt
            .borrow_and_update()
            .running
            .iter()
            .any(|r| r.seq == SEQ_B)
        {
            rt.changed().await.unwrap();
        }
    })
    .await
    .unwrap();

    // HTTP: clip thumbnail, sequence mosaic, static page.
    let http = reqwest::Client::new();
    let r = http
        .get(format!("http://{ui}/thumbs/clip/1792956592496.png"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert!(r
        .bytes()
        .await
        .unwrap()
        .starts_with(&[0x89, 0x50, 0x4E, 0x47]));
    let r = http
        .get(format!("http://{ui}/thumbs/seq/{SEQ_A}.png"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.headers()["content-type"], "image/png");
    // The plugin endpoint: layout with a revision, runtime while running.
    let view = |q: String| {
        let http = http.clone();
        async move {
            http.get(format!("http://{ui}/plugin/v1/view.txt?{q}"))
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap()
        }
    };
    let t = view("name=Loop".into()).await;
    assert!(
        t.starts_with(
            "SEQVIEW 2
status ok
rev "
        ),
        "{t}"
    );
    assert!(
        t.contains(
            "
layout
name Loop
deck 2132323
pass 4
lane ok loop_to_end L2
step clip 2 1792956592496 "
        ),
        "{t}"
    );
    assert!(
        t.contains(
            "
run 1 "
        ),
        "Loop is running: {t}"
    );
    assert!(
        t.contains(
            "
lanestate 0 playing "
        ),
        "{t}"
    );
    let rev = t
        .lines()
        .find_map(|l| l.strip_prefix("rev "))
        .unwrap()
        .to_owned();
    let t2 = view(format!("name=Loop&rev={rev}")).await;
    assert!(
        !t2.contains(
            "
layout
"
        ),
        "unchanged layout is not resent: {t2}"
    );
    assert!(view("name=Nope".into()).await.contains("status not_found"));
    let r = http.get(format!("http://{ui}/")).send().await.unwrap();
    assert!(r
        .text()
        .await
        .unwrap()
        .to_lowercase()
        .starts_with("<!doctype html"));
    app.shutdown().await;
}

#[tokio::test]
async fn rest_stops_a_clip_the_lane_started_on_another_layer() {
    use seq_core::edit::EditOp;
    use seq_core::{ClipRef, Step};
    let h = harness().await;
    let seq = Uuid::from_u128(0xD);
    let clip = |name: &str| ClipRef {
        name: Some(name.into()),
        ..Default::default()
    };
    for op in [
        EditOp::CreateSequence {
            deck: "2132323".into(),
            id: seq,
            name: "Off layer".into(),
            layer: "L1".into(),
        },
        // TWISTED lives on L2, the lane targets L1.
        EditOp::InsertStep {
            seq,
            lane: 0,
            index: 0,
            step: Step {
                clip: Some(clip(TWISTED)),
                length: 2,
            },
        },
        EditOp::InsertStep {
            seq,
            lane: 0,
            index: 1,
            step: Step {
                clip: None,
                length: 2,
            },
        },
    ] {
        h.cmd(EngineCommand::Edit { op });
    }
    h.structure(|s| {
        s.decks.iter().any(|d| {
            d.sequences
                .iter()
                .any(|v| v.sequence.id == seq && v.sequence.lanes[0].steps.len() == 2)
        })
    })
    .await;
    h.cmd(EngineCommand::Start { seq });
    h.runtime(|r| r.running.len() == 1).await;
    assert_eq!(h.osc(1).await, vec![connect(2, 1)]);
    // Wait until Resolume reports the clip playing, then reach the rest.
    h.runtime(|r| r.last_confirm.is_some()).await;
    h.ticks(12);
    let log = h.osc(5).await;
    let [a, b] = clear(1);
    let [c, d] = clear(2);
    assert_eq!(
        log[1..].to_vec(),
        vec![a, b, c, d],
        "lane layer, then the layer the clip plays on"
    );
    assert_eq!(h.mock.clip_state(2, 1).as_deref(), Some("Disconnected"));
    h.app.shutdown().await;
}

#[tokio::test]
async fn seq_plugin_parameter_follows_the_clip_name() {
    let h = harness().await;
    let plugin_clip = |name: &str| {
        let name = name.to_owned();
        move |v: &mut serde_json::Value| {
            let c = &mut v["layers"][3]["clips"][1];
            c["name"]["value"] = name.clone().into();
            c["connected"]["value"] = "Disconnected".into();
            c["video"] = json!({"fileinfo": null, "description": "1000A Sequence", "sourceparams": {
                "Sequence": {"valuetype": "ParamString", "id": 777_000_001u64, "value": ""}}});
        }
    };
    let value = |m: &Mock| {
        m.shared.state.lock().unwrap().comp["layers"][3]["clips"][1]["video"]["sourceparams"]
            ["Sequence"]["value"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };
    let wait_value = |want: &'static str| {
        let m = &h.mock;
        async move {
            let deadline = Instant::now() + Duration::from_secs(3);
            while value(m) != want {
                assert!(
                    Instant::now() < deadline,
                    "plugin parameter is {:?}, expected {want:?}",
                    value(m)
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }
    };
    h.mock.edit(plugin_clip("§ Loop"));
    wait_value("Loop").await;
    // Renaming the clip re-binds the plugin; a clip without the prefix shows nothing.
    h.mock
        .edit(|v| v["layers"][3]["clips"][1]["name"]["value"] = "§ Drop".into());
    wait_value("Drop").await;
    h.mock
        .edit(|v| v["layers"][3]["clips"][1]["name"]["value"] = "notes".into());
    wait_value("").await;
    h.app.shutdown().await;
}

#[tokio::test]
async fn decks_with_the_same_name_are_flagged_and_only_the_selected_one_is_live() {
    let h = harness().await;
    let live = h
        .structure(|s| s.live_deck.is_some())
        .await
        .live_deck
        .unwrap();
    h.mock.edit(|v| {
        let decks = v["decks"].as_array_mut().unwrap();
        let other = decks
            .iter_mut()
            .find(|d| d["selected"]["value"] != true)
            .unwrap();
        other["name"]["value"] = live.clone().into();
    });
    let s = h
        .structure(|s| s.decks.iter().filter(|d| d.name == live).count() == 2)
        .await;
    let same: Vec<_> = s.decks.iter().filter(|d| d.name == live).collect();
    assert!(same.iter().all(|d| d.ambiguous));
    assert_eq!(same.iter().filter(|d| d.live).count(), 1);
    let selected = h.mock.shared.state.lock().unwrap().comp["decks"]
        .as_array()
        .unwrap()
        .iter()
        .position(|d| d["selected"]["value"] == true)
        .unwrap()
        + 1;
    assert_eq!(same.iter().find(|d| d.live).unwrap().index, Some(selected));
    // The other one must not show the live deck's clips through the per-name cache.
    let other = same.iter().find(|d| !d.live).unwrap();
    assert_eq!(other.clips_source, seq_app::engine::view::ClipsSource::None);
    assert!(s
        .decks
        .iter()
        .filter(|d| d.name != live)
        .all(|d| !d.ambiguous));
    h.app.shutdown().await;
}

#[tokio::test]
async fn live_deck_thumbnails_are_cached_without_a_browser() {
    let h = harness().await;
    let Harness { mock, app, dir, .. } = h;
    app.shutdown().await;
    let (app, _clock) = start_app_ui(&mock, dir.path(), Some("127.0.0.1:0".parse().unwrap()));
    let wanted: Vec<(u64, String)> = loop {
        let s = app.outputs.structure.borrow().clone();
        let w: Vec<_> = s
            .decks
            .iter()
            .filter(|d| d.live)
            .flat_map(|d| &d.layers)
            .flat_map(|l| &l.clips)
            .filter_map(|c| Some((c.id, c.thumb.clone()?)))
            .collect();
        if !w.is_empty() {
            break w;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    // No UI client asks for them: the server fetches them so the deck has thumbnails once it
    // is no longer live (Resolume then answers 404).
    let thumbs = dir.path().join("thumbs");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let missing = wanted
            .iter()
            .filter(|(id, v)| !thumbs.join(format!("{id}-{v}.png")).exists())
            .count();
        if missing == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "{missing} of {} live-deck thumbnails not cached",
            wanted.len()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    app.shutdown().await;
}

#[tokio::test]
async fn api_version_and_runtime_file_name_the_actual_ui_port() {
    let h = harness().await;
    let Harness { mock, app, dir, .. } = h;
    app.shutdown().await;
    let (app, _clock) = start_app_ui(&mock, dir.path(), Some("127.0.0.1:0".parse().unwrap()));
    let ui = app.ui_addr.unwrap();
    let v: serde_json::Value = reqwest::get(format!("http://{ui}/api/version"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["app"], seq_app::APP_ID);
    assert_eq!(v["name"], seq_app::APP_NAME);
    assert_eq!(v["version"], seq_app::VERSION);
    // Build info (M11 Phase E): the same values as `--version` and the first log line.
    assert_eq!(v["build"], seq_app::BUILD_NUMBER);
    assert_eq!(v["commit"], seq_app::GIT_COMMIT);
    assert_eq!(v["target"], seq_app::TARGET);
    let date = v["build_date"].as_str().unwrap();
    assert!(
        date.len() == 20 && date.ends_with('Z') && date.as_bytes()[10] == b'T',
        "{date}"
    );
    assert!(
        seq_app::LONG_VERSION.starts_with(&format!(
            "{} (build {}, {}, {})",
            seq_app::VERSION,
            seq_app::BUILD_NUMBER,
            seq_app::GIT_COMMIT,
            &date[..10]
        )),
        "{}",
        seq_app::LONG_VERSION
    );
    assert_eq!(v["ui_port"], ui.port());
    assert_eq!(v["desktop"], false);
    let runtime = dir.path().join("runtime.json");
    let r: serde_json::Value = serde_json::from_slice(&std::fs::read(&runtime).unwrap()).unwrap();
    assert_eq!(r["pid"], std::process::id());
    assert_eq!(r["ui_url"], format!("http://{ui}"));
    let started = r["started_at"].as_str().unwrap();
    assert!(started.len() == 20 && started.ends_with('Z'), "{started}");
    app.shutdown().await;
    assert!(
        !runtime.exists(),
        "runtime.json is removed on clean shutdown"
    );
}

#[tokio::test]
async fn shutdown_ends_running_sequences_per_after_last_pass() {
    let h = harness().await;
    h.cmd(EngineCommand::Start { seq: SEQ_B }); // "Loop" on L2, after_last_pass = clear
    h.runtime(|r| r.running.len() == 1).await;
    h.osc(1).await;
    h.mock.clear_osc_log();
    let Harness { mock, app, .. } = h;
    let started = Instant::now();
    app.shutdown().await;
    assert!(started.elapsed() < Duration::from_secs(2));
    let deadline = Instant::now() + Duration::from_secs(1);
    while mock.osc_log().len() < 2 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let log: Vec<_> = mock
        .osc_log()
        .into_iter()
        .map(|r| (r.addr, r.args))
        .collect();
    assert_eq!(log, clear(2).to_vec(), "the owned layer is cleared");
}

#[tokio::test]
async fn runner_starts_and_stops_through_its_token() {
    let mock = start_mock(MockConfig::default()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let config = seq_app::config::Config::parse_from([
        "seq-app".to_owned(),
        "--rest-port".into(),
        mock.rest_addr.port().to_string(),
        "--osc-port".into(),
        mock.osc_addr.port().to_string(),
        "--ui-port".into(),
        "0".into(),
        "--data-dir".into(),
        dir.path().display().to_string(),
    ]);
    let token = tokio_util::sync::CancellationToken::new();
    let (progress, mut steps_rx) = mpsc::unbounded_channel();
    let running = seq_app::runner::run(config, token.clone(), None, Some(progress))
        .await
        .unwrap();
    let mut steps = Vec::new();
    while let Some(s) = steps_rx.recv().await {
        steps.push(s);
    }
    let indices: Vec<u8> = steps.iter().map(|s| s.index).collect();
    assert!(indices.windows(2).all(|w| w[0] <= w[1]), "{indices:?}");
    assert_eq!(indices.first(), Some(&1));
    let last = steps.last().unwrap();
    assert_eq!(
        (last.index, last.text.as_str()),
        (seq_app::runner::STEPS, "Ready")
    );
    let warnings: Vec<&str> = steps
        .iter()
        .filter(|s| s.warning)
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "only the missing MIDI port: {warnings:?}"
    );
    assert!(warnings[0].contains("No MIDI clock port"));
    assert!(steps
        .iter()
        .any(|s| s.text.starts_with("Loaded composition \"dev\"")));
    let v: serde_json::Value = reqwest::get(format!("{}/api/version", running.ui_url))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["app"], seq_app::APP_ID);
    assert_eq!(v["build"], seq_app::BUILD_NUMBER);
    assert!(dir.path().join("runtime.json").exists());
    let started = Instant::now();
    token.cancel();
    tokio::time::timeout(Duration::from_secs(3), running.done)
        .await
        .expect("shutdown completes")
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert!(!dir.path().join("runtime.json").exists());
}

#[tokio::test]
async fn ui_shell_actions_reach_the_desktop_shell() {
    use futures_util::SinkExt;
    use seq_app::server::ShellAction;
    use tokio_tungstenite::tungstenite::Message;
    let mock = start_mock(MockConfig::default()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (shell_tx, mut shell_rx) = mpsc::unbounded_channel();
    let (_clock, clock_rx) = mpsc::unbounded_channel::<ClockEvent>();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig::default(),
        ui: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        shell: Some(shell_tx),
        quit: None,
        ffgl_dir: None,
    })
    .unwrap();
    let ui = app.ui_addr.unwrap();
    let v: serde_json::Value = reqwest::get(format!("http://{ui}/api/version"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["desktop"], true, "the UI learns that a shell is there");
    let page = reqwest::get(format!("http://{ui}/")).await.unwrap();
    let csp = page.headers()["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("default-src 'self'") && csp.contains("connect-src 'self'"));
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{ui}/ws"))
        .await
        .unwrap();
    for action in ["zoom_in", "zoom_out", "zoom_reset", "quit"] {
        ws.send(Message::Text(
            json!({"cmd": "shell", "action": action}).to_string(),
        ))
        .await
        .unwrap();
    }
    let mut got = Vec::new();
    while got.len() < 4 {
        got.push(
            tokio::time::timeout(Duration::from_secs(3), shell_rx.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    assert_eq!(
        got,
        [
            ShellAction::ZoomIn,
            ShellAction::ZoomOut,
            ShellAction::ZoomReset,
            ShellAction::Quit
        ]
    );
    app.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn preferences_switch_and_save_the_midi_clock_port() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    const PORT: &str = "No Such MIDI Port (test)";
    let mock = start_mock(MockConfig::default()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::None, // a real (switchable) MIDI input, none selected yet
        debounce: DebounceConfig::default(),
        ui: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        shell: None,
        quit: None,
        ffgl_dir: None,
    })
    .unwrap();
    let ui = app.ui_addr.unwrap();
    let ports: serde_json::Value = reqwest::get(format!("http://{ui}/api/midi/ports"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        ports["ports"].is_array() && ports["selected"].is_null(),
        "{ports}"
    );

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{ui}/ws"))
        .await
        .unwrap();
    ws.send(Message::Text(
        json!({"cmd": "set_midi_port", "port": PORT}).to_string(),
    ))
    .await
    .unwrap();
    let mut rt = app.outputs.runtime.clone();
    tokio::time::timeout(
        Duration::from_secs(5),
        rt.wait_for(|r| r.clock.port.as_deref() == Some(PORT) && !r.clock.connected),
    )
    .await
    .expect("the engine follows the new port")
    .unwrap();
    assert_eq!(
        seq_app::settings::Settings::load(dir.path())
            .midi_port
            .as_deref(),
        Some(PORT),
        "saved for the next start"
    );
    app.shutdown().await;

    // The next start without --midi-port uses the saved port.
    let config = seq_app::config::Config::parse_from([
        "seq-app".to_owned(),
        "--rest-port".into(),
        mock.rest_addr.port().to_string(),
        "--osc-port".into(),
        mock.osc_addr.port().to_string(),
        "--ui-port".into(),
        "0".into(),
        "--data-dir".into(),
        dir.path().display().to_string(),
    ]);
    let (progress, mut steps_rx) = mpsc::unbounded_channel();
    let token = tokio_util::sync::CancellationToken::new();
    let running = seq_app::runner::run(config, token.clone(), None, Some(progress))
        .await
        .unwrap();
    let mut texts = Vec::new();
    while let Some(s) = steps_rx.recv().await {
        texts.push(s.text);
    }
    assert!(
        texts
            .iter()
            .any(|t| t == &format!("Opening MIDI clock input \"{PORT}\"")),
        "{texts:?}"
    );
    token.cancel();
    let _ = running.done.await;
}

/// M11 Phase F: the browser UI of the headless app can quit it (File → Quit server), and the License
/// dialog's data is served (MIT text plus the embedded third-party notices or their placeholder).
#[tokio::test]
async fn headless_ui_quits_through_the_token_and_licenses_are_served() {
    use futures_util::SinkExt;
    use tokio_tungstenite::tungstenite::Message;
    let mock = start_mock(MockConfig::default()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_clock, clock_rx) = mpsc::unbounded_channel::<ClockEvent>();
    let quit = tokio_util::sync::CancellationToken::new();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig::default(),
        ui: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        shell: None,
        quit: Some(quit.clone()),
        ffgl_dir: None,
    })
    .unwrap();
    let ui = app.ui_addr.unwrap();

    let v: serde_json::Value = reqwest::get(format!("http://{ui}/api/version"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["desktop"], false);
    assert_eq!(v["repository"], env!("CARGO_PKG_REPOSITORY"));
    assert_eq!(v["homepage"], env!("CARGO_PKG_HOMEPAGE"));
    assert_eq!(v["copyright"], seq_app::COPYRIGHT);

    let lic: serde_json::Value = reqwest::get(format!("http://{ui}/api/licenses"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let mit = lic["license"].as_str().unwrap();
    assert!(mit.starts_with("MIT License") && mit.contains(seq_app::COPYRIGHT));
    let notices = &lic["notices"];
    assert!(
        notices["packages"].is_array() && notices["texts"].is_array(),
        "{notices}"
    );
    if notices["placeholder"] != true {
        // Generated notices: every package points at texts that exist.
        let texts = notices["texts"].as_array().unwrap().len();
        for p in notices["packages"].as_array().unwrap() {
            for t in p["texts"].as_array().unwrap() {
                assert!((t.as_u64().unwrap() as usize) < texts, "{p}");
            }
        }
    }

    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{ui}/ws"))
        .await
        .unwrap();
    // The File menu's other commands are accepted: after them an unknown command (the control) must be
    // the only "bad UI message" (commands are handled in order).
    for cmd in ["save_now", "reload_composition", "no_such_command"] {
        ws.send(Message::Text(json!({ "cmd": cmd }).to_string()))
            .await
            .unwrap();
    }
    let mut rt = app.outputs.runtime.clone();
    let bad = |r: &seq_app::engine::view::RuntimeState| {
        r.notices
            .iter()
            .filter(|n| n.contains("bad UI message"))
            .count()
    };
    let r = tokio::time::timeout(
        Duration::from_secs(3),
        rt.wait_for(|r| r.notices.iter().any(|n| n.contains("no_such_command"))),
    )
    .await
    .expect("the unknown command is reported")
    .unwrap()
    .clone();
    assert_eq!(bad(&r), 1, "{:?}", r.notices);
    assert!(!quit.is_cancelled());
    ws.send(Message::Text(
        json!({"cmd": "shell", "action": "quit"}).to_string(),
    ))
    .await
    .unwrap();
    tokio::time::timeout(Duration::from_secs(3), quit.cancelled())
        .await
        .expect("quit from the UI cancels the headless app's token");
    app.shutdown().await;
}

/// SPEC §12.2: a web page from another origin can't use the UI server (HTTP or WebSocket); the app's own
/// origin and clients without an `Origin` header (the FFGL plugin, tools) can.
#[tokio::test]
async fn requests_from_other_web_pages_are_refused() {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mock = start_mock(MockConfig::default()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_clock, clock_rx) = mpsc::unbounded_channel::<ClockEvent>();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig::default(),
        ui: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        shell: None,
        quit: None,
        ffgl_dir: None,
    })
    .unwrap();
    let ui = app.ui_addr.unwrap();
    let client = reqwest::Client::new();
    let get = |origin: Option<String>| {
        let mut r = client.get(format!("http://{ui}/api/version"));
        if let Some(o) = origin {
            r = r.header("Origin", o);
        }
        r.send()
    };
    assert_eq!(
        get(None).await.unwrap().status(),
        200,
        "no Origin (plugin, tools)"
    );
    assert_eq!(
        get(Some(format!("http://{ui}"))).await.unwrap().status(),
        200,
        "the app's own page"
    );
    for evil in ["https://evil.example", "http://127.0.0.1:1", "null"] {
        assert_eq!(
            get(Some(evil.into())).await.unwrap().status(),
            403,
            "{evil}"
        );
    }

    // WebSocket: a foreign page's handshake is refused, the app's own goes through.
    let ws = |origin: String| {
        let mut req = format!("ws://{ui}/ws").into_client_request().unwrap();
        req.headers_mut().insert("Origin", origin.parse().unwrap());
        tokio_tungstenite::connect_async(req)
    };
    let refused = ws("https://evil.example".into()).await;
    assert!(
        matches!(&refused, Err(tokio_tungstenite::tungstenite::Error::Http(r)) if r.status() == 403),
        "{refused:?}"
    );
    assert!(ws(format!("http://{ui}")).await.is_ok());
    app.shutdown().await;
}

/// SPEC §12.1: the install dialog's API. The test binary has no bundled plugin, so install reports that; the
/// folder is validated and saved; a POST from another web page is refused (§12.2).
#[tokio::test]
async fn ffgl_install_api() {
    let mock = start_mock(MockConfig::default()).await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (_clock, clock_rx) = mpsc::unbounded_channel::<ClockEvent>();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig::default(),
        ui: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        shell: None,
        quit: None,
        ffgl_dir: None,
    })
    .unwrap();
    let ui = app.ui_addr.unwrap();
    let client = reqwest::Client::new();
    let post = |path: &str, body: serde_json::Value| {
        client.post(format!("http://{ui}{path}")).json(&body).send()
    };

    let s: serde_json::Value = reqwest::get(format!("http://{ui}/api/ffgl"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(s["plugin_name"], "1000A Sequence");
    assert_eq!(s["dir_source"], "default");

    let r = post("/api/ffgl/dir", json!({ "dir": "relative/folder" }))
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    let v: serde_json::Value = r.json().await.unwrap();
    assert!(
        v["error"].as_str().unwrap().contains("not a full path"),
        "{v}"
    );

    let fx = dir.path().join("Extra Effects");
    let r = post("/api/ffgl/dir", json!({ "dir": fx.to_string_lossy() }))
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["status"]["dir_source"], "saved");
    assert_eq!(v["status"]["dir"], fx.to_string_lossy().as_ref());
    assert_eq!(
        seq_app::settings::Settings::load(dir.path()).ffgl_dir,
        Some(fx.clone())
    );

    let r = post("/api/ffgl/install", json!({})).await.unwrap();
    assert_eq!(r.status(), 409);
    let v: serde_json::Value = r.json().await.unwrap();
    assert!(
        v["error"].as_str().unwrap().contains("no FFGL plugin"),
        "{v}"
    );
    assert!(!fx.exists(), "nothing created without a plugin to install");

    let r = post("/api/ffgl/remove", json!({})).await.unwrap();
    assert_eq!(r.status(), 409);

    // Back to the default folder.
    let r = post("/api/ffgl/dir", json!({ "dir": null })).await.unwrap();
    let v: serde_json::Value = r.json().await.unwrap();
    assert_eq!(v["status"]["dir_source"], "default");

    // Another web page can't trigger an install.
    let r = client
        .post(format!("http://{ui}/api/ffgl/install"))
        .header("Origin", "https://evil.example")
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    app.shutdown().await;
}
