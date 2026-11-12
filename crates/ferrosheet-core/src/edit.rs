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

use crate::{CellError, CellStyle, CellValue, Locale, Selection, Sheet};

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

/// Formulas of a sheet by (row, col), each starting with `=`.
type Formulas = HashMap<(u32, u32), String>;

pub struct EditSession {
    model: UserModel<'static>,
    locale: Locale,
    /// Per sheet: cells with a formula plus every cell changed here. After any change
    /// these are the only cells whose shown value can differ from the model.
    watched: Vec<BTreeSet<(u32, u32)>>,
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
                ws.sheet_data
                    .cells()
                    .filter(|(_, _, cell)| cell.has_formula())
                    .map(|(r, c, _)| (r as u32 - 1, c as u32 - 1))
                    .collect()
            })
            .collect();
        EditSession {
            model: UserModel::from_model(model),
            locale,
            watched,
        }
    }

    pub fn sheet_names(&self) -> Vec<String> {
        self.model.get_model().workbook.get_worksheet_names()
    }

    /// Sets a cell from what the user typed: a number (in the UI language), text or
    /// a formula starting with `=`.
    pub fn set_input(&mut self, sheet: usize, r: u32, c: u32, input: &str) -> Result<(), String> {
        self.model
            .set_user_input(sheet as u32, r as i32 + 1, c as i32 + 1, input)?;
        self.watch(sheet, r, c, r, c);
        Ok(())
    }

    /// Empties the selected cells, keeping their style.
    pub fn clear(&mut self, sheet: usize, sel: &Selection) -> Result<(), String> {
        self.model.range_clear_contents(&area(sheet, sel))?;
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
        let (r0, c0, r1, c1) = bounds(sel);
        self.watch(sheet, r0, c0, r1, c1);
        Ok(())
    }

    fn watch(&mut self, sheet: usize, r0: u32, c0: u32, r1: u32, c1: u32) {
        if let Some(w) = self.watched.get_mut(sheet) {
            for r in r0..=r1 {
                for c in c0..=c1 {
                    w.insert((r, c));
                }
            }
        }
    }

    /// Undoes the last change; `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        self.model.can_undo() && self.model.undo().is_ok()
    }

    pub fn redo(&mut self) -> bool {
        self.model.can_redo() && self.model.redo().is_ok()
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
        let Some(watched) = self.watched.get(sheet) else {
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
        let Some(watched) = self.watched.get(sheet) else {
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
    fn date_formats_are_recognised() {
        assert!(is_date_format("dd.mm.yyyy"));
        assert!(is_date_format("[$-407]d. mmmm yyyy"));
        assert!(!is_date_format("#,##0.00 \"€\""));
        assert!(!is_date_format("0.00%"));
        assert!(!is_date_format("[Red]0.00"));
    }
}
