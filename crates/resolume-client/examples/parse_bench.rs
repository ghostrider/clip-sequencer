//! Times composition parsing on the M0 samples: `cargo run --release -p resolume-client --example parse_bench`
use std::time::Instant;

fn main() {
    for name in ["composition.json", "deck1-composition.json"] {
        let path = format!("{}/../../docs/samples/{name}", env!("CARGO_MANIFEST_DIR"));
        let pretty = std::fs::read_to_string(&path).unwrap();
        // WS pushes arrive compact; re-serialize to match their size.
        let compact =
            serde_json::to_string(&serde_json::from_str::<serde_json::Value>(&pretty).unwrap())
                .unwrap();
        let runs = 200;
        let t = Instant::now();
        for _ in 0..runs {
            std::hint::black_box(
                resolume_client::composition::parse_composition(&compact).unwrap(),
            );
        }
        println!(
            "{name}: {} KB, {:.2} ms per parse",
            compact.len() / 1024,
            t.elapsed().as_secs_f64() * 1000.0 / runs as f64
        );
    }
}
