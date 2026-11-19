// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! `eon-office://` links and `activity` events, as eon-office-spec's
//! protocol.md defines them. Both arrive from outside -- a link from another
//! program, an event from any application -- so both are parsed strictly and
//! anything that does not fit is refused, not guessed at.

use serde_json::{json, Value};

/// A parsed `eon-office://<app-id>/<item-id>[?<query>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub app: String,
    pub id: String,
    pub query: String,
}

fn is_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 128
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
}

fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[must_use]
pub fn parse(link: &str) -> Option<Link> {
    let rest = link.trim().strip_prefix("eon-office://")?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (app, id) = path.trim_end_matches('/').split_once('/')?;
    let id = percent_decode(id)?;
    let ok = eon_office_market::check::is_app_id(app) && is_name(&id) && query.len() <= 1024;
    ok.then(|| Link {
        app: app.to_owned(),
        id,
        query: percent_decode(query).unwrap_or_default(),
    })
}

/// The most recent items kept.
pub const RECENT: usize = 50;

/// Fold an `activity` event into the recent list (newest first, one entry per
/// item). Returns false, changing nothing, for anything that is not a
/// well-formed activity event from the application it names.
pub fn record(recent: &mut Vec<Value>, event: &Value) -> bool {
    let topic = event["topic"].as_str().unwrap_or_default();
    let from = event["from"].as_str().unwrap_or_default();
    // Only `<app>/activity`, published by that app itself.
    if topic != format!("{from}/activity") || !eon_office_market::check::is_app_id(from) {
        return false;
    }
    let payload = &event["payload"];
    let id = payload["id"].as_str().unwrap_or_default();
    let title = payload["title"].as_str().unwrap_or_default().trim();
    let kind = payload["kind"].as_str().unwrap_or_default();
    if !is_name(id)
        || title.is_empty()
        || title.chars().count() > 200
        || !matches!(kind, "created" | "opened" | "edited" | "shared")
    {
        return false;
    }
    let at = payload["at"].as_u64().unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(0))
    });
    recent.retain(|r| !(r["app"] == from && r["id"] == id));
    recent.insert(
        0,
        json!({ "app": from, "id": id, "title": title, "kind": kind, "at": at }),
    );
    recent.truncate(RECENT);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_parse_strictly() {
        assert_eq!(
            parse("eon-office://org.eon.office.notes/n-2026-11-19?line=4"),
            Some(Link {
                app: "org.eon.office.notes".into(),
                id: "n-2026-11-19".into(),
                query: "line=4".into()
            })
        );
        assert_eq!(
            parse("eon-office://org.a.b/x%3Ay").map(|l| l.id),
            Some("x:y".into())
        );
        for bad in [
            "https://org.a.b/x",
            "eon-office://notes/x",
            "eon-office://org.a.b/",
            "eon-office://org.a.b/../etc",
            "eon-office://org.a.b/a b",
            "eon-office://org.a.b/%zz",
        ] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    fn event(from: &str, topic: &str, id: &str, title: &str) -> Value {
        json!({ "topic": topic, "from": from, "payload": { "id": id, "title": title, "kind": "edited", "at": 5 } })
    }

    #[test]
    fn activity_is_recorded_newest_first_once_per_item() {
        let mut recent = Vec::new();
        assert!(record(
            &mut recent,
            &event("org.a.notes", "org.a.notes/activity", "n1", "One")
        ));
        assert!(record(
            &mut recent,
            &event("org.a.notes", "org.a.notes/activity", "n2", "Two")
        ));
        assert!(record(
            &mut recent,
            &event("org.a.notes", "org.a.notes/activity", "n1", "One again")
        ));
        let titles: Vec<&str> = recent.iter().filter_map(|r| r["title"].as_str()).collect();
        assert_eq!(titles, vec!["One again", "Two"]);
    }

    #[test]
    fn foreign_or_malformed_activity_is_ignored() {
        let mut recent = Vec::new();
        // Another app's namespace, an unknown kind, an empty title: refused.
        assert!(!record(
            &mut recent,
            &event("org.evil.app", "org.a.notes/activity", "n1", "x")
        ));
        assert!(!record(
            &mut recent,
            &event("org.a.notes", "org.a.notes/activity", "n1", "")
        ));
        let mut wrong = event("org.a.notes", "org.a.notes/activity", "n1", "x");
        wrong["payload"]["kind"] = json!("deleted-everything");
        assert!(!record(&mut recent, &wrong));
        assert!(recent.is_empty());
        for _ in 0..(RECENT + 5) {
            let n = recent.len();
            record(
                &mut recent,
                &event("org.a.notes", "org.a.notes/activity", &format!("n{n}"), "t"),
            );
        }
        assert_eq!(recent.len(), RECENT);
    }
}
