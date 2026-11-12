// No console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod grid;
mod loader;
mod recent;
#[cfg(windows)]
mod register;
mod rows;
mod text;
mod update;

use eframe::egui;

fn main() -> eframe::Result {
    let (restore, args) = update::take_restore(std::env::args_os().skip(1).collect());
    let arg = args.first().cloned();
    // Also used by the updater to start a downloaded exe once, without a window.
    if arg.as_deref() == Some(std::ffi::OsStr::new("--version")) {
        println!("ferrosheet {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    #[cfg(windows)]
    if let Some(flag) = arg.as_ref().and_then(|a| a.to_str()) {
        let result = match flag {
            "--register" => Some(register::register().map(|_| text::registered())),
            "--unregister" => Some(register::unregister().map(|_| text::unregistered())),
            _ => None,
        };
        if let Some(result) = result {
            // Release builds have no console, so report in a small dialog.
            let (level, msg) = match result {
                Ok(msg) => (rfd::MessageLevel::Info, msg.to_owned()),
                Err(e) => (
                    rfd::MessageLevel::Error,
                    format!("{}: {e}", text::register_failed()),
                ),
            };
            rfd::MessageDialog::new()
                .set_title(text::APP)
                .set_level(level)
                .set_description(msg)
                .show();
            return Ok(());
        }
    }
    // An update downloaded last time: swap it in and start that instead.
    if restore.is_none() && update::apply_pending(&args) {
        return Ok(());
    }
    update::clean_up();
    let path = arg.map(std::path::PathBuf::from);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title(text::APP)
        .with_inner_size(restore.map_or([1200.0, 800.0], |r| r.size))
        .with_min_inner_size([400.0, 300.0])
        .with_drag_and_drop(true)
        .with_icon(egui::IconData {
            rgba: include_bytes!("../assets/icon-64.rgba").to_vec(),
            width: 64,
            height: 64,
        });
    // After an update restart: same place, same size, same sheet.
    if let Some(r) = restore {
        viewport = viewport.with_position(r.pos).with_maximized(r.maximized);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        text::APP,
        options,
        Box::new(move |cc| {
            let mut app = app::App::new(&cc.egui_ctx, path);
            if let Some(r) = restore {
                app.restore_sheet(r.sheet);
            }
            Ok(Box::new(app))
        }),
    )
}
