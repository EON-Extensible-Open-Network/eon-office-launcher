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

#[test]
#[ignore = "needs EON_OFFICE_CORE_BIN and the network (the live index on GitHub)"]
fn installing_from_the_marketplace_checks_asks_and_grants_nothing() {
    assert!(
        std::env::var_os("EON_OFFICE_CORE_BIN").is_some(),
        "set EON_OFFICE_CORE_BIN"
    );
    let home = std::env::temp_dir().join(format!("eon-launcher-market-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let host = Host::connect(&home).unwrap();

    // Stable channel: the reference apps are previews, so nothing installs, and it says why.
    let stable = host.catalogue().unwrap();
    let hello = stable["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "org.eon.office.hello")
        .unwrap()
        .clone();
    assert_eq!(hello["state"], "unavailable");
    assert!(hello["reason"]
        .as_str()
        .unwrap()
        .contains("no stable release"));

    host.set_market_channel("preview").unwrap();
    let preview = host.catalogue().unwrap();
    let hello = preview["apps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == "org.eon.office.hello")
        .unwrap()
        .clone();
    assert_eq!(
        (hello["state"].clone(), hello["signed"].clone()),
        (json!("install"), json!(false))
    );

    // Unsigned without the person's yes: refused, nothing downloaded or installed.
    let refused = host
        .install_from_market("org.eon.office.hello", false)
        .unwrap_err();
    assert_eq!(refused["code"], "unsigned");
    assert_eq!(host.apps().unwrap()["apps"], json!([]));

    // With it: downloaded, checked, saved where the core looks, installed with nothing granted.
    let done = host
        .install_from_market("org.eon.office.hello", true)
        .unwrap();
    assert_eq!(done["installed"][0]["app"], "org.eon.office.hello");
    let app = host.apps().unwrap()["apps"][0].clone();
    assert_eq!(app["granted"], json!([]));
    let file = home
        .join("installed")
        .join("org.eon.office.hello")
        .join(format!("hello{}", std::env::consts::EXE_SUFFIX));
    assert!(file.is_file());

    // Granted, it runs: the core launches the downloaded file and it connects.
    for capability in app["pending"].as_array().unwrap() {
        host.set_permission("org.eon.office.hello", capability.as_str().unwrap(), true)
            .unwrap();
    }
    host.launch("org.eon.office.hello").unwrap();
    let mut connected = false;
    for _ in 0..100 {
        if host.apps().unwrap()["apps"][0]["connected"] == true {
            connected = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(connected, "the installed application did not connect");
    assert_eq!(
        host.catalogue().unwrap()["apps"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["id"] == "org.eon.office.hello")
            .unwrap()["state"],
        "installed"
    );

    host.stop("org.eon.office.hello").unwrap();
    host.leave();
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
#[ignore = "needs EON_OFFICE_CORE_BIN and the network (the live index on GitHub)"]
fn an_application_without_an_entry_installs_under_its_own_name() {
    let home = std::env::temp_dir().join(format!("eon-launcher-market2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let host = Host::connect(&home).unwrap();
    host.set_market_channel("preview").unwrap();
    // EON Caller recommends Hello but does not require it: only Caller is installed.
    let done = host
        .install_from_market("org.eon.office.caller", true)
        .unwrap();
    assert_eq!(done["installed"].as_array().unwrap().len(), 1);
    let file = home
        .join("installed")
        .join("org.eon.office.caller")
        .join(format!("caller{}", std::env::consts::EXE_SUFFIX));
    assert!(file.is_file());
    host.leave();
    let _ = std::fs::remove_dir_all(&home);
}

#[test]
#[ignore = "needs EON_OFFICE_CORE_BIN; CI builds eon-office-core and sets it"]
fn activity_becomes_recent_items_and_links_reach_the_app() {
    use eon_office_sdk::{Client, Incoming};
    let home = std::env::temp_dir().join(format!("eon-launcher-links-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    let host = Host::connect(&home).unwrap();

    let folder = home.join("notes-app");
    std::fs::create_dir_all(&folder).unwrap();
    let manifest = json!({
        "manifest": 0, "id": "org.example.notes", "name": "Notes", "version": "1.0.0",
        "api": { "min": 0, "max": 0 }, "capabilities": ["events.publish"],
        "provides": [{ "method": "open" }], "publishes": ["activity"],
    });
    std::fs::write(folder.join("manifest.json"), manifest.to_string()).unwrap();
    host.install(&folder.join("manifest.json").to_string_lossy())
        .unwrap();
    host.set_permission("org.example.notes", "events.publish", true)
        .unwrap();

    // The application itself, connecting as an independent program would.
    let token =
        std::fs::read_to_string(home.join("apps").join("org.example.notes").join("token")).unwrap();
    let endpoint = eon_office_sdk::endpoint(&home).unwrap();
    let app = Client::connect(&endpoint, Some("org.example.notes"), token.trim()).unwrap();
    app.publish(
        "activity",
        &json!({ "id": "n1", "title": "Shopping list", "kind": "edited" }),
    )
    .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut recorded = false;
    while Instant::now() < deadline && !recorded {
        if let Some(event) = host.next_event(Duration::from_millis(200)) {
            recorded = host.observe(&event);
        }
    }
    assert!(recorded, "the activity event did not become a recent item");
    assert_eq!(host.recent().unwrap()["items"][0]["title"], "Shopping list");

    // It survives the launcher: a second connection reads it back from the core.
    let again = Host::connect(&home).unwrap();
    assert_eq!(again.recent().unwrap()["items"][0]["id"], "n1");

    // A link reaches the app's `open`, with its query.
    let answering = std::thread::spawn(move || {
        while let Some(message) = app.next(Some(Duration::from_secs(10))) {
            if let Incoming::Invoke {
                id, method, params, ..
            } = message
            {
                assert_eq!(method, "open");
                app.answer(&id, Ok(json!({ "shown": params }))).unwrap();
                return params;
            }
        }
        json!(null)
    });
    let opened = host
        .open_link("eon-office://org.example.notes/n1?line%3D4")
        .unwrap();
    assert_eq!(opened["opened"], true);
    assert_eq!(
        answering.join().unwrap(),
        json!({ "id": "n1", "query": "line=4" })
    );

    let missing = host
        .open_link("eon-office://org.example.nothing/x")
        .unwrap();
    assert_eq!(missing["reason"], "not_installed");
    assert_eq!(
        host.open_link("https://example.org").unwrap_err()["code"],
        "invalid_link"
    );

    drop(again);
    host.leave();
    let _ = std::fs::remove_dir_all(&home);
}
