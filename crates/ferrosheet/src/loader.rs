//! Reads the workbook on a background thread so the window never blocks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use eframe::egui;
use ferrosheet_core::{Sheet, Workbook};

pub enum LoadMsg {
    Opened {
        names: Vec<String>,
    },
    Sheet {
        idx: usize,
        sheet: Sheet,
    },
    Formulas {
        idx: usize,
        map: HashMap<(u32, u32), String>,
    },
    Error(String),
}

/// Handle to the loader thread. Dropping it ends the thread.
pub struct Loader {
    requests: Sender<usize>,
    messages: Receiver<LoadMsg>,
}

impl Loader {
    /// Opens `path`, loads the first sheet and then waits for sheet requests.
    pub fn open(path: PathBuf, ctx: egui::Context) -> Loader {
        let (requests, req_rx) = channel::<usize>();
        let (msg_tx, messages) = channel();
        std::thread::spawn(move || {
            let send = |m: LoadMsg| {
                let _ = msg_tx.send(m);
                ctx.request_repaint();
            };
            let mut wb = match Workbook::open(&path) {
                Ok(wb) => wb,
                Err(e) => return send(LoadMsg::Error(e.to_string())),
            };
            send(LoadMsg::Opened {
                names: wb.sheet_names.clone(),
            });
            let mut loaded = vec![false; wb.sheet_names.len()];
            for idx in std::iter::once(0).chain(req_rx.iter()) {
                if loaded.get(idx).copied().unwrap_or(true) {
                    continue;
                }
                loaded[idx] = true;
                match wb.load_sheet(idx) {
                    Ok(sheet) => send(LoadMsg::Sheet { idx, sheet }),
                    Err(e) => {
                        send(LoadMsg::Error(e.to_string()));
                        continue;
                    }
                }
                // Formulas are extra information for the formula bar; a file without
                // readable formulas simply shows values only.
                if let Ok(map) = wb.load_formulas(idx) {
                    send(LoadMsg::Formulas { idx, map });
                }
            }
        });
        Loader { requests, messages }
    }

    pub fn request_sheet(&self, idx: usize) {
        let _ = self.requests.send(idx);
    }

    pub fn poll(&self) -> Vec<LoadMsg> {
        self.messages.try_iter().collect()
    }
}
