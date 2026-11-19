// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Everything the window does, against a real core, without the window.
//!
//! Needs the core binary: `EON_OFFICE_CORE_BIN=… cargo test -- --include-ignored`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use eon_launcher::host::Host;
use serde_json::json;
use std::time::{Duration, Instant};

fn idle_entry() -> serde_json::Value {
    if cfg!(windows) {
        json!({ "command": "cmd", "args": ["/C", "ping -n 60 127.0.0.1 >NUL"] })
    } else {
        json!({ "command": "sh", "args": ["-c", "sleep 60"] })
    }
}

#[test]
#[ignore = "needs EON_OFFICE_CORE_BIN; CI builds eon-office-core and sets it"]
fn the_owner_s_whole_day_through_the_host() {
    assert!(
        std::env::var_os("EON_OFFICE_CORE_BIN").is_some(),
        "set EON_OFFICE_CORE_BIN"
    );
    let home = std::env::temp_dir().join(format!("eon-launcher-host-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    // No core running: the host starts one.
    let host = Host::connect(&home).unwrap();
    assert!(host.started_core());
    assert_eq!(host.status().unwrap()["core"]["api"], 0);

    // A manifest file, as a person would drop it on the window.
    let folder = home.join("downloads").join("tasks");
    std::fs::create_dir_all(&folder).unwrap();
    let manifest_path = folder.join("manifest.json");
    let manifest = json!({
        "manifest": 0, "id": "org.example.tasks", "name": "Example Tasks", "version": "1.0.0",
        "api": { "min": 0, "max": 0 }, "description": "Test application",
        "entry": idle_entry(),
        "capabilities": ["storage", "notifications"],
    });
    std::fs::write(&manifest_path, manifest.to_string()).unwrap();
    let path = manifest_path.to_string_lossy().into_owned();

    // Review shows what it asks for, and installs nothing.
    let preview = host.preview(&path).unwrap();
    assert_eq!(
        preview["manifest"]["capabilities"],
        json!(["storage", "notifications"])
    );
    assert_eq!(host.apps().unwrap()["apps"], json!([]));
    assert_eq!(
        host.preview("C:/no/such/file.json").unwrap_err()["code"],
        "unreadable"
    );

    // Install grants nothing; each permission is the owner's choice.
    let installed = host.install(&path).unwrap();
    assert_eq!(installed["granted"], json!([]));
    assert_eq!(installed["pending"], json!(["storage", "notifications"]));
    host.set_permission("org.example.tasks", "storage", true)
        .unwrap();
    let apps = host.apps().unwrap();
    assert_eq!(apps["apps"][0]["granted"], json!(["storage"]));
    host.set_permission("org.example.tasks", "storage", false)
        .unwrap();
    assert_eq!(host.apps().unwrap()["apps"][0]["granted"], json!([]));

    // The install event reaches the window's pump.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_install = false;
    while Instant::now() < deadline && !saw_install {
        if let Some(event) = host.next_event(Duration::from_millis(200)) {
            saw_install = event["topic"] == "core/app.installed";
        }
    }
    assert!(saw_install, "core/app.installed was not forwarded");

    // Start and stop it (it ignores the polite request, so it is forced).
    host.launch("org.example.tasks").unwrap();
    let stopped = host.stop("org.example.tasks").unwrap();
    assert_eq!(stopped["stopped"], true);

    // Search and open: nothing indexed; open on an app without `open` says so.
    assert_eq!(host.search("anything").unwrap()["hits"], json!([]));
    let opened = host.open("org.example.tasks", "x").unwrap();
    assert_eq!(
        opened,
        json!({ "opened": false, "reason": "method_not_found" })
    );

    // Preferences.
    host.set_pref("reduce_motion", json!(true)).unwrap();
    assert_eq!(host.prefs().unwrap()["reduce_motion"], true);
    assert_eq!(
        host.set_pref("font_scale", json!(9)).unwrap_err()["code"],
        "invalid_params"
    );

    // The activity record shows what happened and is intact.
    let activity = host.activity().unwrap();
    assert_eq!(activity["verify"]["ok"], true);
    let actions: Vec<&str> = activity["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["action"].as_str().unwrap())
        .collect();
    assert!(actions.contains(&"app.installed") && actions.contains(&"app.granted"));
    assert_eq!(actions.first(), Some(&"prefs.set"), "newest first");

    // Remove it, then leave: nothing is connected, so the core the host
    // started is stopped with it.
    host.uninstall("org.example.tasks").unwrap();
    assert_eq!(host.notifications(false).unwrap()["items"], json!([]));
    host.leave();
    assert!(
        eon_office_sdk::endpoint(&home).is_err(),
        "the core is still running"
    );
    let _ = std::fs::remove_dir_all(&home);
}
