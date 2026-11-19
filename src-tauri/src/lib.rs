// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! The launcher's host, without the window.
//!
//! Everything the window does goes through [`host::Host`], which is plain
//! Rust over `eon-office-sdk` and can therefore be tested against a real core
//! (`tests/host.rs`) without anyone opening a window. [`checks`] holds the
//! static checks `--selftest` runs on the interface and its strings.

pub mod checks;
pub mod host;
pub mod links;
pub mod market;

/// The interface's strings, compiled in.
pub const EN: &str = include_str!("../i18n/en.json");
pub const TR: &str = include_str!("../i18n/tr.json");

/// The strings for `language` (`tr`, or anything else for English).
#[must_use]
pub fn strings(language: &str) -> serde_json::Value {
    let text = if language.starts_with("tr") { TR } else { EN };
    serde_json::from_str(text).unwrap_or(serde_json::Value::Null)
}
