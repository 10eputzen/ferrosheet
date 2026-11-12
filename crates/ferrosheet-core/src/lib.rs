//! Core of ferrosheet: loading spreadsheets into a display-ready model,
//! selection and clipboard text, and search. No GUI dependencies.

mod csv;
mod search;
mod selection;
mod sheet;
mod value;
mod workbook;

pub use search::find_all;
pub use selection::{CellPos, CopyFormat, Selection, copy_text};
pub use sheet::Sheet;
pub use value::{CellError, CellValue, Locale};
pub use workbook::{EXTENSIONS, LoadError, PREVIEW_ROWS, Workbook, cell_address, column_name};
