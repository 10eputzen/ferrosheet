//! A loaded worksheet: a dense row-major grid with precomputed display text.

use std::sync::OnceLock;

use crate::{CellValue, Locale};

/// A worksheet ready for display. Cell `(0, 0)` is `A1`.
#[derive(Debug, Default)]
pub struct Sheet {
    pub name: String,
    pub rows: u32,
    pub cols: u32,
    cells: Vec<CellValue>,
    display: Vec<Box<str>>,
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

    /// Builds a sheet from row-major cells (`cells.len() == rows * cols`), with the
    /// display text in `locale`.
    pub fn from_cells(
        name: String,
        rows: u32,
        cols: u32,
        cells: Vec<CellValue>,
        locale: Locale,
    ) -> Sheet {
        debug_assert_eq!(cells.len(), rows as usize * cols as usize);
        let display = cells
            .iter()
            .map(|c| c.format(locale).into_boxed_str())
            .collect();
        Sheet {
            name,
            rows,
            cols,
            cells,
            display,
            lower: OnceLock::new(),
        }
    }

    fn index(&self, r: u32, c: u32) -> Option<usize> {
        (r < self.rows && c < self.cols).then(|| r as usize * self.cols as usize + c as usize)
    }

    /// The value at `(r, c)`, `Empty` outside the used range.
    pub fn value(&self, r: u32, c: u32) -> &CellValue {
        self.index(r, c).map_or(&EMPTY, |i| &self.cells[i])
    }

    /// The display text at `(r, c)`, `""` outside the used range.
    pub fn display(&self, r: u32, c: u32) -> &str {
        self.index(r, c).map_or("", |i| &self.display[i])
    }

    /// Lower-cased display text of all cells, row-major. Built once.
    pub(crate) fn lower(&self) -> &[Box<str>] {
        self.lower.get_or_init(|| {
            self.display
                .iter()
                .map(|s| s.to_lowercase().into_boxed_str())
                .collect()
        })
    }
}
