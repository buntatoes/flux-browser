//! http(s) documents go through one `--tab-worker` process.
//!
//! A successful frame is the document the window paints. A timeout does not
//! paint the source that was sent. The parent process is placed in the window
//! job first, so the child has to break away before it can enter the worker job.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anvil_browser::worker::exchange_tab_worker;
use anvil_browser::{
    MandatorySandboxGuard, TabWorkerRequest, Viewport, navigation_error_page,
    render_through_tab_worker,
};

#[test]
fn remote_document_comes_from_the_tab_worker_or_shows_an_error() {
    let _guard = MandatorySandboxGuard::initialize_or_fail().expect("window job");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_anvil"));
    let viewport = Viewport {
        width: 640.0,
        height: 480.0,
    };
    let html = "<p id=\"out\">secret-marker</p>\
                <script>document.getElementById('out').textContent='from-worker'</script>";
    let page = render_through_tab_worker(
        &exe,
        &["--tab-worker".to_string()],
        Duration::from_secs(5),
        7,
        "http://127.0.0.1/worker-page",
        html,
        None,
        true,
        viewport,
        None,
    )
    .expect("worker frame");
    let out = page.document.get_element_by_id("out").expect("out");
    assert_eq!(page.document.text_content(out), "from-worker");

    let quiet = render_through_tab_worker(
        &exe,
        &["--tab-worker".to_string()],
        Duration::from_secs(5),
        7,
        "http://127.0.0.1/worker-page",
        html,
        None,
        false,
        viewport,
        None,
    )
    .expect("worker frame without scripts");
    let quiet_out = quiet.document.get_element_by_id("out").expect("out");
    assert_eq!(quiet.document.text_content(quiet_out), "secret-marker");

    let secret = "secret-marker-not-painted";
    let request = TabWorkerRequest {
        tab_id: 9,
        url: "http://127.0.0.1/nope".to_string(),
        html: format!("<p>{secret}</p>"),
        scripts_enabled: true,
        csp_header: None,
        viewport_width: 320,
        viewport_height: 200,
    };
    let err = exchange_tab_worker(
        Path::new("powershell.exe"),
        &[
            "-NoProfile".to_string(),
            "-Command".to_string(),
            "Start-Sleep -Seconds 30".to_string(),
        ],
        &request,
        Duration::from_millis(800),
    )
    .expect_err("sleeping process");
    assert!(err.contains("timed out"), "{err}");
    assert!(!err.contains(secret));
    let failed = navigation_error_page("http://127.0.0.1/nope", &err, viewport);
    let text = failed.document.text_content(failed.document.body());
    assert_eq!(failed.title, "Navigation failed");
    assert!(text.contains("timed out"));
    assert!(!text.contains(secret));
}
