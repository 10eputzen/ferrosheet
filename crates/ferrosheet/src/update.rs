//! Self-update from the GitHub releases. At start a background thread asks GitHub for
//! the latest release tag (the only thing sent is that request). If it is newer, the
//! status bar offers the update; one click downloads the release archive, swaps the
//! running executable and restarts it. Uses the `curl` and `tar` that ship with
//! Windows 11 and every Linux desktop, so no HTTP or archive code is linked in.
//! `FERROSHEET_NO_UPDATE` turns the check off; debug builds never check.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver};

const REPO: &str = "10eputzen/ferrosheet";
#[cfg(windows)]
const ASSET: &str = "ferrosheet-windows-x86_64";
#[cfg(windows)]
const ARCHIVE: &str = "zip";
#[cfg(windows)]
const BIN: &str = "ferrosheet.exe";
#[cfg(not(windows))]
const ASSET: &str = "ferrosheet-linux-x86_64";
#[cfg(not(windows))]
const ARCHIVE: &str = "tar.gz";
#[cfg(not(windows))]
const BIN: &str = "ferrosheet";

/// Starts the check in the background; the receiver yields the newer version, if any.
pub fn check(ctx: eframe::egui::Context) -> Option<Receiver<String>> {
    if cfg!(debug_assertions) || std::env::var_os("FERROSHEET_NO_UPDATE").is_some() {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let Ok(out) = cmd("curl")
            .args(["-fsSL", "--max-time", "10", &url])
            .output()
        else {
            return;
        };
        let json = String::from_utf8_lossy(&out.stdout);
        if let Some(tag) = tag_name(&json)
            && is_newer(tag, env!("CARGO_PKG_VERSION"))
        {
            let _ = tx.send(tag.trim_start_matches('v').to_owned());
            ctx.request_repaint();
        }
    });
    Some(rx)
}

/// Downloads `version` and puts it in place of the running executable.
pub fn install(version: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir().join(format!("ferrosheet-update-{version}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let archive = dir.join(format!("{ASSET}.{ARCHIVE}"));
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/{ASSET}.{ARCHIVE}");
    run(cmd("curl")
        .args(["-fsSL", "--max-time", "300", "-o"])
        .arg(&archive)
        .arg(&url))?;
    run(cmd("tar").arg("-xf").arg(&archive).arg("-C").arg(&dir))?;
    swap(&exe, &dir.join(ASSET).join(BIN)).map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// Starts the (new) executable with the file that is open now.
pub fn restart(file: Option<&Path>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut c = Command::new(exe);
    if let Some(f) = file {
        c.arg(f);
    }
    c.spawn().map(drop).map_err(|e| e.to_string())
}

/// Removes the executable a previous update left behind. On Windows the old process
/// may still be closing, so this retries for a few seconds in the background.
pub fn clean_up() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let old = old_path(&exe);
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

/// A running executable cannot be overwritten on Windows, but it can be renamed:
/// move it aside, copy the new one in, and undo the rename if the copy fails.
fn swap(exe: &Path, new: &Path) -> std::io::Result<()> {
    if !new.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} missing in the release", new.display()),
        ));
    }
    let old = old_path(exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old)?;
    if let Err(e) = std::fs::copy(new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(e);
    }
    Ok(())
}

fn old_path(exe: &Path) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    let name = match exe.extension() {
        Some(ext) => format!("{stem}.old.{}", ext.to_string_lossy()),
        None => format!("{stem}.old"),
    };
    exe.with_file_name(name)
}

fn tag_name(json: &str) -> Option<&str> {
    let rest = &json[json.find("\"tag_name\"")? + "\"tag_name\"".len()..];
    let rest = rest
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    Some(&rest[..rest.find('"')?])
}

fn version(s: &str) -> Option<(u32, u32, u32)> {
    let mut parts = s.trim_start_matches('v').split('.');
    let mut next = || parts.next()?.parse().ok();
    Some((next()?, next()?, next()?))
}

fn is_newer(latest: &str, current: &str) -> bool {
    matches!((version(latest), version(current)), (Some(l), Some(c)) if l > c)
}

#[cfg(not(windows))]
fn cmd(program: &str) -> Command {
    Command::new(program)
}

/// The curl and tar in System32 (tar there also unpacks zip, unlike other tars that
/// may come first on the PATH), started without a console window.
#[cfg(windows)]
fn cmd(program: &str) -> Command {
    use std::os::windows::process::CommandExt;
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    let mut c = Command::new(Path::new(&root).join("System32").join(format!("{program}.exe")));
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
}

fn run(c: &mut Command) -> Result<(), String> {
    let out = c.output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_tag_from_the_release_json() {
        let json = r#"{"url":"x","tag_name": "v0.2.0","name":"v0.2.0"}"#;
        assert_eq!(tag_name(json), Some("v0.2.0"));
        assert_eq!(tag_name(r#"{"message":"Not Found"}"#), None);
    }

    #[test]
    fn compares_versions_as_numbers() {
        assert!(is_newer("v0.1.10", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.1.1", "0.1.1"));
        assert!(!is_newer("v0.1.0", "0.1.1"));
        assert!(!is_newer("nightly", "0.1.1"));
    }

    #[test]
    fn swaps_the_executable_and_keeps_the_old_one_aside() {
        let dir = std::env::temp_dir().join("ferrosheet-swap-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("ferrosheet.exe");
        let new = dir.join("new.exe");
        std::fs::write(&exe, "old").unwrap();
        std::fs::write(&new, "new").unwrap();

        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        let old = dir.join("ferrosheet.old.exe");
        assert_eq!(std::fs::read_to_string(&old).unwrap(), "old");

        // a release without the binary leaves everything as it was
        assert!(swap(&exe, &dir.join("missing")).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
