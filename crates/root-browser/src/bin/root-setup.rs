#![cfg_attr(not(test), windows_subsystem = "windows")]
//! Setup and uninstall binary (`root-setup`).
//!
//! `main` reads argv and calls `installer::install_default`, `uninstall_default`,
//! or `pack_sfx_installer`. The pack path uses `root-compress` inside that
//! installer function. Windows message boxes come from `user32` in this file.
//!
//! Flags this binary actually accepts:
//! - no command: install
//! - `--silent`, `/S`, `-s`, `/quiet`: install or uninstall without message boxes
//! - `--uninstall`, `uninstall`, `/uninstall`: uninstall
//! - a file name containing `uninstall`: uninstall, even with no command flag
//! - `--pack <setup stub> <browser exe> <output exe>`: append a compressed trailer
//! - `--help`, `-h`, `help`: print the command list

use std::env;
use std::fs;
use std::process::{Command, ExitCode};

use root_browser::VERSION;
use root_browser::installer::{
    default_install_dir, install_default, pack_sfx_installer, uninstall_default,
};

/// Windows message-box flags and the `MessageBoxW` call used by the wizard prompts.
#[cfg(windows)]
mod win_dialog {
    use std::ffi::c_void;
    use std::ptr::null_mut;

    /// `MessageBoxW` style: one OK button.
    pub const MB_OK: u32 = 0x0000_0000;
    /// `MessageBoxW` style: OK and Cancel.
    pub const MB_OKCANCEL: u32 = 0x0000_0001;
    /// `MessageBoxW` style: Yes, No, and Cancel.
    pub const MB_YESNOCANCEL: u32 = 0x0000_0003;
    /// `MessageBoxW` style: Yes and No.
    pub const MB_YESNO: u32 = 0x0000_0004;
    /// `MessageBoxW` icon: error.
    pub const MB_ICONERROR: u32 = 0x0000_0010;
    /// `MessageBoxW` icon: question.
    pub const MB_ICONQUESTION: u32 = 0x0000_0020;
    /// `MessageBoxW` icon: information.
    pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
    /// `MessageBoxW` return: OK.
    pub const IDOK: i32 = 1;
    /// `MessageBoxW` return: Yes.
    pub const IDYES: i32 = 6;
    /// `MessageBoxW` return: No.
    pub const IDNO: i32 = 7;

    #[link(name = "user32")]
    unsafe extern "system" {
        /// Win32 message box. `message_box` is the only caller.
        fn MessageBoxW(
            h_wnd: *mut c_void,
            lp_text: *const u16,
            lp_caption: *const u16,
            u_type: u32,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// Attach this process to a console. `main` passes the parent-process id.
        pub fn AttachConsole(dw_process_id: u32) -> i32;
    }

    /// Show a UTF-16 message box and return the button id.
    pub fn message_box(title: &str, body: &str, flags: u32) -> i32 {
        let w_title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let w_body: Vec<u16> = body.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe { MessageBoxW(null_mut(), w_body.as_ptr(), w_title.as_ptr(), flags) }
    }
}

/// Dispatch pack, uninstall, help, or install. Silent flags skip the Windows prompts.
fn main() -> ExitCode {
    #[cfg(windows)]
    unsafe {
        let _ = win_dialog::AttachConsole(0xFFFF_FFFF);
    }
    let args: Vec<String> = env::args().skip(1).collect();
    let silent = args
        .iter()
        .any(|a| matches!(a.as_str(), "--silent" | "/S" | "-s" | "/quiet"));
    let invoked_as_uninstaller = env::current_exe()
        .ok()
        .and_then(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
        })
        .is_some_and(|name| name.contains("uninstall"));

    match args.first().map(String::as_str) {
        // Read two binaries and write `[stub][zlib(browser)][length][ROOTSFX1]`.
        Some("--pack") => {
            if args.len() != 4 {
                eprintln!("Usage: root-setup --pack <setup_stub.exe> <root.exe> <output.exe>");
                return ExitCode::FAILURE;
            }
            let setup_bytes = match fs::read(&args[1]) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Could not read {}: {e}", args[1]);
                    return ExitCode::FAILURE;
                }
            };
            let browser_bytes = match fs::read(&args[2]) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("Could not read {}: {e}", args[2]);
                    return ExitCode::FAILURE;
                }
            };
            let packed = pack_sfx_installer(&setup_bytes, &browser_bytes);
            if let Err(e) = fs::write(&args[3], &packed) {
                eprintln!("Could not write {}: {e}", args[3]);
                return ExitCode::FAILURE;
            }
            println!(
                "Packaged self-extracting installer: {} ({} bytes)",
                args[3],
                packed.len()
            );
            ExitCode::SUCCESS
        }
        Some("--uninstall" | "uninstall" | "/uninstall") => run_uninstall(silent),
        // Print the flag list and exit success.
        Some("--help" | "-h" | "help") => {
            println!("Root Browser Setup & Uninstaller {VERSION} (Author: Buntos)");
            println!("  root-setup              Interactive setup wizard (installs Root Browser)");
            println!("  root-setup --silent     Silent unattended installation");
            println!("  root-setup --uninstall  Remove Root Browser and shortcuts");
            println!("  root-setup --pack <s> <f> <o>  Bundle single-file SFX installer");
            ExitCode::SUCCESS
        }
        // The executable name itself contains "uninstall".
        _ if invoked_as_uninstaller => run_uninstall(silent),
        // Any other argv, including none, installs.
        _ => run_install(silent),
    }
}

/// Ask before uninstall on Windows unless `silent` is set, then call `uninstall_default`.
fn run_uninstall(silent: bool) -> ExitCode {
    #[cfg(windows)]
    if !silent {
        let target_dir = default_install_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "%LOCALAPPDATA%\\Programs\\Root Browser".to_string());
        let title = format!("Root Browser {VERSION} Uninstaller");
        let prompt = format!(
            "Are you sure you want to completely uninstall Root Browser {VERSION}?\n\n\
             This will remove:\n\
             • Application files in {target_dir}\n\
             • Desktop and Start Menu shortcuts\n\
             • Windows Installed Apps registry entry\n\n\
             Click Yes to uninstall, or No to cancel."
        );
        if win_dialog::message_box(
            &title,
            &prompt,
            win_dialog::MB_YESNO | win_dialog::MB_ICONQUESTION,
        ) != win_dialog::IDYES
        {
            return ExitCode::SUCCESS;
        }
    }

    match uninstall_default() {
        Ok(dir) => {
            println!(
                "Root Browser {VERSION} has been uninstalled from {}",
                dir.display()
            );
            #[cfg(windows)]
            if !silent {
                let title = format!("Root Browser {VERSION} Uninstaller");
                win_dialog::message_box(
                    &title,
                    &format!(
                        "Root Browser {VERSION} has been completely uninstalled from your computer."
                    ),
                    win_dialog::MB_OK | win_dialog::MB_ICONINFORMATION,
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Uninstall failed: {err}");
            #[cfg(windows)]
            if !silent {
                let title = format!("Root Browser {VERSION} Uninstaller Error");
                win_dialog::message_box(
                    &title,
                    &format!("Uninstall failed:\n{err}"),
                    win_dialog::MB_OK | win_dialog::MB_ICONERROR,
                );
            }
            ExitCode::FAILURE
        }
    }
}

/// Ask before install on Windows unless `silent` is set, then call `install_default`.
///
/// If `root.exe` is already in the default directory, Yes reinstalls, No uninstalls,
/// and Cancel returns success without changing files. After a successful install the
/// Yes button on the last prompt spawns the installed binary. Silent mode skips both prompts.
fn run_install(silent: bool) -> ExitCode {
    #[cfg(windows)]
    if !silent {
        let install_dir = default_install_dir().ok();
        let already_installed = install_dir
            .as_ref()
            .is_some_and(|dir| dir.join("root.exe").exists());
        let target_dir = install_dir
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "%LOCALAPPDATA%\\Programs\\Root Browser".to_string());
        let title = format!("Root Browser {VERSION} Setup");
        if already_installed {
            let prompt = format!(
                "Root Browser is already installed in:\n{target_dir}\n\n\
                 • Click Yes to Update / Reinstall Root Browser {VERSION}\n\
                 • Click No to Uninstall Root Browser\n\
                 • Click Cancel to exit Setup"
            );
            let choice = win_dialog::message_box(
                &title,
                &prompt,
                win_dialog::MB_YESNOCANCEL | win_dialog::MB_ICONQUESTION,
            );
            if choice == win_dialog::IDNO {
                return run_uninstall(false);
            }
            if choice != win_dialog::IDYES {
                return ExitCode::SUCCESS;
            }
        } else {
            let prompt = format!(
                "Welcome to the Root Browser {VERSION} Setup Wizard (by Buntos).\n\n\
                 This will install Root Browser {VERSION} on your computer:\n\
                 • Destination: {target_dir}\n\
                 • Desktop & Start Menu Shortcuts\n\
                 • Windows Installed Apps / Control Panel Uninstaller Entry\n\n\
                 Click OK to install, or Cancel to exit."
            );
            if win_dialog::message_box(
                &title,
                &prompt,
                win_dialog::MB_OKCANCEL | win_dialog::MB_ICONINFORMATION,
            ) != win_dialog::IDOK
            {
                return ExitCode::SUCCESS;
            }
        }
    }

    match install_default() {
        Ok(report) => {
            println!("Root Browser {VERSION} installed successfully!");
            println!("  Location : {}", report.installed_binary.display());
            for sc in &report.shortcut_paths {
                println!("  Shortcut : {}", sc.display());
            }
            if report.registered_os_entry {
                println!("  OS Entry : Registered per-user application entry");
            }

            #[cfg(windows)]
            if !silent {
                let title = format!("Root Browser {VERSION} Setup");
                let done_msg = format!(
                    "Root Browser {VERSION} has been installed successfully!\n\n\
                     Installed executable:\n{}\n\n\
                     Desktop and Start Menu shortcuts have been created.\n\n\
                     Would you like to launch Root Browser now?",
                    report.installed_binary.display()
                );
                if win_dialog::message_box(
                    &title,
                    &done_msg,
                    win_dialog::MB_YESNO | win_dialog::MB_ICONQUESTION,
                ) == win_dialog::IDYES
                {
                    let _ = Command::new(&report.installed_binary)
                        .current_dir(&report.install_dir)
                        .spawn();
                }
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("Installation failed: {err}");
            #[cfg(windows)]
            if !silent {
                let title = format!("Root Browser {VERSION} Setup Error");
                win_dialog::message_box(
                    &title,
                    &format!("Installation failed:\n{err}"),
                    win_dialog::MB_OK | win_dialog::MB_ICONERROR,
                );
            }
            ExitCode::FAILURE
        }
    }
}
