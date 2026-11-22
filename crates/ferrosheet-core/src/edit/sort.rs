//! Sorting rows: every row of the range moves as a whole, with its formatting, and
//! relative references in its formulas move with it (as Excel and Calc do).

use std::collections::HashMap;

use ironcalc::base::ClipboardData;
use ironcalc::base::expressions::types::Area;

use super::{EditSession, Step, bounds};
use crate::{Selection, sort_order};

impl EditSession {
    /// Sorts the rows of `rows` on `sheet` by their value in column `key`. One undo
    /// step. Returns how many rows moved.
    pub fn sort(
        &mut self,
        sheet: usize,
        rows: &Selection,
        key: u32,
        descending: bool,
    ) -> Result<usize, String> {
        let (r0, c0, r1, c1) = bounds(rows);
        if r1 <= r0 {
            return Ok(0);
        }
        let keys: Vec<_> = (r0..=r1).map(|r| self.cell(sheet, r, key).value).collect();
        let order = sort_order(&keys, descending);
        let moves: Vec<(u32, u32)> = order
            .iter()
            .enumerate()
            .map(|(to, &from)| (r0 + from as u32, r0 + to as u32))
            .filter(|(from, to)| from != to)
            .collect();
        if moves.is_empty() {
            return Ok(0);
        }

        // Take every row as it is now, then write each one at its new place.
        let (s, first, last) = (sheet as u32, r0 as i32 + 1, r1 as i32 + 1);
        let (left, right) = (c0 as i32 + 1, c1 as i32 + 1);
        self.model.set_selected_sheet(s)?;
        self.model.set_selected_cell(first, left)?;
        self.model.set_selected_range(first, left, last, right)?;
        // The copy keeps its parts to itself; its serialized form is the way to them.
        let copied =
            serde_json::to_value(self.model.copy_to_clipboard()?).map_err(|e| e.to_string())?;
        let mut data: ClipboardData =
            serde_json::from_value(copied["data"].clone()).map_err(|e| e.to_string())?;

        self.model.pause_evaluation();
        // Empty the range first: a row that was empty (not in the copy) leaves nothing
        // behind where it lands.
        let area = Area {
            sheet: s,
            row: first,
            column: left,
            width: right - left + 1,
            height: last - first + 1,
        };
        let mut result = self.model.range_clear_all(&area);
        let mut entries = usize::from(result.is_ok());
        for (to, &from) in order.iter().enumerate() {
            if result.is_err() {
                break;
            }
            let (from, to) = (r0 + from as u32, r0 + to as u32);
            let row = from as i32 + 1;
            // Rows past the last used one are empty and not part of the copy.
            let cells = data.remove(&row).unwrap_or_default();
            let one: ClipboardData = HashMap::from([(row, cells)]);
            result = self
                .model
                .set_selected_cell(to as i32 + 1, left)
                .and_then(|()| {
                    self.model
                        .paste_from_clipboard(s, (row, left, row, right), &one, false)
                });
            if result.is_ok() {
                entries += 1;
            }
        }
        self.model.resume_evaluation();
        self.model.evaluate();
        if entries > 0 {
            self.done_n(Step::Cells, entries);
        }
        self.watch(sheet, r0, c0, r1, c1);
        result.map(|()| moves.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CellPos, CellValue, Locale, Sheet};

    fn blank() -> EditSession {
        let sheet = Sheet::empty("S".into());
        EditSession::from_sheets(&[(&sheet, &HashMap::new())], Locale::Invariant).unwrap()
    }

    fn rows(r0: u32, c0: u32, r1: u32, c1: u32) -> Selection {
        Selection {
            anchor: CellPos::new(r0, c0),
            cursor: CellPos::new(r1, c1),
        }
    }

    #[test]
    fn rows_move_as_a_whole_with_their_formulas_and_one_undo_takes_it_back() {
        let mut s = blank();
        for (r, (name, qty)) in [("pear", "3"), ("apple", "1"), ("fig", "2")]
            .iter()
            .enumerate()
        {
            let r = r as u32;
            s.set_input(0, r, 0, name).unwrap();
            s.set_input(0, r, 1, qty).unwrap();
            s.set_input(0, r, 2, &format!("=B{}*10", r + 1)).unwrap();
        }
        s.set_input(0, 5, 0, "=SUM(C1:C3)").unwrap();
        let moved = s.sort(0, &rows(0, 0, 2, 2), 1, false).unwrap();
        assert_eq!(moved, 3);
        let names: Vec<_> = (0..3).map(|r| s.content(0, r, 0)).collect();
        assert_eq!(names, ["apple", "fig", "pear"]);
        // each formula still points at its own row
        assert_eq!(s.formula(0, 0, 2).as_deref(), Some("=B1*10"));
        assert_eq!(s.cell(0, 0, 2).value, CellValue::Number(10.0));
        assert_eq!(s.cell(0, 2, 2).value, CellValue::Number(30.0));
        assert_eq!(s.cell(0, 5, 0).value, CellValue::Number(60.0));

        s.sort(0, &rows(0, 0, 2, 2), 0, true).unwrap();
        let names: Vec<_> = (0..3).map(|r| s.content(0, r, 0)).collect();
        assert_eq!(names, ["pear", "fig", "apple"]);

        assert!(s.undo());
        assert_eq!(s.content(0, 0, 0), "apple");
        assert!(s.undo());
        assert_eq!(s.content(0, 0, 0), "pear");
        assert_eq!(s.content(0, 1, 0), "apple");
    }

    #[test]
    fn an_empty_row_in_the_range_goes_last() {
        let mut s = blank();
        s.set_input(0, 0, 0, "b").unwrap();
        s.set_input(0, 2, 0, "a").unwrap();
        s.set_input(0, 2, 1, "x").unwrap();
        s.sort(0, &rows(0, 0, 2, 1), 0, false).unwrap();
        assert_eq!(s.content(0, 0, 0), "a");
        assert_eq!(s.content(0, 0, 1), "x");
        assert_eq!(s.content(0, 1, 0), "b");
        assert_eq!(s.content(0, 2, 0), "");
        assert_eq!(s.content(0, 2, 1), "");
    }
}
