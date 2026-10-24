//! `profile.flx` read and written by `Session` in `lib.rs`.
//!
//! `snapshot_to_json` builds a `flux-json` object. `flux-compress` then zlib-wraps
//! those bytes. `flux-net` supplies `CookieJar` and `HstsStore`, which are nested
//! JSON values. `DownloadManager` is nested the same way.
//!
//! The snapshot stores `settings` (`gpu_video_acceleration`, `https_only_mode`,
//! `scripts_enabled`, `jit_enabled`, `search_engine` slug), `zoom_percent`,
//! `active_tab`, `tabs` as `{title, url}` pairs where `title` is a copy of the
//! URL, `bookmarks`, `history`, `cookie_jar`, `hsts_store`, `downloads`, and
//! origin-partitioned `local_storage` rows. `Session::apply_profile_snapshot`
//! restores settings, zoom, bookmarks, history, cookies, HSTS, downloads, and
//! `local_storage`. It does not reopen `tabs` or `active_tab`.
//!
//! Seal split: Windows writes `FLXP\x01` and calls `CryptProtectData`. Linux
//! writes `FLXP\x02` and seals with AES-256-GCM from `flux-cipher`. The Linux
//! key file `profile.key` is mode 0600 and is created before the profile bytes.

use std::fs;
use std::path::PathBuf;

#[cfg(not(windows))]
use flux_cipher::{aes256_gcm_open, aes256_gcm_seal};
use flux_compress::{compress_zlib, decompress_zlib_bounded};
use flux_json::{JsonValue, parse_json, to_json_pretty};
use flux_net::{CookieJar, HstsStore};

use crate::downloads::DownloadManager;
use crate::{Bookmark, BrowserSettings, SearchEngine, VERSION};

/// Older profile header. Windows still writes this.
pub const PROFILE_MAGIC: &[u8; 5] = b"FLXP\x01";

/// Linux profile header. Payload is AES-256-GCM.
pub const PROFILE_MAGIC_V2: &[u8; 5] = b"FLXP\x02";

/// Maximum decompressed profile JSON size (8 MiB).
pub const MAX_PROFILE_BYTES: usize = 8 * 1024 * 1024;

/// Failure from reading or writing a profile container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileError {
    /// The first five bytes are not `FLXP\x01` or, on Linux, `FLXP\x02`.
    InvalidMagic,
    /// The byte slice is shorter than a header plus tag.
    TruncatedContainer,
    /// The Windows tag did not match, the Linux GCM tag failed, or `profile.key` was the wrong length.
    IntegrityCheckFailed,
    /// `flux-compress` rejected the zlib payload or it exceeded `MAX_PROFILE_BYTES`.
    DecompressFailed,
    /// JSON parse failed, or the decompressed bytes were not UTF-8.
    JsonError(String),
    /// A directory, key file, or profile file could not be created or read.
    IoError(String),
}

impl std::fmt::Display for ProfileError {
    /// One-line text for each `ProfileError` variant.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid profile container header"),
            Self::TruncatedContainer => write!(f, "truncated profile container"),
            Self::IntegrityCheckFailed => write!(f, "profile integrity verification failed"),
            Self::DecompressFailed => write!(f, "profile decompression failed"),
            Self::JsonError(msg) => write!(f, "profile JSON error: {msg}"),
            Self::IoError(msg) => write!(f, "profile I/O error: {msg}"),
        }
    }
}

impl std::error::Error for ProfileError {}

/// In-memory profile. See the module note for which fields the JSON file keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileSnapshot {
    /// Quality, script, JIT, HTTPS-only, and search-engine settings.
    pub settings: BrowserSettings,
    /// Page zoom from 50 to 250. Values outside that range are clamped on load.
    pub zoom_percent: u16,
    /// Stored active index. `Session::apply_profile_snapshot` does not select this tab.
    pub active_tab: usize,
    /// Stored tab URLs. `Session::apply_profile_snapshot` does not reopen them.
    pub tab_urls: Vec<String>,
    /// Bookmark title and URL rows.
    pub bookmarks: Vec<Bookmark>,
    /// History title and URL rows. The same `Bookmark` shape is used.
    pub history_log: Vec<Bookmark>,
    /// Cookie jar from `flux-net`.
    pub cookie_jar: CookieJar,
    /// HSTS store from `flux-net`.
    pub hsts_store: HstsStore,
    /// Download log from `downloads`.
    pub downloads: DownloadManager,
    /// Origin-partitioned `(origin, key, value)` rows from `window.localStorage`.
    pub local_storage: Vec<(String, String, String)>,
}

impl Default for ProfileSnapshot {
    /// Default settings, 100 percent zoom, no tabs, and empty jars.
    fn default() -> Self {
        Self {
            settings: BrowserSettings::default(),
            zoom_percent: 100,
            active_tab: 0,
            tab_urls: Vec::new(),
            bookmarks: Vec::new(),
            history_log: Vec::new(),
            cookie_jar: CookieJar::new(),
            hsts_store: HstsStore::new(),
            downloads: DownloadManager::new(),
            local_storage: Vec::new(),
        }
    }
}

/// Return the default per-user encrypted profile file path (`profile.flx`).
pub fn default_profile_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA")?;
        Some(PathBuf::from(base).join("Flux Browser").join("profile.flx"))
    }
    #[cfg(not(windows))]
    {
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return Some(PathBuf::from(xdg).join("flux-browser").join("profile.flx"));
        }
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("flux-browser")
                .join("profile.flx"),
        )
    }
}

/// Serialize a [`ProfileSnapshot`] into formatted JSON.
pub fn snapshot_to_json(snapshot: &ProfileSnapshot) -> String {
    let mut root = JsonValue::empty_object();
    root.insert("version", JsonValue::String(VERSION.to_string()));
    root.insert("active_tab", JsonValue::Number(snapshot.active_tab as f64));
    root.insert(
        "zoom_percent",
        JsonValue::Number(snapshot.zoom_percent as f64),
    );

    let mut settings_obj = JsonValue::empty_object();
    settings_obj.insert(
        "gpu_video_acceleration",
        JsonValue::Bool(snapshot.settings.gpu_video_acceleration),
    );
    settings_obj.insert(
        "https_only_mode",
        JsonValue::Bool(snapshot.settings.https_only_mode),
    );
    settings_obj.insert(
        "scripts_enabled",
        JsonValue::Bool(snapshot.settings.scripts_enabled),
    );
    settings_obj.insert(
        "jit_enabled",
        JsonValue::Bool(snapshot.settings.jit_enabled),
    );
    settings_obj.insert(
        "search_engine",
        JsonValue::String(snapshot.settings.search_engine.slug().to_string()),
    );
    root.insert("settings", settings_obj);

    let mut tabs_arr = JsonValue::empty_array();
    for url in &snapshot.tab_urls {
        let mut item = JsonValue::empty_object();
        item.insert("title", JsonValue::String(url.clone()));
        item.insert("url", JsonValue::String(url.clone()));
        tabs_arr.push(item);
    }
    root.insert("tabs", tabs_arr);

    let mut bm_arr = JsonValue::empty_array();
    for bm in &snapshot.bookmarks {
        let mut item = JsonValue::empty_object();
        item.insert("title", JsonValue::String(bm.title.clone()));
        item.insert("url", JsonValue::String(bm.url.clone()));
        bm_arr.push(item);
    }
    root.insert("bookmarks", bm_arr);

    let mut hist_arr = JsonValue::empty_array();
    for entry in &snapshot.history_log {
        let mut item = JsonValue::empty_object();
        item.insert("title", JsonValue::String(entry.title.clone()));
        item.insert("url", JsonValue::String(entry.url.clone()));
        hist_arr.push(item);
    }
    root.insert("history", hist_arr);

    if let Ok(cookies_val) = parse_json(&snapshot.cookie_jar.to_json()) {
        root.insert("cookie_jar", cookies_val);
    }
    if let Ok(hsts_val) = parse_json(&snapshot.hsts_store.to_json()) {
        root.insert("hsts_store", hsts_val);
    }
    root.insert("downloads", snapshot.downloads.to_json_value());

    let mut ls_arr = JsonValue::empty_array();
    for (origin, key, value) in &snapshot.local_storage {
        let mut item = JsonValue::empty_object();
        item.insert("origin", JsonValue::String(origin.clone()));
        item.insert("key", JsonValue::String(key.clone()));
        item.insert("value", JsonValue::String(value.clone()));
        ls_arr.push(item);
    }
    root.insert("local_storage", ls_arr);

    to_json_pretty(&root)
}

/// Deserialize a [`ProfileSnapshot`] from JSON text.
pub fn snapshot_from_json(json_src: &str) -> Result<ProfileSnapshot, ProfileError> {
    let doc = parse_json(json_src).map_err(|e| ProfileError::JsonError(e.to_string()))?;
    let mut snapshot = ProfileSnapshot::default();

    if let Some(active) = doc.get("active_tab").and_then(JsonValue::as_f64) {
        if active >= 0.0 {
            snapshot.active_tab = active as usize;
        }
    }
    if let Some(zoom) = doc.get("zoom_percent").and_then(JsonValue::as_f64) {
        snapshot.zoom_percent = (zoom.round() as u16).clamp(50, 250);
    }
    if let Some(settings) = doc.get("settings") {
        if let Some(v) = settings
            .get("gpu_video_acceleration")
            .and_then(JsonValue::as_bool)
        {
            snapshot.settings.gpu_video_acceleration = v;
        }
        if let Some(v) = settings.get("https_only_mode").and_then(JsonValue::as_bool) {
            snapshot.settings.https_only_mode = v;
        }
        if let Some(v) = settings.get("scripts_enabled").and_then(JsonValue::as_bool) {
            snapshot.settings.scripts_enabled = v;
        }
        if let Some(v) = settings.get("jit_enabled").and_then(JsonValue::as_bool) {
            snapshot.settings.jit_enabled = v;
        }
        if let Some(eng) = settings.get("search_engine").and_then(JsonValue::as_str) {
            snapshot.settings.search_engine = SearchEngine::from_slug(eng);
        }
    }
    if let Some(tabs) = doc.get("tabs").and_then(JsonValue::as_array) {
        for item in tabs {
            if let Some(url) = item.get("url").and_then(JsonValue::as_str) {
                if !url.trim().is_empty() {
                    snapshot.tab_urls.push(url.trim().to_string());
                }
            }
        }
    }
    if let Some(bms) = doc.get("bookmarks").and_then(JsonValue::as_array) {
        for item in bms {
            let title = item.get("title").and_then(JsonValue::as_str).unwrap_or("");
            let url = item.get("url").and_then(JsonValue::as_str).unwrap_or("");
            if !url.trim().is_empty() {
                snapshot.bookmarks.push(Bookmark {
                    title: if title.trim().is_empty() {
                        url.trim().to_string()
                    } else {
                        title.trim().to_string()
                    },
                    url: url.trim().to_string(),
                });
            }
        }
    }
    if let Some(hist) = doc.get("history").and_then(JsonValue::as_array) {
        for item in hist {
            let title = item.get("title").and_then(JsonValue::as_str).unwrap_or("");
            let url = item.get("url").and_then(JsonValue::as_str).unwrap_or("");
            if !url.trim().is_empty() {
                snapshot.history_log.push(Bookmark {
                    title: if title.trim().is_empty() {
                        url.trim().to_string()
                    } else {
                        title.trim().to_string()
                    },
                    url: url.trim().to_string(),
                });
            }
        }
    }
    if let Some(cj_val) = doc.get("cookie_jar") {
        let cj_json = to_json_pretty(cj_val);
        if let Ok(jar) = CookieJar::from_json(&cj_json) {
            snapshot.cookie_jar = jar;
        }
    }
    if let Some(hsts_val) = doc.get("hsts_store") {
        let hsts_json = to_json_pretty(hsts_val);
        if let Ok(store) = HstsStore::from_json(&hsts_json) {
            snapshot.hsts_store = store;
        }
    }
    if let Some(dl_arr) = doc.get("downloads").and_then(JsonValue::as_array) {
        snapshot.downloads = DownloadManager::from_json_array(dl_arr);
    }
    if let Some(ls_arr) = doc.get("local_storage").and_then(JsonValue::as_array) {
        for item in ls_arr {
            let origin = item.get("origin").and_then(JsonValue::as_str).unwrap_or("");
            let key = item.get("key").and_then(JsonValue::as_str).unwrap_or("");
            let value = item.get("value").and_then(JsonValue::as_str).unwrap_or("");
            if !origin.is_empty() && !key.is_empty() {
                snapshot.local_storage.push((
                    origin.to_string(),
                    key.to_string(),
                    value.to_string(),
                ));
            }
        }
    }

    Ok(snapshot)
}

/// JSON, then zlib, then the platform seal.
///
/// Windows wraps a DPAPI blob in `FLXP\x01` and appends `compute_keyed_mac`.
/// Other targets seal with AES-256-GCM under `linux_seal::session_key` and return
/// `FLXP\x02`. A missing session key returns an empty vector. This function does
/// not write `profile.key`.
pub fn encode_encrypted_profile(snapshot: &ProfileSnapshot, nonce: u64) -> Vec<u8> {
    let json_bytes = snapshot_to_json(snapshot).into_bytes();
    let compressed = compress_zlib(&json_bytes);
    // Not Windows: AES-256-GCM (`FLXP\x02`) with the in-memory `session_key`.
    // That key is not `profile.key`. `save_encrypted_profile_to_path` is the path
    // that writes the key file before the profile.
    #[cfg(not(windows))]
    {
        let Ok(key) = linux_seal::session_key() else {
            return Vec::new();
        };
        return linux_seal::seal(&compressed, nonce, &key).unwrap_or_default();
    }
    // Windows: DPAPI seal, then an FNV-style tag, then the `FLXP\x01` wrapper.
    #[cfg(windows)]
    {
        let sealed = os_keystore_seal(&compressed, nonce);
        let tag = compute_keyed_mac(nonce, &sealed);
        let mut out = Vec::with_capacity(5 + 8 + sealed.len() + 8);
        out.extend_from_slice(PROFILE_MAGIC);
        out.extend_from_slice(&nonce.to_le_bytes());
        out.extend_from_slice(&sealed);
        out.extend_from_slice(&tag.to_le_bytes());
        out
    }
}

/// Decode JSON, a Windows `FLXP\x01` blob, or a Linux `FLXP\x02` blob.
///
/// A leading `{` is parsed as JSON with no seal check. On non-Windows, `FLXP\x02`
/// is opened with `linux_seal::session_key`, the in-memory key, not `profile.key`.
/// `load_profile_from_path` uses the key file for that header instead.
/// `FLXP\x01` checks `compute_keyed_mac`, then `os_keystore_unseal`, then zlib.
pub fn decode_profile_bytes(bytes: &[u8]) -> Result<ProfileSnapshot, ProfileError> {
    if bytes.starts_with(b"{") {
        let text =
            std::str::from_utf8(bytes).map_err(|e| ProfileError::JsonError(e.to_string()))?;
        return snapshot_from_json(text);
    }
    if bytes.len() < 5 + 8 + 8 {
        return Err(ProfileError::TruncatedContainer);
    }
    #[cfg(not(windows))]
    if &bytes[0..5] == PROFILE_MAGIC_V2 {
        let key = linux_seal::session_key()?;
        return decode_linux_profile(bytes, &key);
    }
    if &bytes[0..5] != PROFILE_MAGIC {
        return Err(ProfileError::InvalidMagic);
    }
    let nonce = u64::from_le_bytes(bytes[5..13].try_into().unwrap());
    let tag_offset = bytes.len() - 8;
    let sealed = &bytes[13..tag_offset];
    let expected_tag = u64::from_le_bytes(bytes[tag_offset..].try_into().unwrap());
    let actual_tag = compute_keyed_mac(nonce, sealed);
    if actual_tag != expected_tag {
        return Err(ProfileError::IntegrityCheckFailed);
    }
    let compressed = os_keystore_unseal(sealed, nonce)?;
    let decompressed = decompress_zlib_bounded(&compressed, MAX_PROFILE_BYTES)
        .map_err(|_| ProfileError::DecompressFailed)?;
    let text =
        std::str::from_utf8(&decompressed).map_err(|e| ProfileError::JsonError(e.to_string()))?;
    snapshot_from_json(text)
}

/// Direct `crypt32` calls. Used only on Windows, from `os_keystore_seal` and `os_keystore_unseal`.
#[cfg(windows)]
mod win_dpapi {
    use std::ffi::c_void;
    use std::ptr::{null, null_mut};

    /// `CryptProtectData` flag. Do not show a prompt.
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x01;

    /// `DATA_BLOB` layout passed to `crypt32`.
    #[repr(C)]
    struct DataBlob {
        cb_data: u32,
        pb_data: *mut u8,
    }

    #[link(name = "crypt32")]
    unsafe extern "system" {
        /// Seal bytes for the current user. `protect` is the caller.
        fn CryptProtectData(
            p_data_in: *const DataBlob,
            sz_data_descr: *const u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *const c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;

        /// Open bytes sealed for the current user. `unprotect` is the caller.
        fn CryptUnprotectData(
            p_data_in: *const DataBlob,
            ppsz_data_descr: *mut *mut u16,
            p_optional_entropy: *const DataBlob,
            pv_reserved: *mut c_void,
            p_prompt_struct: *const c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// Release the buffer `crypt32` allocated for the output blob.
        fn LocalFree(h_mem: *mut c_void) -> *mut c_void;
    }

    /// Call `CryptProtectData`. `nonce` is the optional entropy. Empty input returns an empty vector.
    pub fn protect(plain: &[u8], nonce: u64) -> Option<Vec<u8>> {
        if plain.is_empty() {
            return Some(Vec::new());
        }
        let mut entropy_bytes = [0u8; 8];
        entropy_bytes.copy_from_slice(&nonce.to_le_bytes());
        let in_blob = DataBlob {
            cb_data: plain.len() as u32,
            pb_data: plain.as_ptr() as *mut u8,
        };
        let entropy_blob = DataBlob {
            cb_data: entropy_bytes.len() as u32,
            pb_data: entropy_bytes.as_mut_ptr(),
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        unsafe {
            let ok = CryptProtectData(
                &in_blob,
                null(),
                &entropy_blob,
                null_mut(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            );
            if ok == 0 || out_blob.pb_data.is_null() {
                return None;
            }
            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let result = slice.to_vec();
            let _ = LocalFree(out_blob.pb_data.cast::<c_void>());
            Some(result)
        }
    }

    /// Call `CryptUnprotectData` with the same nonce entropy. Empty input returns an empty vector.
    pub fn unprotect(cipher: &[u8], nonce: u64) -> Option<Vec<u8>> {
        if cipher.is_empty() {
            return Some(Vec::new());
        }
        let mut entropy_bytes = [0u8; 8];
        entropy_bytes.copy_from_slice(&nonce.to_le_bytes());
        let in_blob = DataBlob {
            cb_data: cipher.len() as u32,
            pb_data: cipher.as_ptr() as *mut u8,
        };
        let entropy_blob = DataBlob {
            cb_data: entropy_bytes.len() as u32,
            pb_data: entropy_bytes.as_mut_ptr(),
        };
        let mut out_blob = DataBlob {
            cb_data: 0,
            pb_data: null_mut(),
        };
        unsafe {
            let ok = CryptUnprotectData(
                &in_blob,
                null_mut(),
                &entropy_blob,
                null_mut(),
                null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out_blob,
            );
            if ok == 0 || out_blob.pb_data.is_null() {
                return None;
            }
            let slice = std::slice::from_raw_parts(out_blob.pb_data, out_blob.cb_data as usize);
            let result = slice.to_vec();
            let _ = LocalFree(out_blob.pb_data.cast::<c_void>());
            Some(result)
        }
    }
}

/// Windows: DPAPI-seal `compressed`. If `CryptProtectData` fails, return the zlib bytes unchanged.
#[cfg(windows)]
fn os_keystore_seal(compressed: &[u8], nonce: u64) -> Vec<u8> {
    #[cfg(windows)]
    {
        let _ = nonce;
        if let Some(protected) = win_dpapi::protect(compressed, nonce) {
            return protected;
        }
    }
    let _ = nonce;
    compressed.to_vec()
}

/// Windows: DPAPI-open `sealed`. If that fails, return `sealed` unchanged.
///
/// On non-Windows this returns `sealed` unchanged. Linux file profiles do not call it.
fn os_keystore_unseal(sealed: &[u8], nonce: u64) -> Result<Vec<u8>, ProfileError> {
    #[cfg(windows)]
    {
        if let Some(unprotected) = win_dpapi::unprotect(sealed, nonce) {
            return Ok(unprotected);
        }
    }
    let _ = nonce;
    Ok(sealed.to_vec())
}

/// Write `path`, creating parent directories first.
///
/// The nonce is `bookmarks.len * 31 + history.len`. It is not random.
/// On Linux, `load_or_create_key` runs before the profile write. That creates
/// `profile.key` with mode 0600 and calls `sync_all` on the new key file when
/// the key does not already exist. The profile bytes are written after that.
/// On Windows, `encode_encrypted_profile` applies DPAPI and the file is `FLXP\x01`.
pub fn save_encrypted_profile_to_path(
    snapshot: &ProfileSnapshot,
    path: &std::path::Path,
) -> Result<(), ProfileError> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| ProfileError::IoError(e.to_string()))?;
        }
    }
    let nonce = (snapshot.bookmarks.len() as u64)
        .wrapping_mul(31)
        .wrapping_add(snapshot.history_log.len() as u64);
    // Linux seal order: key file (0600) first, then zlib, then AES-256-GCM, then the profile.
    #[cfg(not(windows))]
    {
        let key = linux_seal::load_or_create_key(path)?;
        let json_bytes = snapshot_to_json(snapshot).into_bytes();
        let compressed = compress_zlib(&json_bytes);
        let bytes = linux_seal::seal(&compressed, nonce, &key)?;
        fs::write(path, bytes).map_err(|e| ProfileError::IoError(e.to_string()))?;
        return Ok(());
    }
    // Windows seal: DPAPI inside `encode_encrypted_profile`, then write `FLXP\x01`.
    #[cfg(windows)]
    {
        let bytes = encode_encrypted_profile(snapshot, nonce);
        fs::write(path, bytes).map_err(|e| ProfileError::IoError(e.to_string()))?;
        Ok(())
    }
}

/// Load a profile from `path`. JSON and Windows `FLXP\x01` files load as before.
/// A Linux `FLXP\x02` file is opened with `profile.key` in the same directory.
pub fn load_profile_from_path(path: &std::path::Path) -> Result<ProfileSnapshot, ProfileError> {
    let bytes = fs::read(path).map_err(|e| ProfileError::IoError(e.to_string()))?;
    if bytes.starts_with(b"{") || bytes.starts_with(PROFILE_MAGIC) {
        return decode_profile_bytes(&bytes);
    }
    #[cfg(not(windows))]
    if bytes.starts_with(PROFILE_MAGIC_V2) {
        let key = linux_seal::read_key_file(path)?;
        return decode_linux_profile(&bytes, &key);
    }
    decode_profile_bytes(&bytes)
}

/// AES-256-GCM profile seal. Not compiled on Windows. `flux-cipher` performs the seal.
#[cfg(not(windows))]
mod linux_seal {
    use super::ProfileError;
    use std::fs;
    use std::io::{Read, Write};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    /// Process-lifetime key for `encode_encrypted_profile`. Not written to disk.
    static SESSION_KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

    /// Return the in-memory key, creating it from `/dev/urandom` on first use.
    pub fn session_key() -> Result<[u8; 32], ProfileError> {
        let mut slot = SESSION_KEY
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(key) = *slot {
            return Ok(key);
        }
        let key = random_bytes()?;
        *slot = Some(key);
        Ok(key)
    }

    /// Read 32 bytes from `profile.key` next to `profile_path`. Wrong length is an integrity error.
    ///
    /// Also tries to set the existing file to mode 0600.
    pub fn read_key_file(profile_path: &Path) -> Result<[u8; 32], ProfileError> {
        let path = key_file(profile_path);
        let bytes = fs::read(&path).map_err(|e| ProfileError::IoError(e.to_string()))?;
        if bytes.len() != 32 {
            return Err(ProfileError::IntegrityCheckFailed);
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
        Ok(key)
    }

    /// Read `profile.key`, or create it with `write_new_key` before any profile bytes are written.
    ///
    /// If create races and the file appears, the existing file is read instead.
    pub fn load_or_create_key(profile_path: &Path) -> Result<[u8; 32], ProfileError> {
        let path = key_file(profile_path);
        if path.exists() {
            return read_key_file(profile_path);
        }
        match write_new_key(&path) {
            Ok(key) => Ok(key),
            Err(ProfileError::IoError(_)) if path.exists() => read_key_file(profile_path),
            Err(err) => Err(err),
        }
    }

    /// Create `path` with `create_new` and mode 0600, write 32 random bytes, then `sync_all`.
    fn write_new_key(path: &Path) -> Result<[u8; 32], ProfileError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| ProfileError::IoError(e.to_string()))?;
            }
        }
        let key = random_bytes()?;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|e| ProfileError::IoError(e.to_string()))?;
        file.write_all(&key)
            .map_err(|e| ProfileError::IoError(e.to_string()))?;
        file.sync_all()
            .map_err(|e| ProfileError::IoError(e.to_string()))?;
        Ok(key)
    }

    /// `profile.key` in the same directory as the profile, replacing the file name.
    fn key_file(profile_path: &Path) -> PathBuf {
        profile_path.with_file_name("profile.key")
    }

    /// Read 32 bytes from `/dev/urandom`.
    fn random_bytes() -> Result<[u8; 32], ProfileError> {
        let mut buf = [0u8; 32];
        let mut file =
            fs::File::open("/dev/urandom").map_err(|e| ProfileError::IoError(e.to_string()))?;
        file.read_exact(&mut buf)
            .map_err(|e| ProfileError::IoError(e.to_string()))?;
        Ok(buf)
    }

    /// Build `FLXP\x02`: magic, nonce as associated data, 12-byte IV, ciphertext, 16-byte tag.
    ///
    /// The IV is read from `/dev/urandom`. `aes256_gcm_seal` is `flux-cipher`.
    pub fn seal(compressed: &[u8], nonce: u64, key: &[u8; 32]) -> Result<Vec<u8>, ProfileError> {
        let mut iv = [0u8; 12];
        let mut file =
            fs::File::open("/dev/urandom").map_err(|e| ProfileError::IoError(e.to_string()))?;
        file.read_exact(&mut iv)
            .map_err(|e| ProfileError::IoError(e.to_string()))?;
        let aad = nonce.to_le_bytes();
        let (body, tag) = super::aes256_gcm_seal(key, &iv, &aad, compressed);
        let mut out = Vec::with_capacity(5 + 8 + 12 + body.len() + 16);
        out.extend_from_slice(super::PROFILE_MAGIC_V2);
        out.extend_from_slice(&aad);
        out.extend_from_slice(&iv);
        out.extend_from_slice(&body);
        out.extend_from_slice(&tag);
        Ok(out)
    }

    /// Check `FLXP\x02` and open the AES-256-GCM payload. A bad tag is `IntegrityCheckFailed`.
    pub fn open(bytes: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, ProfileError> {
        if bytes.len() < 5 + 8 + 12 + 16 {
            return Err(ProfileError::TruncatedContainer);
        }
        if &bytes[0..5] != super::PROFILE_MAGIC_V2 {
            return Err(ProfileError::InvalidMagic);
        }
        let aad = &bytes[5..13];
        let mut iv = [0u8; 12];
        iv.copy_from_slice(&bytes[13..25]);
        let tag_at = bytes.len() - 16;
        let body = &bytes[25..tag_at];
        let mut tag = [0u8; 16];
        tag.copy_from_slice(&bytes[tag_at..]);
        super::aes256_gcm_open(key, &iv, aad, body, &tag)
            .map_err(|_| ProfileError::IntegrityCheckFailed)
    }
}

/// Open a `FLXP\x02` blob with `key`, zlib-decompress, and parse the JSON snapshot.
#[cfg(not(windows))]
fn decode_linux_profile(bytes: &[u8], key: &[u8; 32]) -> Result<ProfileSnapshot, ProfileError> {
    let compressed = linux_seal::open(bytes, key)?;
    let decompressed = decompress_zlib_bounded(&compressed, MAX_PROFILE_BYTES)
        .map_err(|_| ProfileError::DecompressFailed)?;
    let text =
        std::str::from_utf8(&decompressed).map_err(|e| ProfileError::JsonError(e.to_string()))?;
    snapshot_from_json(text)
}

/// 64-bit FNV-style tag over `nonce` and `payload`. Used on the Windows container, not AES-GCM.
fn compute_keyed_mac(nonce: u64, payload: &[u8]) -> u64 {
    let mut acc = 14695981039346656037u64 ^ nonce;
    for &b in payload {
        acc ^= b as u64;
        acc = acc.wrapping_mul(1099511628211);
    }
    acc ^ (payload.len() as u64)
}

#[cfg(test)]
mod tests {
    //! Round-trip one snapshot through `encode_encrypted_profile`. On Linux, also check two key files differ.
    use super::*;

    #[test]
    /// Seal one snapshot and open it. Flipping a ciphertext byte fails the integrity check.
    fn encrypts_authenticates_and_restores_full_profile_snapshot() {
        let mut jar = CookieJar::new();
        assert!(jar.store_set_cookie(
            "https://example.com/app",
            "__Host-session=tok777; Secure; HttpOnly; Path=/; SameSite=Strict"
        ));
        let mut hsts = HstsStore::new();
        assert!(hsts.record_hsts_header(
            "https://example.com/app",
            "max-age=63072000; includeSubDomains"
        ));
        let snap = ProfileSnapshot {
            settings: BrowserSettings {
                gpu_video_acceleration: false,
                https_only_mode: true,
                scripts_enabled: false,
                jit_enabled: false,
                search_engine: SearchEngine::DuckDuckGo,
            },
            zoom_percent: 125,
            active_tab: 0,
            tab_urls: vec!["flux://home".to_string(), "https://example.com".to_string()],
            bookmarks: vec![Bookmark {
                title: "Example".to_string(),
                url: "https://example.com".to_string(),
            }],
            history_log: vec![Bookmark {
                title: "Flux Start".to_string(),
                url: "flux://home".to_string(),
            }],
            cookie_jar: jar,
            hsts_store: hsts,
            downloads: DownloadManager::new(),
            local_storage: vec![(
                "https://example.com".to_string(),
                "theme".to_string(),
                "obsidian".to_string(),
            )],
        };

        let mut encrypted = encode_encrypted_profile(&snap, 0x1234_5678_9ABC_DEF0);
        #[cfg(windows)]
        assert_eq!(&encrypted[0..5], PROFILE_MAGIC);
        #[cfg(not(windows))]
        assert_eq!(&encrypted[0..5], PROFILE_MAGIC_V2);
        let restored = decode_profile_bytes(&encrypted).expect("valid encrypted profile");
        assert_eq!(restored, snap);

        // Tampering with ciphertext must fail integrity check
        let mid = encrypted.len() / 2;
        encrypted[mid] ^= 0x55;
        assert_eq!(
            decode_profile_bytes(&encrypted),
            Err(ProfileError::IntegrityCheckFailed)
        );
    }

    #[cfg(not(windows))]
    #[test]
    /// Two directories get different 32-byte `profile.key` files, and each profile still loads.
    fn separate_directories_do_not_share_profile_keys() {
        let dir = std::env::temp_dir().join(format!("flux-profile-{}-{}", std::process::id(), 16));
        let _ = fs::remove_dir_all(&dir);
        let one = dir.join("one");
        let two = dir.join("two");
        fs::create_dir_all(&one).unwrap();
        fs::create_dir_all(&two).unwrap();
        let snap = ProfileSnapshot {
            settings: BrowserSettings::default(),
            zoom_percent: 100,
            active_tab: 0,
            tab_urls: vec!["flux://home".to_string()],
            bookmarks: Vec::new(),
            history_log: Vec::new(),
            cookie_jar: CookieJar::new(),
            hsts_store: HstsStore::new(),
            downloads: DownloadManager::new(),
            local_storage: Vec::new(),
        };
        let path_one = one.join("profile.flx");
        let path_two = two.join("profile.flx");
        save_encrypted_profile_to_path(&snap, &path_one).unwrap();
        save_encrypted_profile_to_path(&snap, &path_two).unwrap();
        let key_one = fs::read(one.join("profile.key")).unwrap();
        let key_two = fs::read(two.join("profile.key")).unwrap();
        assert_eq!(key_one.len(), 32);
        assert_eq!(key_two.len(), 32);
        assert_ne!(key_one, key_two);
        assert_eq!(load_profile_from_path(&path_one).unwrap(), snap);
        assert_eq!(load_profile_from_path(&path_two).unwrap(), snap);

        fs::remove_file(two.join("profile.key")).unwrap();
        assert!(load_profile_from_path(&path_two).is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
