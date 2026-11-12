//! Editing and saving. Changes are queued until the IronCalc model is ready (it is
//! built on the first edit, in the background), then applied, and the cells they touch
//! are copied back into the sheets on screen.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, Color32, ViewportCommand};
use ferrosheet_core::{
    CellPos, EditSession, NumberFormat, Selection, Sheet, StyleChange, write_csv,
};

use super::{App, Doc, SheetView};
use crate::grid::{self, CellEdit};
use crate::text;

pub(super) enum Op {
    Input {
        sheet: usize,
        pos: CellPos,
        text: String,
    },
    Clear {
        sheet: usize,
        sel: Selection,
    },
    Style {
        sheet: usize,
        sel: Selection,
        change: StyleChange,
    },
    Undo,
    Redo,
    AddSheet,
    RenameSheet {
        sheet: usize,
        name: String,
    },
    DeleteSheet {
        sheet: usize,
    },
    Save {
        path: PathBuf,
        then: Option<After>,
    },
}

/// What to do once unsaved changes are saved or discarded.
#[derive(Clone)]
pub(super) enum After {
    Close,
    Open(PathBuf),
}

#[derive(Default)]
pub(super) enum Engine {
    /// Nothing edited yet.
    #[default]
    Idle,
    /// Loading the remaining sheets the model is built from.
    WaitingForSheets,
    Building(Receiver<Result<EditSession, String>>),
    Ready(Box<EditSession>),
    /// The file cannot be edited; why.
    Failed(String),
}

/// Text and fill colours offered in the format bar.
const COLORS: [[u8; 3]; 9] = [
    [0, 0, 0],
    [192, 0, 0],
    [255, 0, 0],
    [255, 192, 0],
    [255, 255, 0],
    [146, 208, 80],
    [0, 176, 80],
    [0, 112, 192],
    [112, 48, 160],
];

/// Files saved in place; everything else is saved as a new `.xlsx`. A text file
/// holds one sheet, so once sheets were added it becomes a workbook.
fn saves_in_place(doc: &Doc) -> bool {
    let ext = doc
        .path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext.as_deref() {
        Some("xlsx") => true,
        _ => doc.text_format.is_some() && doc.names.len() == 1,
    }
}

fn is_xlsx(path: &Path) -> bool {
    path.extension().is_some_and(|e| {
        matches!(
            e.to_string_lossy().to_ascii_lowercase().as_str(),
            "xlsx" | "xlsm"
        )
    })
}

/// Writes the cells the session may have changed into the sheet on screen.
fn refresh(v: &mut SheetView, s: &EditSession, idx: usize) {
    let updates = s.updates(idx);
    if updates.is_empty() {
        return;
    }
    let sheet = Arc::make_mut(&mut v.sheet);
    for u in updates {
        v.formulas.remove(&(u.row, u.col));
        sheet.set(u.row, u.col, u.value, u.formatted, u.style);
    }
    v.formulas.extend(s.formulas(idx));
}

pub(super) fn refresh_sheet(doc: &mut Doc, idx: usize) {
    if let (Engine::Ready(s), Some(Some(v))) = (&doc.engine, doc.sheets.get_mut(idx)) {
        refresh(v, s, idx);
    }
}

/// Follows the model's sheet list after adding, renaming, deleting or undoing one.
/// Sheets keep their view (selection, widths); a sheet the viewer never had (a new one,
/// or one brought back by undo) is taken from the model. A new sheet becomes active.
fn sync_sheets(doc: &mut Doc) {
    let Engine::Ready(s) = &doc.engine else {
        return;
    };
    doc.names = s.sheet_names();
    let ids = s.sheet_ids();
    if ids == doc.ids {
        return;
    }
    let active_id = doc.ids.get(doc.active).copied();
    let mut old: Vec<_> = doc
        .ids
        .drain(..)
        .zip(doc.sheets.drain(..).zip(doc.source.drain(..)))
        .collect();
    let mut added = None;
    for (pos, &id) in ids.iter().enumerate() {
        match old.iter().position(|o| o.0 == id) {
            Some(k) => {
                let (_, (view, source)) = old.swap_remove(k);
                doc.sheets.push(view);
                doc.source.push(source);
            }
            None => {
                let (sheet, formulas) = s.snapshot(pos);
                let mut v = SheetView::new(Arc::new(sheet));
                v.formulas = formulas;
                v.formulas_loaded = true;
                doc.sheets.push(Some(v));
                doc.source.push(None);
                added.get_or_insert(pos);
            }
        }
    }
    doc.ids = ids;
    doc.active = added
        .or_else(|| active_id.and_then(|a| doc.ids.iter().position(|&i| i == a)))
        .unwrap_or(doc.active)
        .min(doc.ids.len().saturating_sub(1));
}

impl App {
    pub(super) fn dirty(&self) -> bool {
        self.doc.as_ref().is_some_and(|d| d.dirty)
    }

    /// Queues a change; the first one starts building the model.
    pub(super) fn queue(&mut self, ctx: &egui::Context, op: Op) {
        let Some(doc) = &mut self.doc else { return };
        // Say why instead of silently ignoring the change.
        if let Engine::Failed(why) = &doc.engine {
            self.error = Some(why.clone());
            self.error_title = Some(text::edit_failed());
            return;
        }
        if !matches!(op, Op::Save { .. }) {
            doc.dirty = true;
        }
        doc.pending.push_back(op);
        if matches!(doc.engine, Engine::Idle) {
            if is_xlsx(&doc.path) {
                let (tx, rx) = channel();
                let (path, ctx) = (doc.path.clone(), ctx.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(EditSession::open_xlsx(&path, text::locale()));
                    ctx.request_repaint();
                });
                doc.engine = Engine::Building(rx);
            } else {
                doc.request_missing();
                doc.engine = Engine::WaitingForSheets;
            }
        }
        self.poll_engine(ctx);
    }

    /// Moves the model along: build it once all sheets are there, take it when it is
    /// ready, apply what is queued.
    pub(super) fn poll_engine(&mut self, ctx: &egui::Context) {
        let Some(doc) = &mut self.doc else { return };
        match &doc.engine {
            Engine::WaitingForSheets
                if !doc.names.is_empty()
                    && doc
                        .sheets
                        .iter()
                        .all(|v| v.as_ref().is_some_and(|v| v.formulas_loaded)) =>
            {
                let sheets: Vec<_> = doc
                    .sheets
                    .iter()
                    .flatten()
                    .map(|v| (Arc::clone(&v.sheet), v.formulas.clone()))
                    .collect();
                let (tx, rx) = channel();
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let refs: Vec<_> = sheets.iter().map(|(s, f)| (&**s, f)).collect();
                    let _ = tx.send(EditSession::from_sheets(&refs, text::locale()));
                    ctx.request_repaint();
                });
                doc.engine = Engine::Building(rx);
            }
            Engine::Building(rx) => match rx.try_recv() {
                Ok(Ok(s)) => {
                    doc.ids = s.sheet_ids();
                    doc.engine = Engine::Ready(Box::new(s));
                    sync_sheets(doc);
                    for idx in 0..doc.sheets.len() {
                        refresh_sheet(doc, idx);
                    }
                }
                Ok(Err(e)) => self.edit_failed(e),
                Err(TryRecvError::Disconnected) => self.edit_failed(String::new()),
                Err(TryRecvError::Empty) => {}
            },
            _ => {}
        }
        self.flush(ctx);
    }

    fn edit_failed(&mut self, e: String) {
        let why = text::cannot_edit(&e);
        if let Some(doc) = &mut self.doc {
            doc.engine = Engine::Failed(why.clone());
            doc.pending.clear();
            doc.dirty = false;
            for v in doc.sheets.iter_mut().flatten() {
                v.grid.edit = None;
            }
        }
        self.error = Some(why);
        self.error_title = Some(text::edit_failed());
    }

    fn flush(&mut self, ctx: &egui::Context) {
        loop {
            let Some(doc) = &mut self.doc else { return };
            let Engine::Ready(s) = &mut doc.engine else {
                return;
            };
            let Some(op) = doc.pending.pop_front() else {
                return;
            };
            let result = match op {
                Op::Input { sheet, pos, text } => s.set_input(sheet, pos.row, pos.col, &text),
                Op::Clear { sheet, sel } => s.clear(sheet, &sel),
                Op::Style { sheet, sel, change } => s.set_style(sheet, &sel, change),
                Op::Undo => {
                    s.undo();
                    Ok(())
                }
                Op::Redo => {
                    s.redo();
                    Ok(())
                }
                Op::AddSheet => s.add_sheet(),
                Op::RenameSheet { sheet, name } => s.rename_sheet(sheet, &name),
                Op::DeleteSheet { sheet } => s.delete_sheet(sheet),
                Op::Save { path, then } => {
                    self.write(ctx, path, then);
                    continue;
                }
            };
            sync_sheets(doc);
            for idx in 0..doc.sheets.len() {
                refresh_sheet(doc, idx);
            }
            self.search.dirty = true;
            if let Err(e) = result {
                self.error = Some(e);
                self.error_title = Some(text::edit_failed());
            }
        }
    }

    fn write(&mut self, ctx: &egui::Context, path: PathBuf, then: Option<After>) {
        let Some(doc) = &mut self.doc else { return };
        let Engine::Ready(s) = &doc.engine else {
            return;
        };
        let as_text = path == doc.path && !is_xlsx(&path);
        let result = match (as_text, doc.text_format, doc.sheets.first()) {
            (true, Some(format), Some(Some(v))) => write_text(&path, &v.sheet, format),
            _ => s.save_xlsx(&path),
        };
        match result {
            Ok(()) => {
                if path != doc.path {
                    doc.text_format = None;
                    self.recent.push(&path);
                    doc.path = path;
                }
                doc.dirty = false;
                self.status = Some((text::saved().into(), ctx.input(|i| i.time)));
                match then {
                    Some(After::Close) => {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    Some(After::Open(p)) => self.open(ctx, p),
                    None => {}
                }
            }
            Err(e) => {
                self.error = Some(e);
                self.error_title = Some(text::save_failed());
            }
        }
    }

    /// Ctrl+S: in place for xlsx and text files, otherwise "Save as".
    pub(super) fn save(&mut self, ctx: &egui::Context, then: Option<After>) {
        let Some(doc) = &self.doc else { return };
        if !doc.dirty {
            return;
        }
        if saves_in_place(doc) {
            let path = doc.path.clone();
            self.queue(ctx, Op::Save { path, then });
        } else {
            self.save_as(ctx, then);
        }
    }

    pub(super) fn save_as(&mut self, ctx: &egui::Context, then: Option<After>) {
        let Some(doc) = &self.doc else { return };
        let name = doc.path.with_extension("xlsx");
        let mut dialog = rfd::FileDialog::new().add_filter(text::xlsx_filter(), &["xlsx"]);
        if let Some(n) = name.file_name() {
            dialog = dialog.set_file_name(n.to_string_lossy());
        }
        if let Some(dir) = doc.path.parent() {
            dialog = dialog.set_directory(dir);
        }
        if let Some(mut path) = dialog.save_file() {
            if !is_xlsx(&path) {
                path.set_extension("xlsx");
            }
            self.queue(ctx, Op::Save { path, then });
        }
    }

    /// Opening another file or closing with unsaved changes asks first.
    pub(super) fn confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(after) = self.confirm.clone() else {
            return;
        };
        let file = self
            .doc
            .as_ref()
            .and_then(|d| d.path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut choice = None;
        let resp = egui::Modal::new(egui::Id::unique("unsaved")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(text::unsaved_title());
            ui.label(text::save_question(&file));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(text::save()).clicked() {
                    choice = Some(true);
                }
                if ui.button(text::discard()).clicked() {
                    choice = Some(false);
                }
                if ui.button(text::cancel()).clicked() {
                    self.confirm = None;
                }
            });
        });
        if resp.should_close() {
            self.confirm = None;
        }
        match choice {
            Some(true) => {
                self.confirm = None;
                self.save(ctx, Some(after));
            }
            Some(false) => {
                self.confirm = None;
                if let Some(d) = &mut self.doc {
                    d.dirty = false;
                }
                match after {
                    After::Close => {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                    After::Open(p) => self.open(ctx, p),
                }
            }
            None => {}
        }
    }

    // ---- cell editor -------------------------------------------------------

    /// Opens the editor on the cursor cell, with `typed` replacing the content.
    pub(super) fn start_edit(&mut self, typed: Option<String>) {
        let Some(doc) = &mut self.doc else { return };
        // Say why instead of silently ignoring the key or click.
        if let Engine::Failed(why) = &doc.engine {
            self.error = Some(why.clone());
            self.error_title = Some(text::edit_failed());
            return;
        }
        let idx = doc.active;
        let session = match &doc.engine {
            Engine::Ready(s) => Some(s),
            _ => None,
        };
        let Some(Some(v)) = doc.sheets.get_mut(idx) else {
            return;
        };
        // Without the formulas the editor would start with a result and overwrite the
        // formula behind it. They arrive a moment after the values.
        if session.is_none() && !v.formulas_loaded {
            self.status = Some((text::formulas_loading().into(), f64::NAN));
            return;
        }
        let pos = v.grid.selection.cursor;
        let original = match session {
            Some(s) => s.content(idx, pos.row, pos.col),
            None => v
                .formulas
                .get(&(pos.row, pos.col))
                .cloned()
                .unwrap_or_else(|| v.sheet.display(pos.row, pos.col).into_owned()),
        };
        v.grid.edit = Some(CellEdit {
            pos,
            text: typed.unwrap_or_else(|| original.clone()),
            original,
            focus: true,
            in_bar: false,
        });
    }

    /// Closes the editor and stores what was typed; `step` moves the cursor on.
    pub(super) fn commit_edit(&mut self, ctx: &egui::Context, step: Option<(i64, i64)>) {
        let Some(doc) = &mut self.doc else { return };
        let idx = doc.active;
        let Some(Some(v)) = doc.sheets.get_mut(idx) else {
            return;
        };
        let Some(e) = v.grid.edit.take() else { return };
        ctx.memory_mut(|m| {
            m.surrender_focus(grid::edit_id());
            m.surrender_focus(grid::bar_id());
        });
        if let Some((dr, dc)) = step {
            let g = &mut v.grid;
            g.selection = Selection::single(e.pos);
            let rows = v.sheet.rows.max(e.pos.row + 2);
            let cols = v.sheet.cols.max(e.pos.col + 2);
            g.selection.move_cursor(dr, dc, false, rows, cols);
            g.scroll_to = Some(g.selection.cursor);
        }
        if e.text != e.original {
            self.queue(
                ctx,
                Op::Input {
                    sheet: idx,
                    pos: e.pos,
                    text: e.text,
                },
            );
        }
    }

    pub(super) fn cancel_edit(&mut self, ctx: &egui::Context) {
        if let Some(v) = self.active_view() {
            v.grid.edit = None;
        }
        ctx.memory_mut(|m| {
            m.surrender_focus(grid::edit_id());
            m.surrender_focus(grid::bar_id());
        });
    }

    pub(super) fn editing_cell(&mut self) -> bool {
        self.active_view().is_some_and(|v| v.grid.edit.is_some())
    }

    /// Delete: empties the selected cells.
    pub(super) fn clear_selection(&mut self, ctx: &egui::Context) {
        let Some(doc) = &self.doc else { return };
        let sheet = doc.active;
        let Some(Some(v)) = doc.sheets.get(sheet) else {
            return;
        };
        let sel = v.grid.selection;
        self.queue(ctx, Op::Clear { sheet, sel });
    }

    pub(super) fn undo(&mut self, ctx: &egui::Context, redo: bool) {
        // Nothing was edited yet: nothing to undo, and no reason to build the model.
        if self
            .doc
            .as_ref()
            .is_some_and(|d| matches!(d.engine, Engine::Idle))
        {
            return;
        }
        self.queue(ctx, if redo { Op::Redo } else { Op::Undo });
    }

    // ---- formatting --------------------------------------------------------

    pub(super) fn style(&mut self, ctx: &egui::Context, change: StyleChange) {
        let Some(doc) = &self.doc else { return };
        let sheet = doc.active;
        let Some(Some(v)) = doc.sheets.get(sheet) else {
            return;
        };
        let sel = v.grid.selection;
        self.queue(ctx, Op::Style { sheet, sel, change });
    }

    /// Ctrl+B / Ctrl+I: the opposite of what the cursor cell has.
    pub(super) fn toggle(&mut self, ctx: &egui::Context, italic: bool) {
        let Some(v) = self.active_view() else { return };
        let cur = v.grid.selection.cursor;
        let st = v.sheet.style(cur.row, cur.col);
        let change = if italic {
            StyleChange::Italic(!st.italic)
        } else {
            StyleChange::Bold(!st.bold)
        };
        self.style(ctx, change);
    }

    /// Style of the cursor cell, for the toggles in the toolbar and menus.
    pub(super) fn cursor_style(&mut self) -> ferrosheet_core::CellStyle {
        self.active_view()
            .map(|v| {
                let cur = v.grid.selection.cursor;
                v.sheet.style(cur.row, cur.col)
            })
            .unwrap_or_default()
    }

    /// Toolbar under the menu: save and the most used formatting.
    pub(super) fn format_bar(&mut self, ui: &mut egui::Ui) {
        if self.active_view().is_none() {
            return;
        }
        let st = self.cursor_style();
        let mut change = None;
        ui.horizontal(|ui| {
            let save = ui
                .add_enabled(self.dirty(), egui::Button::new(text::save()))
                .on_hover_text(text::save_hint());
            if save.clicked() {
                self.save(ui.ctx(), None);
            }
            ui.separator();
            let bold = egui::RichText::new(text::bold()).strong();
            if ui
                .selectable_label(st.bold, bold)
                .on_hover_text(text::bold_hint())
                .clicked()
            {
                change = Some(StyleChange::Bold(!st.bold));
            }
            let italic = egui::RichText::new(text::italic()).italics();
            if ui
                .selectable_label(st.italic, italic)
                .on_hover_text(text::italic_hint())
                .clicked()
            {
                change = Some(StyleChange::Italic(!st.italic));
            }
            if let Some(c) = colour_and_number_menus(ui) {
                change = Some(c);
            }
            let status = self.doc.as_ref().is_some_and(|d| {
                matches!(d.engine, Engine::WaitingForSheets | Engine::Building(_))
            });
            if status {
                ui.spinner();
                ui.weak(text::preparing_edit());
            }
        });
        if let Some(c) = change {
            self.style(&ui.ctx().clone(), c);
        }
    }
}

/// Text colour, fill colour and number format as menus (toolbar, Format menu and the
/// right-click menu share them).
pub(super) fn colour_and_number_menus(ui: &mut egui::Ui) -> Option<StyleChange> {
    let mut change = None;
    ui.menu_button(text::text_color(), |ui| {
        if let Some(c) = swatches(ui) {
            change = Some(StyleChange::Color(c));
        }
    });
    ui.menu_button(text::fill_color(), |ui| {
        if let Some(c) = swatches(ui) {
            change = Some(StyleChange::Fill(c));
        }
    });
    ui.menu_button(text::number_format(), |ui| {
        for f in NumberFormat::ALL {
            if ui.button(text::number_format_name(f)).clicked() {
                change = Some(StyleChange::Number(f));
            }
        }
    });
    change
}

/// A row of colour buttons plus "None"; returns the picked colour.
fn swatches(ui: &mut egui::Ui) -> Option<Option<[u8; 3]>> {
    let mut picked = None;
    if ui.button(text::no_color()).clicked() {
        picked = Some(None);
    }
    ui.horizontal(|ui| {
        for c in COLORS {
            let [r, g, b] = c;
            let swatch = egui::Button::new("")
                .fill(Color32::from_rgb(r, g, b))
                .min_size(egui::vec2(18.0, 18.0));
            if ui.add(swatch).clicked() {
                picked = Some(Some(c));
            }
        }
    });
    picked
}

/// Writes a text file the way it was read, through a temporary file next to it.
fn write_text(
    path: &Path,
    sheet: &Sheet,
    format: ferrosheet_core::TextFormat,
) -> Result<(), String> {
    let bytes = write_csv(sheet, format)?;
    let tmp = path.with_extension("ferrosheet-tmp");
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}
