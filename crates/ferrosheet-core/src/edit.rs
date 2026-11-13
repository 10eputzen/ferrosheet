//! Editing: an IronCalc model behind the displayed sheets. It recalculates formulas,
//! keeps the undo history, holds cell styles and writes `.xlsx`.
//!
//! The viewer never needs it; it is built on the first edit. Everything IronCalc-specific
//! stays in this module, so the engine could be swapped later.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use ironcalc::base::cell::CellValue as IcValue;
use ironcalc::base::expressions::types::Area;
use ironcalc::base::types::Color;
use ironcalc::base::{Model, UserModel};

use crate::{CellError, CellPos, CellStyle, CellValue, Locale, Query, Selection, Sheet};

/// Number formats offered in the format bar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberFormat {
    General,
    Decimal2,
    Currency,
    Percent,
    Date,
}

impl NumberFormat {
    pub const ALL: [NumberFormat; 5] = [
        NumberFormat::General,
        NumberFormat::Decimal2,
        NumberFormat::Currency,
        NumberFormat::Percent,
        NumberFormat::Date,
    ];

    fn code(self, locale: Locale) -> &'static str {
        match (self, locale) {
            (NumberFormat::General, _) => "general",
            (NumberFormat::Decimal2, _) => "#,##0.00",
            (NumberFormat::Currency, _) => "#,##0.00 \"€\"",
            (NumberFormat::Percent, _) => "0.00%",
            (NumberFormat::Date, Locale::German) => "dd.mm.yyyy",
            (NumberFormat::Date, Locale::Invariant) => "yyyy-mm-dd",
        }
    }
}

/// One change to the look of the selected cells.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StyleChange {
    Bold(bool),
    Italic(bool),
    Color(Option<[u8; 3]>),
    Fill(Option<[u8; 3]>),
    Number(NumberFormat),
}

/// Current state of one cell after a change, ready for [`Sheet::set`].
#[derive(Clone, Debug, PartialEq)]
pub struct CellUpdate {
    pub row: u32,
    pub col: u32,
    pub value: CellValue,
    pub formatted: Option<String>,
    pub style: CellStyle,
}

/// Rows or columns, for inserting and deleting whole lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lines {
    Rows,
    Cols,
}

/// Formulas of a sheet by (row, col), each starting with `=`.
type Formulas = HashMap<(u32, u32), String>;

/// One entry of the model's undo history, as far as the display cares: whether it moved
/// the cells of a sheet (by its id), so the whole sheet has to be shown anew.
#[derive(Clone, Copy)]
enum Step {
    Cells,
    Moved(u32),
}

pub struct EditSession {
    model: UserModel<'static>,
    locale: Locale,
    /// Per sheet id: cells with a formula plus every cell changed here. After any change
    /// these are the only cells whose shown value can differ from the model.
    watched: HashMap<u32, BTreeSet<(u32, u32)>>,
    /// Mirrors the model's undo and redo stacks: what each step did and how many model
    /// entries it took (a replace over many cells is one step for the user).
    undo_steps: Vec<(Step, usize)>,
    redo_steps: Vec<(Step, usize)>,
    /// Sheet ids whose rows or columns moved since [`EditSession::take_moved`].
    moved: BTreeSet<u32>,
}

fn locale_id(locale: Locale) -> &'static str {
    match locale {
        Locale::German => "de",
        Locale::Invariant => "en",
    }
}

// Formulas are always written and shown in English, as the formula bar shows them.
const LANGUAGE: &str = "en";
const TIMEZONE: &str = "UTC";

impl EditSession {
    /// Loads an `.xlsx`/`.xlsm` file with everything IronCalc understands (styles,
    /// names, widths), so saving keeps as much as possible.
    pub fn open_xlsx(path: &Path, locale: Locale) -> Result<EditSession, String> {
        let file = path.to_str().ok_or("path is not valid Unicode")?;
        let model = ironcalc::import::load_from_xlsx(file, locale_id(locale), TIMEZONE, LANGUAGE)
            .map_err(|e| e.to_string())?;
        Ok(EditSession::new(model, locale))
    }

    /// Builds the model from sheets loaded by the viewer (xls, xlsb, ods, csv): values
    /// and formulas, dates keep a date format, no other styles.
    pub fn from_sheets(
        sheets: &[(&Sheet, &Formulas)],
        locale: Locale,
    ) -> Result<EditSession, String> {
        let mut model = Model::new_empty("Book", locale_id(locale), TIMEZONE, LANGUAGE)?;
        for (i, (sheet, formulas)) in sheets.iter().enumerate() {
            let i = i as u32;
            if i == 0 {
                model.rename_sheet_by_index(0, &sheet.name)?;
            } else {
                model.add_sheet(&sheet.name)?;
            }
            for r in 0..sheet.rows {
                for c in 0..sheet.cols {
                    let (row, col) = (r as i32 + 1, c as i32 + 1);
                    if let Some(f) = formulas.get(&(r, c)) {
                        model.update_cell_with_formula(i, row, col, f.clone())?;
                        continue;
                    }
                    match sheet.value(r, c) {
                        CellValue::Empty => {}
                        CellValue::Text(s) => model.update_cell_with_text(i, row, col, s)?,
                        CellValue::Bool(b) => model.update_cell_with_bool(i, row, col, *b)?,
                        CellValue::Number(n) | CellValue::Duration(n) => {
                            model.update_cell_with_number(i, row, col, *n)?
                        }
                        CellValue::DateTime(n) => {
                            model.update_cell_with_number(i, row, col, *n)?;
                            let mut style = model.get_style_for_cell(i, row, col)?;
                            style.num_fmt = NumberFormat::Date.code(locale).into();
                            model.set_cell_style(i, row, col, &style)?;
                        }
                        CellValue::Error(e) => {
                            model.update_cell_with_text(i, row, col, e.text(Locale::Invariant))?
                        }
                    }
                }
            }
        }
        model.evaluate();
        Ok(EditSession::new(model, locale))
    }

    fn new(model: Model<'static>, locale: Locale) -> EditSession {
        let watched = model
            .workbook
            .worksheets
            .iter()
            .map(|ws| {
                let cells = ws
                    .sheet_data
                    .cells()
                    .filter(|(_, _, cell)| cell.has_formula())
                    .map(|(r, c, _)| (r as u32 - 1, c as u32 - 1))
                    .collect();
                (ws.sheet_id, cells)
            })
            .collect();
        EditSession {
            model: UserModel::from_model(model),
            locale,
            watched,
            undo_steps: Vec::new(),
            redo_steps: Vec::new(),
            moved: BTreeSet::new(),
        }
    }

    fn done(&mut self, step: Step) {
        self.done_n(step, 1);
    }

    fn done_n(&mut self, step: Step, entries: usize) {
        self.undo_steps.push((step, entries));
        self.redo_steps.clear();
        if let Step::Moved(id) = step {
            self.relayout(id);
        }
    }

    /// Cells moved: formulas now sit elsewhere, and the sheet is shown anew.
    fn relayout(&mut self, id: u32) {
        let model = self.model.get_model();
        let Some(ws) = model.workbook.worksheets.iter().find(|w| w.sheet_id == id) else {
            return;
        };
        let formulas: Vec<_> = ws
            .sheet_data
            .cells()
            .filter(|(_, _, cell)| cell.has_formula())
            .map(|(r, c, _)| (r as u32 - 1, c as u32 - 1))
            .collect();
        self.watched.entry(id).or_default().extend(formulas);
        self.moved.insert(id);
    }

    /// Sheets (by position) whose rows or columns moved since the last call; show them
    /// anew from [`EditSession::snapshot`].
    pub fn take_moved(&mut self) -> Vec<usize> {
        let ids = self.sheet_ids();
        std::mem::take(&mut self.moved)
            .into_iter()
            .filter_map(|id| ids.iter().position(|&i| i == id))
            .collect()
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.model.get_model().workbook.get_worksheet_names()
    }

    /// Stable ids of the sheets in order; they survive renames, moves and undo.
    pub fn sheet_ids(&self) -> Vec<u32> {
        let ws = &self.model.get_model().workbook.worksheets;
        ws.iter().map(|w| w.sheet_id).collect()
    }

    fn sheet_id(&self, sheet: usize) -> Option<u32> {
        let ws = &self.model.get_model().workbook.worksheets;
        ws.get(sheet).map(|w| w.sheet_id)
    }

    /// Appends an empty sheet with a free name ("Sheet2", …).
    pub fn add_sheet(&mut self) -> Result<(), String> {
        self.model.new_sheet()?;
        self.done(Step::Cells);
        Ok(())
    }

    pub fn rename_sheet(&mut self, sheet: usize, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("empty sheet name".into());
        }
        self.model.rename_sheet(sheet as u32, name)?;
        self.done(Step::Cells);
        Ok(())
    }

    pub fn delete_sheet(&mut self, sheet: usize) -> Result<(), String> {
        if self.sheet_ids().len() <= 1 {
            return Err("a workbook needs at least one sheet".into());
        }
        self.model.delete_sheet(sheet as u32)?;
        self.done(Step::Cells);
        Ok(())
    }

    /// The whole sheet as the model has it, for sheets the viewer never loaded (a new
    /// one, or one brought back by undo).
    pub fn snapshot(&self, sheet: usize) -> (Sheet, Formulas) {
        let model = self.model.get_model();
        let names = self.sheet_names();
        let name = names.get(sheet).cloned().unwrap_or_default();
        let mut out = Sheet::from_cells(name, 0, 0, Vec::new(), self.locale);
        let mut formulas = HashMap::new();
        let Some(ws) = model.workbook.worksheets.get(sheet) else {
            return (out, formulas);
        };
        let cells: Vec<_> = ws
            .sheet_data
            .cells()
            .map(|(r, c, _)| (r as u32 - 1, c as u32 - 1))
            .collect();
        for (r, c) in cells {
            let u = self.cell(sheet, r, c);
            out.set(r, c, u.value, u.formatted, u.style);
            if let Some(f) = self.formula(sheet, r, c) {
                formulas.insert((r, c), f);
            }
        }
        (out, formulas)
    }

    /// Sets a cell from what the user typed: a number (in the UI language), text or
    /// a formula starting with `=`.
    pub fn set_input(&mut self, sheet: usize, r: u32, c: u32, input: &str) -> Result<(), String> {
        self.model
            .set_user_input(sheet as u32, r as i32 + 1, c as i32 + 1, input)?;
        self.done(Step::Cells);
        self.watch(sheet, r, c, r, c);
        Ok(())
    }

    /// Replaces matches of `query` in the input of cells (formulas as text, like Excel)
    /// and recalculates. `cells` are (sheet, cell); `None` means every filled cell of
    /// `sheets`. One undo step. Returns how many cells changed.
    pub fn replace(
        &mut self,
        sheets: &[usize],
        cells: Option<&[(usize, CellPos)]>,
        query: &Query,
        with: &str,
    ) -> Result<usize, String> {
        let targets: Vec<(usize, CellPos)> = match cells {
            Some(c) => c.to_vec(),
            None => {
                let model = self.model.get_model();
                sheets
                    .iter()
                    .filter_map(|&s| model.workbook.worksheets.get(s).map(|ws| (s, ws)))
                    .flat_map(|(s, ws)| {
                        ws.sheet_data
                            .cells()
                            .map(move |(r, c, _)| (s, CellPos::new(r as u32 - 1, c as u32 - 1)))
                    })
                    .collect()
            }
        };
        let changes: Vec<_> = targets
            .into_iter()
            .filter_map(|(s, p)| {
                let new = query.replace(&self.content(s, p.row, p.col), with)?;
                Some((s, p, new))
            })
            .collect();
        if changes.is_empty() {
            return Ok(0);
        }
        // One recalculation at the end instead of one per cell.
        self.model.pause_evaluation();
        let mut entries = 0;
        let mut result = Ok(());
        for (s, p, new) in &changes {
            result = self
                .model
                .set_user_input(*s as u32, p.row as i32 + 1, p.col as i32 + 1, new);
            if result.is_err() {
                break;
            }
            entries += 1;
            self.watch(*s, p.row, p.col, p.row, p.col);
        }
        self.model.resume_evaluation();
        self.model.evaluate();
        if entries > 0 {
            self.done_n(Step::Cells, entries);
        }
        result.map(|()| entries)
    }

    /// Empties the selected cells, keeping their style.
    pub fn clear(&mut self, sheet: usize, sel: &Selection) -> Result<(), String> {
        self.model.range_clear_contents(&area(sheet, sel))?;
        self.done(Step::Cells);
        let (r0, c0, r1, c1) = bounds(sel);
        self.watch(sheet, r0, c0, r1, c1);
        Ok(())
    }

    pub fn set_style(
        &mut self,
        sheet: usize,
        sel: &Selection,
        change: StyleChange,
    ) -> Result<(), String> {
        let (path, value) = match change {
            StyleChange::Bold(b) => ("font.b", b.to_string()),
            StyleChange::Italic(b) => ("font.i", b.to_string()),
            StyleChange::Color(c) => ("font.color", color_param(c)),
            StyleChange::Fill(c) => ("fill.color", color_param(c)),
            StyleChange::Number(f) => ("num_fmt", f.code(self.locale).to_string()),
        };
        self.model
            .update_range_style(&area(sheet, sel), path, &value)?;
        self.done(Step::Cells);
        let (r0, c0, r1, c1) = bounds(sel);
        self.watch(sheet, r0, c0, r1, c1);
        Ok(())
    }

    /// Pastes tab-separated text (as Excel, Sheets and ferrosheet copy it) at the top
    /// left of `sel`, each value read like typed input. A single value fills the whole
    /// selection. One undo step. Returns the cells that were pasted into.
    pub fn paste(
        &mut self,
        sheet: usize,
        sel: &Selection,
        text: &str,
    ) -> Result<Selection, String> {
        let mut rows = parse_tsv(text);
        let mut width = rows.iter().map(Vec::len).max().unwrap_or(0);
        if width == 0 {
            return Ok(*sel);
        }
        let (r0, c0, r1, c1) = bounds(sel);
        if rows.len() == 1 && width == 1 {
            let v = rows[0][0].clone();
            width = (c1 - c0 + 1) as usize;
            rows = vec![vec![v; width]; (r1 - r0 + 1) as usize];
        }
        // IronCalc expects a rectangle; a short row clears the rest of it anyway.
        let mut out = ::csv::WriterBuilder::new()
            .delimiter(b'\t')
            .terminator(::csv::Terminator::Any(b'\n'))
            .from_writer(Vec::new());
        for row in &mut rows {
            row.resize(width, String::new());
            out.write_record(&*row).map_err(|e| e.to_string())?;
        }
        let bytes = out.into_inner().map_err(|e| e.to_string())?;
        let csv = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        let target = Area {
            sheet: sheet as u32,
            row: r0 as i32 + 1,
            column: c0 as i32 + 1,
            width: width as i32,
            height: rows.len() as i32,
        };
        // The model selects what was pasted and insists its cursor lies inside.
        self.model.set_selected_sheet(target.sheet)?;
        self.model.set_selected_cell(target.row, target.column)?;
        self.model.paste_csv_string(&target, &csv)?;
        self.done(Step::Cells);
        let (r1, c1) = (r0 + rows.len() as u32 - 1, c0 + width as u32 - 1);
        self.watch(sheet, r0, c0, r1, c1);
        Ok(Selection {
            anchor: CellPos::new(r0, c0),
            cursor: CellPos::new(r1, c1),
        })
    }

    /// Inserts `count` empty rows or columns before `at` (0-based). Formulas pointing
    /// at moved cells follow them.
    pub fn insert_lines(
        &mut self,
        sheet: usize,
        lines: Lines,
        at: u32,
        count: u32,
    ) -> Result<(), String> {
        let (s, at, n) = (sheet as u32, at as i32 + 1, count as i32);
        match lines {
            Lines::Rows => self.model.insert_rows(s, at, n)?,
            Lines::Cols => self.model.insert_columns(s, at, n)?,
        }
        self.lines_done(sheet);
        Ok(())
    }

    /// Deletes `count` rows or columns from `at` (0-based). Formulas pointing at deleted
    /// cells show `#REF!`, the others follow their cells.
    pub fn delete_lines(
        &mut self,
        sheet: usize,
        lines: Lines,
        at: u32,
        count: u32,
    ) -> Result<(), String> {
        let (s, at, n) = (sheet as u32, at as i32 + 1, count as i32);
        match lines {
            Lines::Rows => self.model.delete_rows(s, at, n)?,
            Lines::Cols => self.model.delete_columns(s, at, n)?,
        }
        self.lines_done(sheet);
        Ok(())
    }

    fn lines_done(&mut self, sheet: usize) {
        match self.sheet_id(sheet) {
            Some(id) => self.done(Step::Moved(id)),
            None => self.done(Step::Cells),
        }
    }

    fn watch(&mut self, sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) {
        let Some(id) = self.sheet_id(sheet) else {
            return;
        };
        {
            let w = self.watched.entry(id).or_default();
            for r in r0..=r1 {
                for c in c0..=c1 {
                    w.insert((r, c));
                }
            }
        }
    }

    /// Undoes the last change; `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        self.step_back(true)
    }

    pub fn redo(&mut self) -> bool {
        self.step_back(false)
    }

    /// Undoes (`undo`) or redoes the last step with all its model entries.
    fn step_back(&mut self, undo: bool) -> bool {
        let can = if undo {
            self.model.can_undo()
        } else {
            self.model.can_redo()
        };
        if !can {
            return false;
        }
        let from = if undo {
            &mut self.undo_steps
        } else {
            &mut self.redo_steps
        };
        let (step, entries) = from.pop().unwrap_or((Step::Cells, 1));
        if entries > 1 {
            self.model.pause_evaluation();
        }
        let mut ok = true;
        for _ in 0..entries {
            let r = if undo {
                self.model.undo()
            } else {
                self.model.redo()
            };
            ok &= r.is_ok();
        }
        if entries > 1 {
            self.model.resume_evaluation();
            self.model.evaluate();
        }
        if !ok {
            return false;
        }
        let to = if undo {
            &mut self.redo_steps
        } else {
            &mut self.undo_steps
        };
        to.push((step, entries));
        if let Step::Moved(id) = step {
            self.relayout(id);
        }
        true
    }

    /// What the cell editor starts with: the formula, or the value as typed.
    pub fn content(&self, sheet: usize, r: u32, c: u32) -> String {
        self.model
            .get_cell_content(sheet as u32, r as i32 + 1, c as i32 + 1)
            .unwrap_or_default()
    }

    /// The formula of a cell, `None` for a plain value.
    pub fn formula(&self, sheet: usize, r: u32, c: u32) -> Option<String> {
        self.model
            .get_model()
            .get_cell_formula(sheet as u32, r as i32 + 1, c as i32 + 1)
            .ok()
            .flatten()
    }

    pub fn style(&self, sheet: usize, r: u32, c: u32) -> CellStyle {
        let Ok(style) =
            self.model
                .get_model()
                .get_style_for_cell(sheet as u32, r as i32 + 1, c as i32 + 1)
        else {
            return CellStyle::default();
        };
        let rgb = |c: &Color| match c {
            Color::None => None,
            c => parse_rgb(&self.model.resolve_color(c)),
        };
        CellStyle {
            bold: style.font.b,
            italic: style.font.i,
            // black text and white fill are what the theme shows anyway
            color: rgb(&style.font.color).filter(|&c| c != [0, 0, 0]),
            fill: rgb(&style.fill.color).filter(|&c| c != [255, 255, 255]),
        }
    }

    /// The current state of every cell that may have changed in `sheet`.
    pub fn updates(&self, sheet: usize) -> Vec<CellUpdate> {
        let Some(watched) = self.sheet_id(sheet).and_then(|id| self.watched.get(&id)) else {
            return Vec::new();
        };
        watched
            .iter()
            .map(|&(r, c)| self.cell(sheet, r, c))
            .collect()
    }

    fn cell(&self, sheet: usize, r: u32, c: u32) -> CellUpdate {
        let model = self.model.get_model();
        let (s, row, col) = (sheet as u32, r as i32 + 1, c as i32 + 1);
        let num_fmt = model
            .get_style_for_cell(s, row, col)
            .map(|st| st.num_fmt)
            .unwrap_or_default();
        let raw = model
            .get_cell_value_by_index(s, row, col)
            .unwrap_or(IcValue::None);
        let (value, formatted) = match raw {
            IcValue::None => (CellValue::Empty, None),
            IcValue::String(t) => match error_value(&t) {
                Some(e) if self.formula(sheet, r, c).is_some() => (CellValue::Error(e), None),
                _ => (CellValue::Text(t.into()), None),
            },
            IcValue::Boolean(b) => (CellValue::Bool(b), None),
            IcValue::Number(n) if is_date_format(&num_fmt) => (CellValue::DateTime(n), None),
            IcValue::Number(n) if is_general(&num_fmt) => (CellValue::Number(n), None),
            IcValue::Number(n) => (
                CellValue::Number(n),
                model.get_formatted_cell_value(s, row, col).ok(),
            ),
        };
        CellUpdate {
            row: r,
            col: c,
            value,
            formatted,
            style: self.style(sheet, r, c),
        }
    }

    /// Formulas of the watched cells of `sheet`, starting with `=`.
    pub fn formulas(&self, sheet: usize) -> HashMap<(u32, u32), String> {
        let Some(watched) = self.sheet_id(sheet).and_then(|id| self.watched.get(&id)) else {
            return HashMap::new();
        };
        watched
            .iter()
            .filter_map(|&(r, c)| self.formula(sheet, r, c).map(|f| ((r, c), f)))
            .collect()
    }

    /// Writes the workbook as `.xlsx`. Goes through a temporary file next to the target,
    /// so a failure never leaves a half-written file behind.
    pub fn save_xlsx(&self, path: &Path) -> Result<(), String> {
        let tmp = path.with_extension("ferrosheet-tmp.xlsx");
        let _ = std::fs::remove_file(&tmp);
        let tmp_str = tmp.to_str().ok_or("path is not valid Unicode")?;
        ironcalc::export::save_to_xlsx(self.model.get_model(), tmp_str)
            .map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

fn bounds(sel: &Selection) -> (u32, u32, u32, u32) {
    let (a, b) = (sel.anchor, sel.cursor);
    (
        a.row.min(b.row),
        a.col.min(b.col),
        a.row.max(b.row),
        a.col.max(b.col),
    )
}

fn area(sheet: usize, sel: &Selection) -> Area {
    let (r0, c0, r1, c1) = bounds(sel);
    Area {
        sheet: sheet as u32,
        row: r0 as i32 + 1,
        column: c0 as i32 + 1,
        width: (c1 - c0) as i32 + 1,
        height: (r1 - r0) as i32 + 1,
    }
}

/// Splits clipboard text into rows of cells: tabs between cells, a line break between
/// rows, quotes around cells that contain either (as Excel writes them).
fn parse_tsv(text: &str) -> Vec<Vec<String>> {
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    if text.is_empty() {
        return Vec::new();
    }
    let mut reader = ::csv::ReaderBuilder::new()
        .delimiter(b'\t')
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());
    let mut rows = Vec::new();
    for record in reader.records() {
        match record {
            Ok(r) => rows.push(r.iter().map(str::to_owned).collect()),
            // Broken quoting: take the lines as they are.
            Err(_) => {
                return text
                    .lines()
                    .map(|l| l.split('\t').map(str::to_owned).collect())
                    .collect();
            }
        }
    }
    rows
}

fn color_param(c: Option<[u8; 3]>) -> String {
    c.map(|[r, g, b]| format!("#{r:02X}{g:02X}{b:02X}"))
        .unwrap_or_default()
}

fn parse_rgb(s: &str) -> Option<[u8; 3]> {
    let hex = s.strip_prefix('#')?;
    let hex = if hex.len() == 8 { &hex[2..] } else { hex };
    if hex.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn is_general(fmt: &str) -> bool {
    fmt.is_empty() || fmt.eq_ignore_ascii_case("general")
}

/// Day or year codes outside quotes and brackets make a date format.
fn is_date_format(fmt: &str) -> bool {
    let mut quoted = false;
    let mut bracket = false;
    fmt.chars().any(|ch| {
        match ch {
            '"' => quoted = !quoted,
            '[' if !quoted => bracket = true,
            ']' if !quoted => bracket = false,
            _ => {}
        }
        !quoted && !bracket && matches!(ch.to_ascii_lowercase(), 'd' | 'y')
    })
}

fn error_value(s: &str) -> Option<CellError> {
    use CellError::*;
    Some(match s {
        "#DIV/0!" => Div0,
        "#N/A" => NA,
        "#VALUE!" => Value,
        "#REF!" => Ref,
        "#NAME?" => Name,
        "#NUM!" => Num,
        "#NULL!" => Null,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CellPos, Workbook};

    fn demo() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/demo.xlsx")
    }

    fn find(updates: &[CellUpdate], r: u32, c: u32) -> &CellUpdate {
        updates.iter().find(|u| (u.row, u.col) == (r, c)).unwrap()
    }

    #[test]
    fn editing_an_input_recalculates_its_formula_and_undo_restores_it() {
        let mut s = EditSession::open_xlsx(&demo(), Locale::Invariant).unwrap();
        assert_eq!(s.sheet_names(), ["Invoice", "Summary"]);
        // D2 = B2*C2*(1-G2)
        assert_eq!(s.formula(0, 1, 3).as_deref(), Some("=B2*C2*(1-G2)"));
        let before = find(&s.updates(0), 1, 3).value.clone();
        s.set_input(0, 1, 1, "0").unwrap();
        assert_eq!(find(&s.updates(0), 1, 3).value, CellValue::Number(0.0));
        assert!(s.undo());
        assert_eq!(find(&s.updates(0), 1, 3).value, before);
        assert!(s.redo());
        assert_eq!(find(&s.updates(0), 1, 3).value, CellValue::Number(0.0));
    }

    #[test]
    fn german_input_uses_the_decimal_comma() {
        let sheet = Sheet::empty("S".into());
        let mut s = EditSession::from_sheets(&[(&sheet, &HashMap::new())], Locale::German).unwrap();
        s.set_input(0, 0, 0, "2,5").unwrap();
        s.set_input(0, 1, 0, "=A1*2").unwrap();
        let u = s.updates(0);
        assert_eq!(find(&u, 0, 0).value, CellValue::Number(2.5));
        assert_eq!(find(&u, 1, 0).value, CellValue::Number(5.0));
        assert_eq!(s.content(0, 1, 0), "=A1*2");
    }

    #[test]
    fn styles_and_number_formats_reach_the_updates() {
        let sheet = Sheet::empty("S".into());
        let mut s = EditSession::from_sheets(&[(&sheet, &HashMap::new())], Locale::German).unwrap();
        s.set_input(0, 0, 0, "1234,5").unwrap();
        let sel = Selection::single(CellPos::new(0, 0));
        s.set_style(0, &sel, StyleChange::Bold(true)).unwrap();
        s.set_style(0, &sel, StyleChange::Fill(Some([255, 200, 0])))
            .unwrap();
        s.set_style(0, &sel, StyleChange::Number(NumberFormat::Currency))
            .unwrap();
        let u = find(&s.updates(0), 0, 0).clone();
        assert!(u.style.bold);
        assert_eq!(u.style.fill, Some([255, 200, 0]));
        assert_eq!(u.value, CellValue::Number(1234.5));
        assert_eq!(u.formatted.as_deref(), Some("1.234,50 €"));
        s.set_style(0, &sel, StyleChange::Number(NumberFormat::Date))
            .unwrap();
        assert_eq!(find(&s.updates(0), 0, 0).value, CellValue::DateTime(1234.5));
    }

    #[test]
    fn sheets_from_other_formats_keep_values_and_save_as_xlsx() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/date.ods");
        let mut wb = Workbook::open(&path, Locale::Invariant).unwrap();
        let sheet = wb.load_sheet(0).unwrap();
        let formulas = wb.load_formulas(0).unwrap_or_default();
        let mut s = EditSession::from_sheets(&[(&sheet, &formulas)], Locale::Invariant).unwrap();
        s.set_input(0, 0, 0, "edited").unwrap();

        let dir = std::env::temp_dir().join("ferrosheet-tests");
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("edit-save.xlsx");
        s.save_xlsx(&out).unwrap();
        s.save_xlsx(&out).unwrap(); // overwriting works too
        let mut back = Workbook::open(&out, Locale::Invariant).unwrap();
        let reread = back.load_sheet(0).unwrap();
        assert_eq!(reread.display(0, 0), "edited");
        assert_eq!(reread.rows, sheet.rows.max(1));
    }

    #[test]
    fn sheets_can_be_added_renamed_and_deleted_and_undo_brings_them_back() {
        let mut s = EditSession::open_xlsx(&demo(), Locale::German).unwrap();
        let n = s.sheet_names().len();
        let first = s.sheet_names()[0].clone();
        s.add_sheet().unwrap();
        assert_eq!(s.sheet_names().len(), n + 1);
        s.rename_sheet(n, "  Neu ").unwrap();
        assert_eq!(s.sheet_names()[n], "Neu");
        assert!(s.rename_sheet(n, "   ").is_err());
        s.set_input(n, 0, 0, "=1+2").unwrap();
        assert_eq!(find(&s.updates(n), 0, 0).value, CellValue::Number(3.0));

        let id = s.sheet_ids()[0];
        let (before, formulas) = s.snapshot(0);
        s.delete_sheet(0).unwrap();
        assert_eq!(s.sheet_names().len(), n);
        assert!(!s.sheet_ids().contains(&id));
        assert!(s.undo());
        assert_eq!(s.sheet_names()[0], first);
        let (after, again) = s.snapshot(0);
        assert_eq!((after.rows, after.cols), (before.rows, before.cols));
        assert_eq!(after.display(1, 1), before.display(1, 1));
        assert_eq!(again, formulas);
        assert!(!formulas.is_empty());
    }

    #[test]
    fn the_last_sheet_cannot_be_deleted() {
        let sheet = Sheet::from_cells(
            "A".into(),
            1,
            1,
            vec![CellValue::Number(1.0)],
            Locale::German,
        );
        let mut s = EditSession::from_sheets(&[(&sheet, &HashMap::new())], Locale::German).unwrap();
        assert!(s.delete_sheet(0).is_err());
    }

    fn blank(locale: Locale) -> EditSession {
        let sheet = Sheet::empty("S".into());
        EditSession::from_sheets(&[(&sheet, &HashMap::new())], locale).unwrap()
    }

    fn range(r0: u32, c0: u32, r1: u32, c1: u32) -> Selection {
        Selection {
            anchor: CellPos::new(r0, c0),
            cursor: CellPos::new(r1, c1),
        }
    }

    #[test]
    fn pasting_tab_separated_text_fills_a_block_and_is_one_undo_step() {
        let mut s = blank(Locale::German);
        s.set_input(0, 5, 5, "keep").unwrap();
        let text = "Name\tWert\r\nA\t1,5\r\n\"two\nlines\"\t=C3*2\r\n";
        let pasted = s
            .paste(0, &Selection::single(CellPos::new(1, 1)), text)
            .unwrap();
        assert_eq!(pasted, range(1, 1, 3, 2));
        let u = s.updates(0);
        assert_eq!(find(&u, 1, 1).value, CellValue::Text("Name".into()));
        assert_eq!(find(&u, 2, 2).value, CellValue::Number(1.5));
        assert_eq!(find(&u, 3, 1).value, CellValue::Text("two\nlines".into()));
        assert_eq!(s.content(0, 3, 2), "=C3*2");
        assert_eq!(find(&u, 3, 2).value, CellValue::Number(3.0));
        assert!(s.undo());
        let u = s.updates(0);
        assert_eq!(find(&u, 1, 1).value, CellValue::Empty);
        assert_eq!(find(&u, 3, 2).value, CellValue::Empty);
        assert_eq!(find(&u, 5, 5).value, CellValue::Text("keep".into()));
    }

    #[test]
    fn a_single_value_fills_the_selection_and_short_rows_are_padded() {
        let mut s = blank(Locale::Invariant);
        let sel = range(0, 0, 2, 1);
        assert_eq!(s.paste(0, &sel, "7\n").unwrap(), sel);
        let u = s.updates(0);
        for (r, c) in [(0, 0), (1, 1), (2, 0), (2, 1)] {
            assert_eq!(find(&u, r, c).value, CellValue::Number(7.0));
        }
        let pasted = s.paste(0, &range(4, 0, 4, 0), "a\tb\tc\nd").unwrap();
        assert_eq!(pasted, range(4, 0, 5, 2));
        assert_eq!(find(&s.updates(0), 5, 0).value, CellValue::Text("d".into()));
        // nothing on the clipboard: nothing happens
        assert_eq!(s.paste(0, &sel, "").unwrap(), sel);
    }

    #[test]
    fn inserting_and_deleting_rows_and_columns_moves_formulas_along() {
        let mut s = blank(Locale::Invariant);
        s.set_input(0, 0, 0, "2").unwrap();
        s.set_input(0, 1, 0, "3").unwrap();
        s.set_input(0, 2, 1, "=A1*A2").unwrap();
        assert!(s.take_moved().is_empty());

        s.insert_lines(0, Lines::Rows, 1, 2).unwrap();
        assert_eq!(s.take_moved(), [0]);
        assert_eq!(s.formula(0, 4, 1).as_deref(), Some("=A1*A4"));
        let (sheet, formulas) = s.snapshot(0);
        assert_eq!(sheet.display(3, 0), "3");
        assert_eq!(sheet.display(4, 1), "6");
        assert_eq!(formulas.get(&(4, 1)).map(String::as_str), Some("=A1*A4"));

        s.insert_lines(0, Lines::Cols, 0, 1).unwrap();
        assert_eq!(s.formula(0, 4, 2).as_deref(), Some("=B1*B4"));
        s.delete_lines(0, Lines::Cols, 0, 1).unwrap();
        s.delete_lines(0, Lines::Rows, 1, 2).unwrap();
        assert_eq!(s.formula(0, 2, 1).as_deref(), Some("=A1*A2"));

        s.take_moved();

        // undo moves everything back and asks for a fresh display again
        assert!(s.undo());
        assert_eq!(s.take_moved(), [0]);
        assert_eq!(s.formula(0, 4, 1).as_deref(), Some("=A1*A4"));
        assert!(s.redo());
        assert_eq!(s.take_moved(), [0]);
        assert_eq!(s.formula(0, 2, 1).as_deref(), Some("=A1*A2"));
        // undoing a plain input does not
        s.undo();
        s.take_moved();
        s.undo();
        s.undo();
        s.undo();
        s.take_moved();
        s.undo();
        assert!(s.take_moved().is_empty());
        assert_eq!(s.formula(0, 2, 1), None);
    }

    #[test]
    fn deleting_a_referenced_row_breaks_the_formula() {
        let mut s = blank(Locale::Invariant);
        s.set_input(0, 0, 0, "2").unwrap();
        s.set_input(0, 1, 0, "3").unwrap();
        s.set_input(0, 2, 1, "=A1*A2").unwrap();
        s.delete_lines(0, Lines::Rows, 0, 1).unwrap();
        assert_eq!(s.formula(0, 1, 1).as_deref(), Some("=#REF!*A1"));
        assert_eq!(s.snapshot(0).0.display(1, 1), "#REF!");
    }

    #[test]
    fn date_formats_are_recognised() {
        assert!(is_date_format("dd.mm.yyyy"));
        assert!(is_date_format("[$-407]d. mmmm yyyy"));
        assert!(!is_date_format("#,##0.00 \"€\""));
        assert!(!is_date_format("0.00%"));
        assert!(!is_date_format("[Red]0.00"));
    }

    #[test]
    fn replace_all_changes_inputs_and_formulas_in_one_undo_step() {
        let mut s = blank(Locale::Invariant);
        s.set_input(0, 0, 0, "apple pie").unwrap();
        s.set_input(0, 1, 0, "2").unwrap();
        s.set_input(0, 2, 0, "=A2*10").unwrap();
        s.set_input(0, 3, 0, "Apple").unwrap();
        let n = s
            .replace(&[0], None, &Query::text("apple"), "pear")
            .unwrap();
        assert_eq!(n, 2);
        let u = s.updates(0);
        assert_eq!(find(&u, 0, 0).value, CellValue::Text("pear pie".into()));
        assert_eq!(find(&u, 3, 0).value, CellValue::Text("pear".into()));
        // formulas are replaced as text and recalculated
        let n = s.replace(&[0], None, &Query::text("A2"), "A2+1").unwrap();
        assert_eq!(n, 1);
        assert_eq!(s.content(0, 2, 0), "=A2+1*10");
        assert_eq!(find(&s.updates(0), 2, 0).value, CellValue::Number(12.0));
        // one cell only
        let one = [(0, CellPos::new(0, 0))];
        let pie = Query::text("pie");
        assert_eq!(s.replace(&[0], Some(&one), &pie, "tart").unwrap(), 1);
        assert_eq!(s.replace(&[0], Some(&one), &pie, "x").unwrap(), 0);

        assert!(s.undo());
        assert!(s.undo());
        assert_eq!(s.content(0, 2, 0), "=A2*10");
        assert!(s.undo()); // the first replace: both cells at once
        let u = s.updates(0);
        assert_eq!(find(&u, 0, 0).value, CellValue::Text("apple pie".into()));
        assert_eq!(find(&u, 3, 0).value, CellValue::Text("Apple".into()));
        assert_eq!(find(&u, 1, 0).value, CellValue::Number(2.0));
        assert!(s.redo());
        let u = s.updates(0);
        assert_eq!(find(&u, 3, 0).value, CellValue::Text("pear".into()));
    }

    #[test]
    fn typed_dates_become_date_values() {
        for locale in [Locale::German, Locale::Invariant] {
            let mut s = blank(locale);
            s.set_input(0, 0, 0, &crate::today_input(locale)).unwrap();
            let v = find(&s.updates(0), 0, 0).value.clone();
            assert!(matches!(v, CellValue::DateTime(_)), "{locale:?}: {v:?}");
        }
    }
}
