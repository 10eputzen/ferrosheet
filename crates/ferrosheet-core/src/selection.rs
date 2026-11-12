//! Rectangular cell selection and the text it puts on the clipboard.

use crate::{Locale, Sheet};

/// What [`copy_text`] puts on the clipboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyFormat {
    /// The text as shown in the grid.
    Display,
    /// Locale-neutral raw values: decimal point, ISO dates, `TRUE`/`FALSE`.
    Invariant,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CellPos {
    pub row: u32,
    pub col: u32,
}

impl CellPos {
    pub fn new(row: u32, col: u32) -> Self {
        CellPos { row, col }
    }
}

/// A rectangle spanned by the anchor (where selecting started) and the cursor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    pub anchor: CellPos,
    pub cursor: CellPos,
}

impl Selection {
    pub fn single(p: CellPos) -> Self {
        Selection {
            anchor: p,
            cursor: p,
        }
    }

    /// Top-left and bottom-right corner.
    pub fn bounds(&self) -> (CellPos, CellPos) {
        let (a, c) = (self.anchor, self.cursor);
        (
            CellPos::new(a.row.min(c.row), a.col.min(c.col)),
            CellPos::new(a.row.max(c.row), a.col.max(c.col)),
        )
    }

    pub fn contains(&self, p: CellPos) -> bool {
        let (tl, br) = self.bounds();
        (tl.row..=br.row).contains(&p.row) && (tl.col..=br.col).contains(&p.col)
    }

    /// Number of selected cells.
    pub fn cell_count(&self) -> u64 {
        let (tl, br) = self.bounds();
        (br.row - tl.row + 1) as u64 * (br.col - tl.col + 1) as u64
    }

    /// Moves the cursor, clamped to the grid. Without `extend` the selection collapses to it.
    pub fn move_cursor(&mut self, dr: i64, dc: i64, extend: bool, rows: u32, cols: u32) {
        let clamp =
            |v: u32, d: i64, max: u32| (v as i64 + d).clamp(0, max.max(1) as i64 - 1) as u32;
        self.cursor = CellPos::new(
            clamp(self.cursor.row, dr, rows),
            clamp(self.cursor.col, dc, cols),
        );
        if !extend {
            self.anchor = self.cursor;
        }
    }
}

/// Tab-separated text of the selection, rows joined by CRLF. Cells containing
/// tabs, line breaks or quotes are quoted like Excel does. The selection is cut
/// to the used range, so a whole selected column copies only its filled rows.
pub fn copy_text(sheet: &Sheet, sel: &Selection, f: CopyFormat) -> String {
    let (tl, br) = sel.bounds();
    if sheet.rows == 0 || sheet.cols == 0 {
        return String::new();
    }
    let last_row = br.row.min(sheet.rows - 1);
    let last_col = br.col.min(sheet.cols - 1);
    if tl.row > last_row || tl.col > last_col {
        return String::new();
    }
    let mut out = String::new();
    for r in tl.row..=last_row {
        if r > tl.row {
            out.push_str("\r\n");
        }
        for c in tl.col..=last_col {
            if c > tl.col {
                out.push('\t');
            }
            match f {
                CopyFormat::Display => push_cell(&mut out, &sheet.display(r, c)),
                CopyFormat::Invariant => {
                    push_cell(&mut out, &sheet.value(r, c).format(Locale::Invariant))
                }
            }
        }
    }
    out
}

fn push_cell(out: &mut String, s: &str) {
    if s.contains(['\t', '\n', '\r', '"']) {
        out.push('"');
        out.push_str(&s.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(s);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CellValue::{self, *};

    fn sheet(rows: u32, cols: u32, cells: Vec<CellValue>) -> Sheet {
        Sheet::from_cells("t".into(), rows, cols, cells, Locale::German)
    }

    fn all(rows: u32, cols: u32) -> Selection {
        Selection {
            anchor: CellPos::new(0, 0),
            cursor: CellPos::new(rows - 1, cols - 1),
        }
    }

    #[test]
    fn copy_range_tsv() {
        let s = sheet(
            2,
            2,
            vec![Text("a".into()), Number(1.0), Text("b".into()), Number(2.5)],
        );
        assert_eq!(
            copy_text(&s, &all(2, 2), CopyFormat::Display),
            "a\t1\r\nb\t2,5"
        );
        assert_eq!(
            copy_text(&s, &all(2, 2), CopyFormat::Invariant),
            "a\t1\r\nb\t2.5"
        );
    }

    #[test]
    fn copy_single_cell() {
        let s = sheet(1, 1, vec![Text("x".into())]);
        let sel = Selection::single(CellPos::new(0, 0));
        assert_eq!(copy_text(&s, &sel, CopyFormat::Display), "x");
    }

    #[test]
    fn copy_empty_selection() {
        let sel = Selection::single(CellPos::new(3, 3));
        assert_eq!(
            copy_text(&Sheet::empty("e".into()), &sel, CopyFormat::Display),
            ""
        );
        let s = sheet(1, 1, vec![Text("x".into())]);
        assert_eq!(copy_text(&s, &sel, CopyFormat::Display), "");
    }

    #[test]
    fn copy_whole_column_is_cut_to_used_range() {
        let s = sheet(2, 1, vec![Text("a".into()), Text("b".into())]);
        let sel = Selection {
            anchor: CellPos::new(0, 0),
            cursor: CellPos::new(1_048_575, 0),
        };
        assert_eq!(copy_text(&s, &sel, CopyFormat::Display), "a\r\nb");
    }

    #[test]
    fn copy_quotes_tabs_and_newlines() {
        let s = sheet(1, 2, vec![Text("a\tb".into()), Text("x\"y\nz".into())]);
        assert_eq!(
            copy_text(&s, &all(1, 2), CopyFormat::Display),
            "\"a\tb\"\t\"x\"\"y\nz\""
        );
    }

    #[test]
    fn move_and_extend() {
        let mut sel = Selection::single(CellPos::new(0, 0));
        sel.move_cursor(1, 1, true, 10, 10);
        assert_eq!(sel.bounds(), (CellPos::new(0, 0), CellPos::new(1, 1)));
        assert_eq!(sel.cell_count(), 4);
        sel.move_cursor(0, 1, false, 10, 10);
        assert_eq!(sel, Selection::single(CellPos::new(1, 2)));
        let mut sel = Selection::single(CellPos::new(0, 0));
        sel.move_cursor(-1, -1, false, 10, 10);
        assert_eq!(sel, Selection::single(CellPos::new(0, 0)));
        sel.move_cursor(100, 100, false, 10, 10);
        assert_eq!(sel, Selection::single(CellPos::new(9, 9)));
    }
}
