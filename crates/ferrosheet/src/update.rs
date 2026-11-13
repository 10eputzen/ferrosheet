//! Self-update from the GitHub releases, done the way browsers do it:
//!
//! 1. At start a background thread asks GitHub for the latest release tag (the only
//!    thing sent). If it is newer, the same thread downloads the release right away and
//!    leaves the new executable next to the running one as `ferrosheet.new(.exe)`.
//! 2. Only then the status bar offers "restart". The click swaps the files (two renames,
//!    instant) and starts the new version with the same window, file and sheet.
//! 3. Without the click the next start swaps the files before opening the window.
//!
//! Every archive is signed in CI (ed25519, see `release.yml`); the public key is compiled
//! in and anything without a valid signature is thrown away before it is unpacked. The
//! signed message ends with the tag and the asset name, so an older signed build cannot
//! be passed off as a newer release.
//!
//! The previous executable stays as `ferrosheet.old(.exe)` while the new one starts. If
//! the new one exits with an error within [`TRIAL`], the old one is put back, started
//! again, and that version is skipped from then on.
//!
//! Uses the `curl` and `tar` that ship with Windows 11 and every Linux desktop, so no
//! HTTP or archive code is linked in. `FERROSHEET_NO_UPDATE` turns the check off; debug
//! builds never check.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

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

/// Public half of the release signing key (raw ed25519). The private half is the
/// repository secret `UPDATE_SIGNING_KEY` and never leaves CI.
const PUBLIC_KEY: [u8; 32] = [0; 32];

/// How long a freshly swapped-in version has to prove it starts.
const TRIAL: Duration = Duration::from_secs(15);

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
/// once it is downloaded and ready to swap in, or why there is none.
pub fn check(ctx: eframe::egui::Context) -> Option<Receiver<Found>> {
    if cfg!(debug_assertions) || std::env::var_os("FERROSHEET_NO_UPDATE").is_some() {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(look_for_update());
        ctx.request_repaint();
    });
    Some(rx)
}

/// What a check found. The automatic check at start only acts on `Ready`; the others
/// are shown when the user asked.
pub enum Found {
    Ready(String),
    Current,
    Failed(String),
}

fn look_for_update() -> Found {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let out = match cmd("curl")
        .args(["-fsSL", "--max-time", "10", &url])
        .output()
    {
        Ok(out) if out.status.success() => out,
        Ok(out) => return Found::Failed(String::from_utf8_lossy(&out.stderr).trim().into()),
        Err(e) => return Found::Failed(e.to_string()),
    };
    let json = String::from_utf8_lossy(&out.stdout);
    let Some(tag) = tag_name(&json) else {
        return Found::Failed("no release found".into());
    };
    let version = tag.trim_start_matches('v');
    if !is_newer(tag, env!("CARGO_PKG_VERSION")) || is_skipped(version) {
        return Found::Current;
    }
    match download(version) {
        Ok(()) => Found::Ready(version.to_owned()),
        Err(e) => Found::Failed(e),
    }
}

/// Swaps in a downloaded update and starts it with `args`. The caller then closes.
pub fn restart(args: &[OsString], restore: Option<Restore>) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let pending = pending_path(&exe);
    if pending.exists() {
        swap(&exe, &pending).map_err(|e| e.to_string())?;
    }
    let mut c = Command::new(&exe);
    if let Some(r) = restore {
        c.arg(r.to_arg());
    }
    let child = c.args(args).spawn().map_err(|e| e.to_string())?;
    let started = Instant::now();
    *WATCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(Watch {
        child,
        started,
        exe,
        args: args.to_vec(),
        restore,
    });
    Ok(())
}

/// A version started after a swap, watched until it has run for [`TRIAL`].
struct Watch {
    child: std::process::Child,
    /// The trial counts from here, as the new version's [`clean_up`] does.
    started: Instant,
    exe: PathBuf,
    args: Vec<OsString>,
    restore: Option<Restore>,
}

static WATCH: Mutex<Option<Watch>> = Mutex::new(None);

/// Called after the window has closed: if [`restart`] started a new version, waits
/// (without a window) until it has proved it runs, and rolls back if it did not.
pub fn finish() {
    let watch = WATCH.lock().unwrap_or_else(|e| e.into_inner()).take();
    let Some(mut w) = watch else {
        return;
    };
    if survives(&mut w.child, w.started) {
        return;
    }
    // If the files cannot be swapped back, start the old one where it lies, so that
    // the user is not left without a window.
    let exe = match roll_back(&w.exe) {
        Ok(()) => w.exe,
        Err(_) => old_path(&w.exe),
    };
    let mut c = Command::new(exe);
    if let Some(r) = w.restore {
        c.arg(r.to_arg());
    }
    let _ = c.args(&w.args).spawn();
}

/// False if the process exited with an error before [`TRIAL`] was over. A clean exit
/// (the user closed it again) or still running counts as a successful start.
fn survives(child: &mut std::process::Child, start: Instant) -> bool {
    while start.elapsed() < TRIAL {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) => std::thread::sleep(Duration::from_millis(200)),
            Err(_) => return true,
        }
    }
    true
}

/// Puts the previous executable back and remembers the failed version so it is not
/// downloaded again.
fn roll_back(exe: &Path) -> std::io::Result<()> {
    let old = old_path(exe);
    if !old.is_file() {
        return Err(std::io::ErrorKind::NotFound.into());
    }
    let bad = sibling(exe, "bad");
    let _ = std::fs::remove_file(&bad);
    // The crashed exe can stay locked for a moment (crash reporter, virus scanner).
    let mut tries = 0;
    while let Err(e) = std::fs::rename(exe, &bad) {
        tries += 1;
        if tries == 10 {
            return Err(e);
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    if let Err(e) = std::fs::rename(&old, exe) {
        let _ = std::fs::rename(&bad, exe);
        return Err(e);
    }
    let _ = std::fs::remove_file(&bad);
    if let Ok(v) = std::fs::read_to_string(note_path(exe, "update-version")) {
        let _ = std::fs::write(note_path(exe, "skip-version"), v);
    }
    Ok(())
}

fn is_skipped(version: &str) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read_to_string(note_path(&exe, "skip-version")).ok())
        .is_some_and(|v| v.trim() == version)
}

/// At start: if an update is waiting, swap it in and start it instead. Returns true
/// when the new version was started and this process should end. If it fails right
/// away, the swap is undone and this (old) version goes on starting.
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
    let now = Instant::now();
    let started = Command::new(&exe).args(args).spawn();
    if started.is_ok_and(|mut child| survives(&mut child, now)) {
        return true;
    }
    // Back to this version; even if that fails, this process keeps running.
    let _ = roll_back(&exe);
    false
}

/// Removes the executable a previous update left behind, once this version has run
/// longer than the trial the old process waits for (it may still need the file to roll
/// back). On Windows the old process may still be closing, so this retries.
pub fn clean_up() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let old = old_path(&exe);
    if !old.exists() {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(TRIAL + Duration::from_secs(5));
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
    let file = format!("{ASSET}.{ARCHIVE}");
    let archive = dir.join(&file);
    let sig = dir.join(format!("{file}.sig"));
    let url = format!("https://github.com/{REPO}/releases/download/v{version}/{file}");
    run(cmd("curl")
        .args(["-fsSL", "--max-time", "300", "-o"])
        .arg(&archive)
        .arg(&url))?;
    run(cmd("curl")
        .args(["-fsSL", "--max-time", "30", "-o"])
        .arg(&sig)
        .arg(format!("{url}.sig")))?;
    let data = std::fs::read(&archive).map_err(|e| e.to_string())?;
    let sig = std::fs::read(&sig).map_err(|e| e.to_string())?;
    if !verify(&PUBLIC_KEY, &data, &sig, version, &file) {
        return Err(format!("{file}: invalid signature"));
    }
    run(cmd("tar").arg("-xf").arg(&archive).arg("-C").arg(dir))?;
    let unpacked = dir.join(ASSET).join(BIN);
    // Run it once now, hidden: proves it starts at all, and the virus scanner checks a
    // new exe on its first start, which can take seconds. Done here, the restart later
    // is instant.
    let out = hidden(&unpacked)
        .arg("--version")
        .output()
        .map_err(|e| e.to_string())?;
    let said = String::from_utf8_lossy(&out.stdout);
    if !out.status.success()
        || !(said.trim().is_empty() || said.trim() == format!("ferrosheet {version}"))
    {
        return Err(format!("{file}: the new version does not start"));
    }
    // Without this note a failing version could not be skipped after a roll-back.
    std::fs::write(note_path(pending, "update-version"), version).map_err(|e| e.to_string())?;
    // Moved in one step, so a half-finished download never looks like an update.
    std::fs::rename(unpacked, pending).map_err(|e| e.to_string())
}

/// What CI signs: the archive followed by a line naming the release and the asset.
fn verify(key: &[u8; 32], data: &[u8], sig: &[u8], version: &str, file: &str) -> bool {
    use ed25519_compact::{PublicKey, Signature};
    let Ok(sig) = Signature::from_slice(sig) else {
        return false;
    };
    let Ok(mut state) = PublicKey::new(*key).verify_incremental(&sig) else {
        return false;
    };
    state.absorb(data);
    state.absorb(signed_suffix(version, file));
    state.verify().is_ok()
}

fn signed_suffix(version: &str, file: &str) -> String {
    format!("\nferrosheet v{version} {file}\n")
}

#[cfg(not(windows))]
fn hidden(program: &Path) -> Command {
    Command::new(program)
}

#[cfg(windows)]
fn hidden(program: &Path) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new(program);
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
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

/// Small state files next to the executable, e.g. `ferrosheet.skip-version`.
fn note_path(exe: &Path, name: &str) -> PathBuf {
    exe.with_file_name(format!("ferrosheet.{name}"))
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
    fn accepts_only_what_the_key_signed_for_this_release() {
        use ed25519_compact::{KeyPair, Seed};
        let kp = KeyPair::from_seed(Seed::new([7; 32]));
        let key = *kp.pk;
        let file = "ferrosheet-linux-x86_64.tar.gz";
        let data = b"archive bytes".as_slice();
        let mut msg = data.to_vec();
        msg.extend_from_slice(signed_suffix("0.7.0", file).as_bytes());
        let sig = kp.sk.sign(&msg, None);

        assert!(verify(&key, data, sig.as_ref(), "0.7.0", file));
        // older build offered as a newer release, or another platform's archive
        assert!(!verify(&key, data, sig.as_ref(), "0.8.0", file));
        assert!(!verify(
            &key,
            data,
            sig.as_ref(),
            "0.7.0",
            "ferrosheet-windows-x86_64.zip"
        ));
        // changed archive, broken or missing signature, other key
        assert!(!verify(&key, b"archive bytez", sig.as_ref(), "0.7.0", file));
        assert!(!verify(&key, data, &sig.as_ref()[..63], "0.7.0", file));
        assert!(!verify(&key, data, &[], "0.7.0", file));
        let other = *KeyPair::from_seed(Seed::new([8; 32])).pk;
        assert!(!verify(&other, data, sig.as_ref(), "0.7.0", file));
    }

    #[test]
    fn rolls_back_to_the_old_version_and_skips_the_failed_one() {
        let dir = std::env::temp_dir().join("ferrosheet-rollback-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("ferrosheet.exe");
        std::fs::write(&exe, "new").unwrap();
        std::fs::write(old_path(&exe), "old").unwrap();
        std::fs::write(note_path(&exe, "update-version"), "0.7.0").unwrap();

        roll_back(&exe).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "old");
        assert!(!old_path(&exe).exists());
        assert!(!sibling(&exe, "bad").exists());
        let skip = std::fs::read_to_string(note_path(&exe, "skip-version")).unwrap();
        assert_eq!(skip, "0.7.0");

        // nothing to go back to: the current exe stays
        assert!(roll_back(&exe).is_err());
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "old");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crash_right_after_start_fails_the_trial() {
        let (ok, fail) = if cfg!(windows) {
            (["cmd", "/C", "exit 0"], ["cmd", "/C", "exit 3"])
        } else {
            (["sh", "-c", "exit 0"], ["sh", "-c", "exit 3"])
        };
        let start = |c: [&str; 3]| Command::new(c[0]).args(&c[1..]).spawn().unwrap();
        assert!(survives(&mut start(ok), Instant::now()));
        assert!(!survives(&mut start(fail), Instant::now()));
        // the trial is over: a later exit no longer counts
        let late = Instant::now() - TRIAL;
        assert!(survives(&mut start(fail), late));
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
