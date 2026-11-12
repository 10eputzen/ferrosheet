//! Application state and the frame-by-frame UI.

mod editing;

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use eframe::egui::{self, Event, Key, Modifiers, ViewportCommand};
use ferrosheet_core::{
    CellPos, CellStyle, CopyFormat, EXTENSIONS, Query, Selection, Sheet, StyleChange, TextFormat,
    cell_address, copy_text, find_all,
};

use editing::{After, Engine, Op};

use crate::grid::{self, Fit, GridOut, GridState};
use crate::loader::{LoadMsg, Loader};
use crate::recent::Recent;
use crate::text;
use crate::update;

/// Above this many cells the search runs on Enter instead of while typing.
const LIVE_SEARCH_MAX_CELLS: u64 = 200_000;
const STATUS_SECS: f64 = 2.0;
const ZOOM_STEPS: [f32; 13] = [
    0.5, 0.67, 0.75, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

/// Something the user asked for through a menu, a button or the sheet tabs.
enum Cmd {
    Open,
    OpenRecent(PathBuf),
    Save,
    SaveAs,
    Exit,
    Undo,
    Redo,
    Cut,
    Copy(CopyFormat),
    Clear,
    Find,
    Bold,
    Italic,
    Style(StyleChange),
    Zoom(f32),
    SelectCols,
    SelectRows,
    FitCols,
    FitRows,
    Switch(usize),
    NewSheet,
    Rename(usize, String),
    DeleteSheet(usize),
    About,
}

struct SheetView {
    sheet: Arc<Sheet>,
    formulas: HashMap<(u32, u32), String>,
    /// The loader has sent the formulas (the editing model is built from both).
    formulas_loaded: bool,
    grid: GridState,
}

impl SheetView {
    fn new(sheet: Arc<Sheet>) -> SheetView {
        SheetView {
            sheet,
            formulas: HashMap::new(),
            formulas_loaded: false,
            grid: GridState::default(),
        }
    }
}

struct Doc {
    path: PathBuf,
    loader: Loader,
    names: Vec<String>,
    sheets: Vec<Option<SheetView>>,
    /// Per sheet: its index in the file (for the loader); `None` for a sheet added here.
    source: Vec<Option<usize>>,
    /// Per sheet: the model's sheet id, once the model is built.
    ids: Vec<u32>,
    active: usize,
    /// How a text file was written; `None` for workbooks.
    text_format: Option<TextFormat>,
    engine: Engine,
    /// Changes waiting for the model.
    pending: VecDeque<Op>,
    /// Changed since opened or saved.
    dirty: bool,
}

impl Doc {
    /// Position of the sheet the loader calls `loader_idx` (sheets may have moved).
    fn position(&self, loader_idx: usize) -> Option<usize> {
        self.source.iter().position(|s| *s == Some(loader_idx))
    }

    fn request(&self, pos: usize) {
        if let Some(Some(i)) = self.source.get(pos) {
            self.loader.request_sheet(*i);
        }
    }

    fn request_missing(&self) {
        (0..self.sheets.len())
            .filter(|&i| self.sheets[i].is_none())
            .for_each(|i| self.request(i));
    }
}

#[derive(Default)]
struct Search {
    open: bool,
    focus: bool,
    query: String,
    all_sheets: bool,
    /// The query is a regular expression.
    regex: bool,
    /// Why the regular expression is invalid.
    error: Option<String>,
    /// Sorted by (sheet, row, col).
    hits: Vec<(usize, CellPos)>,
    current: Option<usize>,
    dirty: bool,
}

enum ManualCheck {
    Checking,
    Current,
    Failed(String),
}

#[derive(Default)]
enum Update {
    #[default]
    None,
    /// Checking GitHub and downloading in the background.
    Downloading(Receiver<update::Found>),
    /// Downloaded; a restart swaps it in.
    Ready(String),
    /// The new version is being started; this window closes once it runs.
    Restarting(Receiver<Result<(), String>>),
}

#[derive(Default)]
pub struct App {
    doc: Option<Doc>,
    error: Option<String>,
    /// Heading of the error dialog when it is not about loading a file.
    error_title: Option<&'static str>,
    search: Search,
    status: Option<(String, f64)>,
    update: Update,
    /// The About dialog is open.
    about: bool,
    /// Result of a check the user asked for in the About dialog.
    manual_check: Option<ManualCheck>,
    /// Sheet to show once the file is open (after an update restart).
    restore_sheet: Option<usize>,
    /// Asking whether to save before this happens.
    confirm: Option<After>,
    /// Unsaved changes were saved or discarded; the window may close.
    allow_close: bool,
    /// Window title last sent.
    title: String,
    recent: Recent,
    /// Grid zoom, 1.0 = 100 % (0 = not set yet).
    zoom: f32,
    /// Sheet tab being renamed: index, text, focus it next frame.
    rename: Option<(usize, String, bool)>,
    /// Asking whether to delete this sheet.
    confirm_delete: Option<usize>,
}

impl App {
    /// Shows sheet `idx` once the file is open.
    pub fn restore_sheet(&mut self, idx: usize) {
        self.restore_sheet = Some(idx);
    }

    pub fn new(ctx: &egui::Context, path: Option<PathBuf>) -> Self {
        let mut app = App {
            recent: Recent::load(),
            ..App::default()
        };
        // Ctrl+Plus/Minus zoom the grid, not the whole window.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        if let Some(rx) = update::check(ctx.clone()) {
            app.update = Update::Downloading(rx);
        }
        if let Some(p) = path {
            app.open(ctx, p);
        }
        #[cfg(feature = "screenshot")]
        app.prepare_screenshot(ctx);
        app
    }

    /// Screenshot builds wait for the first sheet and show a typical state
    /// (range selection, optional search from `FERROSHEET_SEARCH`).
    #[cfg(feature = "screenshot")]
    fn prepare_screenshot(&mut self, ctx: &egui::Context) {
        match std::env::var("FERROSHEET_THEME").as_deref() {
            Ok("light") => ctx.set_theme(egui::Theme::Light),
            Ok("dark") => ctx.set_theme(egui::Theme::Dark),
            _ => {}
        }
        let start = std::time::Instant::now();
        while self.active_view().is_none() && start.elapsed().as_secs() < 10 {
            std::thread::sleep(std::time::Duration::from_millis(10));
            self.receive(ctx);
        }
        if let Some(v) = self.active_view() {
            v.grid.selection = Selection {
                anchor: CellPos::new(2, 1),
                cursor: CellPos::new(5, 3),
            };
        }
        if let Ok(q) = std::env::var("FERROSHEET_SEARCH") {
            self.search.open = true;
            self.search.query = q;
            self.run_search();
        }
    }

    /// Opens `path`, asking first when the open file has unsaved changes.
    fn request_open(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.dirty() {
            self.confirm = Some(After::Open(path));
        } else {
            self.open(ctx, path);
        }
    }

    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.doc = Some(Doc {
            path: path.clone(),
            loader: Loader::open(path, ctx.clone()),
            names: Vec::new(),
            sheets: Vec::new(),
            source: Vec::new(),
            ids: Vec::new(),
            active: 0,
            text_format: None,
            engine: Engine::Idle,
            pending: VecDeque::new(),
            dirty: false,
        });
        self.update_title(ctx);
        self.search.hits.clear();
        self.search.current = None;
        self.search.dirty = true;
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(text::file_filter(), EXTENSIONS)
            .pick_file()
        {
            self.request_open(ctx, p);
        }
    }

    /// "• name – ferrosheet", the dot for unsaved changes.
    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match &self.doc {
            None => text::APP.to_owned(),
            Some(d) => {
                let name = d
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let dot = if d.dirty { "• " } else { "" };
                format!("{dot}{name} – {}", text::APP)
            }
        };
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let Some(doc) = &mut self.doc else { return };
        for msg in doc.loader.poll() {
            match msg {
                LoadMsg::Opened { names, text_format } => {
                    doc.sheets = names.iter().map(|_| None).collect();
                    doc.source = (0..names.len()).map(Some).collect();
                    doc.names = names;
                    doc.text_format = text_format;
                    self.recent.push(&doc.path);
                    if let Some(i) = self.restore_sheet.take()
                        && i < doc.names.len()
                    {
                        doc.active = i;
                        doc.loader.request_sheet(i);
                    }
                }
                LoadMsg::Sheet { idx, sheet } => {
                    // deleted meanwhile
                    let Some(idx) = doc.position(idx) else {
                        continue;
                    };
                    // A big sheet arrives twice: first a preview of its top rows, then
                    // in full. Keep the view (selection, scroll, widths) when it grows.
                    match &mut doc.sheets[idx] {
                        Some(v) => v.sheet = sheet,
                        slot => *slot = Some(SheetView::new(sheet)),
                    }
                    // edits made while the full sheet was still loading
                    editing::refresh_sheet(doc, idx);
                    self.search.dirty = true;
                }
                LoadMsg::Formulas { idx, map } => {
                    let Some(idx) = doc.position(idx) else {
                        continue;
                    };
                    if let Some(v) = &mut doc.sheets[idx] {
                        v.formulas = map;
                        v.formulas_loaded = true;
                    }
                    editing::refresh_sheet(doc, idx);
                }
                // A sheet that failed shows as empty; the dialog says why.
                LoadMsg::Error {
                    sheet: Some(idx),
                    msg,
                } => {
                    let Some(idx) = doc.position(idx) else {
                        continue;
                    };
                    let mut v = SheetView::new(Arc::new(Sheet::empty(doc.names[idx].clone())));
                    v.formulas_loaded = true;
                    doc.sheets[idx] = Some(v);
                    self.error = Some(msg);
                }
                // The file could not be opened at all: back to the start screen.
                LoadMsg::Error { sheet: None, msg } => {
                    self.doc = None;
                    self.error = Some(msg);
                    self.update_title(ctx);
                    return;
                }
            }
        }
    }

    fn switch_sheet(&mut self, idx: usize) {
        let Some(doc) = &mut self.doc else { return };
        if idx >= doc.names.len() {
            return;
        }
        doc.active = idx;
        if doc.sheets[idx].is_none() {
            doc.request(idx);
        }
    }

    fn zoom(&self) -> f32 {
        if self.zoom > 0.0 { self.zoom } else { 1.0 }
    }

    fn set_zoom(&mut self, z: f32) {
        self.zoom = (z.clamp(grid::MIN_ZOOM, grid::MAX_ZOOM) * 100.0).round() / 100.0;
    }

    /// The next zoom step up or down from the current zoom.
    fn zoom_step(&self, up: bool) -> f32 {
        let z = self.zoom();
        if up {
            ZOOM_STEPS.into_iter().find(|&s| s > z + 0.001)
        } else {
            ZOOM_STEPS.into_iter().rev().find(|&s| s < z - 0.001)
        }
        .unwrap_or(z)
    }

    fn run(&mut self, ctx: &egui::Context, cmd: Cmd) {
        match cmd {
            Cmd::Open => self.open_dialog(ctx),
            Cmd::OpenRecent(p) => self.request_open(ctx, p),
            Cmd::Save => self.save(ctx, None),
            Cmd::SaveAs => self.save_as(ctx, None),
            Cmd::Exit => ctx.send_viewport_cmd(ViewportCommand::Close),
            Cmd::Undo => self.undo(ctx, false),
            Cmd::Redo => self.undo(ctx, true),
            Cmd::Cut => {
                self.copy(ctx, CopyFormat::Display);
                self.clear_selection(ctx);
            }
            Cmd::Copy(f) => self.copy(ctx, f),
            Cmd::Clear => self.clear_selection(ctx),
            Cmd::Find => {
                self.search.open = true;
                self.search.focus = true;
            }
            Cmd::Bold => self.toggle(ctx, false),
            Cmd::Italic => self.toggle(ctx, true),
            Cmd::Style(c) => self.style(ctx, c),
            Cmd::Zoom(z) => self.set_zoom(z),
            Cmd::About => self.about = true,
            Cmd::SelectCols | Cmd::SelectRows | Cmd::FitCols | Cmd::FitRows => {
                self.whole_lines(&cmd)
            }
            Cmd::Switch(i) => self.switch_sheet(i),
            Cmd::NewSheet => self.queue(ctx, Op::AddSheet),
            Cmd::Rename(sheet, name) => self.queue(ctx, Op::RenameSheet { sheet, name }),
            Cmd::DeleteSheet(sheet) => self.queue(ctx, Op::DeleteSheet { sheet }),
        }
    }

    /// Selects or fits the columns or rows the selection touches.
    fn whole_lines(&mut self, cmd: &Cmd) {
        let Some(v) = self.active_view() else { return };
        let (tl, br) = v.grid.selection.bounds();
        let last_row = v.sheet.rows.max(br.row + 1) - 1;
        let last_col = v.sheet.cols.max(br.col + 1) - 1;
        let g = &mut v.grid;
        match cmd {
            Cmd::SelectCols => {
                g.selection = Selection {
                    anchor: CellPos::new(0, tl.col),
                    cursor: CellPos::new(last_row, br.col),
                }
            }
            Cmd::SelectRows => {
                g.selection = Selection {
                    anchor: CellPos::new(tl.row, 0),
                    cursor: CellPos::new(br.row, last_col),
                }
            }
            Cmd::FitCols => g.fit = Some(Fit::Cols(tl.col, br.col)),
            Cmd::FitRows => g.fit = Some(Fit::Rows(tl.row, br.row)),
            _ => {}
        }
    }

    fn active_view(&mut self) -> Option<&mut SheetView> {
        let doc = self.doc.as_mut()?;
        doc.sheets.get_mut(doc.active)?.as_mut()
    }

    fn copy(&mut self, ctx: &egui::Context, f: CopyFormat) {
        let Some(v) = self.active_view() else { return };
        let text = copy_text(&v.sheet, &v.grid.selection, f);
        let n = v.grid.selection.cell_count();
        ctx.copy_text(text);
        self.status = Some((text::copied(n), ctx.input(|i| i.time)));
    }

    // ---- search ------------------------------------------------------------

    fn run_search(&mut self) {
        let s = &mut self.search;
        s.dirty = false;
        s.hits.clear();
        s.current = None;
        s.error = None;
        let Some(doc) = &self.doc else { return };
        if s.query.is_empty() {
            return;
        }
        let query = if s.regex {
            match Query::regex(&s.query) {
                Ok(q) => q,
                Err(e) => {
                    s.error = Some(e);
                    return;
                }
            }
        } else {
            Query::text(&s.query)
        };
        for (idx, view) in doc.sheets.iter().enumerate() {
            if !s.all_sheets && idx != doc.active {
                continue;
            }
            if let Some(v) = view {
                s.hits
                    .extend(find_all(&v.sheet, &query).into_iter().map(|p| (idx, p)));
            }
        }
        // start at the first hit at or after the cursor
        let cursor = doc.sheets[doc.active]
            .as_ref()
            .map(|v| (doc.active, v.grid.selection.cursor));
        if !s.hits.is_empty() {
            s.current = Some(cursor.map_or(0, |c| {
                let i = s
                    .hits
                    .partition_point(|h| (h.0, h.1.row, h.1.col) < (c.0, c.1.row, c.1.col));
                if i == s.hits.len() { 0 } else { i }
            }));
        }
    }

    fn jump(&mut self, step: isize) {
        let n = self.search.hits.len();
        if n == 0 {
            return;
        }
        let cur = self
            .search
            .current
            .map_or(0, |c| (c as isize + step).rem_euclid(n as isize) as usize);
        self.search.current = Some(cur);
        self.reveal_current();
    }

    fn reveal_current(&mut self) {
        let Some((sheet, pos)) = self.search.current.map(|i| self.search.hits[i]) else {
            return;
        };
        self.switch_sheet(sheet);
        if let Some(v) = self.active_view() {
            v.grid.selection = Selection::single(pos);
            v.grid.scroll_to = Some(pos);
        }
    }

    fn large_sheet(&self) -> bool {
        let Some(doc) = &self.doc else { return false };
        doc.sheets
            .iter()
            .flatten()
            .map(|v| v.sheet.rows as u64 * v.sheet.cols as u64)
            .sum::<u64>()
            > LIVE_SEARCH_MAX_CELLS
    }

    fn search_bar(&mut self, ui: &mut egui::Ui) {
        let large = self.large_sheet();
        ui.horizontal(|ui| {
            let label = ui.label(text::search());
            let resp = ui
                .add(egui::TextEdit::singleline(&mut self.search.query).desired_width(240.0))
                .labelled_by(label.id);
            if std::mem::take(&mut self.search.focus) {
                resp.request_focus();
            }
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            if resp.changed() && !large {
                self.search.dirty = true;
            }
            if ui
                .checkbox(&mut self.search.all_sheets, text::all_sheets())
                .changed()
            {
                self.search.dirty = true;
                if self.search.all_sheets
                    && let Some(doc) = &self.doc
                {
                    doc.request_missing();
                }
            }
            if ui
                .checkbox(&mut self.search.regex, text::regex())
                .on_hover_text(text::regex_hint())
                .changed()
            {
                self.search.dirty = true;
            }
            if self.search.dirty && (!large || enter) {
                self.run_search();
                self.reveal_current();
            } else if enter {
                let back = ui.input(|i| i.modifiers.shift);
                self.jump(if back { -1 } else { 1 });
            }
            if enter {
                self.search.focus = true; // keep typing / pressing Enter
            }
            let label = match (self.search.current, self.search.hits.len()) {
                _ if self.search.query.is_empty() => String::new(),
                _ if self.search.dirty && large => text::search_enter().into(),
                (_, 0) => text::no_hits().into(),
                (Some(c), n) => text::hit_counter(c + 1, n),
                (None, n) => text::hit_counter(0, n),
            };
            match &self.search.error {
                Some(e) if !self.search.dirty => {
                    ui.colored_label(ui.visuals().error_fg_color, text::invalid_regex())
                        .on_hover_text(e);
                }
                _ => {
                    ui.weak(label);
                }
            }
            if ui
                .small_button("<")
                .on_hover_text(text::prev_hit())
                .clicked()
            {
                self.jump(-1);
            }
            if ui
                .small_button(">")
                .on_hover_text(text::next_hit())
                .clicked()
            {
                self.jump(1);
            }
            if ui
                .small_button("×")
                .on_hover_text(text::close_search())
                .clicked()
            {
                self.search.open = false;
            }
        });
    }

    // ---- keyboard ----------------------------------------------------------

    fn keyboard(&mut self, ctx: &egui::Context) {
        let mut copy = None;
        let mut cut = false;
        let mut actions = Vec::new();
        let mut typed = String::new();
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    Event::Copy => copy = Some(CopyFormat::Display),
                    Event::Cut => cut = true,
                    Event::Text(t) => typed.push_str(t),
                    Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => actions.push((*key, *modifiers)),
                    _ => {}
                }
            }
        });
        let editing = ctx.memory(|m| m.focused().is_some());
        let modal = self.confirm.is_some() || self.error.is_some();
        // Typing on a cell starts editing it with what was typed.
        if !editing && !modal && !typed.is_empty() && !self.editing_cell() {
            self.start_edit(Some(typed));
            return;
        }

        for (key, m) in actions {
            let ctrl = m.command;
            let cell = self.editing_cell();
            match key {
                _ if modal => {}
                Key::Enter if cell => {
                    self.commit_edit(ctx, Some((if m.shift { -1 } else { 1 }, 0)));
                }
                Key::Tab if cell => self.commit_edit(ctx, Some((0, if m.shift { -1 } else { 1 }))),
                Key::Escape if cell => self.cancel_edit(ctx),
                Key::S if ctrl && m.shift => self.save_as(ctx, None),
                Key::S if ctrl => self.save(ctx, None),
                Key::O if ctrl => self.open_dialog(ctx),
                Key::F if ctrl => {
                    self.search.open = true;
                    self.search.focus = true;
                }
                Key::F3 => self.jump(if m.shift { -1 } else { 1 }),
                Key::Escape if self.search.open => self.search.open = false,
                Key::PageUp | Key::PageDown if ctrl => {
                    if let Some(doc) = &self.doc {
                        let next = if key == Key::PageUp {
                            doc.active.checked_sub(1)
                        } else {
                            Some(doc.active + 1)
                        };
                        if let Some(n) = next {
                            self.switch_sheet(n);
                        }
                    }
                }
                Key::C if ctrl && m.shift && !editing => copy = Some(CopyFormat::Invariant),
                Key::Plus | Key::Equals if ctrl => self.set_zoom(self.zoom_step(true)),
                Key::Minus if ctrl => self.set_zoom(self.zoom_step(false)),
                Key::Num0 if ctrl => self.set_zoom(1.0),
                _ if editing || cell => {}
                Key::Z if ctrl => self.undo(ctx, m.shift),
                Key::Y if ctrl => self.undo(ctx, true),
                Key::B if ctrl => self.toggle(ctx, false),
                Key::I if ctrl => self.toggle(ctx, true),
                Key::F2 => self.start_edit(None),
                Key::Enter if !m.shift && self.active_view().is_some() => self.start_edit(None),
                Key::Delete => self.clear_selection(ctx),
                _ => self.navigate(key, m),
            }
        }
        if let Some(f) = copy.filter(|_| !editing) {
            self.copy(ctx, f);
        }
        if cut && !editing && !modal {
            self.run(ctx, Cmd::Cut);
        }
        // Ctrl + mouse wheel
        let zoom = ctx.input(|i| i.zoom_delta());
        if zoom != 1.0 && self.doc.is_some() {
            self.set_zoom(self.zoom() * zoom);
        }
    }

    fn navigate(&mut self, key: Key, m: Modifiers) {
        let Some(v) = self.active_view() else { return };
        let (rows, cols) = (v.sheet.rows.max(1), v.sheet.cols.max(1));
        let page = v.grid.visible_rows();
        let g = &mut v.grid;
        let big = i64::from(u32::MAX);
        let (dr, dc) = match key {
            Key::ArrowUp if m.command => (-big, 0),
            Key::ArrowDown if m.command => (big, 0),
            Key::ArrowLeft if m.command => (0, -big),
            Key::ArrowRight if m.command => (0, big),
            Key::ArrowUp => (-1, 0),
            Key::ArrowDown => (1, 0),
            Key::ArrowLeft => (0, -1),
            Key::ArrowRight => (0, 1),
            Key::Enter => (if m.shift { -1 } else { 1 }, 0),
            Key::Tab => (0, if m.shift { -1 } else { 1 }),
            Key::PageUp => (-page, 0),
            Key::PageDown => (page, 0),
            Key::Home if m.command => (-big, -big),
            Key::End if m.command => (big, big),
            Key::Home => (0, -big),
            Key::End => (0, big),
            Key::A if m.command => {
                g.selection = Selection {
                    anchor: CellPos::new(0, 0),
                    cursor: CellPos::new(rows - 1, cols - 1),
                };
                return;
            }
            _ => return,
        };
        let extend = m.shift && !matches!(key, Key::Enter | Key::Tab);
        g.selection.move_cursor(dr, dc, extend, rows, cols);
        g.scroll_to = Some(g.selection.cursor);
    }

    // ---- panels ------------------------------------------------------------

    /// File, Edit, View and Format, like every office program.
    fn menu_bar(&mut self, ui: &mut egui::Ui) -> Option<Cmd> {
        let mut cmd = None;
        let view = self.active_view().is_some();
        let doc = self.doc.is_some();
        let dirty = self.dirty();
        let st = self.cursor_style();
        let recent: Vec<PathBuf> = self.recent.existing().cloned().collect();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button(text::menu_file(), |ui| {
                item(ui, &mut cmd, text::open(), "Ctrl+O", true, Cmd::Open);
                ui.menu_button(text::recent(), |ui| recent_items(ui, &recent, &mut cmd));
                ui.separator();
                item(ui, &mut cmd, text::save(), "Ctrl+S", dirty, Cmd::Save);
                item(
                    ui,
                    &mut cmd,
                    text::save_as(),
                    "Ctrl+Shift+S",
                    doc,
                    Cmd::SaveAs,
                );
                ui.separator();
                item(ui, &mut cmd, text::exit(), "", true, Cmd::Exit);
            });
            ui.menu_button(text::menu_edit(), |ui| {
                item(ui, &mut cmd, text::undo(), "Ctrl+Z", view, Cmd::Undo);
                item(ui, &mut cmd, text::redo(), "Ctrl+Y", view, Cmd::Redo);
                ui.separator();
                edit_items(ui, &mut cmd, view);
                ui.separator();
                item(ui, &mut cmd, text::find(), "Ctrl+F", doc, Cmd::Find);
            });
            ui.menu_button(text::menu_view(), |ui| {
                let (up, down) = (self.zoom_step(true), self.zoom_step(false));
                item(ui, &mut cmd, text::zoom_in(), "Ctrl++", doc, Cmd::Zoom(up));
                item(
                    ui,
                    &mut cmd,
                    text::zoom_out(),
                    "Ctrl+-",
                    doc,
                    Cmd::Zoom(down),
                );
                item(
                    ui,
                    &mut cmd,
                    text::zoom_reset(),
                    "Ctrl+0",
                    doc,
                    Cmd::Zoom(1.0),
                );
            });
            ui.add_enabled_ui(view, |ui| {
                ui.menu_button(text::menu_format(), |ui| format_items(ui, &mut cmd, st));
            });
            ui.menu_button(text::menu_help(), |ui| {
                item(ui, &mut cmd, text::about(), "", true, Cmd::About);
            });
        });
        cmd
    }

    /// Menu, toolbar, formula bar and (when open) the search, top to bottom.
    fn top_bar(&mut self, ui: &mut egui::Ui) -> Option<Cmd> {
        let mut cmd = None;
        let mut start_in_bar = false;
        let mut leave_bar = false;
        egui::Panel::top("top").show(ui, |ui| {
            cmd = self.menu_bar(ui);
            self.format_bar(ui);
            if let Some(v) = self.active_view() {
                ui.horizontal(|ui| {
                    let cur = v.grid.selection.cursor;
                    let name = egui::RichText::new(cell_address(cur.row, cur.col)).monospace();
                    let address = ui.add_sized([56.0, 20.0], egui::Label::new(name));
                    ui.weak(text::formula_label());
                    let bar = |text| {
                        egui::TextEdit::singleline(text)
                            .id(grid::bar_id())
                            .desired_width(f32::INFINITY)
                    };
                    match &mut v.grid.edit {
                        // Editing: the bar edits the same text as the cell.
                        Some(e) => {
                            let resp = ui.add(bar(&mut e.text)).labelled_by(address.id);
                            if resp.has_focus() {
                                e.in_bar = true;
                            } else if e.in_bar && resp.lost_focus() {
                                leave_bar = true;
                            }
                        }
                        // Not editing: clicking into the bar starts editing there.
                        None => {
                            let mut content = v
                                .formulas
                                .get(&(cur.row, cur.col))
                                .cloned()
                                .unwrap_or_else(|| v.sheet.display(cur.row, cur.col).into_owned());
                            let resp = ui
                                .add(bar(&mut content))
                                .labelled_by(address.id)
                                .on_hover_text(text::formula_bar_hint());
                            if resp.gained_focus() {
                                start_in_bar = true;
                            }
                        }
                    }
                });
            }
            if self.search.open {
                self.search_bar(ui);
            }
            ui.add_space(2.0);
        });
        let ctx = ui.ctx().clone();
        if start_in_bar {
            self.start_edit(None);
            match self.active_view().and_then(|v| v.grid.edit.as_mut()) {
                Some(e) => {
                    e.in_bar = true;
                    e.focus = false;
                }
                None => ctx.memory_mut(|m| m.surrender_focus(grid::bar_id())),
            }
        }
        if leave_bar {
            self.commit_edit(&ctx, None);
        }
        cmd
    }

    /// Sheet tabs: click switches, double-click renames, right-click for more, "+" adds.
    fn sheet_tabs(&mut self, ui: &mut egui::Ui, cmd: &mut Option<Cmd>) {
        let Some(doc) = &self.doc else { return };
        let names = doc.names.clone();
        let active = doc.active;
        let many = names.len() > 1;
        for (i, name) in names.iter().enumerate() {
            if let Some((r, buf, focus)) = &mut self.rename
                && *r == i
            {
                let resp = ui.add(
                    egui::TextEdit::singleline(buf)
                        .id(egui::Id::unique("rename_sheet"))
                        .desired_width(120.0),
                );
                if std::mem::take(focus) {
                    resp.request_focus();
                } else if resp.lost_focus() {
                    let esc = ui.input(|i| i.key_pressed(Key::Escape));
                    if let Some((r, buf, _)) = self.rename.take() {
                        let new = buf.trim();
                        if !esc && !new.is_empty() && new != name {
                            *cmd = Some(Cmd::Rename(r, new.to_owned()));
                        }
                    }
                }
                continue;
            }
            let resp = ui
                .selectable_label(i == active, name)
                .on_hover_text(text::sheet_rename_hint());
            if resp.clicked() {
                *cmd = Some(Cmd::Switch(i));
            }
            if resp.double_clicked() {
                self.rename = Some((i, name.clone(), true));
            }
            resp.context_menu(|ui| {
                if ui.button(text::rename_sheet()).clicked() {
                    self.rename = Some((i, name.clone(), true));
                }
                if ui
                    .add_enabled(many, egui::Button::new(text::delete_sheet()))
                    .clicked()
                {
                    self.confirm_delete = Some(i);
                }
            });
        }
        if ui.button("+").on_hover_text(text::new_sheet()).clicked() {
            *cmd = Some(Cmd::NewSheet);
        }
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) -> Option<Cmd> {
        let now = ui.input(|i| i.time);
        // a message set without access to the clock starts now
        if let Some((_, t)) = &mut self.status
            && t.is_nan()
        {
            *t = now;
        }
        let mut cmd = None;
        let mut start_update = false;
        let zoom = self.zoom();
        egui::Panel::bottom("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                if self.doc.is_some() {
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs")
                        .max_width(ui.available_width() - 420.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| self.sheet_tabs(ui, &mut cmd));
                        });
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.doc.is_some() {
                        // Like Excel: − slider + and the percentage, which takes typing.
                        let mut pct = zoom * 100.0;
                        let field = ui
                            .add(
                                egui::DragValue::new(&mut pct)
                                    .range(grid::MIN_ZOOM * 100.0..=grid::MAX_ZOOM * 100.0)
                                    .speed(1.0)
                                    .max_decimals(0)
                                    .suffix(" %"),
                            )
                            .on_hover_text(text::zoom_hint());
                        if field.changed() {
                            cmd = Some(Cmd::Zoom(pct / 100.0));
                        }
                        if zoom_button(ui, "+", text::zoom_in()) {
                            cmd = Some(Cmd::Zoom(self.zoom_step(true)));
                        }
                        let slider = ui.add(
                            egui::Slider::new(
                                &mut pct,
                                grid::MIN_ZOOM * 100.0..=grid::MAX_ZOOM * 100.0,
                            )
                            .logarithmic(true)
                            .show_value(false),
                        );
                        slider.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Slider, true, "Zoom")
                        });
                        if slider.changed() {
                            cmd = Some(Cmd::Zoom(pct / 100.0));
                        }
                        if slider.double_clicked() {
                            cmd = Some(Cmd::Zoom(1.0));
                        }
                        if zoom_button(ui, "−", text::zoom_out()) {
                            cmd = Some(Cmd::Zoom(self.zoom_step(false)));
                        }
                    }
                    match &self.update {
                        Update::Ready(v) => {
                            start_update = ui.link(text::update_ready(v)).clicked();
                            ui.separator();
                        }
                        Update::Restarting(_) => {
                            ui.spinner();
                            ui.label(text::restarting());
                            ui.separator();
                        }
                        _ => {}
                    }
                    match &self.status {
                        Some((msg, t)) if now - t < STATUS_SECS => {
                            ui.label(msg);
                            ui.ctx()
                                .request_repaint_after(std::time::Duration::from_secs_f64(
                                    STATUS_SECS,
                                ));
                        }
                        _ => {
                            if let Some(v) = self.active_view() {
                                let n = v.grid.selection.cell_count();
                                if v.grid.edit.is_some() {
                                    ui.label(text::editing_hint());
                                } else if n > 1 {
                                    ui.weak(text::selected(n));
                                }
                            }
                        }
                    }
                });
            });
        });
        if start_update {
            self.restart_for_update(&ui.ctx().clone());
        }
        cmd
    }

    /// Right-click menus of the cells and the column and row headers.
    fn grid_menus(&mut self, out: &GridOut) -> Option<Cmd> {
        let mut cmd = None;
        let st = self.cursor_style();
        if let Some(r) = &out.body {
            r.context_menu(|ui| {
                edit_items(ui, &mut cmd, true);
                ui.separator();
                format_items(ui, &mut cmd, st);
            });
        }
        if let Some(r) = &out.col_hdr {
            r.context_menu(|ui| {
                item(
                    ui,
                    &mut cmd,
                    text::select_column(),
                    "",
                    true,
                    Cmd::SelectCols,
                );
                item(ui, &mut cmd, text::fit_width(), "", true, Cmd::FitCols);
                ui.separator();
                edit_items(ui, &mut cmd, true);
            });
        }
        if let Some(r) = &out.row_hdr {
            r.context_menu(|ui| {
                item(ui, &mut cmd, text::select_row(), "", true, Cmd::SelectRows);
                item(ui, &mut cmd, text::fit_height(), "", true, Cmd::FitRows);
                ui.separator();
                edit_items(ui, &mut cmd, true);
            });
        }
        cmd
    }

    fn delete_sheet_modal(&mut self, ctx: &egui::Context) {
        let Some(i) = self.confirm_delete else { return };
        let name = self
            .doc
            .as_ref()
            .and_then(|d| d.names.get(i).cloned())
            .unwrap_or_default();
        let mut delete = false;
        let resp = egui::Modal::new(egui::Id::unique("delete_sheet")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(text::delete_sheet_title());
            ui.label(text::delete_sheet_question(&name));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                delete = ui.button(text::delete()).clicked();
                if ui.button(text::cancel()).clicked() {
                    self.confirm_delete = None;
                }
            });
        });
        if resp.should_close() {
            self.confirm_delete = None;
        }
        if delete {
            self.confirm_delete = None;
            self.run(ctx, Cmd::DeleteSheet(i));
        }
    }

    fn poll_update(&mut self, ctx: &egui::Context) {
        match &self.update {
            Update::Downloading(rx) => match rx.try_recv() {
                Ok(found) => {
                    self.update = Update::None;
                    // Only a check the user started reports "current" or a failure.
                    let asked = matches!(self.manual_check, Some(ManualCheck::Checking));
                    self.manual_check = match found {
                        update::Found::Ready(v) => {
                            self.update = Update::Ready(v);
                            None
                        }
                        update::Found::Current => asked.then_some(ManualCheck::Current),
                        update::Found::Failed(e) => asked.then_some(ManualCheck::Failed(e)),
                    };
                }
                Err(TryRecvError::Disconnected) => self.update = Update::None,
                Err(TryRecvError::Empty) => {}
            },
            Update::Restarting(rx) => match rx.try_recv() {
                Ok(Ok(())) => {
                    self.allow_close = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                Ok(Err(e)) => {
                    self.update = Update::None;
                    self.update_failed(e);
                }
                Err(TryRecvError::Disconnected) => self.update = Update::None,
                Err(TryRecvError::Empty) => {}
            },
            _ => {}
        }
    }

    /// Swaps in the downloaded version and starts it with the same window, file and
    /// sheet; this process then closes. Feels like a reload.
    fn restart_for_update(&mut self, ctx: &egui::Context) {
        // Unsaved changes first: save or discard, then this runs again.
        if self.dirty() {
            self.confirm = Some(After::Restart);
            return;
        }
        let restore = ctx.input(|i| {
            let vp = i.viewport();
            let inner = vp.inner_rect?;
            Some(update::Restore {
                pos: vp.outer_rect.unwrap_or(inner).min.into(),
                size: inner.size().into(),
                maximized: vp.maximized.unwrap_or(false),
                sheet: self.doc.as_ref().map_or(0, |d| d.active),
            })
        });
        let args: Vec<_> = self
            .doc
            .iter()
            .map(|d| d.path.clone().into_os_string())
            .collect();
        // Starting a fresh exe can block for seconds while the virus scanner checks
        // it, so it runs off the UI thread; the window closes once the new one runs.
        let (tx, rx) = std::sync::mpsc::channel();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(update::restart(&args, restore));
            repaint.request_repaint();
        });
        self.update = Update::Restarting(rx);
    }

    fn update_failed(&mut self, e: String) {
        self.error = Some(e);
        self.error_title = Some(text::update_failed());
    }

    /// Starts a check the user asked for, or joins the one already running.
    fn check_now(&mut self, ctx: &egui::Context) {
        if matches!(self.update, Update::None) {
            match update::check(ctx.clone()) {
                Some(rx) => self.update = Update::Downloading(rx),
                None => {
                    self.manual_check = Some(ManualCheck::Failed(text::update_off().into()));
                    return;
                }
            }
        }
        self.manual_check = Some(ManualCheck::Checking);
    }

    fn about_modal(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let (mut check, mut restart) = (false, false);
        let resp = egui::Modal::new(egui::Id::unique("about")).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading(text::APP);
            ui.label(text::version(env!("CARGO_PKG_VERSION")));
            ui.hyperlink_to("github.com/10eputzen/ferrosheet", text::HOMEPAGE);
            ui.label(text::licence());
            ui.add_space(8.0);
            match (&self.update, &self.manual_check) {
                (Update::Ready(v), _) => {
                    restart = ui.button(text::update_ready(v)).clicked();
                }
                (Update::Restarting(_), _) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(text::restarting());
                    });
                }
                (_, Some(ManualCheck::Checking)) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(text::checking());
                    });
                }
                (_, found) => {
                    match found {
                        Some(ManualCheck::Current) => {
                            ui.label(text::up_to_date());
                        }
                        Some(ManualCheck::Failed(e)) => {
                            ui.label(format!("{}: {e}", text::check_failed()));
                        }
                        _ => {}
                    }
                    check = ui.button(text::check_updates()).clicked();
                }
            }
            ui.add_space(8.0);
            ui.button(text::ok()).clicked()
        });
        if check {
            self.check_now(ctx);
        }
        if restart {
            self.restart_for_update(ctx);
        }
        if resp.inner || resp.should_close() {
            self.about = false;
            self.manual_check = None;
        }
    }

    fn error_modal(&mut self, ctx: &egui::Context) {
        let Some(err) = &self.error else { return };
        let resp = egui::Modal::new(egui::Id::unique("error")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(self.error_title.unwrap_or_else(text::load_failed));
            ui.label(err);
            ui.add_space(8.0);
            ui.button(text::ok()).clicked()
        });
        if resp.inner || resp.should_close() {
            self.error = None;
            self.error_title = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(feature = "screenshot")]
        screenshot(&ctx);
        self.receive(&ctx);
        self.poll_engine(&ctx);
        self.poll_update(&ctx);

        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() && !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.confirm = Some(After::Close);
        }
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(p) = dropped {
            self.request_open(&ctx, p);
        }
        self.keyboard(&ctx);
        if self.search.dirty && self.search.open && !self.large_sheet() {
            self.run_search();
        }

        let mut cmd = self.top_bar(ui);
        if let Some(c) = self.bottom_bar(ui) {
            cmd = Some(c);
        }
        let zoom = self.zoom();
        let mut grid_out = None;
        egui::CentralPanel::no_frame().show(ui, |ui| match &mut self.doc {
            None => {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.25);
                    ui.weak(text::open_hint());
                    ui.add_space(8.0);
                    if ui.button(text::open_button()).clicked() {
                        cmd = Some(Cmd::Open);
                    }
                    let recent: Vec<_> = self.recent.existing().cloned().collect();
                    if !recent.is_empty() {
                        ui.add_space(16.0);
                        ui.strong(text::recent());
                        for p in recent {
                            let name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            if ui
                                .link(name)
                                .on_hover_text(p.display().to_string())
                                .clicked()
                            {
                                cmd = Some(Cmd::OpenRecent(p));
                            }
                        }
                    }
                });
            }
            Some(doc) => match doc.sheets.get_mut(doc.active).and_then(Option::as_mut) {
                Some(v) if v.sheet.rows == 0 => {
                    ui.centered_and_justified(|ui| ui.weak(text::empty_sheet()));
                }
                Some(v) => {
                    let active = doc.active;
                    let hits: Vec<CellPos> = if self.search.open {
                        self.search
                            .hits
                            .iter()
                            .filter(|h| h.0 == active)
                            .map(|h| h.1)
                            .collect()
                    } else {
                        Vec::new()
                    };
                    let current = self
                        .search
                        .current
                        .map(|i| self.search.hits[i])
                        .filter(|h| h.0 == active)
                        .map(|h| h.1);
                    grid_out = Some(grid::show(ui, &v.sheet, &mut v.grid, &hits, current, zoom));
                }
                None => {
                    ui.centered_and_justified(|ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(text::loading());
                        })
                    });
                }
            },
        });
        if let Some(out) = &grid_out {
            if out.edit_left {
                self.commit_edit(&ctx, None);
            } else if out.double_clicked && !self.editing_cell() {
                self.start_edit(None);
            }
            if let Some(c) = self.grid_menus(out) {
                cmd = Some(c);
            }
        }
        if let Some(c) = cmd {
            self.run(&ctx, c);
        }
        self.update_title(&ctx);
        self.confirm_modal(&ctx);
        self.delete_sheet_modal(&ctx);
        self.about_modal(&ctx);
        self.error_modal(&ctx);
    }
}

/// A menu entry with its shortcut; picking it sets `cmd`.
fn item(ui: &mut egui::Ui, cmd: &mut Option<Cmd>, label: &str, keys: &str, on: bool, c: Cmd) {
    let mut b = egui::Button::new(label);
    if !keys.is_empty() {
        b = b.shortcut_text(text::shortcut(keys));
    }
    if ui.add_enabled(on, b).clicked() {
        *cmd = Some(c);
    }
}

fn edit_items(ui: &mut egui::Ui, cmd: &mut Option<Cmd>, on: bool) {
    item(ui, cmd, text::cut(), "Ctrl+X", on, Cmd::Cut);
    item(
        ui,
        cmd,
        text::copy(),
        "Ctrl+C",
        on,
        Cmd::Copy(CopyFormat::Display),
    );
    let raw = Cmd::Copy(CopyFormat::Invariant);
    item(ui, cmd, text::copy_raw(), "Ctrl+Shift+C", on, raw);
    item(ui, cmd, text::clear_contents(), "Del", on, Cmd::Clear);
}

fn format_items(ui: &mut egui::Ui, cmd: &mut Option<Cmd>, st: CellStyle) {
    let bold = egui::Button::new(text::bold_name())
        .selected(st.bold)
        .shortcut_text(text::shortcut("Ctrl+B"));
    if ui.add(bold).clicked() {
        *cmd = Some(Cmd::Bold);
    }
    let italic = egui::Button::new(text::italic_name())
        .selected(st.italic)
        .shortcut_text(text::shortcut("Ctrl+I"));
    if ui.add(italic).clicked() {
        *cmd = Some(Cmd::Italic);
    }
    ui.separator();
    if let Some(c) = editing::colour_and_number_menus(ui) {
        *cmd = Some(Cmd::Style(c));
    }
}

fn recent_items(ui: &mut egui::Ui, recent: &[PathBuf], cmd: &mut Option<Cmd>) {
    if recent.is_empty() {
        ui.add_enabled(false, egui::Button::new(text::no_recent()));
    }
    for p in recent {
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if ui
            .button(name)
            .on_hover_text(p.display().to_string())
            .clicked()
        {
            *cmd = Some(Cmd::OpenRecent(p.clone()));
        }
    }
}

/// Screenshot builds: after a few frames (layout settled) save the window to
/// `FERROSHEET_SCREENSHOT` and close.
#[cfg(feature = "screenshot")]
fn screenshot(ctx: &egui::Context) {
    let Ok(path) = std::env::var("FERROSHEET_SCREENSHOT") else {
        return;
    };
    ctx.request_repaint();
    if ctx.cumulative_frame_nr() != 5 {
        return;
    }
    let ctx2 = ctx.clone();
    ctx.request_screenshot(move |img| {
        let [w, h] = img.size;
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
        image::save_buffer(&path, &bytes, w as u32, h as u32, image::ColorType::Rgba8)
            .expect("save screenshot");
        ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
    });
}

/// A small `+`/`−` zoom button, named for screen readers by what it does.
fn zoom_button(ui: &mut egui::Ui, glyph: &str, name: &'static str) -> bool {
    let r = ui.small_button(glyph).on_hover_text(name);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    r.clicked()
}

#[cfg(test)]
mod tests {
    //! Drive the whole app like a user would: open the demo workbook, press keys,
    //! copy, search, switch sheets. The loader runs on its own thread, so tests wait
    //! for it with [`settle`].

    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use eframe::egui::{Event, Key, Modifiers, OutputCommand};
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use ferrosheet_core::{CellPos, CellValue, Locale, Selection, Workbook};

    use super::{App, Cmd, Engine};
    use crate::{grid, text};
    use eframe::egui::accesskit::Role;

    fn demo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../ferrosheet-core/tests/fixtures/demo.xlsx")
    }

    fn open(path: PathBuf) -> Harness<'static, App> {
        let mut h = Harness::builder()
            .with_size([900.0, 600.0])
            .build_eframe(move |cc| App::new(&cc.egui_ctx, Some(path)));
        settle(&mut h, |app| {
            app.active_view().is_some_and(|v| v.formulas_loaded)
        });
        h
    }

    /// Steps frames until `done` holds; the background loader needs real time.
    fn settle(h: &mut Harness<'_, App>, mut done: impl FnMut(&mut App) -> bool) {
        h.step(); // process queued input before checking
        let start = Instant::now();
        while !done(h.state_mut()) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "app did not settle"
            );
            std::thread::sleep(Duration::from_millis(5));
            h.step();
        }
        h.run();
    }

    fn cursor(h: &mut Harness<'_, App>) -> CellPos {
        h.state_mut().active_view().unwrap().grid.selection.cursor
    }

    fn copied(h: &Harness<'_, App>) -> Option<String> {
        h.output()
            .platform_output
            .commands
            .iter()
            .find_map(|c| match c {
                OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
    }

    #[test]
    fn opens_the_first_sheet_with_values_and_formulas() {
        let mut h = open(demo());
        let doc = h.state().doc.as_ref().unwrap();
        assert_eq!(doc.names, ["Invoice", "Summary"]);
        let v = h.state_mut().active_view().unwrap();
        assert_eq!(v.sheet.display(0, 0), "Item");
        assert_eq!(v.sheet.display(2, 0), "Cable duct 2 m");
        assert_eq!(v.formulas[&(1, 3)], "=B2*C2*(1-G2)");
        assert_eq!(v.formulas[&(6, 3)], "=SUM(D2:D5)");
        assert!(h.state().error.is_none());
    }

    #[test]
    fn arrow_keys_move_and_shift_extends_the_selection() {
        let mut h = open(demo());
        h.key_press(Key::ArrowDown);
        h.key_press(Key::ArrowRight);
        h.run();
        assert_eq!(cursor(&mut h), CellPos::new(1, 1));

        h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowDown);
        h.key_press_modifiers(Modifiers::SHIFT, Key::ArrowRight);
        h.run();
        let sel = h.state_mut().active_view().unwrap().grid.selection;
        assert_eq!(sel.anchor, CellPos::new(1, 1));
        assert_eq!(sel.cell_count(), 4);

        h.key_press_modifiers(Modifiers::COMMAND, Key::End);
        h.run();
        let (rows, cols) = {
            let v = h.state_mut().active_view().unwrap();
            (v.sheet.rows, v.sheet.cols)
        };
        assert_eq!(cursor(&mut h), CellPos::new(rows - 1, cols - 1));
    }

    #[test]
    fn select_all_and_copy_puts_tab_separated_text_on_the_clipboard() {
        let mut h = open(demo());
        h.state_mut().active_view().unwrap().grid.selection = Selection {
            anchor: CellPos::new(0, 0),
            cursor: CellPos::new(1, 1),
        };
        h.event(Event::Copy);
        h.step();
        assert_eq!(copied(&h).as_deref(), Some("Item\tQty\r\nScrews M4\t120"));

        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.run();
        let v = h.state_mut().active_view().unwrap();
        let all = u64::from(v.sheet.rows) * u64::from(v.sheet.cols);
        assert_eq!(v.grid.selection.cell_count(), all);
    }

    #[test]
    fn ctrl_page_down_switches_to_the_next_sheet_and_loads_it() {
        let mut h = open(demo());
        h.key_press_modifiers(Modifiers::COMMAND, Key::PageDown);
        settle(&mut h, |app| {
            app.doc.as_ref().is_some_and(|d| d.active == 1) && app.active_view().is_some()
        });
        let v = h.state_mut().active_view().unwrap();
        assert_eq!(v.sheet.display(0, 0), "Linked to the first sheet");

        // switching past the last sheet stays on it
        h.key_press_modifiers(Modifiers::COMMAND, Key::PageDown);
        h.run();
        assert_eq!(h.state().doc.as_ref().unwrap().active, 1);
        h.key_press_modifiers(Modifiers::COMMAND, Key::PageUp);
        h.run();
        assert_eq!(h.state().doc.as_ref().unwrap().active, 0);
    }

    #[test]
    fn typing_in_the_search_jumps_to_the_hit_and_f3_cycles() {
        let mut h = open(demo());
        h.key_press_modifiers(Modifiers::COMMAND, Key::F);
        h.run();
        assert!(h.state().search.open);
        h.event(Event::Text("power".into()));
        h.run();
        assert_eq!(h.state().search.hits.len(), 1);
        assert_eq!(cursor(&mut h), CellPos::new(3, 0));
        h.get_by_label(&text::hit_counter(1, 1));

        // a query with several hits: F3 moves on, Shift+F3 back
        for _ in 0.."power".len() {
            h.key_press(Key::Backspace);
        }
        h.event(Event::Text("m".into()));
        h.run();
        assert_eq!(h.state().search.query, "m");
        let n = h.state().search.hits.len();
        assert!(n > 2, "expected several hits, got {n}");
        let first = cursor(&mut h);
        h.key_press(Key::Escape); // leave the text field, closes the search
        h.run();
        assert!(!h.state().search.open);
        h.key_press(Key::F3);
        h.run();
        assert_ne!(cursor(&mut h), first);
        h.key_press_modifiers(Modifiers::SHIFT, Key::F3);
        h.run();
        assert_eq!(cursor(&mut h), first);
    }

    #[test]
    fn regex_search_finds_patterns_and_an_invalid_one_shows_a_hint() {
        let mut h = open(demo());
        h.key_press_modifiers(Modifiers::COMMAND, Key::F);
        h.run();
        h.get_by_label(text::regex()).click();
        h.run();
        assert!(h.state().search.regex);
        h.state_mut().search.focus = true;
        h.run();
        h.event(Event::Text("^pow.r".into()));
        h.run();
        assert_eq!(h.state().search.hits.len(), 1);
        assert_eq!(cursor(&mut h), CellPos::new(3, 0));

        h.event(Event::Text("(".into()));
        h.run();
        assert!(h.state().search.hits.is_empty());
        assert!(h.state().search.error.is_some());
        h.get_by_label(text::invalid_regex());
    }

    #[test]
    fn a_damaged_file_shows_an_error_instead_of_crashing() {
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../ferrosheet-core/tests/fixtures/date.xls"),
        )
        .unwrap();
        let dir = std::env::temp_dir().join("ferrosheet-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app-damaged.xls");
        std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();

        let mut h = Harness::builder()
            .with_size([900.0, 600.0])
            .build_eframe(move |cc| App::new(&cc.egui_ctx, Some(path)));
        settle(&mut h, |app| app.error.is_some());
        h.get_by_label(text::load_failed());
        h.get_by_label(text::ok()).click();
        h.run();
        assert!(h.state().error.is_none());
    }

    #[test]
    fn a_missing_file_shows_an_error() {
        let mut h = Harness::builder()
            .with_size([900.0, 600.0])
            .build_eframe(|cc| App::new(&cc.egui_ctx, Some(PathBuf::from("gibt-es-nicht.xlsx"))));
        settle(&mut h, |app| app.error.is_some());
        h.get_by_label(text::load_failed());
    }

    /// A copy of a fixture in a fresh temp folder, so saving never touches the original.
    fn scratch_copy(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join("ferrosheet-tests").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn value(h: &mut Harness<'_, App>, r: u32, c: u32) -> CellValue {
        h.state_mut()
            .active_view()
            .unwrap()
            .sheet
            .value(r, c)
            .clone()
    }

    fn select(h: &mut Harness<'_, App>, r: u32, c: u32) {
        h.state_mut().active_view().unwrap().grid.selection = Selection::single(CellPos::new(r, c));
        h.run();
    }

    #[test]
    fn typing_on_a_cell_edits_it_and_formulas_follow() {
        let mut h = open(demo());
        let CellValue::Number(d2) = value(&mut h, 1, 3) else {
            panic!("D2 is a number")
        };
        select(&mut h, 1, 1); // B2, quantity 120
        h.event(Event::Text("10".into()));
        h.run();
        let edit = h
            .state_mut()
            .active_view()
            .unwrap()
            .grid
            .edit
            .as_ref()
            .map(|e| e.text.clone());
        assert_eq!(edit.as_deref(), Some("10"));
        // the editor has the keyboard, so the text cursor shows
        assert!(h.ctx.memory(|m| m.has_focus(grid::edit_id())));
        h.key_press(Key::Enter);
        settle(&mut h, |app| {
            app.active_view().unwrap().sheet.value(1, 1) == &CellValue::Number(10.0)
        });
        assert_eq!(cursor(&mut h), CellPos::new(2, 1));
        let CellValue::Number(new) = value(&mut h, 1, 3) else {
            panic!("D2 is still a number")
        };
        assert!((new - d2 * 10.0 / 120.0).abs() < 1e-9, "D2 {d2} -> {new}");
        assert!(h.state().dirty());

        // Ctrl+Z brings the old value back, Ctrl+Y the new one
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        settle(&mut h, |app| {
            app.active_view().unwrap().sheet.value(1, 1) == &CellValue::Number(120.0)
        });
        h.key_press_modifiers(Modifiers::COMMAND, Key::Y);
        settle(&mut h, |app| {
            app.active_view().unwrap().sheet.value(1, 1) == &CellValue::Number(10.0)
        });
    }

    #[test]
    fn escape_cancels_and_delete_clears_the_selection() {
        let mut h = open(demo());
        select(&mut h, 1, 1);
        h.key_press(Key::F2);
        h.run();
        let edit = h
            .state_mut()
            .active_view()
            .unwrap()
            .grid
            .edit
            .as_ref()
            .map(|e| e.text.clone());
        assert_eq!(edit.as_deref(), Some("120"));
        h.key_press(Key::Escape);
        h.run();
        assert!(h.state_mut().active_view().unwrap().grid.edit.is_none());
        assert!(!h.state().dirty());

        h.state_mut().active_view().unwrap().grid.selection = Selection {
            anchor: CellPos::new(1, 1),
            cursor: CellPos::new(2, 2),
        };
        h.key_press(Key::Delete);
        settle(&mut h, |app| {
            app.active_view().unwrap().sheet.value(2, 2) == &CellValue::Empty
        });
        assert_eq!(value(&mut h, 1, 1), CellValue::Empty);
    }

    #[test]
    fn ctrl_s_saves_an_xlsx_in_place_with_values_and_formulas() {
        let path = scratch_copy("save.xlsx", &std::fs::read(demo()).unwrap());
        let mut h = open(path.clone());
        select(&mut h, 1, 1);
        h.event(Event::Text("7".into()));
        h.run();
        h.key_press(Key::Enter);
        h.key_press_modifiers(Modifiers::COMMAND, Key::S);
        settle(&mut h, |app| !app.dirty());

        let mut wb = Workbook::open(&path, Locale::Invariant).unwrap();
        let sheet = wb.load_sheet(0).unwrap();
        assert_eq!(sheet.value(1, 1), &CellValue::Number(7.0));
        let formulas = wb.load_formulas(0).unwrap();
        assert_eq!(formulas[&(1, 3)], "=B2*C2*(1-G2)");
    }

    #[test]
    fn a_csv_is_saved_with_its_own_delimiter_and_decimal_comma() {
        let path = scratch_copy("save.csv", "Name;Wert\r\nA;1,5\r\nB;2\r\n".as_bytes());
        let mut h = open(path.clone());
        select(&mut h, 2, 1);
        h.event(Event::Text("3.25".into()));
        h.run();
        h.key_press(Key::Enter);
        h.key_press_modifiers(Modifiers::COMMAND, Key::S);
        settle(&mut h, |app| !app.dirty());
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(saved, "Name;Wert\r\nA;1,5\r\nB;3,25\r\n");
    }

    #[test]
    fn opening_another_file_with_unsaved_changes_asks_first() {
        let mut h = open(demo());
        select(&mut h, 1, 1);
        h.event(Event::Text("1".into()));
        h.run();
        h.key_press(Key::Enter);
        settle(&mut h, |app| {
            app.active_view().unwrap().sheet.value(1, 1) == &CellValue::Number(1.0)
        });
        // opening another file asks first
        let ctx = h.ctx.clone();
        h.state_mut().request_open(&ctx, demo());
        h.run();
        h.get_by_label(&text::save_question("demo.xlsx"));
        h.get_by_label(text::cancel()).click();
        h.run();
        assert!(h.state().confirm.is_none());
        assert!(h.state().dirty());
    }

    #[test]
    fn ctrl_b_makes_the_cell_bold_and_undo_takes_it_back() {
        let mut h = open(demo());
        select(&mut h, 1, 1);
        h.key_press_modifiers(Modifiers::COMMAND, Key::B);
        settle(&mut h, |app| {
            let v = app.active_view().unwrap();
            v.sheet.style(1, 1).bold
        });
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        settle(&mut h, |app| {
            let v = app.active_view().unwrap();
            !v.sheet.style(1, 1).bold
        });
    }

    #[test]
    fn f2_on_a_formula_cell_edits_the_formula_not_its_result() {
        let mut h = open(demo());
        select(&mut h, 1, 3); // D2 holds a formula
        h.key_press(Key::F2);
        h.run();
        let v = h.state_mut().active_view().unwrap();
        let formula = v.formulas.get(&(1, 3)).cloned().expect("D2 has a formula");
        assert!(formula.starts_with('='), "{formula}");
        assert_eq!(v.grid.edit.as_ref().map(|e| e.text.clone()), Some(formula));
        h.key_press(Key::Escape);
        h.run();
    }

    #[test]
    fn help_about_shows_the_version_and_checks_for_updates() {
        let mut h = open(demo());
        h.get_by_label("Help").click();
        h.run();
        h.get_by_label_contains("About").click();
        h.run();
        h.get_by_label(&text::version(env!("CARGO_PKG_VERSION")));
        // Test builds never update, so the check says so instead of failing silently.
        h.get_by_label(text::check_updates()).click();
        h.run();
        h.get_by_label_contains(text::update_off());
        h.get_by_label(text::ok()).click();
        h.run();
        assert!(!h.state().about);
    }

    #[test]
    fn the_edit_menu_copies_and_the_format_menu_makes_bold() {
        let mut h = open(demo());
        select(&mut h, 1, 1);
        h.get_by_label("Edit").click();
        h.run();
        h.get_all_by_label_contains("Copy").next().unwrap().click();
        h.run();
        let status = h.state().status.as_ref().map(|s| s.0.clone());
        assert_eq!(status, Some(text::copied(1)));
        h.get_by_label("Format").click();
        h.run();
        h.get_by_label_contains("Bold").click();
        settle(&mut h, |app| {
            app.active_view().unwrap().sheet.style(1, 1).bold
        });
    }

    fn run(h: &mut Harness<'_, App>, cmd: Cmd) {
        let ctx = h.ctx.clone();
        h.state_mut().run(&ctx, cmd);
        h.run();
    }

    fn names(h: &Harness<'_, App>) -> Vec<String> {
        h.state().doc.as_ref().unwrap().names.clone()
    }

    #[test]
    fn sheets_are_added_renamed_deleted_and_undo_brings_one_back() {
        let mut h = open(demo());
        let before = names(&h);
        let n = before.len();
        h.get_by_label("+").click();
        settle(&mut h, |app| app.doc.as_ref().unwrap().names.len() == n + 1);
        assert_eq!(h.state().doc.as_ref().unwrap().active, n);
        assert!(h.state_mut().active_view().is_some());

        run(&mut h, Cmd::Rename(n, "Notes".into()));
        settle(&mut h, |app| app.doc.as_ref().unwrap().names[n] == "Notes");

        // Delete the first sheet; a sheet not loaded yet still loads by its file index.
        run(&mut h, Cmd::DeleteSheet(0));
        settle(&mut h, |app| app.doc.as_ref().unwrap().names.len() == n);
        assert_eq!(names(&h)[0], before[1]);
        run(&mut h, Cmd::Switch(0));
        settle(&mut h, |app| app.active_view().is_some());
        assert_eq!(h.state_mut().active_view().unwrap().sheet.name, before[1]);

        // Ctrl+Z brings the deleted sheet back with its values.
        h.key_press_modifiers(Modifiers::COMMAND, Key::Z);
        settle(&mut h, |app| {
            app.doc.as_ref().unwrap().names[0] == before[0]
        });
        run(&mut h, Cmd::Switch(0));
        assert_eq!(value(&mut h, 1, 1), CellValue::Number(120.0));
    }

    #[test]
    fn deleting_a_sheet_asks_first() {
        let mut h = open(demo());
        let n = names(&h).len();
        h.state_mut().confirm_delete = Some(1);
        h.run();
        h.get_by_label("Cancel").click();
        h.run();
        assert_eq!(names(&h).len(), n);
        h.state_mut().confirm_delete = Some(1);
        h.run();
        h.get_by_label("Delete").click();
        settle(&mut h, |app| app.doc.as_ref().unwrap().names.len() == n - 1);
    }

    #[test]
    fn ctrl_plus_minus_and_zero_zoom_the_grid() {
        let mut h = open(demo());
        h.run();
        let w = h.state_mut().active_view().unwrap().grid.col_widths[0];
        h.key_press_modifiers(Modifiers::COMMAND, Key::Plus);
        h.run();
        assert_eq!(h.state().zoom(), 1.1);
        let zoomed = h.state_mut().active_view().unwrap().grid.col_widths[0];
        assert!((zoomed - w * 1.1).abs() < 0.01, "{w} -> {zoomed}");
        h.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
        h.key_press_modifiers(Modifiers::COMMAND, Key::Minus);
        h.run();
        assert_eq!(h.state().zoom(), 0.9);
        // The status bar: − slider + like Excel.
        h.get_by_label(text::zoom_in()).click();
        h.run();
        assert_eq!(h.state().zoom(), 1.0);
        let back = h.state_mut().active_view().unwrap().grid.col_widths[0];
        assert!((back - w).abs() < 0.01);
    }

    #[test]
    fn the_start_screen_lists_recent_files() {
        let mut h = Harness::builder()
            .with_size([900.0, 600.0])
            .build_eframe(|cc| App::new(&cc.egui_ctx, None));
        h.state_mut().recent.push(&demo());
        h.run();
        h.get_by_label("demo.xlsx").click();
        settle(&mut h, |app| app.active_view().is_some());
        assert!(h.state().recent.files[0].ends_with("demo.xlsx"));
    }

    #[test]
    fn clicking_into_the_formula_bar_edits_the_cell() {
        let mut h = open(demo());
        select(&mut h, 1, 1);
        h.get_by_role_and_label(Role::TextInput, "B2").click();
        h.run();
        assert!(h.ctx.memory(|m| m.has_focus(grid::bar_id())));
        let edit = h
            .state_mut()
            .active_view()
            .unwrap()
            .grid
            .edit
            .as_ref()
            .map(|e| e.in_bar);
        assert_eq!(edit, Some(true));
        h.key_press_modifiers(Modifiers::COMMAND, Key::A);
        h.event(Event::Text("55".into()));
        h.run();
        h.key_press(Key::Enter);
        settle(&mut h, |app| {
            app.active_view().unwrap().sheet.value(1, 1) == &CellValue::Number(55.0)
        });
        assert!(h.state_mut().active_view().unwrap().grid.edit.is_none());
        assert_eq!(cursor(&mut h), CellPos::new(2, 1));
    }

    #[test]
    fn a_file_that_cannot_be_edited_says_why() {
        let mut h = open(demo());
        h.state_mut().doc.as_mut().unwrap().engine = Engine::Failed("broken".into());
        h.key_press(Key::F2);
        h.run();
        assert!(h.state_mut().active_view().unwrap().grid.edit.is_none());
        h.get_by_label(text::edit_failed());
        assert!(h.state().error.as_deref().unwrap().contains("broken"));
    }
}
