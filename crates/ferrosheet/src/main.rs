// No console window on Windows release builds.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod grid;
mod loader;
#[cfg(windows)]
mod register;
mod rows;
mod text;

use eframe::egui;

fn main() -> eframe::Result {
    let arg = std::env::args_os().nth(1);
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
    let path = arg.map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(text::APP)
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([400.0, 300.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        text::APP,
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(&cc.egui_ctx, path)))),
    )
}
