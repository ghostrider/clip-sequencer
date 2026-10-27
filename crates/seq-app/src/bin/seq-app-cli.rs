//! Console companion of the desktop app: always headless (console output, UI in a browser,
//! Ctrl+C stops), plus --validate, --list-midi-ports and --clock-debug.

fn main() -> std::process::ExitCode {
    seq_app::cli::main(seq_app::cli::Exe::Console)
}
