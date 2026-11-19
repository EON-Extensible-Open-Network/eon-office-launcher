// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The owner's connection to the core, and every action the window offers.
//!
//! The launcher holds the owner's token; the interface never sees it. Every
//! result is plain JSON and every failure is `{ code, message }` with the
//! protocol's stable code, so the interface chooses its own words.

use eon_office_sdk::{Client, Error, Incoming};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

pub type Outcome = Result<Value, Value>;

fn fail(code: &str, message: impl Into<String>) -> Value {
    json!({ "code": code, "message": message.into() })
}

fn from_sdk(error: &Error) -> Value {
    let mut out = fail(error.code(), error.to_string());
    if let Error::Protocol { data, .. } = error {
        out["data"] = data.clone();
    }
    out
}

fn sdk(result: Result<Value, Error>) -> Outcome {
    result.map_err(|e| from_sdk(&e))
}

/// Where the core binary is: `EON_OFFICE_CORE_BIN`, beside this executable,
/// or on `PATH`.
#[must_use]
pub fn core_binary() -> Option<PathBuf> {
    let name = format!("eon-office-core{}", std::env::consts::EXE_SUFFIX);
    if let Some(explicit) = std::env::var_os("EON_OFFICE_CORE_BIN").map(PathBuf::from) {
        if explicit.is_file() {
            return Some(explicit);
        }
    }
    if let Some(beside) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
    {
        if beside.is_file() {
            return Some(beside);
        }
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(&name))
            .find(|candidate| candidate.is_file())
    })
}

pub struct Host {
    client: Client,
    home: PathBuf,
    /// The core, if this launcher started it.
    started: Mutex<Option<Child>>,
}

impl Host {
    /// Connect to the core for `home` as its owner, starting it if it is not
    /// running.
    pub fn connect(home: &Path) -> Result<Host, Value> {
        if let Ok(client) = Client::connect_owner(home) {
            return Host::ready(client, home, None);
        }
        let binary = core_binary().ok_or_else(|| {
            fail(
                "core_missing",
                "eon-office-core was not found beside the launcher or on PATH",
            )
        })?;
        let mut command = Command::new(&binary);
        command
            .arg("serve")
            .arg("--home")
            .arg(home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: the core is a console program; started from a
            // window it would otherwise open an empty console beside it.
            command.creation_flags(0x0800_0000);
        }
        let mut child = command
            .spawn()
            .map_err(|e| fail("core_missing", format!("could not start the core: {e}")))?;
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(client) = Client::connect_owner(home) {
                return Host::ready(client, home, Some(child));
            }
            if let Ok(Some(status)) = child.try_wait() {
                return Err(fail(
                    "core_failed",
                    format!("the core exited at start-up ({status})"),
                ));
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                return Err(fail("core_failed", "the core did not answer in 15 seconds"));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn ready(client: Client, home: &Path, started: Option<Child>) -> Result<Host, Value> {
        // Everything the shell shows live: installs, starts, crashes,
        // notifications, preference changes.
        client.subscribe("core/*").map_err(|e| from_sdk(&e))?;
        Ok(Host {
            client,
            home: home.to_owned(),
            started: Mutex::new(started),
        })
    }

    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    #[must_use]
    pub fn started_core(&self) -> bool {
        self.started
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// The next live event, as `{ topic, from, payload }`.
    pub fn next_event(&self, timeout: Duration) -> Option<Value> {
        match self.client.next(Some(timeout))? {
            Incoming::Event {
                topic,
                from,
                payload,
            } => Some(json!({ "topic": topic, "from": from, "payload": payload })),
            Incoming::Shutdown => Some(json!({ "topic": "core/shutdown" })),
            _ => Some(Value::Null),
        }
    }

    pub fn status(&self) -> Outcome {
        let info = sdk(self.client.call("core.info", json!({})))?;
        let me = sdk(self.client.call("identity.me", json!({})))?;
        Ok(json!({
            "core": info,
            "user": me["user"],
            "home": self.home,
            "launcher_version": env!("CARGO_PKG_VERSION"),
        }))
    }

    pub fn apps(&self) -> Outcome {
        sdk(self.client.call("apps.list", json!({})))
    }

    /// Read a manifest and show what it asks for, without installing.
    pub fn preview(&self, path: &str) -> Outcome {
        let path = PathBuf::from(path.trim().trim_matches('"'));
        let text = std::fs::read_to_string(&path)
            .map_err(|e| fail("unreadable", format!("{}: {e}", path.display())))?;
        let manifest: Value = serde_json::from_str(&text)
            .map_err(|e| fail("invalid_params", format!("not JSON: {e}")))?;
        for key in ["id", "name", "version"] {
            if !manifest[key].is_string() {
                return Err(fail(
                    "invalid_params",
                    format!("the manifest has no `{key}`"),
                ));
            }
        }
        Ok(json!({ "path": path, "manifest": manifest }))
    }

    /// Install from a manifest file. Its folder becomes the install path, so a
    /// relative `entry.command` resolves beside the manifest. Grants nothing.
    pub fn install(&self, path: &str) -> Outcome {
        let preview = self.preview(path)?;
        let file = PathBuf::from(preview["path"].as_str().unwrap_or_default());
        let folder = file
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        sdk(self.client.call(
            "apps.install",
            json!({ "manifest": preview["manifest"], "path": folder }),
        ))
    }

    pub fn set_permission(&self, app: &str, capability: &str, allowed: bool) -> Outcome {
        let method = if allowed { "apps.grant" } else { "apps.revoke" };
        sdk(self
            .client
            .call(method, json!({ "app": app, "capabilities": [capability] })))
    }

    pub fn launch(&self, app: &str) -> Outcome {
        sdk(self.client.call("apps.launch", json!({ "app": app })))
    }

    pub fn stop(&self, app: &str) -> Outcome {
        sdk(self.client.call("apps.stop", json!({ "app": app })))
    }

    pub fn uninstall(&self, app: &str) -> Outcome {
        sdk(self.client.call("apps.uninstall", json!({ "app": app })))
    }

    pub fn notifications(&self, unread_only: bool) -> Outcome {
        sdk(self.client.call(
            "notifications.list",
            json!({ "unread_only": unread_only, "limit": 200 }),
        ))
    }

    pub fn mark_read(&self, id: i64) -> Outcome {
        sdk(self
            .client
            .call("notifications.mark_read", json!({ "id": id })))
    }

    pub fn search(&self, query: &str) -> Outcome {
        sdk(self
            .client
            .call("search.query", json!({ "query": query, "limit": 50 })))
    }

    /// Ask the application that owns a search result to open it (the
    /// well-known `open` method). `opened: false` with a reason when it cannot.
    pub fn open(&self, app: &str, id: &str) -> Outcome {
        match self.client.call_app(app, "open", json!({ "id": id })) {
            Ok(_) => Ok(json!({ "opened": true })),
            Err(error)
                if matches!(
                    error.code(),
                    "method_not_found" | "app_unavailable" | "remote_error" | "timeout"
                ) =>
            {
                Ok(json!({ "opened": false, "reason": error.code() }))
            }
            Err(error) => Err(from_sdk(&error)),
        }
    }

    pub fn prefs(&self) -> Outcome {
        sdk(self.client.prefs())
    }

    pub fn set_pref(&self, key: &str, value: Value) -> Outcome {
        sdk(self
            .client
            .call("prefs.set", json!({ "key": key, "value": value })))
    }

    pub fn activity(&self) -> Outcome {
        // The newest hundred, newest first.
        let verify = sdk(self.client.call("audit.verify", json!({})))?;
        let count = verify["entries"].as_u64().unwrap_or(0);
        let after = count.saturating_sub(100);
        let list = sdk(self
            .client
            .call("audit.list", json!({ "after": after, "limit": 100 })))?;
        let mut entries = list["entries"].as_array().cloned().unwrap_or_default();
        entries.reverse();
        Ok(json!({ "entries": entries, "verify": verify }))
    }

    /// On exit: if this launcher started the core and no application is
    /// connected to it, stop it, so closing the launcher leaves nothing
    /// running that the person did not start. Otherwise leave it for the
    /// applications that use it.
    pub fn leave(&self) {
        let mut started = self.started.lock().unwrap_or_else(PoisonError::into_inner);
        if started.is_none() {
            return;
        }
        let anyone = self
            .apps()
            .ok()
            .and_then(|list| list["apps"].as_array().cloned())
            .is_some_and(|apps| apps.iter().any(|a| a["connected"] == true));
        if !anyone {
            let _ = self.client.call("core.shutdown", json!({}));
            if let Some(mut child) = started.take() {
                let deadline = Instant::now() + Duration::from_secs(5);
                while Instant::now() < deadline {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                let _ = child.kill();
            }
        }
    }
}
