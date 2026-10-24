//! Optional Linux link setup for OpenSSL `libssl` and `libcrypto`.
//!
//! Windows does not use this script. Schannel is linked from `tls.rs` with
//! `secur32`. On Linux this script searches for the shared libraries, symlinks
//! them into `OUT_DIR/openssl-link`, and adds that directory to the rustc
//! native link search path. A missing library or a failed symlink prints a
//! cargo warning and the script still exits successfully. It does not fail
//! the build, and it does not pass link-lib flags itself. `tls.rs` declares
//! the `ssl` and `crypto` link names. There is no HTTP stack here.

/// Cargo build entry. See the module comment for the Linux OpenSSL search.
fn main() {
    // Rebuild only when this script changes. Library paths are not tracked inputs.
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(unix)]
    linux_setup::configure();
}

#[cfg(unix)]
mod linux_setup {
    use std::env;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    pub fn configure() {
        // Non-Linux targets stop here with no link search path.
        if env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() != "linux" {
            return;
        }
        let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
        let link_dir = out_dir.join("openssl-link");
        // The directory is created even when both lookups fail. Errors are ignored.
        let _ = fs::create_dir_all(&link_dir);
        for stem in ["ssl", "crypto"] {
            let dst = link_dir.join(format!("lib{stem}.so"));
            if let Some(src) = find_shared_lib(stem) {
                // Drop a symlink left in this OUT_DIR by an earlier run.
                if dst.exists() {
                    let _ = fs::remove_file(&dst);
                }
                // A failed symlink is a warning. Configure continues.
                if std::os::unix::fs::symlink(&src, &dst).is_err() {
                    println!("cargo:warning=could not link {src:?} for HTTPS");
                }
            } else {
                // Not found is a warning. Configure still succeeds.
                println!("cargo:warning=could not find lib{stem} for HTTPS");
            }
        }
        // Always register the search path on Linux, including when it is empty.
        println!("cargo:rustc-link-search=native={}", link_dir.display());
    }

    /// Locate `lib{stem}.so.3`, then `lib{stem}.so`.
    fn find_shared_lib(stem: &str) -> Option<PathBuf> {
        let names = [format!("lib{stem}.so.3"), format!("lib{stem}.so")];
        if let Some(path) = from_ldconfig(&names) {
            return Some(path);
        }
        let mut dirs = Vec::new();
        if let Ok(out) = Command::new("gcc").args(["-print-multiarch"]).output() {
            if out.status.success() {
                let arch = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !arch.is_empty() {
                    dirs.push(format!("/usr/lib/{arch}"));
                    dirs.push(format!("/lib/{arch}"));
                }
            }
        }
        dirs.extend([
            "/usr/lib64".into(),
            "/lib64".into(),
            "/usr/lib".into(),
            "/lib".into(),
        ]);
        for dir in dirs {
            for name in &names {
                let path = Path::new(&dir).join(name);
                if path.exists() {
                    return Some(path);
                }
            }
        }
        None
    }

    /// Parse `ldconfig -p` for the first existing path of `names`.
    fn from_ldconfig(names: &[String]) -> Option<PathBuf> {
        let out = Command::new("ldconfig").arg("-p").output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        for name in names {
            for line in text.lines() {
                let line = line.trim();
                let Some(rest) = line.strip_prefix(name.as_str()) else {
                    continue;
                };
                if !rest.starts_with(' ') && !rest.starts_with('\t') {
                    continue;
                }
                let Some(path) = line.split("=>").nth(1) else {
                    continue;
                };
                let path = PathBuf::from(path.trim());
                if path.exists() {
                    return Some(path);
                }
            }
        }
        None
    }
}
