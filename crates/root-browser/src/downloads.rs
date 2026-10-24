//! Download log used by `Session` and the `root://downloads` page.
//!
//! `profile.rs` stores the log inside the profile JSON through `to_json_value`.
//! Digests come from `root_net::sha256_digest`. Names are reduced to a short
//! ASCII file name. Extensions in `QUARANTINED_EXTENSIONS` are marked
//! quarantined and, when saved, written with a `.quarantined` suffix. They are
//! still written. Windows saves also try to add a `Zone.Identifier` stream.
//! Other targets do not write that stream. `motw_tagged` is set true on the
//! record even when no file is written.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use root_json::JsonValue;
use root_net::sha256_digest;

/// Maximum number of download entries retained in the session log.
pub const MAX_DOWNLOAD_ENTRIES: usize = 128;

/// Maximum single download payload size (`64 MiB`).
pub const MAX_SINGLE_DOWNLOAD_BYTES: usize = 64 * 1024 * 1024;

/// Extensions that `is_quarantined_extension` treats as executable or script names.
const QUARANTINED_EXTENSIONS: &[&str] = &[
    "exe", "bat", "cmd", "ps1", "scr", "vbs", "js", "msi", "com", "dll", "hta", "reg",
];

/// Security classification of a downloaded file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DownloadState {
    /// Checksum verified and file extension is non-executable.
    VerifiedSafe,
    /// Executable or script file extension quarantined against automatic execution.
    QuarantinedExecutable,
    /// Download or verification failed.
    Failed(String),
}

impl DownloadState {
    /// Short status text shown on the downloads page.
    pub fn label(&self) -> &'static str {
        match self {
            DownloadState::VerifiedSafe => "VERIFIED SAFE",
            DownloadState::QuarantinedExecutable => "QUARANTINED (EXECUTABLE)",
            DownloadState::Failed(_) => "FAILED",
        }
    }

    /// Stable state name stored in profile JSON. The `Failed` message is not stored.
    pub fn code(&self) -> &'static str {
        match self {
            DownloadState::VerifiedSafe => "verified",
            DownloadState::QuarantinedExecutable => "quarantined",
            DownloadState::Failed(_) => "failed",
        }
    }
}

/// Metadata and cryptographic integrity record for a single downloaded file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DownloadEntry {
    /// Id assigned by `DownloadManager`. Restored rows keep the stored id.
    pub id: u64,
    /// Sanitized file name, without a directory.
    pub file_name: String,
    /// URL passed by the caller. Not fetched by this module.
    pub source_url: String,
    /// Caller-supplied type, or `application/octet-stream` when blank.
    pub mime_type: String,
    /// Payload length in bytes, including payloads that were rejected for size.
    pub byte_size: usize,
    /// Lowercase SHA-256 hex, or the literal `exceeded-64mib-cap` when the payload is too large.
    pub sha256_hex: String,
    /// Set true by `record_download`. It does not mean a zone stream was written.
    pub motw_tagged: bool,
    /// Safe, quarantined, or failed.
    pub state: DownloadState,
}

/// Session download manager enforcing size bounds, filename sanitization, SHA-256 hashing, and quarantine policy.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DownloadManager {
    next_id: u64,
    entries: Vec<DownloadEntry>,
}

impl DownloadManager {
    /// Empty log. The next id is 1.
    pub fn new() -> Self {
        Self {
            next_id: 1,
            entries: Vec::new(),
        }
    }

    /// Number of retained entries, at most `MAX_DOWNLOAD_ENTRIES`.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// True when `len` is zero.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Entries in insertion order. The oldest entry is first.
    pub fn entries(&self) -> &[DownloadEntry] {
        &self.entries
    }

    /// Drop every entry. The next id is left unchanged.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Record an in-memory payload. This function does not fetch or write a file.
    ///
    /// Payloads larger than `MAX_SINGLE_DOWNLOAD_BYTES` are stored as `Failed`
    /// and are not hashed. Other payloads are hashed. A quarantined extension
    /// sets `QuarantinedExecutable` and still keeps the digest.
    pub fn record_download(
        &mut self,
        source_url: &str,
        suggested_name: Option<&str>,
        mime_type: &str,
        payload: &[u8],
    ) -> DownloadEntry {
        let id = self.next_id.max(1);
        self.next_id = id.saturating_add(1);

        let raw_name = suggested_name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| infer_filename_from_url(source_url));
        let file_name = sanitize_download_filename(raw_name);

        if payload.len() > MAX_SINGLE_DOWNLOAD_BYTES {
            let entry = DownloadEntry {
                id,
                file_name,
                source_url: source_url.trim().to_string(),
                mime_type: mime_type.trim().to_string(),
                byte_size: payload.len(),
                sha256_hex: String::from("exceeded-64mib-cap"),
                motw_tagged: true,
                state: DownloadState::Failed("exceeds 64 MiB download limit".to_string()),
            };
            self.push_entry(entry.clone());
            return entry;
        }

        let digest = sha256_digest(payload);
        let sha256_hex = hex_encode_32(&digest);
        let state = if is_quarantined_extension(&file_name) {
            DownloadState::QuarantinedExecutable
        } else {
            DownloadState::VerifiedSafe
        };

        let entry = DownloadEntry {
            id,
            file_name,
            source_url: source_url.trim().to_string(),
            mime_type: if mime_type.trim().is_empty() {
                "application/octet-stream".to_string()
            } else {
                mime_type.trim().to_string()
            },
            byte_size: payload.len(),
            sha256_hex,
            motw_tagged: true,
            state,
        };
        self.push_entry(entry.clone());
        entry
    }

    /// Append `entry`, dropping the oldest row when the log is already full.
    fn push_entry(&mut self, entry: DownloadEntry) {
        if self.entries.len() >= MAX_DOWNLOAD_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(entry);
    }

    /// Record `payload`, then write it under `target_dir` when the state is not `Failed`.
    ///
    /// A quarantined name is saved as `<name>.quarantined`. `apply_mark_of_the_web`
    /// runs after the write. A size failure returns `InvalidData` after the failed
    /// row is already in the log.
    pub fn save_payload_to_dir(
        &mut self,
        source_url: &str,
        suggested_name: Option<&str>,
        mime_type: &str,
        payload: &[u8],
        target_dir: &Path,
    ) -> io::Result<(DownloadEntry, PathBuf)> {
        let entry = self.record_download(source_url, suggested_name, mime_type, payload);
        if let DownloadState::Failed(ref reason) = entry.state {
            return Err(io::Error::new(io::ErrorKind::InvalidData, reason.clone()));
        }
        fs::create_dir_all(target_dir)?;
        let safe_name = if entry.state == DownloadState::QuarantinedExecutable {
            format!("{}.quarantined", entry.file_name)
        } else {
            entry.file_name.clone()
        };
        let dest = target_dir.join(safe_name);
        fs::write(&dest, payload)?;
        apply_mark_of_the_web(&dest, source_url);
        Ok((entry, dest))
    }

    /// JSON array of entries. `state` is the code from `DownloadState::code`.
    pub fn to_json_value(&self) -> JsonValue {
        let mut arr = JsonValue::empty_array();
        for item in &self.entries {
            let mut obj = JsonValue::empty_object();
            obj.insert("id", JsonValue::Number(item.id as f64));
            obj.insert("file_name", JsonValue::String(item.file_name.clone()));
            obj.insert("source_url", JsonValue::String(item.source_url.clone()));
            obj.insert("mime_type", JsonValue::String(item.mime_type.clone()));
            obj.insert("byte_size", JsonValue::Number(item.byte_size as f64));
            obj.insert("sha256_hex", JsonValue::String(item.sha256_hex.clone()));
            obj.insert("motw_tagged", JsonValue::Bool(item.motw_tagged));
            obj.insert("state", JsonValue::String(item.state.code().to_string()));
            arr.push(obj);
        }
        arr
    }

    /// Rebuild a log from a JSON array. Rows without `file_name` are skipped.
    ///
    /// At most `MAX_DOWNLOAD_ENTRIES` rows are kept. A `failed` state comes back
    /// as `Failed("restored failed state")` because the original message is not stored.
    pub fn from_json_array(items: &[JsonValue]) -> Self {
        let mut mgr = Self::new();
        for item in items.iter().take(MAX_DOWNLOAD_ENTRIES) {
            let Some(file_name) = item.get_str("file_name") else {
                continue;
            };
            let id = item.get_i64("id").unwrap_or(1).max(1) as u64;
            let source_url = item.get_str("source_url").unwrap_or("").to_string();
            let mime_type = item
                .get_str("mime_type")
                .unwrap_or("application/octet-stream")
                .to_string();
            let byte_size = item.get_i64("byte_size").unwrap_or(0).max(0) as usize;
            let sha256_hex = item.get_str("sha256_hex").unwrap_or("").to_string();
            let motw_tagged = item.get_bool("motw_tagged").unwrap_or(true);
            let state = match item.get_str("state").unwrap_or("verified") {
                "quarantined" => DownloadState::QuarantinedExecutable,
                "failed" => DownloadState::Failed("restored failed state".to_string()),
                _ => DownloadState::VerifiedSafe,
            };
            mgr.next_id = mgr.next_id.max(id.saturating_add(1));
            mgr.entries.push(DownloadEntry {
                id,
                file_name: sanitize_download_filename(file_name),
                source_url,
                mime_type,
                byte_size,
                sha256_hex,
                motw_tagged,
                state,
            });
        }
        mgr
    }
}

/// Strip directory traversal components (`..`, `/`, `\`, `:`) and control characters from a downloaded file name.
pub fn sanitize_download_filename(raw: &str) -> String {
    let base = raw
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("download.bin");
    let mut cleaned = String::with_capacity(base.len().min(128));
    for ch in base.chars() {
        if cleaned.len() >= 120 {
            break;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
            cleaned.push(ch);
        } else if ch == ' ' {
            cleaned.push('_');
        }
    }
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "download.bin".to_string()
    } else {
        trimmed.to_string()
    }
}

/// True when the text after the last dot is one of `QUARANTINED_EXTENSIONS`.
pub fn is_quarantined_extension(file_name: &str) -> bool {
    let lower = file_name.trim().to_ascii_lowercase();
    let Some((_, ext)) = lower.rsplit_once('.') else {
        return false;
    };
    QUARANTINED_EXTENSIONS.contains(&ext)
}

/// Last path segment of `url`, or `download.bin` when that segment has no dot.
fn infer_filename_from_url(url: &str) -> &str {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let last = without_query.rsplit('/').next().unwrap_or("download.bin");
    if last.is_empty() || !last.contains('.') {
        "download.bin"
    } else {
        last
    }
}

/// Lowercase hex for a 32-byte digest. The result is 64 characters.
fn hex_encode_32(digest: &[u8; 32]) -> String {
    /// Lowercase hex digits, index by nibble.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &b in digest {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// On Windows, write a `Zone.Identifier` stream with `ZoneId=3` beside `file_path`.
///
/// Newlines in `source_url` are dropped and the URL is capped at 256 characters.
/// A failed write is ignored. On other targets this function does nothing.
fn apply_mark_of_the_web(file_path: &Path, source_url: &str) {
    #[cfg(target_os = "windows")]
    {
        let mut ads_os = file_path.as_os_str().to_os_string();
        ads_os.push(":Zone.Identifier");
        let ads_path = PathBuf::from(ads_os);
        let safe_url: String = source_url
            .chars()
            .filter(|c| !matches!(c, '\r' | '\n'))
            .take(256)
            .collect();
        let content = format!("[ZoneTransfer]\r\nZoneId=3\r\nHostUrl={safe_url}\r\n");
        let _ = fs::write(ads_path, content);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (file_path, source_url);
    }
}

#[cfg(test)]
mod tests {
    //! Filename cleanup, the extension list, and one safe versus one quarantined record.
    use super::*;

    #[test]
    /// Strip a traversal prefix, hash a JSON payload, and mark an exe name quarantined.
    fn sanitizes_filenames_verifies_sha256_and_quarantines_executables() {
        assert_eq!(
            sanitize_download_filename("../../Windows/System32/cmd.exe"),
            "cmd.exe"
        );
        assert!(is_quarantined_extension("installer.exe"));
        assert!(is_quarantined_extension("script.ps1"));
        assert!(!is_quarantined_extension("report.json"));

        let mut mgr = DownloadManager::new();
        let safe_entry = mgr.record_download(
            "https://hollowcreek.org/reports/audit.json",
            Some("audit.json"),
            "application/json",
            b"{\"status\":\"verified\"}",
        );
        assert_eq!(safe_entry.state, DownloadState::VerifiedSafe);
        assert_eq!(safe_entry.sha256_hex.len(), 64);

        let exec_entry = mgr.record_download(
            "https://untrusted.example.org/dropper.exe",
            Some("../../dropper.exe"),
            "application/octet-stream",
            b"MZ_TEST_BINARY",
        );
        assert_eq!(exec_entry.file_name, "dropper.exe");
        assert_eq!(exec_entry.state, DownloadState::QuarantinedExecutable);
    }
}
