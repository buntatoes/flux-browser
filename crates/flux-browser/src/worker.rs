//! One `--tab-worker` process for a single remote document.
//!
//! The parent writes one `FLUXIPC1` request and reads one response. The wait
//! is bounded by the caller's timeout. On timeout the child is killed. The
//! session cookie jar is not part of the request. `flux://` pages and files
//! do not use this module. One short-lived process is not a multi-process
//! sandbox around the window.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use flux_sandbox::{
    MANDATORY_WORKER_TIMEOUT_MS, TabWorkerRequest, TabWorkerResponse, encode_ipc_frame,
    read_ipc_frame,
};

/// `MANDATORY_WORKER_TIMEOUT_MS` as a `Duration`. The window uses this.
pub fn worker_timeout() -> Duration {
    Duration::from_millis(MANDATORY_WORKER_TIMEOUT_MS)
}

/// Path of the running executable. The window passes this to [`exchange_tab_worker`]
/// with the `--tab-worker` argument.
pub fn current_flux_exe() -> Result<PathBuf, String> {
    std::env::current_exe().map_err(|err| format!("could not find the flux executable: {err}"))
}

/// Start `program` with `args`, send `request`, and wait up to `timeout`.
///
/// A timeout, a spawn failure, or a frame that is not `FLUXIPC1` is `Err`.
/// The child is killed on timeout and on a bad frame. `Ok` is a decoded
/// response, including `ok: false`. The caller must not paint `html` unless
/// `ok` is true.
pub fn exchange_tab_worker(
    program: &Path,
    args: &[String],
    request: &TabWorkerRequest,
    timeout: Duration,
) -> Result<TabWorkerResponse, String> {
    let frame = encode_ipc_frame(&request.to_json()).map_err(|err| err.to_string())?;
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Leave the window job so the child can enter the worker job.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        command.creation_flags(CREATE_BREAKAWAY_FROM_JOB);
    }
    let mut child = command
        .spawn()
        .map_err(|err| format!("could not start the tab worker: {err}"))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "tab worker stdin was not piped".to_string())?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "tab worker stdout was not piped".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "tab worker stderr was not piped".to_string())?;

    let write_thread = thread::spawn(move || {
        let result = stdin.write_all(&frame).and_then(|_| stdin.flush());
        drop(stdin);
        result
    });
    let read_thread = thread::spawn(move || read_ipc_frame(&mut stdout));
    let stderr_thread = thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 512];
        while buf.len() < 2048 {
            match stderr.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let room = 2048 - buf.len();
                    buf.extend_from_slice(&chunk[..n.min(room)]);
                }
            }
        }
        String::from_utf8_lossy(&buf).trim().to_string()
    });

    let deadline = Instant::now() + timeout;
    loop {
        if read_thread.is_finished() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = write_thread.join();
            let _ = read_thread.join();
            let _ = stderr_thread.join();
            return Err("tab worker timed out".to_string());
        }
        thread::sleep(Duration::from_millis(10));
    }

    let read_result = read_thread
        .join()
        .map_err(|_| "tab worker reader stopped".to_string())?;
    let _ = child.wait();
    let _ = write_thread.join();
    let stderr_text = stderr_thread.join().unwrap_or_default();
    let json = read_result.map_err(|err| {
        if stderr_text.is_empty() {
            format!("tab worker frame: {err}")
        } else {
            format!("tab worker frame: {err}; {stderr_text}")
        }
    })?;
    TabWorkerResponse::from_json(&json).map_err(|err| err.to_string())
}
