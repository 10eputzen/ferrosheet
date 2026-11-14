//! Row geometry with individually resized rows.
//!
//! Sheets can have a million rows, so heights are not stored per row: all rows share the
//! default height and only rows the user resized are kept in a sorted map. Lookups walk
//! that map, which stays tiny in practice.

use std::collections::BTreeMap;

pub const ROW_H: f32 = 22.0;
pub const MIN_ROW_H: f32 = 8.0;

pub struct RowHeights {
    custom: BTreeMap<u32, f32>,
    /// Height of a row nobody resized; `ROW_H` at 100 % zoom.
    base: f32,
}

impl Default for RowHeights {
    fn default() -> Self {
        RowHeights {
            custom: BTreeMap::new(),
            base: ROW_H,
        }
    }
}

impl RowHeights {
    pub fn base(&self) -> f32 {
        self.base
    }

    /// Scales every row by `f` (zooming).
    pub fn rescale(&mut self, f: f32) {
        self.base *= f;
        for h in self.custom.values_mut() {
            *h *= f;
        }
    }

    pub fn height(&self, r: u32) -> f32 {
        self.custom.get(&r).copied().unwrap_or(self.base)
    }

    pub fn set(&mut self, r: u32, h: f32) {
        let h = h.max(MIN_ROW_H);
        if (h - self.base).abs() < 0.5 {
            self.custom.remove(&r);
        } else {
            self.custom.insert(r, h);
        }
    }

    /// `count` rows were inserted before row `at`: resized rows below move down. `restore`
    /// gives heights for the new rows (offset from `at`), e.g. when a deletion is undone.
    pub fn insert(&mut self, at: u32, count: u32, restore: &[(u32, f32)]) {
        let below = self.custom.split_off(&at);
        for (r, h) in below {
            self.custom.insert(r.saturating_add(count), h);
        }
        for &(off, h) in restore.iter().filter(|(off, _)| *off < count) {
            self.custom.insert(at + off, h);
        }
    }

    /// `count` rows from `at` were deleted: resized rows below move up. Returns the
    /// heights of the deleted rows that were resized (offset from `at`).
    pub fn delete(&mut self, at: u32, count: u32) -> Vec<(u32, f32)> {
        let mut gone = self.custom.split_off(&at);
        let below = gone.split_off(&at.saturating_add(count));
        for (r, h) in below {
            self.custom.insert(r - count, h);
        }
        gone.into_iter().map(|(r, h)| (r - at, h)).collect()
    }

    /// Distance from the top of the sheet to the top of row `r`.
    pub fn top(&self, r: u32) -> f32 {
        r as f32 * self.base
            + self
                .custom
                .range(..r)
                .map(|(_, h)| h - self.base)
                .sum::<f32>()
    }

    /// Row that contains the vertical position `y` (clamped to the first row).
    pub fn row_at(&self, y: f32) -> u32 {
        let y = y.max(0.0);
        let mut extra = 0.0;
        for (&k, &h) in &self.custom {
            let k_top = k as f32 * self.base + extra;
            if y < k_top {
                break;
            }
            if y < k_top + h {
                return k;
            }
            extra += h - self.base;
        }
        ((y - extra) / self.base) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_rows() {
        let rh = RowHeights::default();
        assert_eq!(rh.top(3), 3.0 * ROW_H);
        assert_eq!(rh.row_at(0.0), 0);
        assert_eq!(rh.row_at(ROW_H * 2.5), 2);
        assert_eq!(rh.row_at(-5.0), 0);
    }

    #[test]
    fn resized_row_shifts_rows_below() {
        let mut rh = RowHeights::default();
        rh.set(2, 60.0);
        assert_eq!(rh.height(2), 60.0);
        assert_eq!(rh.top(2), 2.0 * ROW_H);
        assert_eq!(rh.top(3), 2.0 * ROW_H + 60.0);
        assert_eq!(rh.row_at(2.0 * ROW_H + 59.0), 2);
        assert_eq!(rh.row_at(2.0 * ROW_H + 60.0), 3);
        assert_eq!(rh.row_at(2.0 * ROW_H - 1.0), 1);
    }

    #[test]
    fn several_resized_rows() {
        let mut rh = RowHeights::default();
        rh.set(1, 10.0);
        rh.set(4, 50.0);
        // rows 0..5: 22, 10, 22, 22, 50
        assert_eq!(rh.top(5), 22.0 + 10.0 + 22.0 + 22.0 + 50.0);
        assert_eq!(rh.row_at(22.0 + 5.0), 1);
        assert_eq!(rh.row_at(22.0 + 10.0 + 22.0 + 22.0 + 49.0), 4);
        assert_eq!(rh.row_at(rh.top(5) + 1.0), 5);
    }

    #[test]
    fn rows_move_with_insert_and_delete() {
        let mut rh = RowHeights::default();
        rh.set(1, 10.0);
        rh.set(4, 50.0);
        rh.set(5, 60.0);
        rh.insert(2, 3, &[]);
        assert_eq!(rh.height(1), 10.0);
        assert_eq!(rh.height(4), ROW_H);
        assert_eq!(rh.height(7), 50.0);
        assert_eq!(rh.height(8), 60.0);
        let gone = rh.delete(7, 1);
        assert_eq!(gone, vec![(0, 50.0)]);
        assert_eq!(rh.height(7), 60.0);
        // Undoing the deletion brings the height back
        rh.insert(7, 1, &gone);
        assert_eq!(rh.height(7), 50.0);
        assert_eq!(rh.height(8), 60.0);
    }

    #[test]
    fn default_height_and_minimum() {
        let mut rh = RowHeights::default();
        rh.set(0, 60.0);
        rh.set(0, ROW_H);
        assert!(rh.custom.is_empty());
        rh.set(1, 1.0);
        assert_eq!(rh.height(1), MIN_ROW_H);
    }
}
