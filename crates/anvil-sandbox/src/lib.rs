//! Process limits for the window, plus a separate worker mode that normal browsing does not use.
//!
//! `anvil-browser` calls [`enforce_mandatory_sandbox_or_abort`] at startup and
//! holds the [`MandatorySandboxGuard`] for the life of the process. Page loads
//! run inside [`MandatorySandboxGuard::execute_isolated`], which is
//! `catch_unwind` in this same process. It does not spawn a process. Tabs are
//! still drawn by the window. This crate never starts `--tab-worker`.
//! `anvil-browser` enters that path when this process was launched with the
//! flag. The window also starts that process for one http(s) navigation and
//! waits [`MANDATORY_WORKER_TIMEOUT_MS`]. This crate never starts it. The
//! `ANVILIPC` types are the wire format. One worker process is not a
//! multi-process sandbox for the window.
//!
//! Windows puts the window process in a Job Object with a 2 GiB process
//! memory cap, kill-on-job-close, and die-on-unhandled-exception. That job
//! does not set a process-count limit and does not set UI restrictions.
//! `--tab-worker` on Windows uses a second job: 256 MiB, one active process,
//! and `JOB_OBJECT_UILIMIT_ALL`. If that job cannot be applied the worker
//! exits. The window path does not use it.
//!
//! Linux sets `PR_SET_NO_NEW_PRIVS` and refuses to start if that call is
//! rejected. It sets `RLIMIT_CORE` to 0 and tries to cap `RLIMIT_AS` at 2 GiB
//! without raising a limit that is already lower. A failed address-space cap
//! does not abort the window. `--tab-worker` on Linux applies those same
//! limits and then requires the 256 MiB cap. If that cap is missing or still
//! above 256 MiB, the worker exits. Other operating systems are rejected.
//!
//! JSON frames use `anvil-json` (`to_json_string` and `parse_json_bounded`).
//! There is no GPU work and no script interpreter in this crate.

use anvil_json::{JsonValue, parse_json_bounded, to_json_string};
use std::fmt;
use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Address-space cap for `--tab-worker` (256 MiB). Not applied to the window process.
pub const MANDATORY_TAB_MEMORY_LIMIT_BYTES: usize = 256 * 1024 * 1024;

/// Process-memory cap requested for the window (2 GiB).
///
/// On Linux this is an `RLIMIT_AS` target and may come back lower. On Windows
/// it is `JOB_OBJECT_LIMIT_PROCESS_MEMORY`.
pub const UI_ADDRESS_SPACE_LIMIT_BYTES: usize = 2 * 1024 * 1024 * 1024;

/// `JOB_OBJECT_LIMIT_ACTIVE_PROCESS` value for `--tab-worker` on Windows.
/// The window job leaves the active-process limit at 0, which means unset.
pub const MANDATORY_ACTIVE_PROCESS_LIMIT: u32 = 1;

/// Largest `ANVILIPC` JSON body (8 MiB), not counting the 12-byte header.
pub const MANDATORY_IPC_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Deepest JSON value accepted by [`decode_ipc_frame`], passed to `anvil-json`.
pub const MANDATORY_IPC_MAX_JSON_DEPTH: usize = 32;

/// Worker time budget, in milliseconds.
///
/// This crate does not start a timer. `anvil-browser` waits this long for one
/// `--tab-worker` frame during a remote navigation, then kills the process.
pub const MANDATORY_WORKER_TIMEOUT_MS: u64 = 5_000;

/// Eight-byte header of every worker frame. The next four bytes are a little-endian length.
pub const IPC_MAGIC: &[u8; 8] = b"ANVILIPC";

/// Value stored in `MandatorySandboxGuard::seal`. A guard with any other value fails [`MandatorySandboxGuard::verify_active`].
const SANDBOX_SEAL_TOKEN: u64 = 0x414E_5649_4C53_4258; // "ANVILSBX"

/// Why setup, a guard check, a frame, or an isolated call failed.
///
/// `OsInitializationFailed` is a rejected OS call. `UnverifiedGuard` means the
/// seal or, on Windows, the job handle is missing. `IpcFramingError` is a bad
/// header, length, encoding, or JSON tree. `IpcPayloadTooLarge` is a body over
/// [`MANDATORY_IPC_MAX_BYTES`]. `TabIsolatedFault` is a workload `Err` or a
/// panic caught by [`MandatorySandboxGuard::execute_isolated`]. The panic stays
/// in this process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxError {
    OsInitializationFailed(String),
    UnverifiedGuard,
    IpcFramingError(&'static str),
    IpcPayloadTooLarge(usize),
    TabIsolatedFault(String),
}

impl fmt::Display for SandboxError {
    /// One-line message. Payload-too-large includes the 8 MiB ceiling.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OsInitializationFailed(reason) => {
                write!(f, "sandbox setup failed: {reason}")
            }
            Self::UnverifiedGuard => {
                write!(f, "sandbox guard is not active")
            }
            Self::IpcFramingError(msg) => write!(f, "sandbox IPC frame error: {msg}"),
            Self::IpcPayloadTooLarge(len) => {
                write!(
                    f,
                    "sandbox IPC payload ({len} bytes) exceeds {} MiB",
                    MANDATORY_IPC_MAX_BYTES / (1024 * 1024)
                )
            }
            Self::TabIsolatedFault(reason) => {
                write!(f, "tab isolated after fault: {reason}")
            }
        }
    }
}

// `Display` is the error source. No extra fields.
impl std::error::Error for SandboxError {}

/// Limits recorded on the guard after setup. This is a snapshot, not a new OS call,
/// except `os_enforced`, which re-runs [`MandatorySandboxGuard::verify_active`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxTelemetry {
    pub backend_name: &'static str,
    pub os_enforced: bool,
    pub memory_limit_bytes: usize,
    pub active_process_limit: u32,
    pub ui_restrictions_mask: u32,
    pub kill_on_close: bool,
    pub max_ipc_bytes: usize,
    pub isolated_tab_executions: u64,
    pub contained_tab_faults: u64,
}

/// Guard held by `anvil-browser` for the life of the window process.
///
/// `seal` must stay `SANDBOX_SEAL_TOKEN`. `isolated_executions` counts calls
/// to [`execute_isolated`](Self::execute_isolated). `contained_faults` counts
/// workload errors and panics. On Windows `job_handle` is the Job Object for
/// this process and is closed in `Drop`. Linux has no handle. The guard does
/// not move tabs into another process.
#[derive(Debug)]
pub struct MandatorySandboxGuard {
    seal: u64,
    backend_name: &'static str,
    ui_restrictions_mask: u32,
    address_space_limit: usize,
    active_process_limit: u32,
    kill_on_close: bool,
    isolated_executions: u64,
    contained_faults: u64,
    // Windows only. The Job Object for this process. Linux has no handle field.
    #[cfg(target_os = "windows")]
    job_handle: isize,
}

// Windows only. Linux has no job handle to close.
#[cfg(target_os = "windows")]
impl Drop for MandatorySandboxGuard {
    /// Close the window Job Object. A zero handle means it was already closed.
    ///
    /// `KILL_ON_JOB_CLOSE` is cleared first. The process that owns the guard is
    /// in this job, so leaving the flag set would terminate it inside `Drop`
    /// and hide the result of a test. A tab worker breaks away into its own job.
    fn drop(&mut self) {
        if self.job_handle != 0 {
            unsafe {
                disarm_window_job_kill(self.job_handle);
                CloseHandle(self.job_handle);
            }
            self.job_handle = 0;
        }
    }
}

impl MandatorySandboxGuard {
    /// Set up window-process limits, or return an error.
    ///
    /// Runs the IPC round trip first. Windows assigns the current process to
    /// the UI job. Linux applies `PR_SET_NO_NEW_PRIVS`, a zero core limit, and
    /// the 2 GiB attempt. Any other OS returns `OsInitializationFailed`.
    /// Failure here does not switch the process into `--tab-worker`.
    pub fn initialize_or_fail() -> Result<Self, SandboxError> {
        verify_ipc_self_test()?;

        // Windows window path: memory cap, die-on-unhandled-exception, kill-on-job-close.
        // No UI restriction class and no active-process cap.
        #[cfg(target_os = "windows")]
        {
            let (job_handle, limit) = create_windows_ui_job()?;
            let current = unsafe { GetCurrentProcess() };
            let assigned = unsafe { AssignProcessToJobObject(job_handle, current) };
            if assigned == 0 {
                unsafe {
                    CloseHandle(job_handle);
                }
                return Err(SandboxError::OsInitializationFailed(
                    "AssignProcessToJobObject failed".to_string(),
                ));
            }
            Ok(Self {
                seal: SANDBOX_SEAL_TOKEN,
                backend_name: "Windows job",
                ui_restrictions_mask: 0,
                address_space_limit: limit,
                active_process_limit: 0,
                kill_on_close: true,
                isolated_executions: 0,
                contained_faults: 0,
                job_handle,
            })
        }

        // Linux window path: no-new-privs is mandatory. The address cap is best-effort.
        #[cfg(target_os = "linux")]
        {
            let limit = apply_linux_ui_limits()?;
            Ok(Self {
                seal: SANDBOX_SEAL_TOKEN,
                backend_name: "Linux",
                ui_restrictions_mask: 0,
                address_space_limit: limit,
                active_process_limit: 0,
                kill_on_close: false,
                isolated_executions: 0,
                contained_faults: 0,
            })
        }

        // Not Windows and not Linux. The window refuses to start.
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            Err(SandboxError::OsInitializationFailed(
                "this operating system is not supported".to_string(),
            ))
        }
    }

    /// Reject a guard whose seal was overwritten. On Windows a zero job handle also fails.
    pub fn verify_active(&self) -> Result<(), SandboxError> {
        if self.seal != SANDBOX_SEAL_TOKEN {
            return Err(SandboxError::UnverifiedGuard);
        }
        // Windows: the Job Object must still be open. Linux has no handle field.
        #[cfg(target_os = "windows")]
        if self.job_handle == 0 {
            return Err(SandboxError::UnverifiedGuard);
        }
        Ok(())
    }

    /// Run `workload` in this process and count a panic instead of letting it escape.
    ///
    /// `catch_unwind` is the isolation. There is no child process and no
    /// `--tab-worker` launch. `Ok(Ok)` returns the value. `Ok(Err)` and a panic
    /// both increment `contained_faults` and return `TabIsolatedFault` with
    /// `tab_label`. The guard must already be sealed.
    pub fn execute_isolated<T, F>(
        &mut self,
        tab_label: &str,
        workload: F,
    ) -> Result<T, SandboxError>
    where
        F: FnOnce() -> Result<T, String>,
    {
        self.verify_active()?;
        self.isolated_executions = self.isolated_executions.saturating_add(1);

        match catch_unwind(AssertUnwindSafe(workload)) {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(err)) => {
                self.contained_faults = self.contained_faults.saturating_add(1);
                Err(SandboxError::TabIsolatedFault(format!(
                    "[{tab_label}] {err}"
                )))
            }
            Err(_) => {
                self.contained_faults = self.contained_faults.saturating_add(1);
                Err(SandboxError::TabIsolatedFault(format!(
                    "[{tab_label}] panicked"
                )))
            }
        }
    }

    /// Copy the limits stored on this guard.
    ///
    /// `os_enforced` is the current [`verify_active`](Self::verify_active) result.
    /// `memory_limit_bytes` is whatever setup recorded, which on Linux may be
    /// lower than 2 GiB. `max_ipc_bytes` is always [`MANDATORY_IPC_MAX_BYTES`].
    pub fn telemetry(&self) -> SandboxTelemetry {
        SandboxTelemetry {
            backend_name: self.backend_name,
            os_enforced: self.verify_active().is_ok(),
            memory_limit_bytes: self.address_space_limit,
            active_process_limit: self.active_process_limit,
            ui_restrictions_mask: self.ui_restrictions_mask,
            kill_on_close: self.kill_on_close,
            max_ipc_bytes: MANDATORY_IPC_MAX_BYTES,
            isolated_tab_executions: self.isolated_executions,
            contained_tab_faults: self.contained_faults,
        }
    }
}

/// Set up the window limits, or exit with status 101.
///
/// This is what `anvil-browser` calls for normal browsing. It does not start a
/// tab worker. A printed line goes to stderr before the exit.
pub fn enforce_mandatory_sandbox_or_abort() -> MandatorySandboxGuard {
    match MandatorySandboxGuard::initialize_or_fail() {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("anvil: sandbox setup failed: {err}");
            std::process::exit(101);
        }
    }
}

/// Apply the tighter `--tab-worker` limits, or exit with status 102.
///
/// Called only from the `--tab-worker` branch in `anvil-browser`. Ordinary
/// tabs do not call this and are not spawned into this mode. Windows builds
/// the worker job (256 MiB, one process, all UI limits). Linux reapplies the
/// window limits and then requires the 256 MiB address cap.
pub fn lock_current_worker_process_or_abort() {
    let res: Result<(), SandboxError> = (|| {
        // Windows worker: memory, process count, and UI limits. The UI mask is not stored.
        #[cfg(target_os = "windows")]
        {
            let (job, _) = create_windows_worker_job()?;
            let current = unsafe { GetCurrentProcess() };
            let assigned = unsafe { AssignProcessToJobObject(job, current) };
            if assigned == 0 {
                unsafe {
                    CloseHandle(job);
                }
                return Err(SandboxError::OsInitializationFailed(
                    "AssignProcessToJobObject failed for worker".to_string(),
                ));
            }
            Ok(())
        }
        // Linux worker: no-new-privs, core dumps off, then a required 256 MiB cap.
        #[cfg(target_os = "linux")]
        {
            apply_linux_worker_rlimits()
        }
        // Not Windows and not Linux. The worker exits via the error below.
        #[cfg(not(any(target_os = "windows", target_os = "linux")))]
        {
            Err(SandboxError::OsInitializationFailed(
                "unsupported worker OS".to_string(),
            ))
        }
    })();

    if let Err(err) = res {
        eprintln!("anvil: worker sandbox setup failed: {err}");
        std::process::exit(102);
    }
}

/// JSON object a parent sends to a `--tab-worker` process.
///
/// Fields are `tab_id`, `url`, `html`, `scripts_enabled`, `csp_header`,
/// `viewport_width`, and `viewport_height`. The HTML is the document the
/// parent already fetched. The cookie jar is not in this object.
/// `anvil-browser` writes one of these for an http(s) navigation. Defining the
/// type does not spawn a worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabWorkerRequest {
    pub tab_id: u64,
    pub url: String,
    pub html: String,
    pub scripts_enabled: bool,
    pub csp_header: Option<String>,
    /// CSS pixel width. Zero is treated as 1 by the worker.
    pub viewport_width: u32,
    /// CSS pixel height. Zero is treated as 1 by the worker.
    pub viewport_height: u32,
}

/// JSON object a `--tab-worker` process writes back.
///
/// Fields are `tab_id`, `ok`, `title`, `dom_nodes`, `blocked_csp_resources`,
/// `error`, and `html`. `html` is the document after scripts when `ok` is
/// true. The window paints that HTML only after a successful frame. A failed
/// or missing frame does not include the document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TabWorkerResponse {
    pub tab_id: u64,
    pub ok: bool,
    pub title: String,
    pub dom_nodes: usize,
    pub blocked_csp_resources: usize,
    pub error: Option<String>,
    /// Post-script document. Empty when `ok` is false.
    pub html: String,
}

impl TabWorkerRequest {
    /// Encode the request as a `anvil-json` object. A missing CSP header is JSON null.
    pub fn to_json(&self) -> JsonValue {
        let mut obj = JsonValue::empty_object();
        obj.insert("tab_id", JsonValue::Number(self.tab_id as f64));
        obj.insert("url", JsonValue::String(self.url.clone()));
        obj.insert("html", JsonValue::String(self.html.clone()));
        obj.insert("scripts_enabled", JsonValue::Bool(self.scripts_enabled));
        match &self.csp_header {
            Some(csp) => obj.insert("csp_header", JsonValue::String(csp.clone())),
            None => obj.insert("csp_header", JsonValue::Null),
        }
        obj.insert(
            "viewport_width",
            JsonValue::Number(self.viewport_width as f64),
        );
        obj.insert(
            "viewport_height",
            JsonValue::Number(self.viewport_height as f64),
        );
        obj
    }

    /// Read a request. `tab_id`, `url`, and `html` are required.
    /// `scripts_enabled` defaults to true. `csp_header` is optional.
    /// Missing viewport fields default to 960 by 720.
    pub fn from_json(val: &JsonValue) -> Result<Self, SandboxError> {
        let tab_id = val.get_u64("tab_id").ok_or(SandboxError::IpcFramingError(
            "missing tab_id in worker request",
        ))?;
        let url = val
            .get_str("url")
            .ok_or(SandboxError::IpcFramingError(
                "missing url in worker request",
            ))?
            .to_string();
        let html = val
            .get_str("html")
            .ok_or(SandboxError::IpcFramingError(
                "missing html in worker request",
            ))?
            .to_string();
        let scripts_enabled = val.get_bool("scripts_enabled").unwrap_or(true);
        let csp_header = val.get_str("csp_header").map(ToString::to_string);
        let viewport_width = val.get_u64("viewport_width").unwrap_or(960) as u32;
        let viewport_height = val.get_u64("viewport_height").unwrap_or(720) as u32;
        Ok(Self {
            tab_id,
            url,
            html,
            scripts_enabled,
            csp_header,
            viewport_width,
            viewport_height,
        })
    }
}

impl TabWorkerResponse {
    /// Encode the response as a `anvil-json` object. A missing error is JSON null.
    pub fn to_json(&self) -> JsonValue {
        let mut obj = JsonValue::empty_object();
        obj.insert("tab_id", JsonValue::Number(self.tab_id as f64));
        obj.insert("ok", JsonValue::Bool(self.ok));
        obj.insert("title", JsonValue::String(self.title.clone()));
        obj.insert("dom_nodes", JsonValue::Number(self.dom_nodes as f64));
        obj.insert(
            "blocked_csp_resources",
            JsonValue::Number(self.blocked_csp_resources as f64),
        );
        match &self.error {
            Some(err) => obj.insert("error", JsonValue::String(err.clone())),
            None => obj.insert("error", JsonValue::Null),
        }
        obj.insert("html", JsonValue::String(self.html.clone()));
        obj
    }

    /// Read a response. `tab_id` and `ok` are required.
    /// `title` defaults to "Untitled". Counts default to 0. `error` is optional.
    pub fn from_json(val: &JsonValue) -> Result<Self, SandboxError> {
        let tab_id = val.get_u64("tab_id").ok_or(SandboxError::IpcFramingError(
            "missing tab_id in worker response",
        ))?;
        let ok = val.get_bool("ok").ok_or(SandboxError::IpcFramingError(
            "missing ok in worker response",
        ))?;
        let title = val.get_str("title").unwrap_or("Untitled").to_string();
        let dom_nodes = val.get_u64("dom_nodes").unwrap_or(0) as usize;
        let blocked_csp_resources = val.get_u64("blocked_csp_resources").unwrap_or(0) as usize;
        let error = val.get_str("error").map(ToString::to_string);
        let html = val.get_str("html").unwrap_or("").to_string();
        Ok(Self {
            tab_id,
            ok,
            title,
            dom_nodes,
            blocked_csp_resources,
            error,
            html,
        })
    }
}

/// Serialize `payload` with `anvil-json` and wrap it as `ANVILIPC` + u32le length + bytes.
///
/// The length is the JSON body only. A body larger than [`MANDATORY_IPC_MAX_BYTES`]
/// is `IpcPayloadTooLarge` and is not framed.
pub fn encode_ipc_frame(payload: &JsonValue) -> Result<Vec<u8>, SandboxError> {
    let serialized = to_json_string(payload);
    let bytes = serialized.as_bytes();
    if bytes.len() > MANDATORY_IPC_MAX_BYTES {
        return Err(SandboxError::IpcPayloadTooLarge(bytes.len()));
    }
    let mut frame = Vec::with_capacity(12 + bytes.len());
    frame.extend_from_slice(IPC_MAGIC);
    frame.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    frame.extend_from_slice(bytes);
    Ok(frame)
}

/// Check magic, length, and UTF-8, then parse with `anvil-json`'s bounded parser.
///
/// The frame must be exactly 12 + declared length bytes. The declared length
/// may not exceed [`MANDATORY_IPC_MAX_BYTES`]. Nesting may not exceed
/// [`MANDATORY_IPC_MAX_JSON_DEPTH`].
pub fn decode_ipc_frame(frame: &[u8]) -> Result<JsonValue, SandboxError> {
    if frame.len() < 12 {
        return Err(SandboxError::IpcFramingError(
            "frame shorter than 12-byte header",
        ));
    }
    if &frame[..8] != IPC_MAGIC {
        return Err(SandboxError::IpcFramingError(
            "invalid ANVILIPC magic header",
        ));
    }
    let declared_len = u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]) as usize;
    if declared_len > MANDATORY_IPC_MAX_BYTES {
        return Err(SandboxError::IpcPayloadTooLarge(declared_len));
    }
    if frame.len() != 12 + declared_len {
        return Err(SandboxError::IpcFramingError("frame length mismatch"));
    }
    let text = std::str::from_utf8(&frame[12..])
        .map_err(|_| SandboxError::IpcFramingError("non-UTF8 IPC payload"))?;
    parse_json_bounded(text, MANDATORY_IPC_MAX_JSON_DEPTH, MANDATORY_IPC_MAX_BYTES)
        .map_err(|_| SandboxError::IpcFramingError("malformed or over-nested IPC JSON"))
}

/// Write one frame and flush. A short write is `IpcFramingError`.
///
/// The `--tab-worker` process uses this on stdout. The window uses it to send
/// one request for an http(s) navigation.
pub fn write_ipc_frame<W: Write>(writer: &mut W, payload: &JsonValue) -> Result<(), SandboxError> {
    let frame = encode_ipc_frame(payload)?;
    writer
        .write_all(&frame)
        .and_then(|_| writer.flush())
        .map_err(|_| SandboxError::IpcFramingError("failed to write IPC frame"))
}

/// Read a 12-byte header, reject a bad magic or an overlong body, then read exactly that body.
///
/// The body buffer is sized from the declared length only after the cap check,
/// so a huge length does not allocate. The bytes then go through [`decode_ipc_frame`].
pub fn read_ipc_frame<R: Read>(reader: &mut R) -> Result<JsonValue, SandboxError> {
    let mut header = [0_u8; 12];
    reader
        .read_exact(&mut header)
        .map_err(|_| SandboxError::IpcFramingError("failed to read 12-byte IPC header"))?;
    if &header[..8] != IPC_MAGIC {
        return Err(SandboxError::IpcFramingError(
            "invalid ANVILIPC magic header",
        ));
    }
    let declared_len = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
    if declared_len > MANDATORY_IPC_MAX_BYTES {
        return Err(SandboxError::IpcPayloadTooLarge(declared_len));
    }
    let mut frame = Vec::with_capacity(12 + declared_len);
    frame.extend_from_slice(&header);
    frame.resize(12 + declared_len, 0);
    reader
        .read_exact(&mut frame[12..])
        .map_err(|_| SandboxError::IpcFramingError("truncated IPC frame body"))?;
    decode_ipc_frame(&frame)
}

/// Encode and decode one fixed request before OS limits are applied.
///
/// A mismatch is `OsInitializationFailed`, so a broken framer stops startup.
/// This does not talk to another process.
fn verify_ipc_self_test() -> Result<(), SandboxError> {
    let req = TabWorkerRequest {
        tab_id: 1,
        url: "anvil://security".to_string(),
        html: "<p>sandbox self-test</p>".to_string(),
        scripts_enabled: true,
        csp_header: Some("default-src 'self'".to_string()),
        viewport_width: 960,
        viewport_height: 720,
    };
    let frame = encode_ipc_frame(&req.to_json())?;
    let decoded_json = decode_ipc_frame(&frame)?;
    let decoded_req = TabWorkerRequest::from_json(&decoded_json)?;
    if decoded_req != req {
        return Err(SandboxError::OsInitializationFailed(
            "IPC round-trip self-test mismatch".to_string(),
        ));
    }
    Ok(())
}

// Windows only. kernel32 Job Object for the window process and for `--tab-worker`.
// The window job does not set UI restrictions. The worker job does.
#[cfg(target_os = "windows")]
/// `JobObjectBasicUIRestrictions` information class (4). Worker job only.
const JOB_OBJECT_BASIC_UI_RESTRICTIONS: i32 = 4;
#[cfg(target_os = "windows")]
/// `JobObjectExtendedLimitInformation` information class (9). Both jobs.
const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;

#[cfg(target_os = "windows")]
/// Limit flag: cap how many processes the job may hold. Worker job only.
const JOB_OBJECT_LIMIT_ACTIVE_PROCESS: u32 = 0x0000_0008;
#[cfg(target_os = "windows")]
/// Limit flag: `ProcessMemoryLimit` is in bytes. Both jobs.
const JOB_OBJECT_LIMIT_PROCESS_MEMORY: u32 = 0x0000_0100;
#[cfg(target_os = "windows")]
/// Limit flag: an unhandled exception ends the process. Both jobs.
const JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION: u32 = 0x0000_0400;
#[cfg(target_os = "windows")]
/// Limit flag: closing the last job handle kills assigned processes. Both jobs.
const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;
#[cfg(target_os = "windows")]
/// Limit flag: a child may pass `CREATE_BREAKAWAY_FROM_JOB`. Window job only.
const JOB_OBJECT_LIMIT_BREAKAWAY_OK: u32 = 0x0000_0800;

#[cfg(target_os = "windows")]
/// All basic UI restriction bits (0xFF). Clipboard, desktop, handles, and exits. Worker only.
const JOB_OBJECT_UILIMIT_ALL: u32 = 0x0000_00FF;

/// `JOBOBJECT_BASIC_LIMIT_INFORMATION`. Only `limit_flags` and `active_process_limit` are set.
#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct JobObjectBasicLimitInformation {
    per_process_user_time_limit: i64,
    per_job_user_time_limit: i64,
    limit_flags: u32,
    minimum_working_set_size: usize,
    maximum_working_set_size: usize,
    active_process_limit: u32,
    affinity: usize,
    priority_class: u32,
    scheduling_class: u32,
}

/// `IO_COUNTERS`. Present so the extended struct matches the Windows layout. Left zero.
#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct IoCounters {
    read_operation_count: u64,
    write_operation_count: u64,
    other_operation_count: u64,
    read_transfer_count: u64,
    write_transfer_count: u64,
    other_transfer_count: u64,
}

/// `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`. `process_memory_limit` is the cap in bytes.
#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct JobObjectExtendedLimitInformation {
    basic_limit_information: JobObjectBasicLimitInformation,
    io_info: IoCounters,
    process_memory_limit: usize,
    job_memory_limit: usize,
    peak_process_memory_used: usize,
    peak_job_memory_used: usize,
}

/// `JOBOBJECT_BASIC_UI_RESTRICTIONS`. One `u32` class mask. Worker job only.
#[cfg(target_os = "windows")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct JobObjectBasicUiRestrictions {
    ui_restrictions_class: u32,
}

// kernel32 job calls. Handles are `isize`. A zero return is failure.
#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    /// Anonymous job. Both pointers are null. There is no job name.
    fn CreateJobObjectW(security_attributes: *const core::ffi::c_void, name: *const u16) -> isize;
    /// Writes either extended limits or basic UI restrictions, selected by `info_class`.
    fn SetInformationJobObject(
        job: isize,
        info_class: i32,
        info: *const core::ffi::c_void,
        info_length: u32,
    ) -> i32;
    /// Puts `process` into `job`. The window and the worker each assign themselves.
    fn AssignProcessToJobObject(job: isize, process: isize) -> i32;
    /// Pseudo-handle of this process. Not closed.
    fn GetCurrentProcess() -> isize;
    /// Closes a job handle. Used on failure and when the guard is dropped.
    fn CloseHandle(handle: isize) -> i32;
}

/// Drop `KILL_ON_JOB_CLOSE` so closing the handle does not terminate this process.
#[cfg(target_os = "windows")]
fn disarm_window_job_kill(job: isize) {
    let mut ext = JobObjectExtendedLimitInformation::default();
    ext.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
        | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    ext.process_memory_limit = UI_ADDRESS_SPACE_LIMIT_BYTES;
    unsafe {
        let _ = SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            (&ext as *const JobObjectExtendedLimitInformation).cast(),
            std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
        );
    }
}

/// Apply extended limits. On failure the job handle is closed and an error is returned.
#[cfg(target_os = "windows")]
fn set_job_limits(job: isize, ext: &JobObjectExtendedLimitInformation) -> Result<(), SandboxError> {
    let ok = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
            (ext as *const JobObjectExtendedLimitInformation).cast(),
            std::mem::size_of::<JobObjectExtendedLimitInformation>() as u32,
        )
    };
    if ok == 0 {
        unsafe {
            CloseHandle(job);
        }
        return Err(SandboxError::OsInitializationFailed(
            "SetInformationJobObject failed".to_string(),
        ));
    }
    Ok(())
}

/// Window job: 2 GiB process memory, die on unhandled exception, kill when the job closes.
///
/// `JOB_OBJECT_LIMIT_BREAKAWAY_OK` lets a child started with
/// `CREATE_BREAKAWAY_FROM_JOB` leave this job. The tab worker uses that so it
/// can enter its own job. No active-process limit and no UI restriction class.
/// Returns the handle and the byte cap.
#[cfg(target_os = "windows")]
fn create_windows_ui_job() -> Result<(isize, usize), SandboxError> {
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job == 0 {
        return Err(SandboxError::OsInitializationFailed(
            "CreateJobObjectW returned NULL".to_string(),
        ));
    }
    let mut ext = JobObjectExtendedLimitInformation::default();
    ext.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        | JOB_OBJECT_LIMIT_BREAKAWAY_OK;
    ext.process_memory_limit = UI_ADDRESS_SPACE_LIMIT_BYTES;
    set_job_limits(job, &ext)?;
    Ok((job, UI_ADDRESS_SPACE_LIMIT_BYTES))
}

/// Worker job: one active process, 256 MiB, the same kill flags, then all UI limits.
///
/// Returns the handle and `JOB_OBJECT_UILIMIT_ALL`. The caller discards the mask.
/// A failed UI call closes the job. This is not used for normal browsing.
#[cfg(target_os = "windows")]
fn create_windows_worker_job() -> Result<(isize, u32), SandboxError> {
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job == 0 {
        return Err(SandboxError::OsInitializationFailed(
            "CreateJobObjectW returned NULL".to_string(),
        ));
    }
    let mut ext = JobObjectExtendedLimitInformation::default();
    ext.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_ACTIVE_PROCESS
        | JOB_OBJECT_LIMIT_PROCESS_MEMORY
        | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
        | JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    ext.basic_limit_information.active_process_limit = MANDATORY_ACTIVE_PROCESS_LIMIT;
    ext.process_memory_limit = MANDATORY_TAB_MEMORY_LIMIT_BYTES;
    set_job_limits(job, &ext)?;

    let ui = JobObjectBasicUiRestrictions {
        ui_restrictions_class: JOB_OBJECT_UILIMIT_ALL,
    };
    let ok_ui = unsafe {
        SetInformationJobObject(
            job,
            JOB_OBJECT_BASIC_UI_RESTRICTIONS,
            (&ui as *const JobObjectBasicUiRestrictions).cast(),
            std::mem::size_of::<JobObjectBasicUiRestrictions>() as u32,
        )
    };
    if ok_ui == 0 {
        unsafe {
            CloseHandle(job);
        }
        return Err(SandboxError::OsInitializationFailed(
            "SetInformationJobObject failed".to_string(),
        ));
    }
    Ok((job, JOB_OBJECT_UILIMIT_ALL))
}

// Linux only. prctl and setrlimit. Windows uses the Job Object above instead.
#[cfg(target_os = "linux")]
/// `prctl` option. Arg2 = 1 locks out new privileges for this process.
const PR_SET_NO_NEW_PRIVS: i32 = 38;
#[cfg(target_os = "linux")]
/// Resource id for core dumps. Set to zero so a crash does not write a core.
const RLIMIT_CORE: i32 = 4;
#[cfg(target_os = "linux")]
/// Resource id for address space. The cap is in bytes.
const RLIMIT_AS: i32 = 9;

/// `rlimit`. `rlim_cur` is the soft limit. `rlim_max` is the hard ceiling.
#[cfg(target_os = "linux")]
#[repr(C)]
#[derive(Clone, Copy)]
struct RLimit {
    rlim_cur: usize,
    rlim_max: usize,
}

// libc calls. A non-zero return is failure. errno is not read.
#[cfg(target_os = "linux")]
#[link(name = "c")]
unsafe extern "C" {
    /// Used only as `prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0)`.
    fn prctl(option: i32, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> i32;
    /// Reads the current soft and hard limit for one resource.
    fn getrlimit(resource: i32, rlim: *mut RLimit) -> i32;
    /// Writes a soft and hard limit. The hard limit is not raised.
    fn setrlimit(resource: i32, rlim: *const RLimit) -> i32;
}

/// Try to lower `RLIMIT_AS` to `bytes` without raising either side.
///
/// Returns the soft limit that is in effect afterward. `getrlimit` failure
/// returns 0. `setrlimit` failure returns the previous soft limit. `RLIM_INFINITY`
/// is `usize::MAX` and is replaced rather than kept when it is the hard limit.
#[cfg(target_os = "linux")]
fn cap_address_space(bytes: usize) -> usize {
    /// `RLIM_INFINITY` on this platform.
    const INFINITY: usize = usize::MAX;
    let mut current = RLimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { getrlimit(RLIMIT_AS, &mut current) } != 0 {
        return 0;
    }
    let mut target = if current.rlim_max == INFINITY {
        bytes
    } else {
        bytes.min(current.rlim_max)
    };
    if current.rlim_cur != INFINITY {
        target = target.min(current.rlim_cur);
    }
    if target == current.rlim_cur && current.rlim_cur != INFINITY {
        return current.rlim_cur;
    }
    let mem_limit = RLimit {
        rlim_cur: target,
        rlim_max: current.rlim_max,
    };
    if unsafe { setrlimit(RLIMIT_AS, &mem_limit) } == 0 {
        target
    } else {
        current.rlim_cur
    }
}

/// Window limits. `PR_SET_NO_NEW_PRIVS` must succeed.
///
/// Core dumps are set to zero and a failure there is ignored. The 2 GiB
/// address cap is best-effort: the returned value may be 0 or the old soft limit.
#[cfg(target_os = "linux")]
fn apply_linux_ui_limits() -> Result<usize, SandboxError> {
    let rc = unsafe { prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if rc != 0 {
        return Err(SandboxError::OsInitializationFailed(
            "prctl(PR_SET_NO_NEW_PRIVS) was rejected".to_string(),
        ));
    }
    let core_zero = RLimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let _ = unsafe { setrlimit(RLIMIT_CORE, &core_zero) };
    Ok(cap_address_space(UI_ADDRESS_SPACE_LIMIT_BYTES))
}

/// Worker limits. Runs the window limits, then requires a soft cap at or under 256 MiB.
///
/// `applied == 0` or a cap still above 256 MiB is a hard error. The caller exits.
/// Normal browsing does not call this.
#[cfg(target_os = "linux")]
fn apply_linux_worker_rlimits() -> Result<(), SandboxError> {
    apply_linux_ui_limits()?;
    let applied = cap_address_space(MANDATORY_TAB_MEMORY_LIMIT_BYTES);
    if applied == 0 || applied > MANDATORY_TAB_MEMORY_LIMIT_BYTES {
        return Err(SandboxError::OsInitializationFailed(
            "address space cap was not applied".to_string(),
        ));
    }
    Ok(())
}

/// Window-guard startup, panic accounting, and one IPC round trip. No child process.
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn initializes_mandatory_os_sandbox_and_seals_guard() {
        let mut guard = MandatorySandboxGuard::initialize_or_fail().unwrap();
        assert!(guard.verify_active().is_ok());

        let telemetry = guard.telemetry();
        assert!(telemetry.os_enforced);
        assert_eq!(telemetry.active_process_limit, 0);
        assert_eq!(telemetry.ui_restrictions_mask, 0);
        // Linux window job is not kill-on-close. There is no job handle.
        #[cfg(target_os = "linux")]
        assert!(!telemetry.kill_on_close);
        // Windows window job sets JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE.
        #[cfg(target_os = "windows")]
        assert!(telemetry.kill_on_close);

        let ok_res = guard
            .execute_isolated("tab-1", || Ok::<_, String>(42))
            .unwrap();
        assert_eq!(ok_res, 42);
        assert_eq!(guard.telemetry().isolated_tab_executions, 1);
        assert_eq!(guard.telemetry().contained_tab_faults, 0);

        let fault_res: Result<(), SandboxError> =
            guard.execute_isolated("tab-2", || Err("script exceeded recursion cap".to_string()));
        assert!(matches!(fault_res, Err(SandboxError::TabIsolatedFault(_))));
        assert_eq!(guard.telemetry().contained_tab_faults, 1);
    }

    #[test]
    fn frames_and_validates_ipc_messages_with_bounded_limits() {
        let resp = TabWorkerResponse {
            tab_id: 7,
            ok: true,
            title: "Anvil Sandbox Worker".to_string(),
            dom_nodes: 128,
            blocked_csp_resources: 2,
            error: None,
            html: "<p>ok</p>".to_string(),
        };
        let mut pipe = Vec::new();
        write_ipc_frame(&mut pipe, &resp.to_json()).unwrap();

        let mut cursor = Cursor::new(pipe);
        let decoded_val = read_ipc_frame(&mut cursor).unwrap();
        let decoded = TabWorkerResponse::from_json(&decoded_val).unwrap();
        assert_eq!(decoded, resp);

        assert!(matches!(
            decode_ipc_frame(b"BADMAGIC\x02\x00\x00\x00{}"),
            Err(SandboxError::IpcFramingError(_))
        ));
    }
}
