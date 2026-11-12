//! Opening workbooks in any supported format and loading sheets on demand.

use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use calamine::{CellErrorType, Data, ExcelDateTime, Reader, Sheets, open_workbook_auto};
use chrono::NaiveDate;

use crate::{CellError, CellValue, Sheet};

/// File extensions ferrosheet can open.
pub const EXTENSIONS: &[&str] = &["xls", "xlsx", "xlsm", "xlsb", "ods"];

/// Why a file or sheet could not be loaded.
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Format(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "Datei konnte nicht gelesen werden: {e}"),
            LoadError::Format(e) => write!(f, "Datei ist keine lesbare Tabelle: {e}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<calamine::Error> for LoadError {
    fn from(e: calamine::Error) -> Self {
        match e {
            calamine::Error::Io(e) => LoadError::Io(e),
            other => LoadError::Format(other.to_string()),
        }
    }
}

/// An open workbook. Sheets are read only when asked for.
pub struct Workbook {
    pub path: PathBuf,
    pub sheet_names: Vec<String>,
    inner: Sheets<BufReader<File>>,
}

impl Workbook {
    pub fn open(path: &Path) -> Result<Workbook, LoadError> {
        if !path.exists() {
            return Err(LoadError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                path.display().to_string(),
            )));
        }
        let inner = open_workbook_auto(path)?;
        Ok(Workbook {
            path: path.to_path_buf(),
            sheet_names: inner.sheet_names(),
            inner,
        })
    }

    /// Reads sheet `idx` and converts it into a display-ready [`Sheet`].
    pub fn load_sheet(&mut self, idx: usize) -> Result<Sheet, LoadError> {
        let name = self.sheet_name(idx)?;
        let range = self
            .inner
            .worksheet_range_at(idx)
            .ok_or_else(|| LoadError::Format(format!("Blatt {idx} fehlt")))??;
        let Some((r0, c0)) = range.start() else {
            return Ok(Sheet::empty(name));
        };
        let (height, width) = range.get_size();
        let (rows, cols) = (r0 + height as u32, c0 + width as u32);
        let mut cells = vec![CellValue::Empty; rows as usize * cols as usize];
        for (r, c, data) in range.used_cells() {
            let idx = (r0 as usize + r) * cols as usize + c0 as usize + c;
            cells[idx] = convert(data);
        }
        Ok(Sheet::from_cells(name, rows, cols, cells))
    }

    /// Formulas of sheet `idx`, keyed by `(row, col)`, always starting with `=`.
    pub fn load_formulas(&mut self, idx: usize) -> Result<HashMap<(u32, u32), String>, LoadError> {
        let name = self.sheet_name(idx)?;
        let range = self.inner.worksheet_formula(&name)?;
        let Some((r0, c0)) = range.start() else {
            return Ok(HashMap::new());
        };
        Ok(range
            .used_cells()
            .filter(|(_, _, f)| !f.is_empty())
            .map(|(r, c, f)| {
                let f = if f.starts_with('=') {
                    f.clone()
                } else {
                    format!("={f}")
                };
                ((r0 + r as u32, c0 + c as u32), f)
            })
            .collect())
    }

    fn sheet_name(&self, idx: usize) -> Result<String, LoadError> {
        self.sheet_names
            .get(idx)
            .cloned()
            .ok_or_else(|| LoadError::Format(format!("Blatt {idx} fehlt")))
    }
}

fn convert(data: &Data) -> CellValue {
    match data {
        Data::Empty => CellValue::Empty,
        Data::String(s) => CellValue::Text(s.as_str().into()),
        Data::DateTimeIso(s) => {
            iso_datetime(s).map_or_else(|| CellValue::Text(s.as_str().into()), CellValue::DateTime)
        }
        Data::DurationIso(s) => {
            iso_duration(s).map_or_else(|| CellValue::Text(s.as_str().into()), CellValue::Duration)
        }
        Data::Int(i) => CellValue::Number(*i as f64),
        Data::Float(f) => CellValue::Number(*f),
        Data::Bool(b) => CellValue::Bool(*b),
        Data::DateTime(d) if d.is_duration() => CellValue::Duration(d.as_f64()),
        Data::DateTime(d) => CellValue::DateTime(serial_1900(d)),
        Data::Error(e) => CellValue::Error(match e {
            CellErrorType::Div0 => CellError::Div0,
            CellErrorType::NA => CellError::NA,
            CellErrorType::Name => CellError::Name,
            CellErrorType::Null => CellError::Null,
            CellErrorType::Num => CellError::Num,
            CellErrorType::Ref => CellError::Ref,
            CellErrorType::Value => CellError::Value,
            CellErrorType::GettingData => CellError::GettingData,
        }),
    }
}

/// Normalises a date to the 1900 date system (files may use the 1904 system).
fn serial_1900(d: &ExcelDateTime) -> f64 {
    let raw = d.as_f64();
    if raw < 61.0 {
        return raw; // pure times and the first days of 1900 need no conversion
    }
    d.as_datetime()
        .map(|dt| (dt - base_1900()).num_milliseconds() as f64 / 86_400_000.0)
        .unwrap_or(raw)
}

fn base_1900() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(1899, 12, 30)
        .and_then(|b| b.and_hms_opt(0, 0, 0))
        .expect("valid base date")
}

/// ODS dates (`2021-01-01`, `2021-01-01T10:10:10[.fff]`) as 1900 serial.
fn iso_datetime(s: &str) -> Option<f64> {
    let dt = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| {
            NaiveDate::parse_from_str(s, "%Y-%m-%d").map(|d| d.and_time(Default::default()))
        })
        .ok()?;
    Some((dt - base_1900()).num_milliseconds() as f64 / 86_400_000.0)
}

/// ODS durations (`PT10H10M10.5S`, `P1DT2H`) in days.
fn iso_duration(s: &str) -> Option<f64> {
    let (neg, s) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
    let s = s.strip_prefix('P')?;
    let (mut secs, mut num, mut in_time) = (0.0, String::new(), false);
    for ch in s.chars() {
        match ch {
            'T' => in_time = true,
            '0'..='9' | '.' | ',' => num.push(if ch == ',' { '.' } else { ch }),
            unit => {
                let n: f64 = num.parse().ok()?;
                num.clear();
                secs += n * match (unit, in_time) {
                    ('D', false) => 86_400.0,
                    ('H', true) => 3_600.0,
                    ('M', true) => 60.0,
                    ('S', true) => 1.0,
                    _ => return None,
                };
            }
        }
    }
    num.is_empty()
        .then_some(if neg { -secs } else { secs } / 86_400.0)
}

/// Column letters: 0 → `A`, 25 → `Z`, 26 → `AA`.
pub fn column_name(mut c: u32) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'A' + (c % 26) as u8);
        if c < 26 {
            break;
        }
        c = c / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ASCII")
}

/// Cell address like `B3` for `(2, 1)`.
pub fn cell_address(r: u32, c: u32) -> String {
    format!("{}{}", column_name(c), r + 1)
}
