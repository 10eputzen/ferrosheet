//! Cell values and how they are rendered as text.

use chrono::{Duration, NaiveDate};

/// A single cell value, independent of the file format it came from.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum CellValue {
    #[default]
    Empty,
    Text(Box<str>),
    Number(f64),
    Bool(bool),
    /// Excel serial date (days since 1899-12-30, fractional part = time of day).
    DateTime(f64),
    /// A duration in days (e.g. `[h]:mm` formatted cells).
    Duration(f64),
    Error(CellError),
}

/// Spreadsheet error values such as `#DIV/0!`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CellError {
    Div0,
    NA,
    Value,
    Ref,
    Name,
    Num,
    Null,
    GettingData,
}

/// How values are turned into text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyFormat {
    /// German display format: decimal comma, `TT.MM.JJJJ`, `WAHR`/`FALSCH`.
    Display,
    /// Locale-neutral: decimal point, ISO dates, `TRUE`/`FALSE`.
    Invariant,
}

impl CellValue {
    /// Renders the value as text in the given format.
    pub fn format(&self, f: CopyFormat) -> String {
        match self {
            CellValue::Empty => String::new(),
            CellValue::Text(s) => s.to_string(),
            CellValue::Number(n) => format_number(*n, f),
            CellValue::Bool(b) => match (b, f) {
                (true, CopyFormat::Display) => "WAHR".into(),
                (false, CopyFormat::Display) => "FALSCH".into(),
                (true, CopyFormat::Invariant) => "TRUE".into(),
                (false, CopyFormat::Invariant) => "FALSE".into(),
            },
            CellValue::DateTime(serial) => {
                format_datetime(*serial, f).unwrap_or_else(|| format_number(*serial, f))
            }
            CellValue::Duration(days) => format_duration(*days),
            CellValue::Error(e) => e.text(f).to_string(),
        }
    }

    /// Numbers, dates and durations are right-aligned like in Excel.
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            CellValue::Number(_) | CellValue::DateTime(_) | CellValue::Duration(_)
        )
    }
}

impl CellError {
    pub fn text(self, f: CopyFormat) -> &'static str {
        use CellError::*;
        match (self, f) {
            (Div0, _) => "#DIV/0!",
            (NA, CopyFormat::Display) => "#NV",
            (NA, CopyFormat::Invariant) => "#N/A",
            (Value, CopyFormat::Display) => "#WERT!",
            (Value, CopyFormat::Invariant) => "#VALUE!",
            (Ref, CopyFormat::Display) => "#BEZUG!",
            (Ref, CopyFormat::Invariant) => "#REF!",
            (Name, _) => "#NAME?",
            (Num, CopyFormat::Display) => "#ZAHL!",
            (Num, CopyFormat::Invariant) => "#NUM!",
            (Null, _) => "#NULL!",
            (GettingData, CopyFormat::Display) => "#DATEN!",
            (GettingData, CopyFormat::Invariant) => "#GETTING_DATA",
        }
    }
}

/// At most 15 significant digits (like Excel), shortest representation, no `.0`.
fn format_number(n: f64, f: CopyFormat) -> String {
    if !n.is_finite() {
        return n.to_string();
    }
    let rounded: f64 = format!("{n:.14e}").parse().unwrap_or(n);
    let rounded = if rounded == 0.0 { 0.0 } else { rounded }; // no "-0"
    let s = rounded.to_string();
    match f {
        CopyFormat::Display => s.replace('.', ","),
        CopyFormat::Invariant => s,
    }
}

const SECS_PER_DAY: f64 = 86_400.0;

/// Splits a non-negative day count into whole days and rounded seconds of the day.
fn split_days(value: f64) -> (i64, i64) {
    let mut days = value.trunc() as i64;
    let mut secs = ((value - value.trunc()) * SECS_PER_DAY).round() as i64;
    if secs >= SECS_PER_DAY as i64 {
        days += 1;
        secs -= SECS_PER_DAY as i64;
    }
    (days, secs)
}

fn hhmm(secs: i64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if s == 0 {
        format!("{h:02}:{m:02}")
    } else {
        format!("{h:02}:{m:02}:{s:02}")
    }
}

fn format_datetime(serial: f64, f: CopyFormat) -> Option<String> {
    if !serial.is_finite() || serial < 0.0 {
        return None;
    }
    let (days, secs) = split_days(serial);
    if days == 0 {
        return Some(hhmm(secs));
    }
    // Excel's 1900 system counts the non-existent 1900-02-29 (serial 60).
    let base = if days < 61 {
        NaiveDate::from_ymd_opt(1899, 12, 31)?
    } else {
        NaiveDate::from_ymd_opt(1899, 12, 30)?
    };
    let date = base.checked_add_signed(Duration::try_days(days)?)?;
    let date = match f {
        CopyFormat::Display => date.format("%d.%m.%Y").to_string(),
        CopyFormat::Invariant => date.format("%Y-%m-%d").to_string(),
    };
    Some(if secs == 0 {
        date
    } else {
        format!("{date} {}", hhmm(secs))
    })
}

fn format_duration(days: f64) -> String {
    let sign = if days < 0.0 { "-" } else { "" };
    let (d, secs) = split_days(days.abs());
    format!("{sign}{}", hhmm(d * SECS_PER_DAY as i64 + secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use CopyFormat::*;

    #[test]
    fn format_numbers() {
        assert_eq!(CellValue::Number(42.0).format(Display), "42");
        assert_eq!(CellValue::Number(0.1 + 0.2).format(Display), "0,3");
        assert_eq!(CellValue::Number(3.5).format(Invariant), "3.5");
        assert_eq!(
            CellValue::Number(1234567.891).format(Display),
            "1234567,891"
        );
    }

    #[test]
    fn format_large_and_negative() {
        assert_eq!(CellValue::Number(-0.5).format(Display), "-0,5");
        assert_eq!(
            CellValue::Number(1e20).format(Display),
            "100000000000000000000"
        );
        assert_eq!(CellValue::Number(-0.0).format(Display), "0");
    }

    #[test]
    fn format_dates() {
        assert_eq!(CellValue::DateTime(45231.0).format(Display), "01.11.2023");
        assert_eq!(
            CellValue::DateTime(45231.5).format(Display),
            "01.11.2023 12:00"
        );
        assert_eq!(
            CellValue::DateTime(45231.0 + 45296.0 / 86400.0).format(Display),
            "01.11.2023 12:34:56"
        );
        assert_eq!(CellValue::DateTime(0.75).format(Display), "18:00");
        assert_eq!(
            CellValue::DateTime(45231.5).format(Invariant),
            "2023-11-01 12:00"
        );
        assert_eq!(CellValue::DateTime(1.0).format(Display), "01.01.1900");
    }

    #[test]
    fn format_invalid_date_falls_back() {
        assert_eq!(CellValue::DateTime(-5.0).format(Display), "-5");
    }

    #[test]
    fn format_durations() {
        assert_eq!(CellValue::Duration(1.5).format(Display), "36:00");
        assert_eq!(CellValue::Duration(-0.25).format(Display), "-06:00");
    }

    #[test]
    fn format_bool_and_error() {
        assert_eq!(CellValue::Bool(true).format(Display), "WAHR");
        assert_eq!(CellValue::Bool(false).format(Invariant), "FALSE");
        assert_eq!(CellValue::Error(CellError::Div0).format(Display), "#DIV/0!");
        assert_eq!(CellValue::Error(CellError::NA).format(Display), "#NV");
        assert_eq!(CellValue::Error(CellError::NA).format(Invariant), "#N/A");
    }
}
