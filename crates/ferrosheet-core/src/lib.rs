//! Core of ferrosheet: loading spreadsheets into a display-ready model,
//! selection and clipboard text, and search. No GUI dependencies.

mod csv;
mod edit;
mod formula;
mod losses;
mod search;
mod selection;
mod sheet;
mod value;
mod workbook;

pub use csv::{Encoding, TextFormat, write as write_csv};
pub use edit::{CellUpdate, EditSession, Lines, NumberFormat, StyleChange};
pub use formula::{
    FormulaRef, accepts_ref, find_refs, parse_reference, ref_slot, ref_text, splice,
};
pub use losses::{Loss, save_losses};
pub use search::{Query, find_all};
pub use selection::{CellPos, CopyFormat, Selection, copy_text};
pub use sheet::{CellStyle, Sheet, Summary};
pub use value::{CellError, CellValue, Locale, today_input};
pub use workbook::{EXTENSIONS, LoadError, PREVIEW_ROWS, Workbook, cell_address, column_name};
