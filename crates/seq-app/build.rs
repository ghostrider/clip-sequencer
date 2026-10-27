//! rust-embed needs `web/dist` to exist at compile time. If the UI has not been built yet,
//! create a placeholder page so the workspace still compiles and tests run.
//! With the `desktop` feature, tauri-build embeds `tauri.conf.json`, the icon and the Windows
//! manifest (M12).

use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
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
    #[cfg(feature = "desktop")]
    tauri_build::build();
}
