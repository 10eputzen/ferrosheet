use std::path::{Path, PathBuf};

use ferrosheet_core::{LoadError, Locale, PREVIEW_ROWS, Workbook, column_name};
use rust_xlsxwriter::{ExcelDateTime, Format};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn tmp(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(dir).unwrap();
    dir.join(name)
}

/// Writes an .xlsx with `build` and returns its path.
fn xlsx(name: &str, build: impl FnOnce(&mut rust_xlsxwriter::Workbook)) -> PathBuf {
    let mut wb = rust_xlsxwriter::Workbook::new();
    build(&mut wb);
    let path = tmp(name);
    wb.save(&path).unwrap();
    path
}

#[test]
fn opens_all_formats() {
    for f in ["date.xls", "date.xlsb", "date.ods"] {
        let mut wb = Workbook::open(&fixture(f), Locale::German).unwrap();
        assert_eq!(wb.sheet_names, ["Sheet1"], "{f}");
        let s = wb.load_sheet(0).unwrap();
        assert_eq!(s.display(0, 0), "01.01.2021", "{f}");
        assert_eq!(s.display(0, 1), "15", "{f}");
    }
}

#[test]
fn durations_from_binary_and_ods() {
    let mut xls = Workbook::open(&fixture("date.xls"), Locale::German).unwrap();
    assert_eq!(xls.load_sheet(0).unwrap().display(2, 0), "255:10:10");
    let mut ods = Workbook::open(&fixture("date.ods"), Locale::German).unwrap();
    let s = ods.load_sheet(0).unwrap();
    assert_eq!(s.display(1, 0), "01.01.2021 10:10:10");
    assert_eq!(s.display(2, 0), "10:10:10");
}

#[test]
fn xlsx_types() {
    let path = xlsx("types.xlsx", |wb| {
        let ws = wb.add_worksheet();
        ws.write_string(0, 0, "Hallo").unwrap();
        ws.write_number(0, 1, 3.5).unwrap();
        let date = ExcelDateTime::from_ymd(2023, 11, 1).unwrap();
        ws.write_datetime_with_format(0, 2, &date, &Format::new().set_num_format("dd.mm.yyyy"))
            .unwrap();
        ws.write_boolean(0, 3, true).unwrap();
    });
    let s = Workbook::open(&path, Locale::German)
        .unwrap()
        .load_sheet(0)
        .unwrap();
    assert_eq!(
        [
            s.display(0, 0),
            s.display(0, 1),
            s.display(0, 2),
            s.display(0, 3)
        ],
        ["Hallo", "3,5", "01.11.2023", "WAHR"]
    );
    let mut en = Workbook::open(&path, Locale::Invariant).unwrap();
    let s_en = en.load_sheet(0).unwrap();
    assert_eq!(
        [s_en.display(0, 1), s_en.display(0, 2), s_en.display(0, 3)],
        ["3.5", "2023-11-01", "TRUE"]
    );
    assert!(s.value(0, 1).is_numeric());
    assert!(!s.value(0, 0).is_numeric());
}

#[test]
fn range_offset_preserved() {
    let path = xlsx("offset.xlsx", |wb| {
        wb.add_worksheet().write_string(4, 2, "x").unwrap();
    });
    let s = Workbook::open(&path, Locale::German)
        .unwrap()
        .load_sheet(0)
        .unwrap();
    assert_eq!(s.display(4, 2), "x");
    assert_eq!(s.display(0, 0), "");
    assert!(s.rows >= 5 && s.cols >= 3);
}

#[test]
fn empty_sheet() {
    let path = xlsx("empty.xlsx", |wb| {
        wb.add_worksheet().write_string(0, 0, "a").unwrap();
        wb.add_worksheet();
    });
    let mut wb = Workbook::open(&path, Locale::German).unwrap();
    let s = wb.load_sheet(1).unwrap();
    assert_eq!((s.rows, s.cols), (0, 0));
    assert_eq!(s.display(0, 0), "");
}

#[test]
fn open_corrupt_file_errors() {
    let path = tmp("corrupt.xlsx");
    std::fs::write(&path, "a;b;c\n1;2;3\n").unwrap();
    assert!(matches!(
        Workbook::open(&path, Locale::German),
        Err(LoadError::Format(_))
    ));
}

#[test]
fn open_missing_file_errors() {
    assert!(matches!(
        Workbook::open(Path::new("does/not/exist.xlsx"), Locale::German),
        Err(LoadError::Io(_))
    ));
}

#[test]
fn formulas_loaded() {
    let path = xlsx("formula.xlsx", |wb| {
        let ws = wb.add_worksheet();
        ws.write_number(0, 0, 1).unwrap();
        ws.write_formula(1, 0, "=A1*2").unwrap();
    });
    let mut wb = Workbook::open(&path, Locale::German).unwrap();
    let f = wb.load_formulas(0).unwrap();
    assert_eq!(f.get(&(1, 0)).map(String::as_str), Some("=A1*2"));
    assert_eq!(f.len(), 1);
}

#[test]
fn column_names() {
    assert_eq!(column_name(0), "A");
    assert_eq!(column_name(25), "Z");
    assert_eq!(column_name(26), "AA");
    assert_eq!(column_name(701), "ZZ");
    assert_eq!(column_name(702), "AAA");
}

#[test]
fn big_xlsx_sends_a_preview_of_the_first_rows_then_the_whole_sheet() {
    let rows = PREVIEW_ROWS + 100;
    let path = xlsx("preview.xlsx", |wb| {
        let ws = wb.add_worksheet();
        for r in 0..rows {
            ws.write_number(r, 0, f64::from(r)).unwrap();
            ws.write_string(r, 1, format!("row {r}")).unwrap();
        }
    });
    let mut wb = Workbook::open(&path, Locale::Invariant).unwrap();
    let mut preview = None;
    let full = wb
        .load_sheet_with_preview(0, |p| preview = Some(p))
        .unwrap();
    let preview = preview.expect("a preview for a big sheet");
    assert_eq!((preview.rows, preview.cols), (PREVIEW_ROWS, 2));
    assert_eq!(preview.display(PREVIEW_ROWS - 1, 1), "row 199");
    assert_eq!((full.rows, full.cols), (rows, 2));
    assert_eq!(full.display(rows - 1, 0), "299");

    // small sheets come in one piece
    let mut small = Workbook::open(&fixture("demo.xlsx"), Locale::Invariant).unwrap();
    let mut called = false;
    small.load_sheet_with_preview(0, |_| called = true).unwrap();
    assert!(!called);
}
