// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors
//
// Additional permission under GNU GPL version 3 section 7:
// see LICENSE-EXCEPTION.md (EON Module ABI Exception 1.0).

//! Installing from the marketplace, as `eon-office-spec/docs/marketplace.md`
//! says a client does: read the index, choose the newest compatible release,
//! download it, compare size and SHA-256, check the signature, save it where
//! the core looks for it, and install its manifest -- granting nothing.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

/// The project's index. A person can point the launcher at another one.
pub const DEFAULT_INDEX: &str =
    "https://raw.githubusercontent.com/EON-Extensible-Open-Network/eon-office-marketplace/main/index.json";

/// The index itself is small; anything larger is refused rather than read.
const INDEX_LIMIT: u64 = 8 * 1024 * 1024;

fn fail(code: &str, message: impl Into<String>) -> Value {
    json!({ "code": code, "message": message.into() })
}

/// This machine, in the index's platform names.
#[must_use]
pub fn platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    };
    format!("{os}-{}", std::env::consts::ARCH)
}

/// Download over https only, at most `limit` bytes.
pub fn fetch(url: &str, limit: u64) -> Result<Vec<u8>, Value> {
    if !url.starts_with("https://") {
        return Err(fail("insecure", "only https:// addresses are used"));
    }
    let response = ureq::get(url)
        .set(
            "User-Agent",
            concat!("eon-office-launcher/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| fail("network", format!("could not download: {e}")))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fail("network", format!("download interrupted: {e}")))?;
    if bytes.len() as u64 > limit {
        return Err(fail("too_large", "larger than the index says"));
    }
    Ok(bytes)
}

pub fn index(url: &str) -> Result<Value, Value> {
    let bytes = fetch(url, INDEX_LIMIT)?;
    let index: Value = serde_json::from_slice(&bytes)
        .map_err(|e| fail("invalid_index", format!("the index is not JSON: {e}")))?;
    if index["index"] != 0 || !index["apps"].is_array() {
        return Err(fail(
            "invalid_index",
            "not an EON Office index this launcher reads (index 0)",
        ));
    }
    Ok(index)
}

/// The SHA-256 of `bytes`, lowercase hex.
#[must_use]
pub fn sha256(bytes: &[u8]) -> String {
    eon_office_market::check_hex(&Sha256::digest(bytes))
}

/// The bytes are the file the index lists, or nothing is installed.
pub fn verify_bytes(bytes: &[u8], size: u64, expected: &str) -> Result<String, Value> {
    let actual = sha256(bytes);
    if bytes.len() as u64 != size || actual != expected {
        return Err(fail(
            "checksum",
            format!(
                "the downloaded file is not the listed one ({} bytes, sha256 {actual}; listed {size} bytes, {expected})",
                bytes.len()
            ),
        ));
    }
    Ok(actual)
}

/// Where a single-file artefact goes: `<home>/installed/<id>/<command><suffix>`,
/// which is where the core looks for a relative `entry.command`.
pub fn save(home: &Path, id: &str, command: &str, bytes: &[u8]) -> Result<PathBuf, Value> {
    let bad = || {
        fail(
            "invalid_manifest",
            "the manifest's entry command is not a plain file name",
        )
    };
    if command.is_empty() || command.contains(['/', '\\', ':']) || command.starts_with('.') {
        return Err(bad());
    }
    if !eon_office_market::check::is_app_id(id) {
        return Err(fail("invalid_manifest", "not an application id"));
    }
    let folder = home.join("installed").join(id);
    std::fs::create_dir_all(&folder).map_err(|e| fail("io", e.to_string()))?;
    let file = folder.join(format!("{command}{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&file, bytes).map_err(|e| fail("io", e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| fail("io", e.to_string()))?;
    }
    Ok(folder)
}

/// The keys this machine trusts to sign Office releases: `<home>/trust.json`,
/// empty when absent -- which is everyone's state until madde 30's legal
/// entity holds keys.
#[must_use]
pub fn trust(home: &Path) -> Value {
    std::fs::read_to_string(home.join("trust.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| json!({ "keys": [] }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_file_is_refused() {
        let bytes = b"hello";
        let good = sha256(bytes);
        assert!(verify_bytes(bytes, 5, &good).is_ok());
        assert_eq!(
            verify_bytes(bytes, 6, &good).unwrap_err()["code"],
            "checksum"
        );
        assert_eq!(
            verify_bytes(b"hellO", 5, &good).unwrap_err()["code"],
            "checksum"
        );
    }

    #[test]
    fn only_https_is_fetched() {
        assert_eq!(
            fetch("http://example.org/x", 10).unwrap_err()["code"],
            "insecure"
        );
        assert_eq!(
            fetch("file:///etc/passwd", 10).unwrap_err()["code"],
            "insecure"
        );
    }

    #[test]
    fn an_entry_command_cannot_escape_its_folder() {
        let home = std::env::temp_dir().join(format!("eon-market-save-{}", std::process::id()));
        for command in ["../evil", "a/b", "C:x", ".hidden", ""] {
            assert!(save(&home, "org.a.b", command, b"x").is_err(), "{command}");
        }
        let folder = save(&home, "org.a.b", "tool", b"x").unwrap();
        assert!(folder
            .join(format!("tool{}", std::env::consts::EXE_SUFFIX))
            .is_file());
        let _ = std::fs::remove_dir_all(&home);
    }
}
