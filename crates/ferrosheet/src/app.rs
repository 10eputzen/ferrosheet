//! Application state and the frame-by-frame UI.

use std::collections::HashMap;
use std::path::PathBuf;

use eframe::egui::{self, Event, Key, Modifiers, ViewportCommand};
use ferrosheet_core::{
    CellPos, CopyFormat, EXTENSIONS, Selection, Sheet, cell_address, copy_text, find_all,
};

use crate::grid::{self, GridState};
use crate::loader::{LoadMsg, Loader};
use crate::text;

/// Above this many cells the search runs on Enter instead of while typing.
const LIVE_SEARCH_MAX_CELLS: u64 = 200_000;
const STATUS_SECS: f64 = 2.0;

struct SheetView {
    sheet: Sheet,
    formulas: HashMap<(u32, u32), String>,
    grid: GridState,
}

struct Doc {
    loader: Loader,
    names: Vec<String>,
    sheets: Vec<Option<SheetView>>,
    active: usize,
}

#[derive(Default)]
struct Search {
    open: bool,
    focus: bool,
    query: String,
    all_sheets: bool,
    /// Sorted by (sheet, row, col).
    hits: Vec<(usize, CellPos)>,
    current: Option<usize>,
    dirty: bool,
}

#[derive(Default)]
pub struct App {
    doc: Option<Doc>,
    error: Option<String>,
    search: Search,
    status: Option<(String, f64)>,
}

impl App {
    pub fn new(ctx: &egui::Context, path: Option<PathBuf>) -> Self {
        let mut app = App::default();
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

    fn open(&mut self, ctx: &egui::Context, path: PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        ctx.send_viewport_cmd(ViewportCommand::Title(format!("{name} – {}", text::APP)));
        self.doc = Some(Doc {
            loader: Loader::open(path, ctx.clone()),
            names: Vec::new(),
            sheets: Vec::new(),
            active: 0,
        });
        self.search.hits.clear();
        self.search.current = None;
        self.search.dirty = true;
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter(text::file_filter(), EXTENSIONS)
            .pick_file()
        {
            self.open(ctx, p);
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let Some(doc) = &mut self.doc else { return };
        for msg in doc.loader.poll() {
            match msg {
                LoadMsg::Opened { names } => {
                    doc.sheets = names.iter().map(|_| None).collect();
                    doc.names = names;
                }
                LoadMsg::Sheet { idx, sheet } => {
                    doc.sheets[idx] = Some(SheetView {
                        sheet,
                        formulas: HashMap::new(),
                        grid: GridState::default(),
                    });
                    self.search.dirty = true;
                }
                LoadMsg::Formulas { idx, map } => {
                    if let Some(v) = &mut doc.sheets[idx] {
                        v.formulas = map;
                    }
                }
                // A sheet that failed shows as empty; the dialog says why.
                LoadMsg::Error {
                    sheet: Some(idx),
                    msg,
                } => {
                    doc.sheets[idx] = Some(SheetView {
                        sheet: Sheet::empty(doc.names[idx].clone()),
                        formulas: HashMap::new(),
                        grid: GridState::default(),
                    });
                    self.error = Some(msg);
                }
                // The file could not be opened at all: back to the start screen.
                LoadMsg::Error { sheet: None, msg } => {
                    self.doc = None;
                    self.error = Some(msg);
                    ctx.send_viewport_cmd(ViewportCommand::Title(text::APP.into()));
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
            doc.loader.request_sheet(idx);
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
        let Some(doc) = &self.doc else { return };
        if s.query.is_empty() {
            return;
        }
        for (idx, view) in doc.sheets.iter().enumerate() {
            if !s.all_sheets && idx != doc.active {
                continue;
            }
            if let Some(v) = view {
                s.hits
                    .extend(find_all(&v.sheet, &s.query).into_iter().map(|p| (idx, p)));
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
                    (0..doc.names.len())
                        .filter(|&i| doc.sheets[i].is_none())
                        .for_each(|i| doc.loader.request_sheet(i));
                }
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
            ui.weak(label);
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
        let mut actions = Vec::new();
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    Event::Copy => copy = Some(CopyFormat::Display),
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

        for (key, m) in actions {
            let ctrl = m.command;
            match key {
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
                _ if !editing => self.navigate(key, m),
                _ => {}
            }
        }
        if let Some(f) = copy.filter(|_| !editing) {
            self.copy(ctx, f);
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

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                if ui.button(text::open()).clicked() {
                    self.open_dialog(ui.ctx());
                }
                if let Some(v) = self.active_view() {
                    let cur = v.grid.selection.cursor;
                    let content = v
                        .formulas
                        .get(&(cur.row, cur.col))
                        .cloned()
                        .unwrap_or_else(|| v.sheet.display(cur.row, cur.col).to_owned());
                    ui.separator();
                    ui.monospace(cell_address(cur.row, cur.col));
                    let mut s = content.as_str();
                    ui.add(egui::TextEdit::singleline(&mut s).desired_width(f32::INFINITY));
                }
            });
            if self.search.open {
                self.search_bar(ui);
            }
            ui.add_space(2.0);
        });
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        let now = ui.input(|i| i.time);
        egui::Panel::bottom("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut switch = None;
                if let Some(doc) = &self.doc {
                    egui::ScrollArea::horizontal()
                        .id_salt("tabs")
                        .max_width(ui.available_width() - 200.0)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, name) in doc.names.iter().enumerate() {
                                    if ui.selectable_label(i == doc.active, name).clicked() {
                                        switch = Some(i);
                                    }
                                }
                            });
                        });
                }
                if let Some(i) = switch {
                    self.switch_sheet(i);
                }
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| match &self.status {
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
                                if n > 1 {
                                    ui.weak(text::selected(n));
                                }
                            }
                        }
                    },
                );
            });
        });
    }

    fn error_modal(&mut self, ctx: &egui::Context) {
        let Some(err) = &self.error else { return };
        let resp = egui::Modal::new(egui::Id::unique("error")).show(ctx, |ui| {
            ui.set_max_width(420.0);
            ui.heading(text::load_failed());
            ui.label(err);
            ui.add_space(8.0);
            ui.button(text::ok()).clicked()
        });
        if resp.inner || resp.should_close() {
            self.error = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(feature = "screenshot")]
        screenshot(&ctx);
        self.receive(&ctx);

        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(p) = dropped {
            self.open(&ctx, p);
        }
        self.keyboard(&ctx);
        if self.search.dirty && self.search.open && !self.large_sheet() {
            self.run_search();
        }

        self.top_bar(ui);
        self.bottom_bar(ui);
        egui::CentralPanel::no_frame().show(ui, |ui| match &mut self.doc {
            None => {
                ui.centered_and_justified(|ui| ui.weak(text::open_hint()));
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
                    grid::show(ui, &v.sheet, &mut v.grid, &hits, current);
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
        self.error_modal(&ctx);
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
    use ferrosheet_core::{CellPos, Selection};

    use super::App;
    use crate::text;

    fn demo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../ferrosheet-core/tests/fixtures/demo.xlsx")
    }

    fn open(path: PathBuf) -> Harness<'static, App> {
        let mut h = Harness::builder()
            .with_size([900.0, 600.0])
            .build_eframe(move |cc| App::new(&cc.egui_ctx, Some(path)));
        settle(&mut h, |app| {
            app.active_view().is_some_and(|v| !v.formulas.is_empty())
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
}
