//! Sorting: which cells a sort covers, whether their first row is a header, and the
//! order of the rows. Moving the cells is done by [`crate::EditSession::sort`].

use std::cmp::Ordering;

use crate::{CellPos, CellValue, Selection, Sheet};

/// What a sort covers, as worked out from the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortRange {
    /// The cells, header included.
    pub area: Selection,
    /// The first row is a header and stays where it is.
    pub header: bool,
    /// The column the rows are ordered by.
    pub key: u32,
}

impl SortRange {
    /// The rows that move: the area without its header.
    pub fn rows(&self) -> Selection {
        let (mut tl, br) = self.area.bounds();
        if self.header {
            tl.row += 1;
        }
        Selection {
            anchor: tl,
            cursor: br,
        }
    }
}

impl Sheet {
    /// The block of filled cells around `at`, bounded by empty rows and columns (what
    /// Excel calls the current region). A single empty cell is its own region.
    pub fn region(&self, at: CellPos) -> Selection {
        let filled = |r: u32, c: u32| !matches!(self.value(r, c), CellValue::Empty);
        let (mut r0, mut c0, mut r1, mut c1) = (at.row, at.col, at.row, at.col);
        loop {
            let (lo_c, hi_c) = (c0.saturating_sub(1), c1 + 1);
            let (lo_r, hi_r) = (r0.saturating_sub(1), r1 + 1);
            let mut grew = false;
            if r0 > 0 && (lo_c..=hi_c).any(|c| filled(r0 - 1, c)) {
                r0 -= 1;
                grew = true;
            }
            if r1 + 1 < self.rows && (lo_c..=hi_c).any(|c| filled(r1 + 1, c)) {
                r1 += 1;
                grew = true;
            }
            if c0 > 0 && (lo_r..=hi_r).any(|r| filled(r, c0 - 1)) {
                c0 -= 1;
                grew = true;
            }
            if c1 + 1 < self.cols && (lo_r..=hi_r).any(|r| filled(r, c1 + 1)) {
                c1 += 1;
                grew = true;
            }
            if !grew {
                break;
            }
        }
        Selection {
            anchor: CellPos::new(r0, c0),
            cursor: CellPos::new(r1, c1),
        }
    }

    /// Whether the first row of `area` reads as a header: only text there, and below
    /// it at least one column holds something else (a number, date, …) or the header
    /// cell is bold.
    pub fn has_header(&self, area: &Selection) -> bool {
        let (tl, br) = area.bounds();
        if br.row == tl.row {
            return false;
        }
        let mut any = false;
        for c in tl.col..=br.col {
            match self.value(tl.row, c) {
                CellValue::Text(_) => any = true,
                CellValue::Empty => {}
                _ => return false,
            }
        }
        any && (tl.col..=br.col).any(|c| {
            (self.style(tl.row, c).bold && !self.style(tl.row + 1, c).bold)
                || (tl.row + 1..=br.row)
                    .any(|r| !matches!(self.value(r, c), CellValue::Empty | CellValue::Text(_)))
        })
    }

    /// What a sort by the cursor's column covers: the selection, or the region around
    /// the cursor when only one cell is selected. A header is guessed when the cells
    /// start at the top of the sheet or were found as a region.
    pub fn sort_range(&self, sel: &Selection) -> SortRange {
        let single = sel.cell_count() == 1;
        let area = if single {
            self.region(sel.cursor)
        } else {
            // a whole column selected stops at the last used row
            let (tl, br) = sel.bounds();
            let last = self.rows.max(1) - 1;
            Selection {
                anchor: tl,
                cursor: CellPos::new(br.row.min(last).max(tl.row), br.col),
            }
        };
        let (tl, _) = area.bounds();
        let header = (single || tl.row == 0) && self.has_header(&area);
        SortRange {
            area,
            header,
            key: sel.cursor.col,
        }
    }
}

/// Rank of a value's kind in an ascending sort, as in Excel: numbers (and dates),
/// text, logical values, errors. Empty cells are not ranked; they always go last.
fn kind(v: &CellValue) -> u8 {
    match v {
        CellValue::Number(_) | CellValue::DateTime(_) | CellValue::Duration(_) => 0,
        CellValue::Text(_) => 1,
        CellValue::Bool(_) => 2,
        CellValue::Error(_) => 3,
        CellValue::Empty => 4,
    }
}

fn compare(a: &CellValue, b: &CellValue) -> Ordering {
    use CellValue::*;
    match (a, b) {
        (Number(x) | DateTime(x) | Duration(x), Number(y) | DateTime(y) | Duration(y)) => {
            x.total_cmp(y)
        }
        (Text(x), Text(y)) => x.to_lowercase().cmp(&y.to_lowercase()),
        (Bool(x), Bool(y)) => x.cmp(y),
        _ => kind(a).cmp(&kind(b)),
    }
}

/// The order the rows with these key values go in: `order[i]` is the row (index into
/// `keys`) that ends up at position `i`. Stable; empty keys stay last either way.
pub fn sort_order(keys: &[CellValue], descending: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..keys.len()).collect();
    order.sort_by(|&i, &j| {
        let (a, b) = (&keys[i], &keys[j]);
        match (a, b) {
            (CellValue::Empty, CellValue::Empty) => Ordering::Equal,
            (CellValue::Empty, _) => Ordering::Greater,
            (_, CellValue::Empty) => Ordering::Less,
            _ if descending => compare(b, a),
            _ => compare(a, b),
        }
    });
    order
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Locale;

    fn t(s: &str) -> CellValue {
        CellValue::Text(s.into())
    }

    fn n(x: f64) -> CellValue {
        CellValue::Number(x)
    }

    #[test]
    fn numbers_come_before_text_and_empty_cells_stay_last() {
        let keys = [t("b"), n(10.0), CellValue::Empty, t("A"), n(2.0), t("a")];
        assert_eq!(sort_order(&keys, false), [4, 1, 3, 5, 0, 2]);
        assert_eq!(sort_order(&keys, true), [0, 3, 5, 1, 4, 2]);
    }

    fn sheet() -> Sheet {
        // Name | Qty, three rows, then an empty row and an unrelated cell
        let cells = vec![
            t("Name"),
            t("Qty"),
            t("pear"),
            n(3.0),
            t("apple"),
            n(1.0),
            t("fig"),
            n(2.0),
            CellValue::Empty,
            CellValue::Empty,
            t("note"),
            CellValue::Empty,
        ];
        Sheet::from_cells("S".into(), 6, 2, cells, Locale::Invariant)
    }

    #[test]
    fn a_single_cell_sorts_its_region_and_spares_the_header() {
        let s = sheet();
        let sel = Selection::single(CellPos::new(2, 1));
        let range = s.sort_range(&sel);
        assert_eq!(
            range.area.bounds(),
            (CellPos::new(0, 0), CellPos::new(3, 1))
        );
        assert!(range.header);
        assert_eq!(range.key, 1);
        assert_eq!(range.rows().bounds().0, CellPos::new(1, 0));
    }

    #[test]
    fn a_selection_below_the_top_has_no_header_guessed() {
        let s = sheet();
        let sel = Selection {
            anchor: CellPos::new(1, 0),
            cursor: CellPos::new(3, 1),
        };
        let range = s.sort_range(&sel);
        assert!(!range.header);
        assert_eq!(range.area, sel);
        // text only, nothing bold: no header
        let words = Sheet::from_cells(
            "W".into(),
            3,
            1,
            vec![t("b"), t("a"), t("c")],
            Locale::Invariant,
        );
        let all = words.sort_range(&Selection::single(CellPos::new(0, 0)));
        assert!(!all.header);
    }
}
