//! Autofill: dragging the handle at the corner of a selection continues it. Series
//! (1, 2, 3 · Item 1, Item 2 · Mon, Tue · Jan, Feb) go on, everything else is repeated,
//! and relative references in formulas move along. Fill down and right (Ctrl+D,
//! Ctrl+R) copy the first row or column over the selection, like Excel.

use ironcalc::base::expressions::types::Area;

use super::{EditSession, Lines, Step, bounds};
use crate::{CellPos, Selection};

/// Where a drag of the fill handle extends a selection to: rows (down or up) or
/// columns (right or left), up to and including line `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub lines: Lines,
    pub to: u32,
}

impl Fill {
    /// Where the fill handle of `src` dragged onto cell `p` extends it to, if anywhere.
    /// The direction is the one the pointer left the selection farthest in, as in Excel.
    pub fn target(src: &Selection, p: CellPos) -> Option<Fill> {
        let (tl, br) = src.bounds();
        let beyond = |v: u32, lo: u32, hi: u32| {
            if v > hi { v - hi } else { lo.saturating_sub(v) }
        };
        let down = beyond(p.row, tl.row, br.row);
        let across = beyond(p.col, tl.col, br.col);
        if down == 0 && across == 0 {
            return None;
        }
        Some(if down >= across {
            Fill {
                lines: Lines::Rows,
                to: p.row,
            }
        } else {
            Fill {
                lines: Lines::Cols,
                to: p.col,
            }
        })
    }

    /// The selection `src` grows into: the source plus the filled cells.
    pub fn covering(&self, src: &Selection) -> Selection {
        let (mut tl, mut br) = src.bounds();
        match self.lines {
            Lines::Rows => {
                tl.row = tl.row.min(self.to);
                br.row = br.row.max(self.to);
            }
            Lines::Cols => {
                tl.col = tl.col.min(self.to);
                br.col = br.col.max(self.to);
            }
        }
        Selection {
            anchor: tl,
            cursor: br,
        }
    }
}

impl EditSession {
    /// Continues the cells `src` on `sheet` as far as `fill` says. One undo step.
    /// Returns the cells covered afterwards (source and filled ones).
    pub fn fill(&mut self, sheet: usize, src: &Selection, fill: Fill) -> Result<Selection, String> {
        let (r0, c0, r1, c1) = bounds(src);
        let area = Area {
            sheet: sheet as u32,
            row: r0 as i32 + 1,
            column: c0 as i32 + 1,
            width: (c1 - c0 + 1) as i32,
            height: (r1 - r0 + 1) as i32,
        };
        let to = fill.to as i32 + 1;
        match fill.lines {
            Lines::Rows => self.model.auto_fill_rows(&area, to)?,
            Lines::Cols => self.model.auto_fill_columns(&area, to)?,
        }
        self.done(Step::Cells);
        let out = fill.covering(src);
        let (tl, br) = out.bounds();
        self.watch(sheet, tl.row, tl.col, br.row, br.col);
        Ok(out)
    }

    /// Ctrl+D (`lines` = rows) or Ctrl+R (columns): copies the first row or column of
    /// `sel` over the rest of it, formulas moved along. A selection one line thick takes
    /// the line before it instead. One undo step; returns the cells filled.
    pub fn fill_copy(
        &mut self,
        sheet: usize,
        sel: &Selection,
        lines: Lines,
    ) -> Result<Selection, String> {
        let (tl, br) = sel.bounds();
        let (from, into) = match lines {
            Lines::Rows if br.row > tl.row => (
                (CellPos::new(tl.row, tl.col), CellPos::new(tl.row, br.col)),
                (CellPos::new(tl.row + 1, tl.col), br),
            ),
            Lines::Rows if tl.row > 0 => (
                (
                    CellPos::new(tl.row - 1, tl.col),
                    CellPos::new(tl.row - 1, br.col),
                ),
                (tl, br),
            ),
            Lines::Cols if br.col > tl.col => (
                (CellPos::new(tl.row, tl.col), CellPos::new(br.row, tl.col)),
                (CellPos::new(tl.row, tl.col + 1), br),
            ),
            Lines::Cols if tl.col > 0 => (
                (
                    CellPos::new(tl.row, tl.col - 1),
                    CellPos::new(br.row, tl.col - 1),
                ),
                (tl, br),
            ),
            // nothing before the first row or column to copy
            _ => return Ok(*sel),
        };
        let from = Selection {
            anchor: from.0,
            cursor: from.1,
        };
        let into = Selection {
            anchor: into.0,
            cursor: into.1,
        };
        self.paste_copied(sheet, &from, sheet, &into)?;
        Ok(*sel)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CellValue, Locale, Sheet};
    use std::collections::HashMap;

    fn sel(r0: u32, c0: u32, r1: u32, c1: u32) -> Selection {
        Selection {
            anchor: CellPos::new(r0, c0),
            cursor: CellPos::new(r1, c1),
        }
    }

    fn blank() -> EditSession {
        let sheet = Sheet::empty("S".into());
        EditSession::from_sheets(&[(&sheet, &HashMap::new())], Locale::Invariant).unwrap()
    }

    fn number(s: &EditSession, r: u32, c: u32) -> CellValue {
        s.cell(0, r, c).value
    }

    #[test]
    fn the_handle_extends_in_the_direction_it_left_farthest() {
        let src = sel(2, 2, 3, 3);
        assert_eq!(Fill::target(&src, CellPos::new(3, 3)), None);
        assert_eq!(
            Fill::target(&src, CellPos::new(9, 4)),
            Some(Fill {
                lines: Lines::Rows,
                to: 9
            })
        );
        assert_eq!(
            Fill::target(&src, CellPos::new(4, 7)),
            Some(Fill {
                lines: Lines::Cols,
                to: 7
            })
        );
        assert_eq!(
            Fill::target(&src, CellPos::new(0, 2)),
            Some(Fill {
                lines: Lines::Rows,
                to: 0
            })
        );
        let up = Fill {
            lines: Lines::Rows,
            to: 0,
        };
        assert_eq!(up.covering(&src), sel(0, 2, 3, 3));
    }

    #[test]
    fn dragging_continues_a_series_and_repeats_text() {
        let mut s = blank();
        s.set_input(0, 0, 0, "1").unwrap();
        s.set_input(0, 1, 0, "3").unwrap();
        s.set_input(0, 0, 1, "Mon").unwrap();
        s.set_input(0, 1, 1, "Tue").unwrap();
        s.set_input(0, 0, 2, "x").unwrap();
        s.set_input(0, 1, 2, "x").unwrap();
        let fill = Fill {
            lines: Lines::Rows,
            to: 3,
        };
        let out = s.fill(0, &sel(0, 0, 1, 2), fill).unwrap();
        assert_eq!(out, sel(0, 0, 3, 2));
        assert_eq!(number(&s, 2, 0), CellValue::Number(5.0));
        assert_eq!(number(&s, 3, 0), CellValue::Number(7.0));
        assert_eq!(s.content(0, 2, 1), "Wed");
        assert_eq!(s.content(0, 3, 1), "Thu");
        assert_eq!(s.content(0, 3, 2), "x");
        // one undo takes the whole fill back
        assert!(s.undo());
        assert_eq!(s.content(0, 2, 0), "");
        assert_eq!(s.content(0, 1, 0), "3");
    }

    #[test]
    fn dragging_a_formula_moves_its_references() {
        let mut s = blank();
        for r in 0..3 {
            s.set_input(0, r, 0, &(r + 1).to_string()).unwrap();
        }
        s.set_input(0, 0, 1, "=A1*2").unwrap();
        let fill = Fill {
            lines: Lines::Rows,
            to: 2,
        };
        s.fill(0, &sel(0, 1, 0, 1), fill).unwrap();
        assert_eq!(s.formula(0, 2, 1).as_deref(), Some("=A3*2"));
        assert_eq!(number(&s, 2, 1), CellValue::Number(6.0));
    }

    #[test]
    fn fill_down_and_right_copy_the_first_line() {
        let mut s = blank();
        s.set_input(0, 0, 0, "1").unwrap();
        s.set_input(0, 0, 1, "=A1+1").unwrap();
        s.fill_copy(0, &sel(0, 0, 3, 1), Lines::Rows).unwrap();
        // a copy, not a series
        assert_eq!(number(&s, 3, 0), CellValue::Number(1.0));
        assert_eq!(s.formula(0, 3, 1).as_deref(), Some("=A4+1"));
        assert!(s.undo());
        assert_eq!(s.content(0, 3, 0), "");

        // one row selected: the row above comes down
        s.fill_copy(0, &sel(1, 0, 1, 1), Lines::Rows).unwrap();
        assert_eq!(s.formula(0, 1, 1).as_deref(), Some("=A2+1"));

        s.fill_copy(0, &sel(0, 1, 0, 3), Lines::Cols).unwrap();
        assert_eq!(s.formula(0, 0, 3).as_deref(), Some("=C1+1"));
        // nothing left of column A
        assert_eq!(
            s.fill_copy(0, &sel(0, 0, 0, 0), Lines::Cols).unwrap(),
            sel(0, 0, 0, 0)
        );
    }
}
