//! Windows file association: `ferrosheet --register` / `--unregister`.
//!
//! Writes only to the current user's registry (no admin rights). Windows does not let
//! programs make themselves the default app, so `register` adds ferrosheet to "Open with"
//! and to the default-apps settings page, which it then opens for the one click that is left.

use std::io;
use std::path::Path;

use ferrosheet_core::EXTENSIONS;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const PROG_ID: &str = "ferrosheet.sheet";
const APP_NAME: &str = "ferrosheet";
const CLASSES: &str = r"Software\Classes";
const CAPABILITIES: &str = r"Software\ferrosheet\Capabilities";
const REGISTERED_APPS: &str = r"Software\RegisteredApplications";

/// Every registry value `register` writes, as (key, value name, data) relative to the root.
fn entries(exe: &Path) -> Vec<(String, String, String)> {
    let exe = exe.display();
    let open = format!("\"{exe}\" \"%1\"");
    let app_key = format!(r"{CLASSES}\Applications\{APP_NAME}.exe");
    let mut v = vec![
        (
            format!(r"{CLASSES}\{PROG_ID}"),
            String::new(),
            crate::text::file_type().into(),
        ),
        (
            format!(r"{CLASSES}\{PROG_ID}\DefaultIcon"),
            String::new(),
            format!("\"{exe}\",0"),
        ),
        (
            format!(r"{CLASSES}\{PROG_ID}\shell\open\command"),
            String::new(),
            open.clone(),
        ),
        (
            format!(r"{app_key}\shell\open\command"),
            String::new(),
            open,
        ),
        (
            CAPABILITIES.into(),
            "ApplicationName".into(),
            APP_NAME.into(),
        ),
        (
            CAPABILITIES.into(),
            "ApplicationDescription".into(),
            crate::text::app_description().into(),
        ),
        (REGISTERED_APPS.into(), APP_NAME.into(), CAPABILITIES.into()),
    ];
    for ext in EXTENSIONS {
        let ext = format!(".{ext}");
        v.push((
            format!(r"{CLASSES}\{ext}\OpenWithProgids"),
            PROG_ID.into(),
            String::new(),
        ));
        v.push((
            format!(r"{app_key}\SupportedTypes"),
            ext.clone(),
            String::new(),
        ));
        v.push((
            format!(r"{CAPABILITIES}\FileAssociations"),
            ext,
            PROG_ID.into(),
        ));
    }
    v
}

fn apply(root: &RegKey, exe: &Path) -> io::Result<()> {
    for (key, name, data) in entries(exe) {
        root.create_subkey(&key)?.0.set_value(&name, &data)?;
    }
    Ok(())
}

/// Removes exactly what [`apply`] wrote; missing keys are fine.
fn remove(root: &RegKey) -> io::Result<()> {
    let ignore_missing = |r: io::Result<()>| match r {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    for key in [
        format!(r"{CLASSES}\{PROG_ID}"),
        format!(r"{CLASSES}\Applications\{APP_NAME}.exe"),
        r"Software\ferrosheet".into(),
    ] {
        ignore_missing(root.delete_subkey_all(&key))?;
    }
    let delete_value = |key: &str, name: &str| {
        ignore_missing(
            root.open_subkey_with_flags(key, winreg::enums::KEY_SET_VALUE)?
                .delete_value(name),
        )
    };
    for ext in EXTENSIONS {
        ignore_missing(delete_value(
            &format!(r"{CLASSES}\.{ext}\OpenWithProgids"),
            PROG_ID,
        ))?;
    }
    ignore_missing(delete_value(REGISTERED_APPS, APP_NAME))
}

/// Tells Explorer that associations changed, so icons and "Open with" update at once.
fn notify_shell() {
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    // SAFETY: documented call with null item pointers for SHCNE_ASSOCCHANGED.
    unsafe {
        SHChangeNotify(
            SHCNE_ASSOCCHANGED as i32,
            SHCNF_IDLIST,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
}

/// The exe the "open" command of an existing registration points at, if any.
fn registered_exe(command: &str) -> Option<&Path> {
    let rest = command.strip_prefix('"')?;
    Some(Path::new(&rest[..rest.find('"')?]))
}

/// Re-registers silently when ferrosheet was registered from a place that no longer has
/// the exe (moved or unzipped elsewhere). Does nothing for other copies next to a working one,
/// so a second copy does not take the association over.
pub fn refresh_if_moved() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey(format!(r"{CLASSES}\{PROG_ID}\shell\open\command")) else {
        return;
    };
    let Ok(command) = key.get_value::<String, _>("") else {
        return;
    };
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    if registered_exe(&command).is_some_and(|p| p != exe && !p.exists())
        && apply(&hkcu, &exe).is_ok()
    {
        notify_shell();
    }
}

#[derive(Clone, Copy)]
pub enum Action {
    Register,
    Unregister,
}

/// Runs `action` and reports the outcome in a small dialog (release builds have no console).
pub fn run(action: Action) {
    let result = match action {
        Action::Register => register().map(|_| crate::text::registered()),
        Action::Unregister => unregister().map(|_| crate::text::unregistered()),
    };
    let (level, msg) = match result {
        Ok(msg) => (rfd::MessageLevel::Info, msg.to_owned()),
        Err(e) => (
            rfd::MessageLevel::Error,
            format!("{}: {e}", crate::text::register_failed()),
        ),
    };
    rfd::MessageDialog::new()
        .set_title(crate::text::APP)
        .set_level(level)
        .set_description(msg)
        .show();
}

fn register() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    apply(&RegKey::predef(HKEY_CURRENT_USER), &exe)?;
    notify_shell();
    let page = format!("ms-settings:defaultapps?registeredAppUser={APP_NAME}");
    std::process::Command::new("explorer").arg(page).spawn()?;
    Ok(())
}

fn unregister() -> io::Result<()> {
    remove(&RegKey::predef(HKEY_CURRENT_USER))?;
    notify_shell();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_command_quotes_the_exe_and_the_file() {
        let e = entries(Path::new(r"C:\Program Files\ferrosheet\ferrosheet.exe"));
        let cmd = e
            .iter()
            .find(|(k, _, _)| k.ends_with(r"ferrosheet.sheet\shell\open\command"))
            .unwrap();
        assert_eq!(
            cmd.2,
            r#""C:\Program Files\ferrosheet\ferrosheet.exe" "%1""#
        );
        for ext in EXTENSIONS {
            assert!(
                e.iter()
                    .any(|(k, n, _)| k.ends_with(&format!(r".{ext}\OpenWithProgids"))
                        && n == PROG_ID)
            );
        }
    }

    #[test]
    fn registered_exe_is_the_quoted_part() {
        assert_eq!(
            registered_exe(r#""C:\a b\ferrosheet.exe" "%1""#),
            Some(Path::new(r"C:\a b\ferrosheet.exe"))
        );
        assert_eq!(registered_exe("ferrosheet.exe %1"), None);
        assert_eq!(registered_exe(r#""unterminated"#), None);
    }

    /// Writes into a throwaway key instead of the real classes, then removes it again.
    #[test]
    fn apply_then_remove_leaves_nothing_behind() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let test_root = format!(r"Software\ferrosheet-test-{}", std::process::id());
        let (root, _) = hkcu.create_subkey(&test_root).unwrap();
        let exe = Path::new(r"C:\x\ferrosheet.exe");

        apply(&root, exe).unwrap();
        for (key, name, data) in entries(exe) {
            let got: String = root.open_subkey(&key).unwrap().get_value(&name).unwrap();
            assert_eq!(got, data, "{key}\\{name}");
        }
        remove(&root).unwrap();
        remove(&root).unwrap(); // twice is fine
        assert!(root.open_subkey(format!(r"{CLASSES}\{PROG_ID}")).is_err());
        assert!(root.open_subkey(r"Software\ferrosheet").is_err());
        let progids = root
            .open_subkey(format!(r"{CLASSES}\.xls\OpenWithProgids"))
            .unwrap();
        assert!(progids.get_value::<String, _>(PROG_ID).is_err());
        let apps = root.open_subkey(REGISTERED_APPS).unwrap();
        assert!(apps.get_value::<String, _>(APP_NAME).is_err());

        hkcu.delete_subkey_all(&test_root).unwrap();
    }
}
