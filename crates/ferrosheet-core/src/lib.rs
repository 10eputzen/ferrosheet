//! Core of ferrosheet: loading spreadsheets into a display-ready model,
//! selection and clipboard text, and search. No GUI dependencies.

mod search;
mod selection;
mod sheet;
mod value;
mod workbook;

pub use search::find_all;
pub use selection::{CellPos, Selection, copy_text};
pub use sheet::Sheet;
pub use value::{CellError, CellValue, CopyFormat};
pub use workbook::{EXTENSIONS, LoadError, Workbook, cell_address, column_name};
