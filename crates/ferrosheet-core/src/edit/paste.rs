//! Paste of cells copied inside ferrosheet: formulas follow the new position, and
//! formatting and links come along. A block pasted over a selection that is a
//! multiple of its size is repeated to fill it, as in Excel and Calc.

use ironcalc::base::ClipboardData;

use super::{EditSession, Step, bounds};
use crate::{CellPos, Selection};

/// How often a block of `block` (rows, columns) is repeated over a selection of
/// `target`: in both directions a whole multiple, otherwise once.
pub(super) fn tiles(block: (u32, u32), target: (u32, u32)) -> (u32, u32) {
    let (bh, bw) = block;
    let (th, tw) = target;
    if bh == 0 || bw == 0 || th % bh != 0 || tw % bw != 0 {
        return (1, 1);
    }
    (th / bh, tw / bw)
}

impl EditSession {
    /// Pastes the cells at `from` on sheet `from_sheet` (as they are now) at the top
    /// left of `sel` on `sheet`, with formatting, links and relative references moved
    /// along, repeated over `sel` if it is a multiple of the block. One undo step.
    /// Returns the cells that were pasted into.
    pub fn paste_copied(
        &mut self,
        from_sheet: usize,
        from: &Selection,
        sheet: usize,
        sel: &Selection,
    ) -> Result<Selection, String> {
        let (fr0, fc0, fr1, fc1) = bounds(from);
        self.model.set_selected_sheet(from_sheet as u32)?;
        self.model
            .set_selected_cell(fr0 as i32 + 1, fc0 as i32 + 1)?;
        self.model.set_selected_range(
            fr0 as i32 + 1,
            fc0 as i32 + 1,
            fr1 as i32 + 1,
            fc1 as i32 + 1,
        )?;
        // The copy keeps its parts to itself; its serialized form is the way to them.
        let copied =
            serde_json::to_value(self.model.copy_to_clipboard()?).map_err(|e| e.to_string())?;
        let range: (i32, i32, i32, i32) =
            serde_json::from_value(copied["range"].clone()).map_err(|e| e.to_string())?;
        let data: ClipboardData =
            serde_json::from_value(copied["data"].clone()).map_err(|e| e.to_string())?;
        // The copy stops at the last used row and column of the source sheet.
        let block = (
            (range.2 - range.0 + 1) as u32,
            (range.3 - range.1 + 1) as u32,
        );

        let (r0, c0, r1, c1) = bounds(sel);
        let (down, across) = tiles(block, (r1 - r0 + 1, c1 - c0 + 1));
        self.model.set_selected_sheet(sheet as u32)?;
        let mut entries = 0;
        let mut result = Ok(());
        'tiles: for i in 0..down {
            for j in 0..across {
                let (row, col) = (r0 + i * block.0, c0 + j * block.1);
                result = self
                    .model
                    .set_selected_cell(row as i32 + 1, col as i32 + 1)
                    .and_then(|()| {
                        self.model
                            .paste_from_clipboard(from_sheet as u32, range, &data, false)
                    });
                if result.is_err() {
                    break 'tiles;
                }
                entries += 1;
            }
        }
        if entries > 0 {
            self.done_n(Step::Cells, entries);
        }
        let (r1, c1) = (r0 + down * block.0 - 1, c0 + across * block.1 - 1);
        self.watch(sheet, r0, c0, r1, c1);
        result?;
        Ok(Selection {
            anchor: CellPos::new(r0, c0),
            cursor: CellPos::new(r1, c1),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CellValue, Locale};

    #[test]
    fn tiles_only_whole_multiples() {
        assert_eq!(tiles((2, 1), (6, 3)), (3, 3));
        assert_eq!(tiles((2, 2), (2, 2)), (1, 1));
        assert_eq!(tiles((2, 2), (3, 4)), (1, 1));
        assert_eq!(tiles((2, 2), (1, 1)), (1, 1));
        assert_eq!(tiles((0, 2), (4, 4)), (1, 1));
    }

    fn sel(r0: u32, c0: u32, r1: u32, c1: u32) -> Selection {
        Selection {
            anchor: CellPos::new(r0, c0),
            cursor: CellPos::new(r1, c1),
        }
    }

    fn blank() -> EditSession {
        let sheet = crate::Sheet::empty("S".into());
        EditSession::from_sheets(
            &[(&sheet, &std::collections::HashMap::new())],
            Locale::Invariant,
        )
        .unwrap()
    }

    #[test]
    fn formulas_shift_and_formatting_comes_along() {
        let mut s = blank();
        s.set_input(0, 0, 0, "2").unwrap();
        s.set_input(0, 1, 0, "3").unwrap();
        s.set_input(0, 0, 1, "=A1*10").unwrap();
        s.set_input(0, 1, 1, "=$A$1+A2").unwrap();
        s.set_style(0, &sel(0, 1, 1, 1), crate::StyleChange::Bold(true))
            .unwrap();
        s.set_style(
            0,
            &sel(0, 1, 0, 1),
            crate::StyleChange::Fill(Some([255, 0, 0])),
        )
        .unwrap();

        let pasted = s
            .paste_copied(0, &sel(0, 1, 1, 1), 0, &sel(4, 3, 4, 3))
            .unwrap();
        assert_eq!(pasted, sel(4, 3, 5, 3));
        assert_eq!(s.formula(0, 4, 3).as_deref(), Some("=C5*10"));
        assert_eq!(s.formula(0, 5, 3).as_deref(), Some("=$A$1+C6"));
        assert!(s.style(0, 4, 3).bold);
        assert!(s.style(0, 5, 3).bold);
        assert_eq!(s.style(0, 4, 3).fill, s.style(0, 0, 1).fill);
        assert_ne!(s.style(0, 5, 3).fill, s.style(0, 4, 3).fill);
        // the new cells are watched, so the grid gets their values
        let u = s.updates(0);
        assert!(u.iter().any(|c| (c.row, c.col) == (5, 3)));

        assert!(s.undo());
        assert_eq!(s.content(0, 4, 3), "");
        assert!(!s.style(0, 4, 3).bold);
    }

    #[test]
    fn a_block_tiles_a_larger_selection_in_one_undo_step() {
        let mut s = blank();
        s.set_input(0, 0, 0, "1").unwrap();
        s.set_input(0, 0, 1, "=A1+1").unwrap();
        let pasted = s
            .paste_copied(0, &sel(0, 0, 0, 1), 0, &sel(2, 0, 4, 3))
            .unwrap();
        assert_eq!(pasted, sel(2, 0, 4, 3));
        for r in 2..=4 {
            assert_eq!(s.content(0, r, 0), "1");
            assert_eq!(s.content(0, r, 2), "1");
            assert_eq!(s.formula(0, r, 1), Some(format!("=A{}+1", r + 1)));
            assert_eq!(s.formula(0, r, 3), Some(format!("=C{}+1", r + 1)));
        }
        assert!(s.undo());
        for r in 2..=4 {
            for c in 0..=3 {
                assert_eq!(s.content(0, r, c), "", "{r},{c}");
            }
        }
        assert_eq!(s.content(0, 0, 1), "=A1+1");
    }

    #[test]
    fn a_selection_that_is_no_multiple_gets_the_block_once() {
        let mut s = blank();
        s.set_input(0, 0, 0, "x").unwrap();
        s.set_input(0, 1, 0, "y").unwrap();
        let pasted = s
            .paste_copied(0, &sel(0, 0, 1, 0), 0, &sel(3, 0, 5, 0))
            .unwrap();
        assert_eq!(pasted, sel(3, 0, 4, 0));
        assert_eq!(s.content(0, 5, 0), "");
    }

    #[test]
    fn pastes_onto_another_sheet() {
        let mut s = blank();
        s.add_sheet().unwrap();
        s.set_input(0, 0, 0, "5").unwrap();
        s.set_input(0, 0, 1, "=A1*2").unwrap();
        s.paste_copied(0, &sel(0, 0, 0, 1), 1, &sel(1, 1, 1, 1))
            .unwrap();
        assert_eq!(s.formula(1, 1, 2).as_deref(), Some("=B2*2"));
        let u = s.updates(1);
        let v = u.iter().find(|c| (c.row, c.col) == (1, 1)).unwrap();
        assert_eq!(v.value, CellValue::Number(5.0));
    }
}
