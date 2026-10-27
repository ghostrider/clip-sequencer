//! Desktop shell (docs/M12-desktop-shell.md, Phase D): a Tauri v2 window around the unchanged UI.
//!
//! Threads: Tauri's event loop owns the main thread. The sequencer (engine, server, Resolume
//! clients) runs on a separate tokio runtime ([`seq-runtime`] threads), which Tauri also uses for
//! its own async work, so nothing time-critical ever runs on the UI thread.
//!
//! Startup (Phase D2): the splash window (bundled `shell/` page) is created first and shows the
//! steps `runner::run` reports. Then the main window loads the UI from the in-process server, is
//! created hidden on the UI's background colour and shown once the page has loaded; the splash
//! fades out after it. A fatal startup error turns the splash into an error panel (Retry, Open
//! log, Copy details, Quit). The remote UI in the main window gets no Tauri IPC (no
//! capabilities): the menu, close handling and dialogs are native and talk to the engine through
//! its command channel. Only the local splash page calls one command, `splash_action`.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuBuilder, MenuEvent, MenuItemBuilder, SubmenuBuilder};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::window::Color;
use tauri::{AppHandle, Manager, RunEvent, Url, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::engine::view::RuntimeState;
use crate::engine::EngineCommand;
use crate::runner::{self, StartError, StartupStep};
use crate::server::ShellAction;
use crate::{APP_NAME, BUILD_NUMBER, COPYRIGHT, GIT_COMMIT, VERSION};

const MAIN: &str = "main";
const SPLASH: &str = "splash";
const SPLASH_SIZE: (f64, f64) = (560.0, 340.0);
/// The splash stays at least this long (no flicker on fast starts).
const SPLASH_MIN: Duration = Duration::from_millis(800);
/// A startup warning stays readable this long before the next step.
const WARNING_PAUSE: Duration = Duration::from_millis(1500);
/// The splash's own CSS fade (`body.fade`) before it is closed.
const SPLASH_FADE: Duration = Duration::from_millis(150);
/// Remembered between runs (Phase A: size and position, yes). Visibility is ours to decide.
const WINDOW_STATE: StateFlags = StateFlags::SIZE
    .union(StateFlags::POSITION)
    .union(StateFlags::MAXIMIZED);
/// `--bg` of web/src/tokens.css: shown until the UI has painted (no white flash).
const BACKGROUND: Color = Color(0x0E, 0x0F, 0x11, 0xFF);
/// The editor layout needs about this much (Phase D).
const MIN_SIZE: (f64, f64) = (1100.0, 700.0);
const DEFAULT_SIZE: (f64, f64) = (1440.0, 900.0);
const ZOOM_STEP: f64 = 0.1;
const ZOOM_RANGE: (f64, f64) = (0.5, 2.0);
/// Upper bound for waiting on `runner`'s shutdown (which itself aims for 2 s).
const SHUTDOWN_WAIT: Duration = Duration::from_secs(3);

/// Shell state, shared by the event handlers.
struct Shell {
    shutdown: CancellationToken,
    /// For Retry on the splash error panel.
    config: Config,
    data_dir: PathBuf,
    /// When the splash was created; `None` with `--no-splash`.
    splash_since: Mutex<Option<Instant>>,
    /// The splash page has loaded (its `window.splash` can take steps).
    splash_ready: watch::Sender<bool>,
    backend: Mutex<Option<Backend>>,
    zoom: Mutex<f64>,
    quitting: AtomicBool,
}

struct Backend {
    commands: mpsc::UnboundedSender<EngineCommand>,
    runtime: watch::Receiver<RuntimeState>,
    done: Option<JoinHandle<()>>,
}

pub fn run(config: Config) -> ExitCode {
    log::debug!("desktop: starting the shell");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("seq-runtime")
        .build()
        .expect("tokio runtime");
    tauri::async_runtime::set(rt.handle().clone());

    let shell = Shell {
        shutdown: CancellationToken::new(),
        config: config.clone(),
        data_dir: config.data_dir(),
        splash_since: Mutex::new(None),
        splash_ready: watch::channel(false).0,
        backend: Mutex::new(None),
        zoom: Mutex::new(1.0),
        quitting: AtomicBool::new(false),
    };
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(WINDOW_STATE)
                .build(),
        )
        .manage(shell)
        .invoke_handler(tauri::generate_handler![splash_action])
        .menu(build_menu)
        .on_menu_event(on_menu)
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == MAIN {
                    api.prevent_close();
                    request_quit(window.app_handle());
                }
            }
        })
        .setup(move |app| {
            // First, so it appears right away; the sequencer starts behind it.
            if !config.no_splash {
                splash_window(app.handle())?;
                log::debug!("desktop: splash window created");
            }
            tauri::async_runtime::spawn(start(app.handle().clone(), config));
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("desktop shell");
    app.run(|app, event| {
        // Closing the last window must go through the shutdown above, not end the process. The
        // only other way to close every window is closing the splash before the main window
        // exists (Alt+F4): that ends startup.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            let shell = app.state::<Shell>();
            if code.is_none() && !shell.quitting.load(Ordering::SeqCst) {
                if shell.backend.lock().unwrap().is_some() {
                    api.prevent_exit();
                    request_quit(app);
                } else {
                    shell.shutdown.cancel();
                }
            }
        }
    });
    ExitCode::SUCCESS
}

/// Starts the sequencer, then the main window (or reports why it could not start).
async fn start(app: AppHandle, config: Config) {
    let shell = app.state::<Shell>();
    let (progress, steps) = mpsc::unbounded_channel();
    let presenter = tauri::async_runtime::spawn(present_steps(app.clone(), steps));
    let (actions_tx, mut actions) = mpsc::unbounded_channel();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(action) = actions.recv().await {
            match action {
                ShellAction::Quit => request_quit(&handle),
                ShellAction::ZoomIn => zoom(&handle, Some(ZOOM_STEP)),
                ShellAction::ZoomOut => zoom(&handle, Some(-ZOOM_STEP)),
                ShellAction::ZoomReset => zoom(&handle, None),
                ShellAction::OpenLogs => open_logs(&handle),
            }
        }
    });
    let result = runner::run(
        config,
        shell.shutdown.clone(),
        Some(actions_tx),
        Some(progress),
    )
    .await;
    let _ = presenter.await; // every step shown (warnings stay readable)
    match result {
        Ok(running) => {
            let url = match Url::parse(&running.ui_url) {
                Ok(u) => u,
                Err(e) => return fatal(&app, format!("bad UI URL {}: {e}", running.ui_url), false),
            };
            *shell.backend.lock().unwrap() = Some(Backend {
                commands: running.commands,
                runtime: running.outputs.runtime,
                done: Some(running.done),
            });
            let since = *shell.splash_since.lock().unwrap();
            if let Some(since) = since {
                tokio::time::sleep(SPLASH_MIN.saturating_sub(since.elapsed())).await;
            }
            if let Err(e) = main_window(&app, url) {
                fatal(&app, format!("could not open the main window: {e}"), false);
            }
        }
        Err(StartError::AlreadyRunning { url }) => already_running(&app, url),
        Err(e) => {
            let retryable = e.retryable();
            fatal(&app, e.to_string(), retryable);
        }
    }
}

/// The splash: small, frameless, centred, on top while visible, not in the taskbar.
fn splash_window(app: &AppHandle) -> tauri::Result<()> {
    let info = serde_json::json!({
        "name": APP_NAME,
        "version": VERSION,
        "build": BUILD_NUMBER,
        "commit": GIT_COMMIT,
        "copyright": COPYRIGHT,
    });
    WebviewWindowBuilder::new(app, SPLASH, WebviewUrl::App("index.html".into()))
        .title(APP_NAME)
        .inner_size(SPLASH_SIZE.0, SPLASH_SIZE.1)
        .resizable(false)
        .decorations(false)
        .center()
        .skip_taskbar(true)
        .always_on_top(true)
        .focused(true)
        .background_color(BACKGROUND)
        .initialization_script(format!("window.SPLASH_INFO = {info};"))
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished {
                log::debug!("desktop: splash page loaded");
                window.state::<Shell>().splash_ready.send_replace(true);
            }
        })
        .build()?;
    *app.state::<Shell>().splash_since.lock().unwrap() = Some(Instant::now());
    Ok(())
}

/// Shows the startup steps on the splash; warnings stay for `WARNING_PAUSE`. Ends when `runner`
/// is done reporting.
async fn present_steps(app: AppHandle, mut steps: mpsc::UnboundedReceiver<StartupStep>) {
    let splash = app.get_webview_window(SPLASH);
    if splash.is_some() {
        let mut ready = app.state::<Shell>().splash_ready.subscribe();
        let _ = tokio::time::timeout(Duration::from_secs(2), ready.wait_for(|r| *r)).await;
    }
    while let Some(step) = steps.recv().await {
        let Some(splash) = &splash else { continue };
        let _ = splash.eval(format!(
            "window.splash && splash.step({}, {}, {}, {})",
            step.index,
            step.total,
            serde_json::Value::String(step.text),
            step.warning
        ));
        if step.warning {
            tokio::time::sleep(WARNING_PAUSE).await;
        }
    }
}

/// The main window has shown the UI: fade the splash out and close it.
fn close_splash(app: &AppHandle) {
    let Some(splash) = app.get_webview_window(SPLASH) else {
        return;
    };
    let _ = splash.set_always_on_top(false);
    let _ = splash.eval("window.splash && splash.fadeOut()");
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SPLASH_FADE).await;
        let _ = splash.close();
    });
}

/// Buttons of the splash's error panel.
#[tauri::command]
fn splash_action(app: AppHandle, action: String) {
    let shell = app.state::<Shell>();
    match action.as_str() {
        "quit" => app.exit(1),
        "retry" => {
            if let Some(splash) = app.get_webview_window(SPLASH) {
                let _ = splash.eval("window.splash && splash.reset()");
                let _ = splash.set_always_on_top(true);
            }
            tauri::async_runtime::spawn(start(app.clone(), shell.config.clone()));
        }
        "open_log" => {
            open_logs(&app);
        }
        other => log::warn!("unknown splash action {other:?}"),
    }
}

fn main_window(app: &AppHandle, url: Url) -> tauri::Result<()> {
    let origin = url.origin();
    let opener = app.clone();
    let new_window_opener = app.clone();
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::External(url))
        .title(format!("{APP_NAME} {VERSION}"))
        .inner_size(DEFAULT_SIZE.0, DEFAULT_SIZE.1)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .visible(false)
        .background_color(BACKGROUND)
        // Only the local UI inside the window; everything else goes to the system browser.
        .on_navigation(move |target| {
            let local = target.origin() == origin;
            if !local {
                open_external(&opener, target);
            }
            local
        })
        .on_new_window(move |target, _| {
            open_external(&new_window_opener, &target);
            NewWindowResponse::Deny
        })
        .on_page_load(|window, payload| {
            if payload.event() == PageLoadEvent::Finished && !window.is_visible().unwrap_or(true) {
                let _ = window.show();
                let _ = window.set_focus();
                close_splash(window.app_handle());
            }
        })
        .build()?;
    Ok(())
}

fn open_external(app: &AppHandle, url: &Url) {
    if matches!(url.scheme(), "http" | "https" | "mailto") {
        if let Err(e) = app.opener().open_url(url.as_str(), None::<&str>) {
            log::warn!("could not open {url}: {e}");
        }
    }
}

/// A second desktop instance: offer to bring up the running one, then exit (Phase B). Shown from
/// the splash, which closes with the dialog.
fn already_running(app: &AppHandle, url: String) {
    log::error!("{APP_NAME} is already running ({url}); not starting a second engine");
    let port = url.rsplit(':').next().unwrap_or("?").to_owned();
    let handle = app.clone();
    let data_dir = app.state::<Shell>().data_dir.clone();
    let mut dialog = app
        .dialog()
        .message(format!(
            "The sequencer is already running (port {port}). Open it?"
        ))
        .title(APP_NAME)
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Open".into(),
            "Cancel".into(),
        ));
    if let Some(splash) = app.get_webview_window(SPLASH) {
        let _ = splash.set_always_on_top(false);
        dialog = dialog.parent(&splash);
    }
    dialog.show(move |open| {
        if let Some(splash) = handle.get_webview_window(SPLASH) {
            let _ = splash.close();
        }
        if open && !focus_running_instance(&data_dir, &url) {
            let _ = handle.opener().open_url(&url, None::<&str>);
        }
        handle.exit(0);
    });
}

/// Brings the running instance's window to the front (its pid is in `runtime.json`). False if
/// there is no such window (e.g. a headless `seq-app-cli`).
fn focus_running_instance(data_dir: &std::path::Path, url: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(data_dir.join("runtime.json")) else {
        return false;
    };
    let Ok(info) = serde_json::from_str::<serde_json::Value>(&text) else {
        return false;
    };
    match (info["pid"].as_u64(), info["ui_url"].as_str()) {
        (Some(pid), Some(u)) if u == url => focus_process_window(pid as u32),
        _ => false,
    }
}

#[cfg(windows)]
fn focus_process_window(pid: u32) -> bool {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
        SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE,
    };
    struct Search {
        pid: u32,
        found: HWND,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: `lparam` is the `Search` passed to EnumWindows below, alive for the call.
        let search = unsafe { &mut *(lparam as *mut Search) };
        let mut pid = 0u32;
        // SAFETY: plain Win32 calls on a window handle EnumWindows gave us.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == search.pid
                && IsWindowVisible(hwnd) != 0
                && GetWindow(hwnd, GW_OWNER).is_null()
            {
                search.found = hwnd;
                return 0; // stop
            }
        }
        1
    }
    let mut search = Search {
        pid,
        found: std::ptr::null_mut(),
    };
    // SAFETY: `search` outlives the synchronous EnumWindows call that uses it.
    unsafe {
        EnumWindows(Some(visit), &mut search as *mut Search as LPARAM);
        if search.found.is_null() {
            return false;
        }
        if IsIconic(search.found) != 0 {
            ShowWindow(search.found, SW_RESTORE);
        }
        SetForegroundWindow(search.found) != 0
    }
}

#[cfg(not(windows))]
fn focus_process_window(_pid: u32) -> bool {
    false
}

/// Startup failed before there is a UI: the splash turns into an error panel (Retry, Open log,
/// Copy details, Quit) and stays; without a splash a dialog says why, then the app exits.
fn fatal(app: &AppHandle, message: String, retryable: bool) {
    log::error!("{message}");
    if let Some(splash) = app.get_webview_window(SPLASH) {
        let details = format!(
            "{APP_NAME} {VERSION} (build {BUILD_NUMBER}, {GIT_COMMIT}) could not start:\n{message}"
        );
        let _ = splash.set_always_on_top(false);
        let _ = splash.eval(format!(
            "window.splash && splash.error({}, {}, {retryable})",
            serde_json::Value::String(message),
            serde_json::Value::String(details)
        ));
        let _ = splash.set_focus();
        return;
    }
    let handle = app.clone();
    app.dialog()
        .message(message)
        .title(format!("{APP_NAME} could not start"))
        .kind(MessageDialogKind::Error)
        .show(move |_| handle.exit(1));
}

/// Window close, Ctrl+Q and File → Quit: confirm if sequences run, then shut down cleanly.
fn request_quit(app: &AppHandle) {
    let shell = app.state::<Shell>();
    if shell.quitting.load(Ordering::SeqCst) {
        return;
    }
    let running: Vec<String> = shell
        .backend
        .lock()
        .unwrap()
        .as_ref()
        .map(|b| {
            b.runtime
                .borrow()
                .running
                .iter()
                .map(|r| r.name.clone())
                .collect()
        })
        .unwrap_or_default();
    if running.is_empty() {
        return quit(app.clone());
    }
    let what = match running.as_slice() {
        [one] => format!("{one} is running. Quit and stop it?"),
        many => format!("{} are running. Quit and stop them?", many.join(", ")),
    };
    let handle = app.clone();
    app.dialog()
        .message(what)
        .title(APP_NAME)
        .kind(MessageDialogKind::Warning)
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Quit".into(),
            "Cancel".into(),
        ))
        .show(move |ok| {
            if ok {
                quit(handle);
            }
        });
}

fn quit(app: AppHandle) {
    let shell = app.state::<Shell>();
    if shell.quitting.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Err(e) = app.save_window_state(WINDOW_STATE) {
        log::warn!("window state: {e}");
    }
    shell.shutdown.cancel();
    let done = shell
        .backend
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|b| b.done.take());
    tauri::async_runtime::spawn(async move {
        if let Some(done) = done {
            let _ = tokio::time::timeout(SHUTDOWN_WAIT, done).await;
        }
        app.exit(0);
    });
}

// ---- menu ----

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let item = |id: &str, text: &str| MenuItemBuilder::with_id(id, text).build(app);
    let item_key = |id: &str, text: &str, key: &str| {
        MenuItemBuilder::with_id(id, text)
            .accelerator(key)
            .build(app)
    };
    let file = SubmenuBuilder::new(app, "File")
        .item(&item("reload", "Reload composition")?)
        .item(&item("save", "Save now")?)
        .item(&item("data", "Open data folder")?)
        .item(&item("prefs", "Preferences…")?)
        .separator()
        .item(&item_key("quit", "Quit", "CmdOrCtrl+Q")?)
        .build()?;
    let view = SubmenuBuilder::new(app, "View")
        .item(&item_key("zoom_in", "Zoom in", "CmdOrCtrl+=")?)
        .item(&item_key("zoom_out", "Zoom out", "CmdOrCtrl+-")?)
        .item(&item_key("zoom_reset", "Reset zoom", "CmdOrCtrl+0")?)
        .separator()
        .item(&item("console", "Toggle Console")?)
        .build()?;
    let help = SubmenuBuilder::new(app, "Help")
        .item(&item("shortcuts", "Keyboard shortcuts")?)
        .build()?;
    MenuBuilder::new(app).items(&[&file, &view, &help]).build()
}

fn on_menu(app: &AppHandle, event: MenuEvent) {
    let shell = app.state::<Shell>();
    let send = |cmd: EngineCommand| {
        if let Some(b) = shell.backend.lock().unwrap().as_ref() {
            let _ = b.commands.send(cmd);
        }
    };
    match event.id().as_ref() {
        "reload" => send(EngineCommand::ReloadComposition),
        "save" => send(EngineCommand::SaveNow),
        "data" => {
            if let Err(e) = app
                .opener()
                .open_path(shell.data_dir.to_string_lossy(), None::<&str>)
            {
                log::warn!("could not open {}: {e}", shell.data_dir.display());
            }
        }
        "quit" => request_quit(app),
        "zoom_in" => zoom(app, Some(ZOOM_STEP)),
        "zoom_out" => zoom(app, Some(-ZOOM_STEP)),
        "zoom_reset" => zoom(app, None),
        "prefs" => {
            if let Some(window) = app.get_webview_window(MAIN) {
                let _ = window.eval("window.__seqOpenPrefs && window.__seqOpenPrefs()");
            }
        }
        "console" => {
            // The tab lives in the page; Ctrl+1 / Ctrl+2 switch it there too.
            if let Some(window) = app.get_webview_window(MAIN) {
                let _ = window.eval("window.__seqToggleConsole && window.__seqToggleConsole()");
            }
        }
        "shortcuts" => {
            app.dialog()
                .message(SHORTCUTS)
                .title("Keyboard shortcuts")
                .kind(MessageDialogKind::Info)
                .show(|_| {});
        }
        _ => {}
    }
}

/// `<data-dir>/logs` (the daily log files) in Explorer; the data folder if there are none yet.
fn open_logs(app: &AppHandle) {
    let shell = app.state::<Shell>();
    let dir = crate::logging::logs_dir()
        .filter(|d| d.is_dir())
        .unwrap_or_else(|| shell.data_dir.clone());
    if let Err(e) = app.opener().open_path(dir.to_string_lossy(), None::<&str>) {
        log::warn!("could not open {}: {e}", dir.display());
    }
}

/// `Some(step)` changes the zoom by `step`, `None` resets it.
fn zoom(app: &AppHandle, step: Option<f64>) {
    let shell = app.state::<Shell>();
    let mut zoom = shell.zoom.lock().unwrap();
    *zoom = match step {
        Some(s) => ((*zoom + s) * 10.0).round() / 10.0,
        None => 1.0,
    }
    .clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.set_zoom(*zoom);
    }
}

/// The shortcuts of SPEC §9.3, plus the window's own.
const SHORTCUTS: &str = "\
Space\tstart / stop the selected sequence
Del\tremove the selected step
R\tinsert a rest after the selected step
Esc\tdeselect
Ctrl+Z\tundo
Ctrl+Shift+Z\tredo
Alt+drag\tduplicate a step
Ctrl+wheel\tzoom the lane grid

Ctrl+1 / Ctrl+2\tSequencer / Console tab
Ctrl+,\tPreferences
Ctrl+= / Ctrl+- / Ctrl+0\tzoom the window
Ctrl+Q\tquit";
