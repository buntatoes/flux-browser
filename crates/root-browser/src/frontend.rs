//! Built-in `root://` pages.
//!
//! `Session::open_target` in `lib.rs` calls the page builders. `load_target`
//! calls `resolve_builtin_page` and `resolve_navigation_query`. `ui.rs` is used
//! for `normalize_address_input_with_engine`. `root-dom::parse_html` parses the
//! shell string in `render_react_shell_document`. `root-video::GpuVideoSession`
//! only supplies the default backend label for the inspect and settings snapshots.
//!
//! `frontend/App.tsx`, `frontend/components/*.tsx`, `frontend/styles/browser.css`,
//! and `frontend/scripts/*.js` are stored as text. `compile_tsx_component` copies
//! the `return (` body and replaces `{props.name}`, `={{props.name}}`, `{{SLOT}}`,
//! and `className=` with `class=`. There is no component compiler and no Node build.

use root_dom::{Document, parse_html};
use root_video::GpuVideoSession;

use crate::downloads::DownloadEntry;
use crate::ui::{UiState, normalize_address_input_with_engine};
use crate::{Bookmark, SearchEngine, Session, VERSION};

/// Canonical URL for the start page.
pub const HOME_URL: &str = "root://home";
/// Canonical URL for the inspector page.
pub const INSPECT_URL: &str = "root://inspect";
/// Canonical URL for the security page.
pub const SECURITY_URL: &str = "root://security";
/// Canonical URL for the settings page.
pub const SETTINGS_URL: &str = "root://settings";
/// Canonical URL for the history page.
pub const HISTORY_URL: &str = "root://history";
/// Canonical URL for the downloads page.
pub const DOWNLOADS_URL: &str = "root://downloads";
/// Prefix for the in-page navigation form. The query key is `q`.
pub const NAVIGATE_URL_PREFIX: &str = "root://navigate";

/// Text of `frontend/App.tsx`. The shell page fills its `return (` body.
pub const APP_TSX: &str = include_str!("../../../frontend/App.tsx");

/// Text of `frontend/components/StartDashboard.tsx`.
pub const START_DASHBOARD_TSX: &str =
    include_str!("../../../frontend/components/StartDashboard.tsx");

/// Text of `frontend/components/InspectorPage.tsx`.
pub const INSPECTOR_PAGE_TSX: &str = include_str!("../../../frontend/components/InspectorPage.tsx");

/// Text of `frontend/components/SecurityCenter.tsx`.
pub const SECURITY_CENTER_TSX: &str =
    include_str!("../../../frontend/components/SecurityCenter.tsx");

/// Text of `frontend/components/SettingsPage.tsx`.
pub const SETTINGS_PAGE_TSX: &str = include_str!("../../../frontend/components/SettingsPage.tsx");

/// Text of `frontend/components/HistoryPage.tsx`.
pub const HISTORY_PAGE_TSX: &str = include_str!("../../../frontend/components/HistoryPage.tsx");

/// Text of `frontend/components/DownloadsPage.tsx`.
pub const DOWNLOADS_PAGE_TSX: &str = include_str!("../../../frontend/components/DownloadsPage.tsx");

/// Embedded stylesheet (`frontend/styles/browser.css`).
pub const BROWSER_CSS: &str = include_str!("../../../frontend/styles/browser.css");

/// Shared front-end controller utilities (`frontend/scripts/root_core.js`).
pub const ROOT_CORE_JS: &str = include_str!("../../../frontend/scripts/root_core.js");

/// Text of `frontend/scripts/home.js`, inserted as a script on the start page.
pub const HOME_JS: &str = include_str!("../../../frontend/scripts/home.js");

/// Text of `frontend/scripts/settings.js`, inserted as a script on the settings page.
pub const SETTINGS_JS: &str = include_str!("../../../frontend/scripts/settings.js");

/// Text of `frontend/scripts/inspect.js`, inserted as a script on the inspector page.
pub const INSPECT_JS: &str = include_str!("../../../frontend/scripts/inspect.js");

/// Text of `frontend/scripts/security.js`, inserted as a script on the security page.
pub const SECURITY_JS: &str = include_str!("../../../frontend/scripts/security.js");

/// Text of `frontend/scripts/downloads.js`, inserted as a script on the downloads page.
pub const DOWNLOADS_JS: &str = include_str!("../../../frontend/scripts/downloads.js");

/// Text of `frontend/scripts/history.js`, inserted as a script on the history page.
pub const HISTORY_JS: &str = include_str!("../../../frontend/scripts/history.js");

/// Slice between the first `return (` and the last `);`.
///
/// If `return (` is missing, the whole source is returned. This is a string slice, not a parse.
pub fn extract_tsx_return_body(tsx_source: &str) -> &str {
    let Some(ret_idx) = tsx_source.find("return (") else {
        return tsx_source;
    };
    let after_ret = &tsx_source[ret_idx + "return (".len()..];
    let Some(end_idx) = after_ret.rfind(");") else {
        return after_ret.trim();
    };
    after_ret[..end_idx].trim()
}

/// Replace prop and slot tokens in the `return (` body, then turn `className=` into `class=`.
///
/// `props` replaces `{props.NAME}` and `={{props.NAME}}`. `slots` replaces `{{NAME}}`.
/// No other syntax is interpreted.
pub fn compile_tsx_component(
    tsx_source: &str,
    props: &[(&str, &str)],
    slots: &[(&str, &str)],
) -> String {
    let mut markup = extract_tsx_return_body(tsx_source).to_string();

    for (prop_name, prop_val) in props {
        let quoted_prop = format!("{{props.{prop_name}}}");
        let attr_expr = format!("={{props.{prop_name}}}");
        let quoted_attr_replacement = format!("=\"{prop_val}\"");
        markup = markup.replace(&attr_expr, &quoted_attr_replacement);
        markup = markup.replace(&quoted_prop, prop_val);
    }

    for (slot_name, slot_html) in slots {
        let token = format!("{{{{{slot_name}}}}}");
        markup = markup.replace(&token, slot_html);
    }

    markup.replace("className=", "class=")
}

/// Escape plain text for safe insertion into HTML/JSX text or attribute contexts.
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Decode URL-encoded form query strings (e.g. `q=https%3A%2F%2Fexample.com`).
pub fn urldecode_component(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Encode a search query component for URL query strings (`application/x-www-form-urlencoded`).
pub fn urlencode_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for b in input.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Resolve `root://navigate?q=...` with `SearchEngine::Google`.
///
/// `Session::open_target` does not use this. It calls `resolve_navigation_query_with_engine`
/// with the engine stored in settings. `load_target` does call this function.
pub fn resolve_navigation_query(target: &str) -> Option<String> {
    resolve_navigation_query_with_engine(target, SearchEngine::Google)
}

/// Resolve `root://navigate?q=...` for `engine`.
///
/// An empty `q` returns that engine's home URL. Other text goes through
/// `normalize_address_input_with_engine`. Google Search, DuckDuckGo, and
/// Microsoft Bing are the three `SearchEngine` values.
pub fn resolve_navigation_query_with_engine(target: &str, engine: SearchEngine) -> Option<String> {
    let trimmed = target.trim();
    let rest = trimmed.strip_prefix(NAVIGATE_URL_PREFIX)?;
    let query = rest.strip_prefix('?').unwrap_or("");
    for pair in query.split('&') {
        let Some((k, v)) = pair.split_once('=') else {
            continue;
        };
        if k == "q" {
            let decoded = urldecode_component(v);
            if decoded.trim().is_empty() {
                return Some(engine.home_url().to_string());
            }
            return normalize_address_input_with_engine(&decoded, engine)
                .or_else(|| Some(HOME_URL.to_string()));
        }
    }
    Some(engine.home_url().to_string())
}

/// Return `(canonical_url, html_source)` if `target` names a built-in `root://` page.
pub fn resolve_builtin_page(target: &str) -> Option<(String, String)> {
    let trimmed = target.trim().to_ascii_lowercase();
    match trimmed.as_str() {
        "root://home" | "root://" | "about:home" | "about:blank" => {
            Some((HOME_URL.to_string(), home_html()))
        }
        s if s.starts_with("root://home?") => {
            let engine = if s.contains("engine=duckduckgo") || s.contains("engine=ddg") {
                SearchEngine::DuckDuckGo
            } else if s.contains("engine=bing") {
                SearchEngine::Bing
            } else {
                SearchEngine::Google
            };
            Some((HOME_URL.to_string(), home_html_with_engine(engine)))
        }
        s if s == "root://inspect"
            || s == "root://devtools"
            || s.starts_with("root://inspect?") =>
        {
            Some((INSPECT_URL.to_string(), inspect_html()))
        }
        "root://security" | "root://shields" => Some((SECURITY_URL.to_string(), security_html())),
        s if s == "root://settings" || s.starts_with("root://settings?") => {
            Some((SETTINGS_URL.to_string(), settings_html()))
        }
        s if s == "root://history" || s.starts_with("root://history?") => {
            Some((HISTORY_URL.to_string(), history_html()))
        }
        s if s == "root://downloads" || s.starts_with("root://downloads?") => {
            Some((DOWNLOADS_URL.to_string(), downloads_html()))
        }
        _ => None,
    }
}

/// Start page HTML for Google Search. Calls `home_html_with_engine`.
pub fn home_html() -> String {
    home_html_with_engine(SearchEngine::Google)
}

/// Start page HTML for `engine`, including `browser.css`, `root_core.js`, and `home.js`.
///
/// The engine mark is a `root://engine-*.png` URL. `lib.rs` draws that URL later.
pub fn home_html_with_engine(engine: SearchEngine) -> String {
    let placeholder = format!("Search {} or enter web address...", engine.label());
    let engine_logo = match engine {
        SearchEngine::Google => "root://engine-google.png",
        SearchEngine::DuckDuckGo => "root://engine-ddg.png",
        SearchEngine::Bing => "root://engine-bing.png",
    };
    let body = compile_tsx_component(
        START_DASHBOARD_TSX,
        &[
            ("version", VERSION),
            ("defaultQuery", ""),
            ("activeEngineName", engine.label()),
            ("activeEngineLogo", engine_logo),
            ("searchPlaceholder", &placeholder),
        ],
        &[],
    );
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n  <title>Root Start</title>\n  <style>\n{BROWSER_CSS}\n  </style>\n</head>\n<body>\n{body}\n  <script>\n{ROOT_CORE_JS}\n{HOME_JS}\n  </script>\n</body>\n</html>"
    )
}

/// Inspector page HTML filled with the counts passed in, plus `root_core.js` and `inspect.js`.
#[allow(clippy::too_many_arguments)]
pub fn inspect_html_with_state(
    tab_count: usize,
    zoom_percent: u32,
    gpu_backend: &str,
    shield_blocked: u64,
    hsts_count: usize,
    download_count: usize,
    cookie_count: usize,
) -> String {
    let tabs_str = tab_count.to_string();
    let zoom_str = format!("{zoom_percent}%");
    let shield_str = shield_blocked.to_string();
    let hsts_str = hsts_count.to_string();
    let dl_str = download_count.to_string();
    let ck_str = cookie_count.to_string();
    let body = compile_tsx_component(
        INSPECTOR_PAGE_TSX,
        &[
            ("version", VERSION),
            ("tabCount", &tabs_str),
            ("activeZoom", &zoom_str),
            ("gpuBackend", gpu_backend),
            ("shieldBlocked", &shield_str),
            ("hstsCount", &hsts_str),
            ("downloadCount", &dl_str),
            ("cookieCount", &ck_str),
        ],
        &[],
    );
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n  <title>Root Developer Inspector</title>\n  <style>\n{BROWSER_CSS}\n  </style>\n</head>\n<body>\n{body}\n  <script>\n{ROOT_CORE_JS}\n{INSPECT_JS}\n  </script>\n</body>\n</html>"
    )
}

/// Inspector HTML with placeholder counts and the default `GpuVideoSession` backend label.
pub fn inspect_html() -> String {
    let gpu = GpuVideoSession::default();
    inspect_html_with_state(1, 100, gpu.backend().label(), 0, 5, 0, 0)
}

/// Security page HTML with `root_core.js` and `security.js`.
pub fn security_html() -> String {
    let body = compile_tsx_component(SECURITY_CENTER_TSX, &[("version", VERSION)], &[]);
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n  <title>Root Security Center</title>\n  <style>\n{BROWSER_CSS}\n  </style>\n</head>\n<body>\n{body}\n  <script>\n{ROOT_CORE_JS}\n{SECURITY_JS}\n  </script>\n</body>\n</html>"
    )
}

/// Settings HTML with scripts and JIT shown as enabled and Google Search selected.
pub fn settings_html_with_state(
    gpu_backend_label: &str,
    gpu_video_enabled: bool,
    https_only: bool,
    scripts_enabled: bool,
    frames_decoded: u64,
    frames_encoded: u64,
) -> String {
    settings_html_with_profile_state_and_engine(
        gpu_backend_label,
        gpu_video_enabled,
        https_only,
        scripts_enabled,
        true,
        SearchEngine::Google,
        frames_decoded,
        frames_encoded,
        0,
        0,
        0,
    )
}

/// Settings HTML with bookmark, history, and cookie counts. JIT stays enabled and the engine is Google Search.
#[allow(clippy::too_many_arguments)]
pub fn settings_html_with_profile_state(
    gpu_backend_label: &str,
    gpu_video_enabled: bool,
    https_only: bool,
    scripts_enabled: bool,
    frames_decoded: u64,
    frames_encoded: u64,
    bookmark_count: usize,
    history_count: usize,
    cookie_count: usize,
) -> String {
    settings_html_with_profile_state_and_engine(
        gpu_backend_label,
        gpu_video_enabled,
        https_only,
        scripts_enabled,
        true,
        SearchEngine::Google,
        frames_decoded,
        frames_encoded,
        bookmark_count,
        history_count,
        cookie_count,
    )
}

/// Settings HTML for the given toggles, engine, frame counters, and profile counts.
///
/// The profile status line inserted into the page is a fixed string. It is not
/// read back from `profile.flx`. The GPU status text says frames are decoded on
/// the CPU when the toggle is on. `root_core.js` and `settings.js` are included as a script tag.
#[allow(clippy::too_many_arguments)]
pub fn settings_html_with_profile_state_and_engine(
    gpu_backend_label: &str,
    gpu_video_enabled: bool,
    https_only: bool,
    scripts_enabled: bool,
    jit_enabled: bool,
    search_engine: SearchEngine,
    frames_decoded: u64,
    frames_encoded: u64,
    bookmark_count: usize,
    history_count: usize,
    cookie_count: usize,
) -> String {
    let (gpu_status, gpu_toggle_label, gpu_toggle_cls) = if gpu_video_enabled {
        ("On. Frames are decoded on the CPU.", "On", "toggle-pill-on")
    } else {
        ("Off.", "Off", "toggle-pill-off")
    };
    let (https_status, https_toggle_label, https_toggle_cls) = if https_only {
        (
            "ENABLED (Strict HTTPS-Only)",
            "Enabled (Click to Disable)",
            "toggle-pill-on",
        )
    } else {
        (
            "STANDARD (HTTPS Preferred)",
            "Standard (Click to Enable Strict)",
            "toggle-pill-off",
        )
    };
    let (scripts_status, scripts_toggle_label, scripts_toggle_cls) = if scripts_enabled {
        (
            "ENABLED (100K-Step Budget & SOP)",
            "Enabled (Click to Disable)",
            "toggle-pill-on",
        )
    } else {
        (
            "DISABLED (No Scripts Executed)",
            "Disabled (Click to Enable)",
            "toggle-pill-off",
        )
    };
    let (jit_status, jit_toggle_label, jit_toggle_cls) = if jit_enabled {
        (
            "On. Math can run in the C++ helper.",
            "On",
            "toggle-pill-on",
        )
    } else {
        (
            "Off. Scripts stay in the Rust interpreter.",
            "Off",
            "toggle-pill-off",
        )
    };
    let google_card_cls = if search_engine == SearchEngine::Google {
        "engine-card-active"
    } else {
        "engine-card"
    };
    let ddg_card_cls = if search_engine == SearchEngine::DuckDuckGo {
        "engine-card-active"
    } else {
        "engine-card"
    };
    let bing_card_cls = if search_engine == SearchEngine::Bing {
        "engine-card-active"
    } else {
        "engine-card"
    };
    let dec_str = frames_decoded.to_string();
    let enc_str = frames_encoded.to_string();
    let bm_str = bookmark_count.to_string();
    let hist_str = history_count.to_string();
    let ck_str = cookie_count.to_string();
    let profile_status = "ENCRYPTED & AUTHENTICATED (FLXP v1 / Windows DPAPI + 64-Bit Tag)";
    let body = compile_tsx_component(
        SETTINGS_PAGE_TSX,
        &[
            ("version", VERSION),
            ("activeSearchEngineLabel", search_engine.label()),
            ("googleCardClass", google_card_cls),
            ("ddgCardClass", ddg_card_cls),
            ("bingCardClass", bing_card_cls),
            ("scriptsEnabledStatus", scripts_status),
            ("scriptsToggleLabel", scripts_toggle_label),
            ("scriptsToggleClass", scripts_toggle_cls),
            ("jitEnabledStatus", jit_status),
            ("jitToggleLabel", jit_toggle_label),
            ("jitToggleClass", jit_toggle_cls),
            ("httpsOnlyStatus", https_status),
            ("httpsToggleLabel", https_toggle_label),
            ("httpsToggleClass", https_toggle_cls),
            ("gpuBackendLabel", gpu_backend_label),
            ("gpuVideoStatus", gpu_status),
            ("gpuToggleLabel", gpu_toggle_label),
            ("gpuToggleClass", gpu_toggle_cls),
            ("framesDecoded", &dec_str),
            ("framesEncoded", &enc_str),
            ("profileStatus", profile_status),
            ("bookmarkCount", &bm_str),
            ("historyCount", &hist_str),
            ("cookieCount", &ck_str),
        ],
        &[],
    );
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n  <title>Root Settings</title>\n  <style>\n{BROWSER_CSS}\n  </style>\n</head>\n<body>\n{body}\n  <script>\n{ROOT_CORE_JS}\n{SETTINGS_JS}\n  </script>\n</body>\n</html>"
    )
}

/// Settings HTML from a default `GpuVideoSession`, with placeholder frame counts of 12.
pub fn settings_html() -> String {
    let session = GpuVideoSession::default();
    settings_html_with_state(
        session.backend().label(),
        session.is_hardware_decode_enabled(),
        false,
        true,
        12,
        12,
    )
}

/// History page HTML. Entries are escaped and listed newest first. Includes `root_core.js` and `history.js`.
pub fn history_html_with_entries(entries: &[Bookmark]) -> String {
    let count_str = entries.len().to_string();
    let mut items_html = String::new();
    if entries.is_empty() {
        items_html.push_str(
            r#"<div class="history-item"><p class="sec-desc">No pages recorded in the current browsing session.</p></div>"#,
        );
    } else {
        for (idx, item) in entries.iter().enumerate().rev() {
            let title = escape_html(&item.title);
            let url = escape_html(&item.url);
            let num = idx + 1;
            items_html.push_str(&format!(
                r#"<div class="history-item"><p class="sec-title">{num}. {title}</p><p class="sec-desc"><a href="{url}">{url}</a></p></div>"#
            ));
        }
    }
    let body = compile_tsx_component(
        HISTORY_PAGE_TSX,
        &[("version", VERSION), ("entryCount", &count_str)],
        &[("HISTORY_ITEMS_SLOT", &items_html)],
    );
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n  <title>Root History</title>\n  <style>\n{BROWSER_CSS}\n  </style>\n</head>\n<body>\n{body}\n  <script>\n{ROOT_CORE_JS}\n{HISTORY_JS}\n  </script>\n</body>\n</html>"
    )
}

/// History page HTML with no entries.
pub fn history_html() -> String {
    history_html_with_entries(&[])
}

/// Downloads page HTML. Entries are escaped and listed newest first. Includes `root_core.js` and `downloads.js`.
pub fn downloads_html_with_entries(entries: &[DownloadEntry]) -> String {
    let count_str = entries.len().to_string();
    let mut items_html = String::new();
    if entries.is_empty() {
        items_html.push_str(
            r#"<div class="history-item"><p class="sec-desc">No files downloaded in the current session. Use the verification links above to test SHA-256 download verification.</p></div>"#,
        );
    } else {
        for item in entries.iter().rev() {
            let name = escape_html(&item.file_name);
            let url = escape_html(&item.source_url);
            let mime = escape_html(&item.mime_type);
            let sha = escape_html(&item.sha256_hex);
            let status = item.state.label();
            let motw = if item.motw_tagged {
                "MOTW: ZoneId=3"
            } else {
                "MOTW: Local"
            };
            let bytes = item.byte_size;
            items_html.push_str(&format!(
                r#"<div class="history-item"><p class="sec-title">#{id} — {name} ({bytes} bytes) [{status} | {motw}]</p><p class="sec-desc">Source: {url} ({mime})</p><p class="sec-desc">SHA-256: {sha}</p></div>"#,
                id = item.id
            ));
        }
    }
    let body = compile_tsx_component(
        DOWNLOADS_PAGE_TSX,
        &[("version", VERSION), ("entryCount", &count_str)],
        &[("DOWNLOAD_ITEMS_SLOT", &items_html)],
    );
    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n  <title>Root Downloads</title>\n  <style>\n{BROWSER_CSS}\n  </style>\n</head>\n<body>\n{body}\n  <script>\n{ROOT_CORE_JS}\n{DOWNLOADS_JS}\n  </script>\n</body>\n</html>"
    )
}

/// Downloads page HTML with no entries.
pub fn downloads_html() -> String {
    downloads_html_with_entries(&[])
}

/// Fill `frontend/App.tsx` text from `session` and `ui`.
///
/// The function name is historical. The return value is an HTML string. The
/// viewport slot is left empty. `shell` and `ui` paint the page separately.
pub fn render_react_shell_html(session: &Session, ui: &UiState) -> String {
    let active_idx = session.active_index();
    let tab_count = session.tabs().len();
    let mut tabs_html = String::new();
    for (idx, tab) in session.tabs().iter().enumerate() {
        let class_name = if idx == active_idx {
            "tab-active"
        } else {
            "tab-pill"
        };
        let title = escape_html(tab.title());
        let close_btn = if tab_count > 1 {
            format!(
                r#" <span class="tab-close" data-ui-action="close-tab" data-tab-index="{idx}">x</span>"#
            )
        } else {
            String::new()
        };
        tabs_html.push_str(&format!(
            r#"<div class="{class_name}" data-ui-action="select-tab" data-tab-index="{idx}">{title}{close_btn}</div>"#
        ));
    }

    let bookmarks_html = if session.bookmarks().is_empty() {
        String::new()
    } else {
        let mut bar = String::from(r#"<div id="root-bookmarks-bar" class="bookmarks-bar">"#);
        for (idx, bm) in session.bookmarks().iter().enumerate() {
            let label = escape_html(&bm.title);
            bar.push_str(&format!(
                r#"<button class="bookmark-pill" data-ui-action="open-bookmark" data-bookmark-index="{idx}">{label}</button>"#
            ));
        }
        bar.push_str("</div>");
        bar
    };

    let active_url = session
        .active()
        .current()
        .map(|p| p.url.as_str())
        .unwrap_or(HOME_URL);

    let (sec_label, sec_class) = if active_url.starts_with("https://") {
        ("TLS", "security-pill-tls")
    } else if active_url.starts_with("root://") || active_url.starts_with("about:") {
        ("SYS", "security-pill-sys")
    } else if active_url.starts_with("http://") {
        ("HTTP", "security-pill-http")
    } else {
        ("FILE", "security-pill-file")
    };

    let (address_display, address_class) = if ui.is_editing_address() {
        (
            format!("{}|", escape_html(ui.input())),
            "address-input-focused",
        )
    } else {
        (escape_html(active_url), "address-input")
    };

    let star_class = if session.is_active_bookmarked() {
        "nav-btn-active"
    } else {
        "nav-btn"
    };
    let inspector_class = if ui.is_inspector_open() {
        "nav-btn-active"
    } else {
        "nav-btn"
    };
    let app_menu_class = if ui.is_app_menu_open() {
        "nav-btn-active"
    } else {
        "nav-btn"
    };
    let zoom_display = format!("{}%", session.zoom_percent());

    let find_bar_html = if ui.is_find_open() {
        let q = escape_html(ui.find_query());
        format!(
            r#"<div id="root-find-bar" class="find-bar"><span id="root-find-input">Find: {q}|</span><button id="root-find-prev" data-ui-action="find-prev">&lt;</button><button id="root-find-next" data-ui-action="find-next">&gt;</button><button id="root-find-close" data-ui-action="find-close">x</button></div>"#
        )
    } else {
        String::new()
    };

    let app_menu_html = if ui.is_app_menu_open() {
        r#"<div id="root-app-menu" class="app-menu"><div data-ui-action="menu-new-tab">New Tab</div><div data-ui-action="menu-home">Start Dashboard</div><div data-ui-action="menu-downloads">Downloads &amp; Vault</div><div data-ui-action="menu-history">Browsing History</div><div data-ui-action="menu-security">Security &amp; Shields</div><div data-ui-action="menu-find">Find in Page</div><div data-ui-action="menu-inspector-dock">Developer Inspector Dock</div><div data-ui-action="menu-inspector-page">Full-Page Inspector</div><div data-ui-action="menu-zoom">Adjust / Reset Zoom</div><div data-ui-action="menu-settings">Browser Settings</div></div>"#.to_string()
    } else {
        String::new()
    };

    let context_menu_html = if ui.context_menu_pos().is_some() {
        r#"<div id="root-context-menu" class="context-menu"><div data-ui-action="ctx-back">Back</div><div data-ui-action="ctx-forward">Forward</div><div data-ui-action="ctx-reload">Reload</div><div data-ui-action="ctx-copy">Copy</div><div data-ui-action="ctx-paste">Paste</div><div data-ui-action="ctx-find">Find in Page</div><div data-ui-action="ctx-zoom">Reset Zoom</div><div data-ui-action="ctx-inspect">Inspect Page</div><div data-ui-action="ctx-security">Security Shield</div></div>"#.to_string()
    } else {
        String::new()
    };

    let inspector_dock_html = if ui.is_inspector_open() {
        let tab_label = ui.inspector_tab().label();
        format!(
            r#"<div id="root-inspector-dock" class="inspector-dock"><span id="root-inspector-active-tab">DEVTOOLS — {tab_label}</span><button id="root-inspector-full-page" data-ui-action="inspector-full-page">Full Page</button><button id="root-inspector-close" data-ui-action="inspector-close">x</button></div>"#
        )
    } else {
        String::new()
    };

    let status_left = if ui.is_loading() {
        "Loading resource...".to_string()
    } else if let Some(link) = ui.hovered_link() {
        format!("Link: {}", escape_html(link))
    } else {
        format!("Ready — {}", escape_html(active_url))
    };

    let status_right = format!("Root {VERSION}");

    let body = compile_tsx_component(
        APP_TSX,
        &[
            ("version", VERSION),
            ("activeUrl", &escape_html(active_url)),
            ("addressDisplay", &address_display),
            ("addressClass", address_class),
            ("securityLabel", sec_label),
            ("securityClass", sec_class),
            ("zoomDisplay", &zoom_display),
            ("starClass", star_class),
            ("inspectorClass", inspector_class),
            ("appMenuClass", app_menu_class),
            ("statusLeft", &status_left),
            ("statusRight", &status_right),
        ],
        &[
            ("TABS_SLOT", &tabs_html),
            ("BOOKMARKS_SLOT", &bookmarks_html),
            ("FIND_BAR_SLOT", &find_bar_html),
            ("APP_MENU_SLOT", &app_menu_html),
            ("CONTEXT_MENU_SLOT", &context_menu_html),
            ("INSPECTOR_DOCK_SLOT", &inspector_dock_html),
            ("VIEWPORT_SLOT", ""),
        ],
    );

    format!(
        "<!DOCTYPE html>\n<html>\n<head>\n  <title>Root Browser React Shell</title>\n  <style>\n{BROWSER_CSS}\n  </style>\n</head>\n<body>\n{body}\n</body>\n</html>"
    )
}

/// Parse `render_react_shell_html` with `root-dom`. This does not run scripts.
pub fn render_react_shell_document(session: &Session, ui: &UiState) -> Document {
    let html = render_react_shell_html(session, ui);
    parse_html(&html)
}

#[cfg(test)]
mod tests {
    //! Checks that the text replacements remove `className=` and leave the expected element ids.
    use super::*;

    #[test]
    /// The text replacement removes `className=` and leaves the expected element ids.
    fn compiles_react_tsx_shell_and_builtin_dashboards_into_dom() {
        let home = home_html();
        assert!(home.contains("Google Search"));
        assert!(!home.contains("className="));
        let home_doc = parse_html(&home);
        assert!(home_doc.get_element_by_id("home-q").is_some());
        assert!(home_doc.get_element_by_id("home-engine-badge").is_some());

        let insp = inspect_html();
        assert!(insp.contains("Developer Inspector"));
        assert!(!insp.contains("className="));
        let insp_doc = parse_html(&insp);
        assert!(insp_doc.get_element_by_id("root-inspector-table").is_some());

        let sec = security_html();
        assert!(sec.contains("Security"));
        assert!(!sec.contains("className="));

        let settings = settings_html();
        assert!(settings.contains("Video"));
        assert!(settings.contains("Google Search"));
        assert!(settings.contains("DuckDuckGo"));
        assert!(settings.contains("Microsoft Bing"));
        assert!(settings.contains("JavaScript"));
        assert!(!settings.contains("className="));
        let settings_doc = parse_html(&settings);
        assert!(settings_doc.get_element_by_id("gpu-demo-video").is_some());
        assert!(settings_doc.get_element_by_id("nav-sec-search").is_some());
        assert!(settings_doc.get_element_by_id("nav-sec-js").is_some());

        let hist = history_html();
        assert!(hist.contains("Browsing History"));
        assert!(!hist.contains("className="));
        let hist_doc = parse_html(&hist);
        assert!(hist_doc.get_element_by_id("root-history-list").is_some());

        let dl = downloads_html();
        assert!(dl.contains("Download Manager"));
        assert!(!dl.contains("className="));
        let dl_doc = parse_html(&dl);
        assert!(dl_doc.get_element_by_id("root-downloads-list").is_some());

        let session = Session::new(crate::Viewport {
            width: 960.0,
            height: 720.0,
        });
        let mut ui = UiState::new();
        ui.toggle_app_menu();
        ui.toggle_inspector();
        let shell_doc = render_react_shell_document(&session, &ui);
        assert!(shell_doc.get_element_by_id("root-react-shell").is_some());
        assert!(shell_doc.get_element_by_id("root-shell-logo").is_some());
        assert!(shell_doc.get_element_by_id("root-tab-strip").is_some());
        assert!(shell_doc.get_element_by_id("root-nav-bar").is_some());
        assert!(shell_doc.get_element_by_id("root-security-badge").is_some());
        assert!(shell_doc.get_element_by_id("root-address-bar").is_some());
        assert!(shell_doc.get_element_by_id("root-zoom-pill").is_some());
        assert!(shell_doc.get_element_by_id("root-btn-settings").is_some());
        assert!(shell_doc.get_element_by_id("root-btn-downloads").is_some());
        assert!(shell_doc.get_element_by_id("root-btn-inspector").is_some());
        assert!(shell_doc.get_element_by_id("root-btn-app-menu").is_some());
        assert!(shell_doc.get_element_by_id("root-app-menu").is_some());
        assert!(shell_doc.get_element_by_id("root-inspector-dock").is_some());
        assert!(shell_doc.get_element_by_id("root-status-bar").is_some());
    }
}
