//! Install and uninstall helpers.
//!
//! `src/main.rs` calls `install_default` and `uninstall_default` for the
//! `install` and `uninstall` commands. `src/bin/anvil-setup.rs` calls the same
//! functions, plus `pack_sfx_installer` for `--pack`.
//!
//! The self-extracting trailer is zlib from `anvil-compress`: setup stub, compressed
//! browser bytes, a little-endian length, then `ANVILSFX`. Icons written into the
//! install directory come from `logo`. This module does not download files and it
//! does not show message boxes. Those prompts live in `anvil-setup`.
//!
//! Windows copies `anvil.exe` under `LOCALAPPDATA\Programs\Anvil Browser` (or
//! `USERPROFILE\AppData\Local\Programs\Anvil Browser`), writes Start Menu and
//! Desktop shortcuts named `Anvil Browser.lnk`, and adds
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AnvilBrowser`.
//! Linux copies `anvil` under `HOME/.local/share/anvil-browser`, copies it again to
//! `HOME/.local/bin/anvil`, and writes `HOME/.local/share/applications/anvil-browser.desktop`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anvil_compress::{compress_zlib, decompress_zlib_bounded};

use crate::VERSION;
use crate::logo::{encode_anvil_ico, encode_anvil_png};

/// Magic 8-byte trailer footer identifying a self-extracting Anvil Setup binary.
pub const SFX_MAGIC: &[u8; 8] = b"ANVILSFX";

/// Maximum decompressed size of an embedded `anvil.exe` binary payload (32 MiB).
pub const MAX_SFX_BINARY_BYTES: usize = 32 * 1024 * 1024;

/// Sample page copied into `examples/hello.html` in the install directory.
const EXAMPLE_HELLO_HTML: &str = include_str!("../../../examples/hello.html");
/// Sample page copied into `examples/second.html` in the install directory.
const EXAMPLE_SECOND_HTML: &str = include_str!("../../../examples/second.html");
/// Sample stylesheet copied into `examples/theme.css` in the install directory.
const EXAMPLE_THEME_CSS: &str = include_str!("../../../examples/theme.css");

/// Combine a compiled `anvil-setup` stub executable and a compiled `anvil` browser executable
/// into a single self-extracting installer binary (`[setup_exe][zlib(anvil_exe)][u64_len]["ANVILSFX"]`).
pub fn pack_sfx_installer(setup_exe: &[u8], browser_exe: &[u8]) -> Vec<u8> {
    let compressed = compress_zlib(browser_exe);
    let payload_len = (compressed.len() as u64).to_le_bytes();
    let mut out = Vec::with_capacity(setup_exe.len() + compressed.len() + 16);
    out.extend_from_slice(setup_exe);
    out.extend_from_slice(&compressed);
    out.extend_from_slice(&payload_len);
    out.extend_from_slice(SFX_MAGIC);
    out
}

/// Inspect an installer binary's tail for a `ANVILSFX` trailer and decompress the embedded `anvil` binary.
pub fn extract_sfx_payload(installer_bytes: &[u8]) -> Result<Option<Vec<u8>>, String> {
    if installer_bytes.len() < 16 {
        return Ok(None);
    }
    let tail_start = installer_bytes.len() - 8;
    if &installer_bytes[tail_start..] != SFX_MAGIC {
        return Ok(None);
    }
    let len_bytes: [u8; 8] = installer_bytes[tail_start - 8..tail_start]
        .try_into()
        .map_err(|_| "invalid SFX length trailer".to_string())?;
    let payload_len = u64::from_le_bytes(len_bytes) as usize;
    if payload_len == 0 || payload_len + 16 > installer_bytes.len() {
        return Err("corrupted SFX payload length".to_string());
    }
    let payload_start = installer_bytes.len() - 16 - payload_len;
    let compressed = &installer_bytes[payload_start..payload_start + payload_len];
    let decompressed = decompress_zlib_bounded(compressed, MAX_SFX_BINARY_BYTES)
        .map_err(|e| format!("could not decompress embedded browser binary: {e}"))?;
    Ok(Some(decompressed))
}

/// Return the setup stub slice without any appended `ANVILSFX` trailer.
pub fn strip_sfx_trailer(installer_bytes: &[u8]) -> &[u8] {
    if installer_bytes.len() >= 16 {
        let tail_start = installer_bytes.len() - 8;
        if &installer_bytes[tail_start..] == SFX_MAGIC {
            if let Ok(len_bytes) = installer_bytes[tail_start - 8..tail_start].try_into() {
                let payload_len = u64::from_le_bytes(len_bytes) as usize;
                if payload_len > 0 && payload_len + 16 <= installer_bytes.len() {
                    return &installer_bytes[..installer_bytes.len() - 16 - payload_len];
                }
            }
        }
    }
    installer_bytes
}

/// Paths and flags recorded by one call to `install_to_dir`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallReport {
    /// Directory that received the browser binary.
    pub install_dir: PathBuf,
    /// Path of the installed `anvil` or `anvil.exe`.
    pub installed_binary: PathBuf,
    /// Path of `Installer/setup.exe` or `Installer/anvil-setup`.
    pub uninstaller_binary: PathBuf,
    /// Shortcut or desktop-file paths that were created. Empty when integration is off or failed.
    pub shortcut_paths: Vec<PathBuf>,
    /// True when the Windows registry key or the Linux desktop file was written.
    pub registered_os_entry: bool,
}

/// Return the default per-user installation directory for the current OS.
pub fn default_install_dir() -> Result<PathBuf, String> {
    if cfg!(windows) {
        if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(local_app_data)
                .join("Programs")
                .join("Anvil Browser"));
        }
        if let Some(profile) = env::var_os("USERPROFILE") {
            return Ok(PathBuf::from(profile)
                .join("AppData")
                .join("Local")
                .join("Programs")
                .join("Anvil Browser"));
        }
        Err("could not locate LOCALAPPDATA or USERPROFILE environment variable".to_string())
    } else {
        let home = env::var_os("HOME").ok_or("could not locate HOME environment variable")?;
        Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("anvil-browser"))
    }
}

/// Install Anvil Browser into the default OS per-user directory and register shortcuts/uninstaller.
pub fn install_default() -> Result<InstallReport, String> {
    let dir = default_install_dir()?;
    install_to_dir(&dir, true)
}

/// Uninstall Anvil Browser from the default OS per-user directory and remove shortcuts/uninstaller.
pub fn uninstall_default() -> Result<PathBuf, String> {
    let dir = default_install_dir()?;
    uninstall_from_dir(&dir, true)?;
    Ok(dir)
}

/// Install the current binary into `install_dir`.
///
/// Writes `anvil-logo.ico`, `anvil-logo.png`, and the three embedded example files.
/// If a local `examples` directory exists, its top-level files are copied over those.
/// When `integrate_os` is true, Windows shortcuts and the uninstall registry key are
/// created, or the Linux desktop entry and `HOME/.local/bin/anvil` copy are created.
pub fn install_to_dir(install_dir: &Path, integrate_os: bool) -> Result<InstallReport, String> {
    fs::create_dir_all(install_dir)
        .map_err(|e| format!("could not create {}: {e}", install_dir.display()))?;

    let current_exe =
        env::current_exe().map_err(|e| format!("could not locate running executable: {e}"))?;
    let bin_name = if cfg!(windows) { "anvil.exe" } else { "anvil" };
    let setup_sub_name = if cfg!(windows) {
        "setup.exe"
    } else {
        "anvil-setup"
    };
    let installer_subdir = install_dir.join("Installer");
    let _ = fs::create_dir_all(&installer_subdir);
    let installed_binary = install_dir.join(bin_name);
    let uninstaller_binary = installer_subdir.join(setup_sub_name);

    // If this executable has a ANVILSFX trailer, write the decompressed browser
    // as the installed binary and the stripped stub as Installer/setup.exe.
    let mut wrote_embedded = false;
    if let Ok(self_bytes) = fs::read(&current_exe) {
        if let Some(browser_bytes) = extract_sfx_payload(&self_bytes)? {
            fs::write(&installed_binary, &browser_bytes).map_err(|e| {
                format!(
                    "could not write extracted browser binary to {}: {e}",
                    installed_binary.display()
                )
            })?;
            let stub_bytes = strip_sfx_trailer(&self_bytes);
            let _ = fs::write(&uninstaller_binary, stub_bytes);
            wrote_embedded = true;
        }
    }

    // No trailer: copy a sibling browser binary, or this executable if none exists.
    // The uninstaller copy prefers a sibling `anvil-setup` binary, else this executable.
    if !wrote_embedded {
        let sibling_anvil = current_exe.parent().map(|p| p.join(bin_name));
        let source_bin = match sibling_anvil {
            Some(ref candidate) if candidate.exists() => candidate.clone(),
            _ => current_exe.clone(),
        };
        if source_bin != installed_binary {
            fs::copy(&source_bin, &installed_binary).map_err(|e| {
                format!(
                    "could not copy binary to {}: {e}",
                    installed_binary.display()
                )
            })?;
        }
        let setup_bin_name = if cfg!(windows) {
            "anvil-setup.exe"
        } else {
            "anvil-setup"
        };
        let sibling_setup = current_exe.parent().map(|p| p.join(setup_bin_name));
        let uninstaller_src = match sibling_setup {
            Some(ref candidate) if candidate.exists() => candidate.clone(),
            _ => current_exe.clone(),
        };
        if uninstaller_src != uninstaller_binary {
            let _ = fs::copy(&uninstaller_src, &uninstaller_binary);
        }
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&installed_binary, fs::Permissions::from_mode(0o755));
        let _ = fs::set_permissions(&uninstaller_binary, fs::Permissions::from_mode(0o755));
    }

    // Remove any legacy standalone uninstaller from older releases
    let _ = fs::remove_file(install_dir.join("AnvilBrowser-Uninstall.exe"));

    // Write bundled brand icon (`anvil-logo.ico` and `anvil-logo.png`) and `examples/` directory
    let ico_path = install_dir.join("anvil-logo.ico");
    let png_path = install_dir.join("anvil-logo.png");
    let _ = fs::write(&ico_path, encode_anvil_ico());
    let _ = fs::write(&png_path, encode_anvil_png());

    let examples_dst = install_dir.join("examples");
    if fs::create_dir_all(&examples_dst).is_ok() {
        let _ = fs::write(examples_dst.join("hello.html"), EXAMPLE_HELLO_HTML);
        let _ = fs::write(examples_dst.join("second.html"), EXAMPLE_SECOND_HTML);
        let _ = fs::write(examples_dst.join("theme.css"), EXAMPLE_THEME_CSS);
    }
    let examples_src = Path::new("examples");
    if examples_src.is_dir() {
        let _ = copy_dir_flat(examples_src, &examples_dst);
    }

    let mut shortcut_paths = Vec::new();
    let mut registered_os_entry = false;

    if integrate_os {
        if cfg!(windows) {
            if let Ok(paths) = create_windows_shortcuts(&installed_binary, &ico_path, install_dir) {
                shortcut_paths.extend(paths);
            }
            registered_os_entry =
                register_windows_uninstaller(&uninstaller_binary, &ico_path, install_dir).is_ok();
        } else if let Ok(paths) = install_linux_desktop_integration(&installed_binary, &png_path) {
            shortcut_paths.extend(paths);
            registered_os_entry = true;
        }
    }

    Ok(InstallReport {
        install_dir: install_dir.to_path_buf(),
        installed_binary,
        uninstaller_binary,
        shortcut_paths,
        registered_os_entry,
    })
}

/// Remove Anvil Browser files from `install_dir` and optionally clean up OS shortcuts and registry entries.
pub fn uninstall_from_dir(install_dir: &Path, integrate_os: bool) -> Result<(), String> {
    if integrate_os {
        if cfg!(windows) {
            remove_windows_shortcuts();
            let _ = unregister_windows_uninstaller();
        } else {
            remove_linux_desktop_integration();
        }
    }

    if install_dir.exists() {
        if let Ok(entries) = fs::read_dir(install_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                } else {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        let _ = fs::remove_dir_all(install_dir);
    }
    Ok(())
}

/// Copy files that sit directly in `src` onto `dst`. Nested directories are skipped.
fn copy_dir_flat(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name() {
                let _ = fs::copy(&path, dst.join(name));
            }
        }
    }
    Ok(())
}

/// Create `Anvil Browser.lnk` on the Desktop and in the Start Menu Programs folder.
///
/// The script runs `powershell` with `WScript.Shell`. Failures return an empty list.
fn create_windows_shortcuts(
    exe_path: &Path,
    ico_path: &Path,
    work_dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let mut created = Vec::new();
    let ico_loc = format!("{},0", ico_path.to_string_lossy());
    let desc = format!("Anvil Browser {VERSION}");

    let script = format!(
        "$ws = New-Object -ComObject WScript.Shell; \
         $desk = [Environment]::GetFolderPath('Desktop', 'DoNotVerify'); \
         $progs = [Environment]::GetFolderPath('Programs', 'DoNotVerify'); \
         $folders = @($desk, $progs); \
         foreach ($dir in $folders) {{ \
             if ($dir) {{ \
                 if (-not (Test-Path $dir)) {{ New-Item -ItemType Directory -Force -Path $dir | Out-Null }}; \
                 $lnk = Join-Path $dir 'Anvil Browser.lnk'; \
                 $s = $ws.CreateShortcut($lnk); \
                 $s.TargetPath = '{}'; \
                 $s.WorkingDirectory = '{}'; \
                 $s.IconLocation = '{}'; \
                 $s.Description = '{}'; \
                 $s.Save(); \
                 Write-Output $lnk \
             }} \
         }}",
        escape_ps(&exe_path.to_string_lossy()),
        escape_ps(&work_dir.to_string_lossy()),
        escape_ps(&ico_loc),
        escape_ps(&desc),
    );

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-Command", &script]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    if let Ok(output) = cmd.output() {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    created.push(PathBuf::from(trimmed));
                }
            }
        }
    }

    Ok(created)
}

/// Delete the Start Menu and Desktop shortcut files this installer creates.
///
/// Also deletes a legacy `Uninstall Anvil Browser.lnk` if that file is present.
/// Missing files are ignored.
fn remove_windows_shortcuts() {
    if let Some(appdata) = env::var_os("APPDATA") {
        let start_programs = PathBuf::from(appdata)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");
        let _ = fs::remove_file(start_programs.join("Anvil Browser.lnk"));
        let _ = fs::remove_file(start_programs.join("Uninstall Anvil Browser.lnk"));
    }
    if let Some(profile) = env::var_os("USERPROFILE") {
        let profile_dir = PathBuf::from(profile);
        let _ = fs::remove_file(profile_dir.join("Desktop").join("Anvil Browser.lnk"));
        let _ = fs::remove_file(
            profile_dir
                .join("OneDrive")
                .join("Desktop")
                .join("Anvil Browser.lnk"),
        );
    }
}

/// Write the per-user uninstall registry values with `reg add`.
///
/// The quiet uninstall command is the uninstaller path plus `--uninstall --silent`.
fn register_windows_uninstaller(
    uninstaller_path: &Path,
    ico_path: &Path,
    install_dir: &Path,
) -> Result<(), String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AnvilBrowser";
    let ico_str = ico_path.to_string_lossy();
    let uninst_str = uninstaller_path.to_string_lossy();
    let dir_str = install_dir.to_string_lossy();
    let uninstall_cmd = format!("\"{uninst_str}\" --uninstall");
    let quiet_uninstall_cmd = format!("\"{uninst_str}\" --uninstall --silent");

    let entries: &[(&str, &str, &str)] = &[
        ("DisplayName", "REG_SZ", "Anvil Browser"),
        ("DisplayVersion", "REG_SZ", VERSION),
        ("Publisher", "REG_SZ", "Buntos"),
        ("InstallLocation", "REG_SZ", &dir_str),
        ("DisplayIcon", "REG_SZ", &ico_str),
        ("UninstallString", "REG_SZ", &uninstall_cmd),
        ("QuietUninstallString", "REG_SZ", &quiet_uninstall_cmd),
        ("NoModify", "REG_DWORD", "1"),
        ("NoRepair", "REG_DWORD", "1"),
    ];

    for &(name, kind, data) in entries {
        let mut cmd = Command::new("reg");
        cmd.args(["add", key, "/v", name, "/t", kind, "/d", data, "/f"]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        let status = cmd.status().map_err(|e| e.to_string())?;
        if !status.success() {
            return Err("could not write Windows uninstall registry key".to_string());
        }
    }
    Ok(())
}

/// Run `reg delete` on the uninstall key. A failed delete is ignored.
fn unregister_windows_uninstaller() -> Result<(), String> {
    let key = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\AnvilBrowser";
    let mut cmd = Command::new("reg");
    cmd.args(["delete", key, "/f"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd.status();
    Ok(())
}

/// Copy the binary to `HOME/.local/bin/anvil`, copy the PNG into the hicolor icon
/// theme, and write `anvil-browser.desktop`.
fn install_linux_desktop_integration(
    exe_path: &Path,
    png_path: &Path,
) -> Result<Vec<PathBuf>, String> {
    let home = env::var_os("HOME").ok_or("missing HOME")?;
    let home_dir = PathBuf::from(home);
    let mut created = Vec::new();

    let local_bin = home_dir.join(".local").join("bin");
    if fs::create_dir_all(&local_bin).is_ok() {
        let bin_link = local_bin.join("anvil");
        if fs::copy(exe_path, &bin_link).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = fs::set_permissions(&bin_link, fs::Permissions::from_mode(0o755));
            }
            created.push(bin_link);
        }
    }

    let icons_dir = home_dir
        .join(".local")
        .join("share")
        .join("icons")
        .join("hicolor")
        .join("64x64")
        .join("apps");
    if fs::create_dir_all(&icons_dir).is_ok() {
        let _ = fs::copy(png_path, icons_dir.join("anvil-browser.png"));
    }

    let apps_dir = home_dir.join(".local").join("share").join("applications");
    if fs::create_dir_all(&apps_dir).is_ok() {
        let desktop_file = apps_dir.join("anvil-browser.desktop");
        let contents = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Anvil Browser\n\
             Comment=Proprietary Web Browser ({VERSION})\n\
             Exec=\"{}\" %u\n\
             Icon={}\n\
             Terminal=false\n\
             Categories=Network;WebBrowser;\n\
             MimeType=text/html;x-scheme-handler/http;x-scheme-handler/https;\n",
            exe_path.display(),
            png_path.display()
        );
        if fs::write(&desktop_file, contents).is_ok() {
            created.push(desktop_file);
        }
    }

    Ok(created)
}

/// Remove `HOME/.local/bin/anvil` and the desktop file. The copied icon is left in place.
fn remove_linux_desktop_integration() {
    if let Some(home) = env::var_os("HOME") {
        let home_dir = PathBuf::from(home);
        let _ = fs::remove_file(home_dir.join(".local").join("bin").join("anvil"));
        let _ = fs::remove_file(
            home_dir
                .join(".local")
                .join("share")
                .join("applications")
                .join("anvil-browser.desktop"),
        );
    }
}

/// Double single quotes so a path can sit inside a PowerShell single-quoted string.
fn escape_ps(input: &str) -> String {
    input.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    //! Installs into a temporary directory with OS integration off, then checks the SFX trailer.
    use super::*;

    #[test]
    /// Install into a temp directory with OS integration off, remove it, and round-trip an SFX trailer.
    fn stages_and_removes_portable_installation_directory() {
        let staging = env::temp_dir().join(format!("anvil_installer_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&staging);

        let report = install_to_dir(&staging, false).expect("install to staging directory");
        assert!(report.installed_binary.exists());
        assert!(report.uninstaller_binary.exists());
        assert!(staging.join("examples").join("hello.html").exists());
        assert_eq!(report.install_dir, staging);

        uninstall_from_dir(&staging, false).expect("uninstall from staging directory");
        assert!(!staging.exists());

        let fake_setup = b"MZ_SETUP_STUB";
        let fake_anvil = b"MZ_ANVIL_BROWSER_PAYLOAD_BINARY_DATA";
        let packed = pack_sfx_installer(fake_setup, fake_anvil);
        let extracted = extract_sfx_payload(&packed)
            .expect("valid SFX")
            .expect("has SFX payload");
        assert_eq!(extracted, fake_anvil);
        assert_eq!(strip_sfx_trailer(&packed), fake_setup);
    }
}
