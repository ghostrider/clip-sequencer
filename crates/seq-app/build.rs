//! rust-embed needs `web/dist` to exist at compile time. If the UI has not been built yet,
//! create a placeholder page so the workspace still compiles and tests run.
//! Build info (`SEQ_BUILD_NUMBER`, `SEQ_GIT_COMMIT`) comes from git, with fallbacks.
//! With the `desktop` feature, tauri-build embeds `tauri.conf.json`, the icon, the Windows
//! manifest and the splash page in `shell/` (M12), which gets the UI's design tokens copied in.

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

    // The splash page uses the same tokens as the UI (one source: web/src/tokens.css).
    let tokens = manifest.join("../../web/src/tokens.css");
    println!("cargo:rerun-if-changed={}", tokens.display());
    std::fs::copy(&tokens, manifest.join("shell/tokens.css")).expect("copy tokens.css to shell/");

    #[cfg(feature = "desktop")]
    tauri_build::build();
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
    if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        for f in ["HEAD", "index", "packed-refs"] {
            println!("cargo:rerun-if-changed={dir}/{f}");
        }
    }
}
