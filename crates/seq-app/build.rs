//! rust-embed needs `web/dist` to exist at compile time. If the UI has not been built yet,
//! create a placeholder page so the workspace still compiles and tests run.
//! Build info (`SEQ_BUILD_NUMBER`, `SEQ_GIT_COMMIT`) comes from git, with fallbacks; `SEQ_BUILD_DATE` (UTC,
//! ISO 8601; `SOURCE_DATE_EPOCH` if set), `SEQ_TARGET` and `SEQ_LONG_VERSION` (`--version`) are derived here.
//! With the `desktop` feature, tauri-build embeds `tauri.conf.json`, the icon, the Windows
//! manifest and the splash page in `shell/` (M12), which gets the UI's design tokens copied in.
//! Product name and copyright come from `tauri.conf.json` (`SEQ_APP_NAME`, `SEQ_COPYRIGHT`), so
//! the code and the exe's version resource share one source; the version comes from Cargo only.

use std::path::Path;
use std::process::Command;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dist = manifest.join("../../web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
    let index = dist.join("index.html");
    if !index.exists() {
        std::fs::create_dir_all(&dist).expect("create web/dist");
        std::fs::write(
            &index,
            "<!doctype html><meta charset=utf-8><title>Sequencer</title>\
             <p>UI not built: run <code>npm install &amp;&amp; npm run build</code> in <code>web/</code>.</p>",
        )
        .expect("write placeholder index.html");
    }

    build_info(manifest);
    product_info(manifest);
    notices(manifest);

    // The splash page uses the same tokens as the UI (one source: web/src/tokens.css).
    let tokens = manifest.join("../../web/src/tokens.css");
    println!("cargo:rerun-if-changed={}", tokens.display());
    std::fs::copy(&tokens, manifest.join("shell/tokens.css")).expect("copy tokens.css to shell/");

    #[cfg(feature = "desktop")]
    tauri_build::build();
}

/// Third-party notices for the License dialog (M11 Phase F): `THIRD_PARTY_LICENSES.json` in the repo root,
/// written by `scripts/third-party-licenses.mjs` (release builds run it before `cargo build`). Without it a
/// placeholder is written there (git-ignored), so the dialog says the notices were not generated, and cargo
/// does not rerun this script on every build (as it would for a missing watched file).
fn notices(manifest: &Path) {
    let path = manifest.join("../../THIRD_PARTY_LICENSES.json");
    if !path.exists() {
        std::fs::write(
            &path,
            "{\"placeholder\": true, \"packages\": [], \"texts\": [], \"note\": \"Third-party notices were not \
             generated for this build (scripts/third-party-licenses.mjs). Release builds include them.\"}\n",
        )
        .expect("write placeholder THIRD_PARTY_LICENSES.json");
    }
    println!("cargo:rerun-if-changed={}", path.display());
    let license = manifest.join("../../LICENSE");
    println!("cargo:rerun-if-changed={}", license.display());
}

/// Name and copyright from `tauri.conf.json`, which tauri-build also writes into the exe. The config
/// must not carry a version of its own: Tauri then takes Cargo's (`[workspace.package]`).
fn product_info(manifest: &Path) {
    let path = manifest.join("tauri.conf.json");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(&path).expect("read tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&text).expect("parse tauri.conf.json");
    assert!(
        conf.get("version").is_none(),
        "tauri.conf.json must not set \"version\": it comes from [workspace.package]"
    );
    let field = |ptr: &str| {
        conf.pointer(ptr)
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("tauri.conf.json: {ptr} missing"))
            .to_owned()
    };
    println!("cargo:rustc-env=SEQ_APP_NAME={}", field("/productName"));
    println!(
        "cargo:rustc-env=SEQ_COPYRIGHT={}",
        field("/bundle/copyright")
    );
}

/// Build number = commit count, the same in every checkout; commit = short hash, `-dirty` if the
/// workspace has uncommitted changes. `0` / `unknown` without git.
fn build_info(manifest: &Path) {
    let workspace = manifest.join("../..");
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&workspace)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let count = git(&["rev-list", "--count", "HEAD"]).unwrap_or_else(|| "0".into());
    let commit = match git(&["rev-parse", "--short", "HEAD"]) {
        Some(hash) => {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no", "--", "."])
                .is_some_and(|s| !s.is_empty());
            if dirty {
                format!("{hash}-dirty")
            } else {
                hash
            }
        }
        None => "unknown".into(),
    };
    println!("cargo:rustc-env=SEQ_BUILD_NUMBER={count}");
    println!("cargo:rustc-env=SEQ_GIT_COMMIT={commit}");

    // The date is taken when this script runs, i.e. on the first build of a commit (it reruns when HEAD or
    // the index changes). SOURCE_DATE_EPOCH (reproducible builds) overrides the clock.
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let epoch = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
        });
    let date = iso8601_utc(epoch);
    println!("cargo:rustc-env=SEQ_BUILD_DATE={date}");
    let target = std::env::var("TARGET").unwrap_or_else(|_| "unknown".into());
    println!("cargo:rustc-env=SEQ_TARGET={target}");
    let version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    println!(
        "cargo:rustc-env=SEQ_LONG_VERSION={version} (build {count}, {commit}, {})",
        &date[..10]
    );
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for f in ["HEAD", "index", "packed-refs"] {
            println!("cargo:rerun-if-changed={dir}/{f}");
        }
    }
}

/// `2026-11-03T14:05:09Z` from Unix seconds (UTC; civil-from-days, no date crate needed in build.rs).
fn iso8601_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}
