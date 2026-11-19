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
    /// Recent items from `activity` events, newest first; kept in the core
    /// as the owner's setting `launcher.recent`, on this device only.
    recent: Mutex<Vec<Value>>,
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
        // notifications, preference changes -- and every application's
        // `activity` events, for recent items. The owner may subscribe to all.
        client.subscribe("*").map_err(|e| from_sdk(&e))?;
        let recent = client
            .call("settings.get", json!({ "key": "launcher.recent" }))
            .ok()
            .and_then(|v| v["value"].as_array().cloned())
            .unwrap_or_default();
        Ok(Host {
            client,
            home: home.to_owned(),
            started: Mutex::new(started),
            recent: Mutex::new(recent),
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

    /// Fold a live event into what the launcher keeps. True when it changed
    /// the recent items.
    pub fn observe(&self, event: &Value) -> bool {
        let mut recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        if !crate::links::record(&mut recent, event) {
            return false;
        }
        let _ = self.client.call(
            "settings.set",
            json!({ "key": "launcher.recent", "value": *recent }),
        );
        true
    }

    pub fn recent(&self) -> Outcome {
        let recent = self.recent.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(json!({ "items": *recent }))
    }

    /// Follow an `eon-office://` link: the application must be installed; if
    /// it is launchable and not running it is started; then its `open` method
    /// is asked for the item.
    pub fn open_link(&self, link: &str) -> Outcome {
        let parsed = crate::links::parse(link).ok_or_else(|| {
            fail(
                "invalid_link",
                "not an eon-office:// link this launcher understands",
            )
        })?;
        let apps = self.apps()?;
        let Some(app) = apps["apps"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|a| a["id"] == parsed.app.as_str())
            .cloned()
        else {
            return Ok(json!({ "opened": false, "reason": "not_installed", "app": parsed.app }));
        };
        if app["connected"] != true && app["launchable"] == true {
            self.launch(&parsed.app)?;
            let deadline = Instant::now() + Duration::from_secs(10);
            while Instant::now() < deadline {
                let up = self.apps()?["apps"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|a| a["id"] == parsed.app.as_str() && a["connected"] == true);
                if up {
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        match self.client.call_app(
            &parsed.app,
            "open",
            json!({ "id": parsed.id, "query": parsed.query }),
        ) {
            Ok(_) => Ok(json!({ "opened": true, "app": parsed.app })),
            Err(error) => Ok(json!({ "opened": false, "reason": error.code(), "app": parsed.app })),
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

    // ---- the marketplace ----------------------------------------------------

    /// Where the index is and which channel the person follows. Stored in the
    /// core as the owner's settings, so they survive the launcher.
    pub fn market_settings(&self) -> Outcome {
        let get = |key: &str| {
            self.client
                .call("settings.get", json!({ "key": key }))
                .map(|v| v["value"].clone())
        };
        let url = sdk(get("market.url"))?;
        let channel = sdk(get("market.channel"))?;
        Ok(json!({
            "url": url.as_str().unwrap_or(crate::market::DEFAULT_INDEX),
            "channel": channel.as_str().unwrap_or("stable"),
            "platform": crate::market::platform(),
        }))
    }

    pub fn set_market_channel(&self, channel: &str) -> Outcome {
        if !matches!(channel, "stable" | "preview") {
            return Err(fail("invalid_params", "a channel is stable or preview"));
        }
        sdk(self.client.call(
            "settings.set",
            json!({ "key": "market.channel", "value": channel }),
        ))
    }

    fn installed_versions(&self) -> Result<std::collections::BTreeMap<String, String>, Value> {
        let apps = self.apps()?;
        Ok(apps["apps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| {
                Some((
                    a["id"].as_str()?.to_owned(),
                    a["version"].as_str()?.to_owned(),
                ))
            })
            .collect())
    }

    fn target(&self) -> Result<eon_office_market::plan::Target, Value> {
        let settings = self.market_settings()?;
        let api = sdk(self.client.call("core.info", json!({})))?["api"]
            .as_u64()
            .unwrap_or(0);
        Ok(eon_office_market::plan::Target {
            api,
            platform: crate::market::platform(),
            channel: settings["channel"].as_str().unwrap_or("stable").to_owned(),
        })
    }

    /// The index, as this machine sees it: for each application, the release
    /// it would install (or why none), and whether it is installed already.
    pub fn catalogue(&self) -> Outcome {
        let settings = self.market_settings()?;
        let index = crate::market::index(settings["url"].as_str().unwrap_or_default())?;
        let target = self.target()?;
        let installed = self.installed_versions()?;
        let mut out = Vec::new();
        for entry in index["apps"].as_array().into_iter().flatten() {
            let id = entry["id"].as_str().unwrap_or_default();
            let have = installed.get(id).cloned();
            let mut item = json!({
                "id": id, "name": entry["name"], "summary": entry["summary"],
                "description": entry["description"], "license": entry["license"],
                "source": entry["source"], "publisher": entry["publisher"],
                "official": entry["official"], "installed": have,
            });
            match eon_office_market::plan::choose(entry, &target) {
                Ok((release, artifact)) => {
                    let version = release["version"].as_str().unwrap_or_default();
                    let newer = have.as_deref().is_none_or(|h| {
                        eon_office_market::semver::Version::parse(version)
                            > eon_office_market::semver::Version::parse(h)
                    });
                    item["version"] = json!(version);
                    item["channel"] = release["channel"].clone();
                    item["capabilities"] = release["manifest"]["capabilities"].clone();
                    item["size"] = artifact["size"].clone();
                    item["signed"] = json!(artifact.get("signature").is_some_and(|s| !s.is_null()));
                    item["state"] = json!(match (&have, newer) {
                        (None, _) => "install",
                        (Some(_), true) => "update",
                        (Some(_), false) => "installed",
                    });
                }
                Err(reason) => {
                    item["state"] = json!("unavailable");
                    item["reason"] = json!(reason);
                }
            }
            out.push(item);
        }
        Ok(json!({ "apps": out, "settings": settings }))
    }

    /// Install (or update) `app` and whatever it requires, from the index.
    /// An unsigned release needs `allow_unsigned`: the person saw that it is
    /// unsigned and said yes. A signature that does not verify is refused
    /// whatever they say.
    pub fn install_from_market(&self, app: &str, allow_unsigned: bool) -> Outcome {
        use eon_office_market::signature::{verify, Verdict};
        let settings = self.market_settings()?;
        let index = crate::market::index(settings["url"].as_str().unwrap_or_default())?;
        let apps: Vec<Value> = index["apps"].as_array().cloned().unwrap_or_default();
        let target = self.target()?;
        let installed = self.installed_versions()?;
        let steps = eon_office_market::plan::plan(&apps, app, &installed, &target)
            .map_err(|reason| fail("unavailable", reason))?;
        let trust = crate::market::trust(&self.home);
        // Check every step's signature before downloading anything, so a
        // refusal half-way does not leave half an install.
        for step in &steps {
            let id = step["app"].as_str().unwrap_or_default();
            if step["signed"] != true && !allow_unsigned {
                return Err(json!({
                    "code": "unsigned",
                    "message": format!("{id} {} is not signed", step["version"].as_str().unwrap_or("?")),
                    "data": { "app": id },
                }));
            }
        }
        let mut done = Vec::new();
        for step in &steps {
            let id = step["app"].as_str().unwrap_or_default();
            let version = step["version"].as_str().unwrap_or_default();
            let size = step["size"].as_u64().unwrap_or(0);
            let bytes = crate::market::fetch(step["url"].as_str().unwrap_or_default(), size)?;
            let hash = crate::market::verify_bytes(
                &bytes,
                size,
                step["sha256"].as_str().unwrap_or_default(),
            )?;
            let signature = apps
                .iter()
                .find(|a| a["id"] == id)
                .and_then(|a| a["releases"].as_array())
                .and_then(|r| r.iter().find(|r| r["version"] == version))
                .and_then(|r| r["artifacts"].as_array())
                .and_then(|a| a.iter().find(|a| a["platform"] == target.platform.as_str()))
                .and_then(|a| a.get("signature"))
                .cloned();
            match verify(signature.as_ref(), id, version, &hash, &trust) {
                Verdict::Trusted(_) => {}
                Verdict::Unsigned if allow_unsigned => {}
                Verdict::Unsigned => {
                    return Err(fail("unsigned", format!("{id} {version} is not signed")));
                }
                Verdict::Refused(reason) => {
                    return Err(fail(
                        "signature_refused",
                        format!("{id} {version}: {reason}"),
                    ));
                }
            }
            // An application with no `entry` starts on its own; its file is
            // named after the last label of its id (marketplace.md, Install).
            let fallback = id.rsplit('.').next().unwrap_or_default();
            let command = step["manifest"]["entry"]["command"]
                .as_str()
                .unwrap_or(fallback);
            let folder = crate::market::save(&self.home, id, command, &bytes)?;
            sdk(self.client.call(
                "apps.install",
                json!({ "manifest": step["manifest"], "path": folder }),
            ))?;
            done.push(json!({ "app": id, "version": version, "reason": step["reason"] }));
        }
        Ok(json!({ "installed": done }))
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
