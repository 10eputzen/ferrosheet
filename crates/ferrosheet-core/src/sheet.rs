//! A loaded worksheet: a dense row-major grid. Display text is made on demand, so
//! loading a big sheet does not format millions of cells nobody looks at.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::{CellPos, CellValue, Locale, Selection};

/// Look of a cell beyond its value. Only cells that were edited carry one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellStyle {
    pub bold: bool,
    pub italic: bool,
    /// Text colour, `None` = the theme's text colour.
    pub color: Option<[u8; 3]>,
    /// Background, `None` = no fill.
    pub fill: Option<[u8; 3]>,
}

/// Sum, average and count of a selection, as Excel shows them in the status bar.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Summary {
    /// Cells that are not empty.
    pub count: u64,
    /// Cells holding a number (dates and durations count as numbers).
    pub numbers: u64,
    pub sum: f64,
}

impl Summary {
    /// Average of the numbers, `None` without any.
    pub fn average(&self) -> Option<f64> {
        (self.numbers > 0).then(|| self.sum / self.numbers as f64)
    }
}

/// A worksheet ready for display. Cell `(0, 0)` is `A1`.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub rows: u32,
    pub cols: u32,
    cells: Vec<CellValue>,
    locale: Locale,
    /// Display text from a number format of the cell (e.g. `1.234,50 €`), edited cells only.
    formatted: HashMap<(u32, u32), Box<str>>,
    styles: HashMap<(u32, u32), CellStyle>,
    /// Lower-cased display text, built on first search.
    lower: OnceLock<Vec<Box<str>>>,
}

static EMPTY: CellValue = CellValue::Empty;

impl Sheet {
    /// An empty sheet without cells.
    pub fn empty(name: String) -> Sheet {
        Sheet {
            name,
            ..Default::default()
        }
    }

    /// Builds a sheet from row-major cells (`cells.len() == rows * cols`), shown in
    /// `locale`.
    pub fn from_cells(
        name: String,
        rows: u32,
        cols: u32,
        cells: Vec<CellValue>,
        locale: Locale,
    ) -> Sheet {
        debug_assert_eq!(cells.len(), rows as usize * cols as usize);
        Sheet {
            name,
            rows,
            cols,
            cells,
            locale,
            ..Default::default()
        }
    }

    pub fn locale(&self) -> Locale {
        self.locale
    }

    fn index(&self, r: u32, c: u32) -> Option<usize> {
        (r < self.rows && c < self.cols).then(|| r as usize * self.cols as usize + c as usize)
    }

    /// The value at `(r, c)`, `Empty` outside the used range.
    pub fn value(&self, r: u32, c: u32) -> &CellValue {
        self.index(r, c).map_or(&EMPTY, |i| &self.cells[i])
    }

    /// The display text at `(r, c)`, `""` outside the used range.
    pub fn display(&self, r: u32, c: u32) -> Cow<'_, str> {
        if let Some(s) = self.formatted.get(&(r, c)) {
            return Cow::Borrowed(s);
        }
        match self.value(r, c) {
            CellValue::Text(s) => Cow::Borrowed(s),
            v => Cow::Owned(v.format(self.locale)),
        }
    }

    pub fn style(&self, r: u32, c: u32) -> CellStyle {
        self.styles.get(&(r, c)).copied().unwrap_or_default()
    }

    /// Replaces one cell, growing the sheet when it lies outside. `formatted` is the
    /// text a number format gives the value, if it has one.
    pub fn set(
        &mut self,
        r: u32,
        c: u32,
        value: CellValue,
        formatted: Option<String>,
        style: CellStyle,
    ) {
        if r >= self.rows || c >= self.cols {
            self.grow((r + 1).max(self.rows), (c + 1).max(self.cols));
        }
        let i = r as usize * self.cols as usize + c as usize;
        self.cells[i] = value;
        match formatted {
            Some(f) => self.formatted.insert((r, c), f.into_boxed_str()),
            None => self.formatted.remove(&(r, c)),
        };
        if style == CellStyle::default() {
            self.styles.remove(&(r, c));
        } else {
            self.styles.insert((r, c), style);
        }
        self.lower = OnceLock::new();
    }

    fn grow(&mut self, rows: u32, cols: u32) {
        if cols == self.cols {
            self.cells
                .resize(rows as usize * cols as usize, CellValue::Empty);
        } else {
            let mut cells = vec![CellValue::Empty; rows as usize * cols as usize];
            if self.cols > 0 {
                for (r, row) in self.cells.chunks(self.cols as usize).enumerate() {
                    let start = r * cols as usize;
                    cells[start..start + row.len()].clone_from_slice(row);
                }
            }
            self.cells = cells;
        }
        self.rows = rows;
        self.cols = cols;
    }

    /// Sum, average and count of the selected cells; only the used range is visited,
    /// so a whole selected column costs as much as its filled rows.
    pub fn summary(&self, sel: &Selection) -> Summary {
        let mut out = Summary::default();
        let (tl, br) = sel.bounds();
        if self.rows == 0 || self.cols == 0 || tl.row >= self.rows || tl.col >= self.cols {
            return out;
        }
        let (last_row, last_col) = (br.row.min(self.rows - 1), br.col.min(self.cols - 1));
        let cols = self.cols as usize;
        for r in tl.row..=last_row {
            let start = r as usize * cols;
            let row = &self.cells[start + tl.col as usize..=start + last_col as usize];
            for v in row {
                match v {
                    CellValue::Empty => {}
                    CellValue::Number(n) | CellValue::DateTime(n) | CellValue::Duration(n) => {
                        out.count += 1;
                        out.numbers += 1;
                        out.sum += n;
                    }
                    _ => out.count += 1,
                }
            }
        }
        out
    }

    /// The cells AutoSum adds up for a formula at `at`: the unbroken run of numbers right
    /// above it, or, with none above, right left of it (like Excel). `None` without any.
    pub fn autosum_range(&self, at: CellPos) -> Option<(CellPos, CellPos)> {
        let num = |r: u32, c: u32| self.value(r, c).is_numeric();
        if at.row > 0 && num(at.row - 1, at.col) {
            let mut top = at.row - 1;
            while top > 0 && num(top - 1, at.col) {
                top -= 1;
            }
            return Some((CellPos::new(top, at.col), CellPos::new(at.row - 1, at.col)));
        }
        if at.col > 0 && num(at.row, at.col - 1) {
            let mut left = at.col - 1;
            while left > 0 && num(at.row, left - 1) {
                left -= 1;
            }
            return Some((CellPos::new(at.row, left), CellPos::new(at.row, at.col - 1)));
        }
        None
    }

    /// Builds the search index now (e.g. on a background thread right after loading),
    /// so the first search is as fast as every later one.
    pub fn prepare_search(&self) {
        self.lower();
    }

    /// Lower-cased display text of all cells, row-major. Built once.
    pub(crate) fn lower(&self) -> &[Box<str>] {
        self.lower.get_or_init(|| {
            let cols = self.cols.max(1) as usize;
            self.cells
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    let formatted = if self.formatted.is_empty() {
                        None
                    } else {
                        self.formatted.get(&((i / cols) as u32, (i % cols) as u32))
                    };
                    match (formatted, v) {
                        (Some(f), _) => f.to_lowercase().into_boxed_str(),
                        (None, CellValue::Text(s)) => s.to_lowercase().into_boxed_str(),
                        (None, v) => v.format(self.locale).to_lowercase().into_boxed_str(),
                    }
                })
                .collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CellValue::*;
    use crate::{Query, find_all};

    #[test]
    fn set_grows_the_sheet_and_keeps_the_cells() {
        let mut s = Sheet::from_cells(
            "t".into(),
            2,
            2,
            vec![Number(1.0), Number(2.0), Number(3.0), Number(4.0)],
            Locale::Invariant,
        );
        s.set(2, 3, Text("x".into()), None, CellStyle::default());
        assert_eq!((s.rows, s.cols), (3, 4));
        assert_eq!(s.value(0, 1), &Number(2.0));
        assert_eq!(s.value(1, 0), &Number(3.0));
        assert_eq!(s.value(1, 2), &Empty);
        assert_eq!(s.display(2, 3), "x");

        let mut e = Sheet::empty("e".into());
        e.set(1, 1, Number(5.0), None, CellStyle::default());
        assert_eq!((e.rows, e.cols), (2, 2));
        assert_eq!(e.value(1, 1), &Number(5.0));
    }

    #[test]
    fn formatted_text_and_style_show_and_are_searched() {
        let mut s = Sheet::from_cells("t".into(), 1, 1, vec![Empty], Locale::German);
        assert!(find_all(&s, &Query::text("€")).is_empty());
        let bold = CellStyle {
            bold: true,
            ..Default::default()
        };
        s.set(0, 0, Number(2.5), Some("2,50 €".into()), bold);
        assert_eq!(s.display(0, 0), "2,50 €");
        assert_eq!(s.value(0, 0), &Number(2.5));
        assert!(s.style(0, 0).bold);
        assert_eq!(find_all(&s, &Query::text("€")).len(), 1);

        s.set(0, 0, Number(3.0), None, CellStyle::default());
        assert_eq!(s.display(0, 0), "3");
        assert_eq!(s.style(0, 0), CellStyle::default());
    }

    #[test]
    fn summary_counts_filled_cells_and_adds_the_numbers() {
        let s = Sheet::from_cells(
            "t".into(),
            3,
            2,
            vec![
                Text("a".into()),
                Number(1.5),
                Empty,
                Number(2.5),
                Bool(true),
                Empty,
            ],
            Locale::Invariant,
        );
        let all = Selection {
            anchor: crate::CellPos::new(0, 0),
            // far beyond the used range, like a whole selected column
            cursor: crate::CellPos::new(1_048_575, 5),
        };
        let sum = s.summary(&all);
        assert_eq!((sum.count, sum.numbers, sum.sum), (4, 2, 4.0));
        assert_eq!(sum.average(), Some(2.0));
        let text = Selection::single(crate::CellPos::new(0, 0));
        assert_eq!(s.summary(&text).average(), None);
        let outside = Selection::single(crate::CellPos::new(9, 9));
        assert_eq!(s.summary(&outside), Summary::default());
    }

    #[test]
    fn autosum_takes_the_numbers_above_or_else_left() {
        // A: header, 1, 2, (empty) / B: 5 / C: target
        let s = Sheet::from_cells(
            "t".into(),
            4,
            3,
            vec![
                Text("h".into()),
                Empty,
                Empty,
                Number(1.0),
                Empty,
                Empty,
                Number(2.0),
                Number(5.0),
                Number(6.0),
                Empty,
                Empty,
                Empty,
            ],
            Locale::Invariant,
        );
        let p = CellPos::new;
        assert_eq!(s.autosum_range(p(3, 0)), Some((p(1, 0), p(2, 0))));
        // nothing above C3 but numbers to its left
        assert_eq!(s.autosum_range(p(2, 2)), Some((p(2, 0), p(2, 1))));
        assert_eq!(s.autosum_range(p(0, 0)), None);
        assert_eq!(s.autosum_range(p(0, 2)), None);
    }
}
