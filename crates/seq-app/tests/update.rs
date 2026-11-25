//! In-app update against a local fake of GitHub's release API (SPEC §15): check, download, verify, unpack.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use seq_app::update::{self, CheckResult};

#[derive(Clone, Default)]
struct Fake {
    /// The release JSON; `None` answers 404.
    release: Arc<Mutex<Option<serde_json::Value>>>,
    zip: Arc<Mutex<Vec<u8>>>,
    sums: Arc<Mutex<String>>,
}

async fn serve(fake: Fake) -> String {
    let app = Router::new()
        .route(
            "/repos/ghostrider/clip-sequencer/releases/latest",
            get(|State(f): State<Fake>| async move {
                match f.release.lock().unwrap().clone() {
                    Some(v) => axum::Json(v).into_response(),
                    None => StatusCode::NOT_FOUND.into_response(),
                }
            }),
        )
        .route(
            "/dl/zip",
            get(|State(f): State<Fake>| async move { f.zip.lock().unwrap().clone() }),
        )
        .route(
            "/dl/sums",
            get(|State(f): State<Fake>| async move { f.sums.lock().unwrap().clone() }),
        )
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn sha256(data: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A release zip as `release.ps1` packs it, plus its `SHA256SUMS.txt`.
fn release_files(version: &str) -> (Vec<u8>, String) {
    let files: [(&str, &[u8]); 3] = [
        ("clip-sequencer/seq-app.exe", b"gui 0.9"),
        ("clip-sequencer/seq-app-cli.exe", b"cli 0.9"),
        ("clip-sequencer/ffgl/SeqView.dll", b"dll 0.9"),
    ];
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut zw = zip::ZipWriter::new(&mut buf);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (p, d) in files {
            zw.start_file(p, opts).unwrap();
            zw.write_all(d).unwrap();
        }
        zw.finish().unwrap();
    }
    let zip = buf.into_inner();
    let mut lines = vec![format!("{}  {}", sha256(&zip), update::zip_name(version))];
    for (p, d) in files {
        lines.push(format!("{}  {p}", sha256(d)));
    }
    (zip, lines.join("\n") + "\n")
}

fn release_json(base: &str, version: &str, size: usize) -> serde_json::Value {
    serde_json::json!({
        "tag_name": format!("v{version}"),
        "body": "## What's new\n- things",
        "html_url": format!("{base}/page"),
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": update::zip_name(version), "browser_download_url": format!("{base}/dl/zip"), "size": size},
            {"name": "SHA256SUMS.txt", "browser_download_url": format!("{base}/dl/sums"), "size": 300}
        ]
    })
}

#[tokio::test]
async fn check_download_and_verify_against_a_fake_github() {
    let fake = Fake::default();
    let base = serve(fake.clone()).await;
    let client = update::client("0.5.0");

    // No release yet: GitHub answers 404 → an error, nothing else.
    let r = update::check(&client, &base, "0.5.0").await;
    assert!(
        matches!(&r, CheckResult::Error { message, .. } if message.contains("404")),
        "{r:?}"
    );

    // The same version: up to date.
    let (zip, sums) = release_files("0.9.0");
    *fake.zip.lock().unwrap() = zip.clone();
    *fake.sums.lock().unwrap() = sums;
    *fake.release.lock().unwrap() = Some(release_json(&base, "0.5.0", zip.len()));
    assert_eq!(
        update::check(&client, &base, "0.5.0").await,
        CheckResult::UpToDate {
            current: "0.5.0".into()
        }
    );

    // A newer one: offered with its notes and assets.
    *fake.release.lock().unwrap() = Some(release_json(&base, "0.9.0", zip.len()));
    let CheckResult::Available { release, .. } = update::check(&client, &base, "0.5.0").await
    else {
        panic!("expected an update");
    };
    assert_eq!(release.version, "0.9.0");
    assert!(release.notes.contains("What's new"));

    // Download with progress, verify, unpack.
    let tmp = tempfile::tempdir().unwrap();
    let seen: Arc<Mutex<Vec<(u64, u64)>>> = Arc::default();
    let s2 = seen.clone();
    let folder = update::download(&client, &release, tmp.path(), &move |d, t| {
        s2.lock().unwrap().push((d, t))
    })
    .await
    .unwrap();
    assert_eq!(
        std::fs::read(folder.join("seq-app-cli.exe")).unwrap(),
        b"cli 0.9"
    );
    {
        let seen = seen.lock().unwrap();
        assert_eq!(seen.first(), Some(&(0, zip.len() as u64)));
        assert_eq!(seen.last(), Some(&(zip.len() as u64, zip.len() as u64)));
    }

    // A damaged download is refused.
    let mut broken = zip.clone();
    let mid = broken.len() / 2;
    broken[mid] ^= 0xff;
    *fake.zip.lock().unwrap() = broken;
    let err = update::download(&client, &release, tmp.path(), &|_, _| {})
        .await
        .unwrap_err();
    assert!(err.contains("does not match"), "{err}");
}

#[tokio::test]
async fn no_network_is_an_error_not_a_hang() {
    // A port nothing listens on.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let client = update::client("0.5.0");
    let r = update::check(&client, &format!("http://127.0.0.1:{port}"), "0.5.0").await;
    assert!(
        matches!(&r, CheckResult::Error { message, .. } if message.contains("could not reach")),
        "{r:?}"
    );
}

#[test]
fn sums_match_release_ps1_output() {
    // The release workflow writes `<hex>  <name>` lines (two spaces), LF, ASCII.
    let (_, sums) = release_files("0.9.0");
    let parsed: BTreeMap<_, _> = update::parse_sums(&sums);
    assert_eq!(parsed.len(), 4);
    assert!(parsed.contains_key("clip-sequencer-0.9.0-windows-x64.zip"));
}
