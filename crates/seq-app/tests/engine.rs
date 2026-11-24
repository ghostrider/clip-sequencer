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
use seq_app::settings::ClockSourceKind;
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
    /// Timestamps for [`Harness::ticks_at_128`].
    virtual_clock: std::cell::Cell<Instant>,
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
    harness_with(false).await
}

/// `resolume_clock`: ticks from the app's own Resolume tempo clock (SPEC §3.1) instead of the
/// injected channel.
async fn harness_with(resolume_clock: bool) -> Harness {
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

    let (app, clock) = start_app_clock(&mock, dir.path(), None, resolume_clock);
    let h = Harness {
        mock,
        app,
        clock,
        dir,
        virtual_clock: std::cell::Cell::new(Instant::now()),
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
    start_app_clock(mock, dir, ui, false)
}

fn start_app_clock(
    mock: &Mock,
    dir: &std::path::Path,
    ui: Option<std::net::SocketAddr>,
    resolume_clock: bool,
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
        clock: if resolume_clock {
            ClockSource::Resolume { midi_port: None }
        } else {
            ClockSource::Channel(clock_rx)
        },
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

#[tokio::test]
async fn lut_effects_are_listed_and_can_be_pointed_at_a_file() {
    let h = harness().await;
    let fx: serde_json::Value =
        serde_json::from_str(include_str!("../../../docs/samples/lut-effect.json")).unwrap();
    let file_param = fx["params"]["LUT 3D"]["id"].as_u64().unwrap();
    h.mock
        .edit(|v| v["layers"][1]["video"] = json!({"effects": [fx]}));
    let s = h.structure(|s| !s.lut_effects.is_empty()).await;
    assert_eq!(s.lut_effects[0].file_param, file_param);
    assert!(matches!(
        &s.lut_effects[0].place,
        resolume_client::composition::LutPlace::Layer { layer: 2, .. }
    ));

    let path = r"C:\data\luts\Night.cube".to_owned();
    h.app
        .commands
        .send(EngineCommand::SetLutFile {
            file_param,
            path: path.clone(),
        })
        .unwrap();
    h.structure(|s| s.lut_effects[0].path == path).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while h.mock.read(|v| {
            v["layers"][1]["video"]["effects"][0]["params"]["LUT 3D"]["value"] != path.as_str()
        }) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the mock never got the new path");
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

    /// Ticks stamped 128 BPM apart, so the engine's tempo (and with it the slot read
    /// look-ahead, ~8 ticks) is what it would be live.
    fn ticks_at_128(&self, n: u64) {
        let dt = Duration::from_secs_f64(60.0 / 128.0 / 24.0);
        for _ in 0..n {
            let at = self.virtual_clock.get() + dt;
            self.virtual_clock.set(at);
            self.clock
                .send(ClockEvent::Msg {
                    msg: ClockMsg::Tick,
                    at,
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
        json!({"cmd":"push_thumbnail","seq": SEQ_A})
            .to_string()
            .into(),
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
        json!({"cmd":"start","seq": SEQ_B}).to_string().into(),
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
                column: None,
                clip: Some(clip(TWISTED)),
                length: 2,
                ..Default::default()
            },
        },
        EditOp::InsertStep {
            seq,
            lane: 0,
            index: 1,
            step: Step {
                column: None,
                clip: None,
                length: 2,
                ..Default::default()
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
            json!({"cmd": "shell", "action": action}).to_string().into(),
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
        json!({"cmd": "set_midi_port", "port": PORT})
            .to_string()
            .into(),
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

const SEQ_SLOT: Uuid = Uuid::from_u128(0x51);

/// A slot lane on L2 (columns 1, 2, 3; one bar each), created through edit operations.
/// L2 in the mock: column 1 "Clip 13", column 2 empty, column 3 "Clip 08".
async fn add_slot_sequence(h: &Harness, repeat: seq_core::Repeat) {
    use seq_core::edit::EditOp;
    let ops = [
        EditOp::CreateSequence {
            deck: "2132323".into(),
            id: SEQ_SLOT,
            name: "Slots".into(),
            layer: "L2".into(),
        },
        EditOp::SetLaneMode {
            seq: SEQ_SLOT,
            lane: 0,
            mode: seq_core::LaneMode::Slot,
        },
        EditOp::SetRepeat {
            seq: SEQ_SLOT,
            repeat,
        },
    ];
    for op in ops {
        h.cmd(EngineCommand::Edit { op });
    }
    for (i, col) in [1, 2, 3].into_iter().enumerate() {
        h.cmd(EngineCommand::Edit {
            op: EditOp::InsertStep {
                seq: SEQ_SLOT,
                lane: 0,
                index: i,
                step: seq_core::Step::slot(Some(col), 16),
            },
        });
    }
    let s = h
        .structure(|s| slot_states(s).is_some_and(|st| st == ["slot", "empty", "slot"]))
        .await;
    assert!(s
        .decks
        .iter()
        .any(|d| d.sequences.iter().any(|v| v.sequence.id == SEQ_SLOT)));
}

/// Resolution states of the slot sequence's steps.
fn slot_states(s: &StructureState) -> Option<Vec<String>> {
    let v = s
        .decks
        .iter()
        .flat_map(|d| &d.sequences)
        .find(|v| v.sequence.id == SEQ_SLOT)?;
    Some(
        v.resolution.as_ref()?.lanes[0]
            .steps
            .iter()
            .map(|st| st.state.to_owned())
            .collect(),
    )
}

/// SPEC §4.5 and notes §15: a drag in Resolume's UI sends no composition push. The slot read
/// before each slot step still sees it: the moved clip plays in its new column, and the slot it
/// left is a rest.
#[tokio::test]
async fn slot_lane_plays_what_is_in_the_slot_after_a_silent_swap() {
    let h = harness().await;
    add_slot_sequence(&h, seq_core::Repeat::Once).await;
    h.cmd(EngineCommand::Start { seq: SEQ_SLOT });
    h.runtime(|r| r.running.len() == 1).await;
    assert_eq!(h.osc(1).await, vec![connect(2, 1)]);

    h.ticks_at_128(80);
    // Let the composition push that follows the trigger land first: it must not carry the swap.
    tokio::time::sleep(Duration::from_millis(250)).await;
    // The user drags "Clip 08" from column 3 onto the empty column 2.
    h.mock.swap_slots_silently(2, 3, 2);
    h.ticks_at_128(15); // position 95: the read of column 2 went out at ~88
    h.structure(|s| slot_states(s).is_some_and(|st| st == ["slot", "slot", "empty"]))
        .await;
    h.ticks_at_128(1);
    assert_eq!(
        h.osc(2).await[1],
        connect(2, 2),
        "column 2 now holds Clip 08"
    );

    h.ticks_at_128(96);
    let log = h.osc(4).await;
    let [c1, c2] = clear(2);
    assert_eq!(
        log[2..4].to_vec(),
        vec![c1, c2],
        "column 3 is empty now: a rest"
    );
    let r = h.runtime(|r| r.last_confirm.is_some()).await;
    assert!(
        r.notices
            .iter()
            .all(|n| !n.contains("not confirmed") && !n.contains("yielded")),
        "{:?}",
        r.notices
    );
    h.app.shutdown().await;
}

/// SPEC §4.5: new media dropped onto the playing slot stops it in Resolume. The sequencer does
/// not yield and does not re-trigger; the slot plays again at its next step, with the new media.
#[tokio::test]
async fn media_dropped_on_the_playing_slot_plays_at_its_next_step() {
    let h = harness().await;
    add_slot_sequence(&h, seq_core::Repeat::Count { count: 2 }).await;
    h.cmd(EngineCommand::Start { seq: SEQ_SLOT });
    h.runtime(|r| r.running.len() == 1).await;
    assert_eq!(h.osc(1).await, vec![connect(2, 1)]);
    tokio::time::sleep(Duration::from_millis(150)).await; // outside the override window
    assert_eq!(h.mock.clip_state(2, 1).as_deref(), Some("Connected"));

    h.mock.clear_osc_log();
    h.mock
        .replace_media_silently(2, 1, "New Media", "D:\\media\\new.mov");
    h.ticks_at_128(40);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        h.mock.osc_log().is_empty(),
        "no re-trigger before the next step"
    );
    let r = h.runtime(|r| !r.running.is_empty()).await;
    assert_eq!(r.running[0].lanes[0].state, "playing", "not yielded");

    // Pass 1: column 2 empty (clear), column 3 Clip 08; pass 2 starts with column 1 again.
    h.ticks_at_128(3 * 96 - 40);
    let log = h.osc(4).await;
    let [c1, c2] = clear(2);
    assert_eq!(log, vec![c1, c2, connect(2, 3), connect(2, 1)]);
    let s = h
        .structure(|s| {
            s.decks
                .iter()
                .flat_map(|d| &d.layers)
                .any(|l| l.clips.iter().any(|c| c.name == "New Media"))
        })
        .await;
    assert!(s.decks.iter().any(|d| d.live));
    h.app.shutdown().await;
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
        ws.send(Message::Text(json!({ "cmd": cmd }).to_string().into()))
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
        json!({"cmd": "shell", "action": "quit"}).to_string().into(),
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

/// Preferences → Appearance (SPEC §9.7): every UI gets the theme settings on connect and on each change;
/// themes are saved in settings.json; invalid themes are refused.
#[tokio::test(flavor = "multi_thread")]
async fn theme_settings_are_saved_and_sent_to_every_ui() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;
    type Ws = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;
    /// The next `appearance` message that satisfies `ok`.
    async fn appearance(ws: &mut Ws, ok: impl Fn(&serde_json::Value) -> bool) -> serde_json::Value {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Some(Ok(Message::Text(t))) = ws.next().await else {
                    panic!("socket closed")
                };
                let v: serde_json::Value = serde_json::from_str(&t).unwrap();
                if v["type"] == "appearance" && ok(&v["data"]) {
                    return v["data"].clone();
                }
            }
        })
        .await
        .expect("appearance message")
    }
    async fn send(ws: &mut Ws, v: serde_json::Value) {
        ws.send(Message::Text(v.to_string().into())).await.unwrap();
    }

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
        clock: ClockSource::None,
        debounce: DebounceConfig::default(),
        ui: Some(std::net::TcpListener::bind("127.0.0.1:0").unwrap()),
        shell: None,
        quit: None,
        ffgl_dir: None,
    })
    .unwrap();
    let ui = app.ui_addr.unwrap();
    let (mut a, _) = tokio_tungstenite::connect_async(format!("ws://{ui}/ws"))
        .await
        .unwrap();
    let (mut b, _) = tokio_tungstenite::connect_async(format!("ws://{ui}/ws"))
        .await
        .unwrap();
    let first = appearance(&mut a, |_| true).await;
    assert_eq!(first, json!({"theme": null, "custom_themes": []}));

    let theme = json!({
        "id": "custom-0a1b", "name": "Club night", "base": "ink",
        "colors": {"accent": "#FF9900", "scrim": "#000000AA"}, "fonts": {"ui": "manrope"}
    });
    send(
        &mut a,
        json!({"cmd": "save_theme", "theme": theme, "select": true}),
    )
    .await;
    // The other UI follows.
    let got = appearance(&mut b, |d| d["theme"] == "custom-0a1b").await;
    assert_eq!(got["custom_themes"][0]["name"], "Club night");
    let saved = seq_app::settings::Settings::load(dir.path());
    assert_eq!(saved.theme.as_deref(), Some("custom-0a1b"));
    assert_eq!(saved.custom_themes[0].colors["accent"], "#FF9900");

    // Refused: not a saved-theme id, bad colour. Nothing changes.
    send(
        &mut a,
        json!({"cmd": "save_theme", "select": true,
        "theme": {"id": "rack", "name": "x", "base": "rack", "colors": {}}}),
    )
    .await;
    send(
        &mut a,
        json!({"cmd": "save_theme", "select": true,
        "theme": {"id": "custom-ff", "name": "x", "base": "rack", "colors": {"bg": "url(x)"}}}),
    )
    .await;
    send(&mut a, json!({"cmd": "set_theme", "id": "graphite"})).await;
    let got = appearance(&mut b, |d| d["theme"] == "graphite").await;
    assert_eq!(got["custom_themes"].as_array().unwrap().len(), 1, "{got}");

    // Deleting the selected theme falls back to the default.
    send(&mut a, json!({"cmd": "set_theme", "id": "custom-0a1b"})).await;
    appearance(&mut b, |d| d["theme"] == "custom-0a1b").await;
    send(&mut a, json!({"cmd": "delete_theme", "id": "custom-0a1b"})).await;
    let got = appearance(&mut b, |d| {
        d["custom_themes"].as_array().unwrap().is_empty()
    })
    .await;
    assert!(got["theme"].is_null(), "{got}");
    let saved = seq_app::settings::Settings::load(dir.path());
    assert_eq!((saved.theme, saved.custom_themes.len()), (None, 0));
    app.shutdown().await;
}

// ---- Resolume tempo as clock (SPEC §3.1) ----

fn resolume_clock_ready(r: &RuntimeState) -> bool {
    r.clock.source == ClockSourceKind::Resolume
        && r.clock.connected
        && r.clock.bpm == Some(120.0)
        && r.clock.transport.as_deref() == Some("Play")
}

#[tokio::test]
async fn resolume_tempo_drives_sequences_and_resync_jumps_to_bar_one() {
    let h = harness_with(true).await;
    let r = h.runtime(resolume_clock_ready).await;
    assert!(!r.clock.synced, "not in phase before the first resync");

    h.cmd(EngineCommand::Start { seq: SEQ_A });
    assert_eq!(h.osc(2).await, vec![connect(2, 1), connect(1, 2)]);
    // The app's own ticks move it on.
    h.runtime(|r| r.clock.running && r.running.first().is_some_and(|s| s.pos > 10))
        .await;

    // Resync (press and release): step 1 again at once, and only once.
    h.mock.clear_osc_log();
    h.mock.tempo_controller("resync", serde_json::Value::Null);
    assert_eq!(h.osc(2).await, vec![connect(2, 1), connect(1, 2)]);
    let r = h.runtime(|r| r.clock.synced).await;
    assert!(r.running[0].pos < 24, "back at bar 1: {}", r.running[0].pos);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        h.mock.osc_log().len(),
        2,
        "the release is not a second resync"
    );
    h.app.shutdown().await;
}

#[tokio::test]
async fn resolume_tempo_and_transport_changes_are_followed() {
    let h = harness_with(true).await;
    h.runtime(resolume_clock_ready).await;
    h.cmd(EngineCommand::Start { seq: SEQ_B });
    h.runtime(|r| r.clock.running && !r.running.is_empty())
        .await;

    h.mock.tempo_controller("tempo", json!(140.0));
    h.runtime(|r| r.clock.bpm == Some(140.0)).await;

    // Pause freezes the sequence (it keeps running, without ticks) with its own message.
    h.mock.tempo_controller("play_state", json!("Pause"));
    let r = h
        .runtime(|r| !r.clock.running && r.notices.iter().any(|n| n.contains("Resolume paused")))
        .await;
    let frozen = r.running[0].pos;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let r = h.runtime(|_| true).await;
    assert_eq!(r.running[0].pos, frozen, "no ticks while paused");
    assert_eq!(r.clock.transport.as_deref(), Some("Pause"));

    // Play after Pause continues without a resync; Play after Stop is one.
    h.mock.tempo_controller("play_state", json!("Play"));
    h.runtime(|r| r.clock.running).await;
    assert!(!h.runtime(|_| true).await.clock.synced);
    h.mock.tempo_controller("play_state", json!("Stop"));
    h.runtime(|r| r.clock.transport.as_deref() == Some("Stop"))
        .await;
    h.mock.tempo_controller("play_state", json!("Play"));
    h.runtime(|r| r.clock.synced && r.clock.running).await;
    h.app.shutdown().await;
}

#[tokio::test]
async fn switching_the_clock_source_stops_the_resolume_ticks() {
    let h = harness_with(true).await;
    h.runtime(resolume_clock_ready).await;
    h.cmd(EngineCommand::Start { seq: SEQ_B });
    h.runtime(|r| r.clock.running).await;
    h.cmd(EngineCommand::ClockSourceChanged {
        source: ClockSourceKind::Midi,
    });
    let r = h
        .runtime(|r| r.clock.source == ClockSourceKind::Midi && !r.clock.running)
        .await;
    let pos = r.running[0].pos;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(h.runtime(|_| true).await.running[0].pos, pos);
    // A resync means nothing without the Resolume source.
    h.mock.tempo_controller("resync", serde_json::Value::Null);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!h.runtime(|_| true).await.clock.synced);
    h.app.shutdown().await;
}
