//! Engine against the mock Resolume with an injected clock (SPEC §7).

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

const TIME_TRAVEL: &str =
    "Clip 07";
const TWISTED: &str = "Clip 13";
const LIQUID: &str =
    "Clip 08";
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

    let (clock, clock_rx) = mpsc::unbounded_channel();
    let app = app::start(AppConfig {
        resolume: ResolumeAddr {
            host: "127.0.0.1".into(),
            rest_port: mock.rest_addr.port(),
            osc_port: mock.osc_addr.port(),
        },
        storage: Storage::new(dir.path()),
        engine: EngineConfig::default(),
        clock: ClockSource::Channel(clock_rx),
        debounce: DebounceConfig {
            quiet: Duration::from_millis(50),
            max_wait: Duration::from_millis(200),
        },
    })
    .unwrap();
    let h = Harness {
        mock,
        app,
        clock,
        dir,
    };
    h.structure(|s| s.decks.iter().any(|d| !d.sequences.is_empty()))
        .await;
    h
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
    assert_eq!(drop.binding, Some(1), "§ Drop sits in SEQ column 1");
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

#[tokio::test]
async fn starting_a_sequence_of_another_deck_switches_first() {
    let h = harness().await;
    h.cmd(EngineCommand::Start { seq: SEQ_A });
    h.runtime(|r| r.running.len() == 1).await;
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
