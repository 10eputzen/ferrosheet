//! Self-update from the GitHub releases, done the way browsers do it:
//!
//! 1. At start a background thread asks GitHub for the latest release tag (the only
//!    thing sent). If it is newer, the same thread downloads the release right away and
//!    leaves the new executable next to the running one as `ferrosheet.new(.exe)`.
//! 2. Only then the status bar offers "restart". The click swaps the files (two renames,
//!    instant) and starts the new version with the same window, file and sheet.
//! 3. Without the click the next start swaps the files before opening the window.
//!
//! Uses the `curl` and `tar` that ship with Windows 11 and every Linux desktop, so no
//! HTTP or archive code is linked in. `FERROSHEET_NO_UPDATE` turns the check off; debug
//! builds never check.

use std::ffi::OsString;
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

/// The command-line flag that carries [`Restore`] to the restarted process.
const RESTORE_FLAG: &str = "--restore=";

/// Window and sheet to bring back after an update restart.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Restore {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub maximized: bool,
    pub sheet: usize,
}

impl Restore {
    fn to_arg(self) -> String {
        let [x, y] = self.pos;
        let [w, h] = self.size;
        let m = u8::from(self.maximized);
        format!("{RESTORE_FLAG}{x},{y},{w},{h},{m},{}", self.sheet)
    }

    fn parse(arg: &str) -> Option<Self> {
        let mut v = arg.strip_prefix(RESTORE_FLAG)?.split(',');
        let mut f = || v.next()?.parse::<f32>().ok();
        let (x, y, w, h, m, sheet) = (f()?, f()?, f()?, f()?, f()?, f()?);
        Some(Restore {
            pos: [x, y],
            size: [w, h],
            maximized: m != 0.0,
            sheet: sheet as usize,
        })
    }
}

/// Splits the command line into the restore state (if this is an update restart) and
/// the remaining arguments.
pub fn take_restore(mut args: Vec<OsString>) -> (Option<Restore>, Vec<OsString>) {
    let restore = args
        .first()
        .and_then(|a| a.to_str())
        .and_then(Restore::parse);
    if restore.is_some() {
        args.remove(0);
    }
    (restore, args)
}

/// Starts the check and download in the background; the receiver yields the version
/// once it is downloaded and ready to swap in.
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
        let Some(tag) = tag_name(&json) else { return };
        if !is_newer(tag, env!("CARGO_PKG_VERSION")) {
            return;
        }
        let version = tag.trim_start_matches('v');
        // A failed download stays silent: the next start simply tries again.
        if download(version).is_ok() {
            let _ = tx.send(version.to_owned());
            ctx.request_repaint();
        }
    });
    Some(rx)
}

/// Swaps in a downloaded update and starts it with `args`. The caller then closes.
pub fn restart(args: &[OsString], restore: Option<Restore>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let pending = pending_path(&exe);
    if pending.exists() {
        swap(&exe, &pending).map_err(|e| e.to_string())?;
    }
    let mut c = Command::new(exe);
    if let Some(r) = restore {
        c.arg(r.to_arg());
    }
    c.args(args).spawn().map(drop).map_err(|e| e.to_string())
}

/// At start: if an update is waiting, swap it in and start it instead. Returns true
/// when the new version was started and this process should end.
pub fn apply_pending(args: &[OsString]) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let pending = pending_path(&exe);
    if !pending.exists() {
        return false;
    }
    if swap(&exe, &pending).is_err() {
        // Do not try again on every start; the next check downloads it anew.
        let _ = std::fs::remove_file(&pending);
        return false;
    }
    Command::new(exe).args(args).spawn().is_ok()
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

/// Downloads `version` and leaves its executable next to the running one.
fn download(version: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    // Staged next to the exe, not in the shared temp dir: that folder has to be
    // writable for the swap anyway, and other users cannot plant files in it.
    let parent = exe.parent().ok_or("no folder for the executable")?;
    let dir = parent.join(format!(".ferrosheet-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).map_err(|e| e.to_string())?;
    let result = download_into(version, &dir, &pending_path(&exe));
    let _ = std::fs::remove_dir_all(&dir);
    result
}

fn download_into(version: &str, dir: &Path, pending: &Path) -> Result<(), String> {
    let archive = dir.join(format!("{ASSET}.{ARCHIVE}"));
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/{ASSET}.{ARCHIVE}");
    run(cmd("curl")
        .args(["-fsSL", "--max-time", "300", "-o"])
        .arg(&archive)
        .arg(&url))?;
    run(cmd("tar").arg("-xf").arg(&archive).arg("-C").arg(dir))?;
    // Moved in one step, so a half-finished download never looks like an update.
    std::fs::rename(dir.join(ASSET).join(BIN), pending).map_err(|e| e.to_string())
}

/// A running executable cannot be overwritten on Windows, but it can be renamed:
/// move it aside, move the new one in, and undo the first rename if the second fails.
fn swap(exe: &Path, new: &Path) -> std::io::Result<()> {
    if !new.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} missing", new.display()),
        ));
    }
    let old = old_path(exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old)?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(e);
    }
    Ok(())
}

fn old_path(exe: &Path) -> PathBuf {
    sibling(exe, "old")
}

fn pending_path(exe: &Path) -> PathBuf {
    sibling(exe, "new")
}

/// `ferrosheet.exe` → `ferrosheet.<tag>.exe`, `ferrosheet` → `ferrosheet.<tag>`.
fn sibling(exe: &Path, tag: &str) -> PathBuf {
    let stem = exe.file_stem().unwrap_or_default().to_string_lossy();
    let name = match exe.extension() {
        Some(ext) => format!("{stem}.{tag}.{}", ext.to_string_lossy()),
        None => format!("{stem}.{tag}"),
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
    let mut c = Command::new(
        Path::new(&root)
            .join("System32")
            .join(format!("{program}.exe")),
    );
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
        let new = pending_path(&exe);
        std::fs::write(&exe, "old").unwrap();
        std::fs::write(&new, "new").unwrap();

        swap(&exe, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old_path(&exe)).unwrap(), "old");
        assert!(!new.exists());

        // nothing downloaded: everything stays as it was
        assert!(swap(&exe, &new).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restore_state_survives_the_command_line() {
        let r = Restore {
            pos: [-1912.0, 34.5],
            size: [1200.0, 800.0],
            maximized: true,
            sheet: 2,
        };
        let args = vec![OsString::from(r.to_arg()), OsString::from("C:/a b/x.xlsx")];
        let (back, rest) = take_restore(args);
        assert_eq!(back, Some(r));
        assert_eq!(rest, [OsString::from("C:/a b/x.xlsx")]);

        let plain = vec![OsString::from("x.xlsx")];
        assert_eq!(take_restore(plain.clone()), (None, plain));
    }
}
