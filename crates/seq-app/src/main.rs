// The desktop app. Release builds are GUI-subsystem programs (no console window); console use
// (--headless, --validate, ...) goes through seq-app-cli.exe (src/bin/seq-app-cli.rs).
#![cfg_attr(
    all(not(debug_assertions), feature = "desktop"),
    windows_subsystem = "windows"
)]

fn main() -> std::process::ExitCode {
    seq_app::cli::main(seq_app::cli::Exe::Desktop)
}
