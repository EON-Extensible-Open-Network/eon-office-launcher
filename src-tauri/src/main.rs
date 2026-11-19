// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! EON Launcher -- the window.
//!
//! ```text
//! eon-office-launcher                    open the window
//! eon-office-launcher --selftest [FILE]  check what a machine can check, exit non-zero if not ok
//! eon-office-launcher --version
//! ```

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eon_launcher::checks;
use eon_launcher::host::{Host, Outcome};
use serde_json::{json, Value};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;
use tauri::{Emitter, Manager};

const INDEX: &str = include_str!("../../src/index.html");
const SCRIPT: &str = include_str!("../../src/app.js");

#[derive(Default)]
struct Shared {
    host: Mutex<Option<Arc<Host>>>,
}

impl Shared {
    fn host(&self) -> Result<Arc<Host>, Value> {
        self.host
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or_else(
                || json!({ "code": "not_connected", "message": "not connected to the core" }),
            )
    }
}

type State<'a> = tauri::State<'a, Shared>;

/// Connect (starting the core if needed) and start forwarding live events.
fn connect(app: &tauri::AppHandle, shared: &Shared) -> Outcome {
    if let Ok(host) = shared.host() {
        return host.status();
    }
    let home = eon_office_sdk::default_home()
        .ok_or_else(|| json!({ "code": "no_home", "message": "no home directory" }))?;
    let host = Arc::new(Host::connect(&home)?);
    *shared.host.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&host));
    let pump = Arc::clone(&host);
    let handle = app.clone();
    std::thread::spawn(move || loop {
        if let Some(event) = pump.next_event(Duration::from_millis(500)) {
            if event.is_null() {
                continue;
            }
            let _ = handle.emit("core-event", &event);
            if event["topic"] == "core/shutdown" {
                break;
            }
        }
    });
    host.status()
}

// ---- the IPC surface: everything the interface may ask for ----------------

#[tauri::command(async)]
fn strings(language: String) -> Value {
    eon_launcher::strings(&language)
}

#[tauri::command(async)]
fn status(app: tauri::AppHandle, state: State<'_>) -> Outcome {
    connect(&app, &state)
}

#[tauri::command(async)]
fn apps(state: State<'_>) -> Outcome {
    state.host()?.apps()
}

#[tauri::command(async)]
fn preview(state: State<'_>, path: String) -> Outcome {
    state.host()?.preview(&path)
}

#[tauri::command(async)]
fn install(state: State<'_>, path: String) -> Outcome {
    state.host()?.install(&path)
}

#[tauri::command(async)]
fn permission(state: State<'_>, app: String, capability: String, allowed: bool) -> Outcome {
    state.host()?.set_permission(&app, &capability, allowed)
}

#[tauri::command(async)]
fn launch(state: State<'_>, app: String) -> Outcome {
    state.host()?.launch(&app)
}

#[tauri::command(async)]
fn stop(state: State<'_>, app: String) -> Outcome {
    state.host()?.stop(&app)
}

#[tauri::command(async)]
fn uninstall(state: State<'_>, app: String) -> Outcome {
    state.host()?.uninstall(&app)
}

#[tauri::command(async)]
fn notifications(state: State<'_>, unread_only: bool) -> Outcome {
    state.host()?.notifications(unread_only)
}

#[tauri::command(async)]
fn mark_read(state: State<'_>, id: i64) -> Outcome {
    state.host()?.mark_read(id)
}

#[tauri::command(async)]
fn search(state: State<'_>, query: String) -> Outcome {
    state.host()?.search(&query)
}

#[tauri::command(async)]
fn open(state: State<'_>, app: String, id: String) -> Outcome {
    state.host()?.open(&app, &id)
}

#[tauri::command(async)]
fn prefs(state: State<'_>) -> Outcome {
    state.host()?.prefs()
}

#[tauri::command(async)]
fn set_pref(state: State<'_>, key: String, value: Value) -> Outcome {
    state.host()?.set_pref(&key, value)
}

#[tauri::command(async)]
fn catalogue(state: State<'_>) -> Outcome {
    state.host()?.catalogue()
}

#[tauri::command(async)]
fn market_channel(state: State<'_>, channel: String) -> Outcome {
    state.host()?.set_market_channel(&channel)
}

#[tauri::command(async)]
fn install_market(state: State<'_>, app: String, allow_unsigned: bool) -> Outcome {
    state.host()?.install_from_market(&app, allow_unsigned)
}

#[tauri::command(async)]
fn activity(state: State<'_>) -> Outcome {
    state.host()?.activity()
}

/// The window, built here rather than in tauri.conf.json so a debug build can
/// pass the webview extra arguments (`EON_WEBVIEW_ARGS`, e.g.
/// `--remote-debugging-port=9333`) for `tools/ui-check.mjs`. The release
/// build ignores the variable: it is not a switch a person's environment
/// should be able to flip.
fn main_window(app: &tauri::AppHandle) -> tauri::Result<()> {
    #[allow(unused_mut)]
    let mut builder =
        tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
            .title("EON Launcher")
            .inner_size(1040.0, 720.0)
            .min_inner_size(640.0, 480.0);
    #[cfg(debug_assertions)]
    if let Ok(extra) = std::env::var("EON_WEBVIEW_ARGS") {
        builder = builder.additional_browser_args(&format!(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection {extra}"
        ));
    }
    builder.build().map(|_| ())
}

fn window() {
    let app = tauri::Builder::default()
        .manage(Shared::default())
        .setup(|app| {
            main_window(app.handle())?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Dropping a manifest onto the window is how an application is
            // installed without a terminal. The interface shows what it asks
            // for before anything is installed.
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event {
                if let Some(first) = paths.first() {
                    let _ = window.emit("manifest-dropped", first.to_string_lossy());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            strings,
            status,
            apps,
            preview,
            install,
            permission,
            launch,
            stop,
            uninstall,
            notifications,
            mark_read,
            search,
            open,
            prefs,
            set_pref,
            activity,
            catalogue,
            market_channel,
            install_market,
        ])
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(|handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                if let Ok(host) = handle.state::<Shared>().host() {
                    host.leave();
                }
            }
        }),
        Err(e) => {
            eprintln!("EON Launcher could not start: {e}");
            std::process::exit(1);
        }
    }
}

/// Everything that can be checked without a window. CI runs it against the
/// binary it is about to publish, with the core beside it.
fn selftest() -> String {
    let mut report = format!("EON Launcher {} self-test\n", env!("CARGO_PKG_VERSION"));
    let mut failed = false;
    let mut section = |name: &str, problems: Vec<String>| {
        if problems.is_empty() {
            let _ = writeln!(report, "  ok    {name}");
        } else {
            failed = true;
            for problem in problems {
                let _ = writeln!(report, "  FAIL  {name}: {problem}");
            }
        }
    };
    section(
        "strings (en, tr)",
        checks::catalogue_problems(eon_launcher::EN, eon_launcher::TR),
    );
    section("interface", checks::ui_problems(INDEX, SCRIPT));

    // The core: found, started in a private home, answered, and stopped again.
    let core = if eon_launcher::host::core_binary().is_none() {
        vec!["eon-office-core is not beside the launcher or on PATH".to_owned()]
    } else {
        let home =
            std::env::temp_dir().join(format!("eon-launcher-selftest-{}", std::process::id()));
        let problems = match Host::connect(&home) {
            Err(e) => vec![format!("could not start the core: {e}")],
            Ok(host) => {
                let mut problems = Vec::new();
                if !host.started_core() {
                    problems.push("a core was already running for a fresh home".to_owned());
                }
                match host.status() {
                    Ok(status) if status["core"]["api"] == 0 => {}
                    Ok(status) => problems.push(format!("unexpected status {status}")),
                    Err(e) => problems.push(format!("status failed: {e}")),
                }
                if let Err(e) = host.apps() {
                    problems.push(format!("apps failed: {e}"));
                }
                host.leave();
                problems
            }
        };
        let _ = std::fs::remove_dir_all(&home);
        problems
    };
    section("core: start, answer, stop", core);
    report.push_str(if failed { "FAILED\n" } else { "ok\n" });
    report
}

fn main() {
    let mut arguments = std::env::args().skip(1);
    match arguments.next().as_deref() {
        Some("--version" | "-V") => println!("EON Launcher {}", env!("CARGO_PKG_VERSION")),
        Some("--selftest") => {
            let report = selftest();
            match arguments.next() {
                Some(path) => {
                    if let Err(e) = std::fs::write(&path, &report) {
                        eprintln!("could not write {path}: {e}");
                        std::process::exit(2);
                    }
                }
                None => print!("{report}"),
            }
            if !report.ends_with("ok\n") {
                std::process::exit(1);
            }
        }
        _ => window(),
    }
}
