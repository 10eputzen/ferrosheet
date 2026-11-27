//! Cell formats read from `.xlsx` files: fonts, fills, alignment and merged cells.

use std::path::{Path, PathBuf};

use ferrosheet_core::{CellPos, CellStyle, HAlign, Locale, PREVIEW_ROWS, Workbook};
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatPattern};

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

#[test]
fn the_styles_fixture_shows_its_fonts_fills_and_merged_title() {
    let mut wb = Workbook::open(&fixture("styles.xlsx"), Locale::German).unwrap();
    let s = wb.load_sheet(0).unwrap();

    // title: bold 14 pt, A1:B1 merged
    let title = s.style(0, 0);
    assert!(title.bold);
    assert_eq!(title.size, Some(140));
    assert_eq!(title.fill, None);
    assert_eq!(
        s.merged(0, 1),
        Some((CellPos::new(0, 0), CellPos::new(0, 1)))
    );
    assert_eq!(s.merged(2, 0), None);

    // header: white bold text on teal
    for c in 0..2 {
        let h = s.style(2, c);
        assert!(h.bold, "header {c}");
        assert_eq!(h.fill, Some([0x00, 0x79, 0x6B]), "header {c}");
        assert_eq!(h.color, Some([0xFF, 0xFF, 0xFF]), "header {c}");
    }

    // row fills, the default font stays the theme's
    for c in 0..2 {
        assert_eq!(s.style(3, c).fill, Some([0xFC, 0xE4, 0xEC]), "pink {c}");
        assert_eq!(s.style(4, c).fill, Some([0xE8, 0xF5, 0xE8]), "green {c}");
        assert_eq!(s.style(3, c).color, None);
        assert!(!s.style(3, c).bold);
    }

    // one bold red cell, its neighbour plain
    let red = s.style(5, 0);
    assert!(red.bold);
    assert_eq!(red.color, Some([0xB7, 0x1C, 0x1C]));
    assert_eq!(red.fill, None);
    assert_eq!(s.style(5, 1), CellStyle::default());
}

#[test]
fn editing_a_cell_replaces_the_style_from_the_file() {
    let mut wb = Workbook::open(&fixture("styles.xlsx"), Locale::German).unwrap();
    let mut s = wb.load_sheet(0).unwrap();
    let v = s.value(5, 0).clone();
    s.set(5, 0, v, None, CellStyle::default());
    assert_eq!(s.style(5, 0), CellStyle::default());
}

#[test]
fn big_sheets_keep_styles_alignment_and_merges() {
    let path = tmp("big_styles.xlsx");
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    let head = Format::new()
        .set_bold()
        .set_italic()
        .set_font_size(16)
        .set_align(FormatAlign::Center)
        .set_pattern(FormatPattern::Solid)
        .set_background_color(Color::RGB(0x123456));
    ws.merge_range(0, 0, 0, 2, "head", &head).unwrap();
    let right = Format::new().set_align(FormatAlign::Right);
    for r in 1..=PREVIEW_ROWS + 50 {
        ws.write_string(r, 0, "x").unwrap();
    }
    ws.write_string_with_format(PREVIEW_ROWS + 10, 1, "late", &right)
        .unwrap();
    wb.save(&path).unwrap();

    let mut wb = Workbook::open(&path, Locale::German).unwrap();
    let mut previewed = false;
    let s = wb.load_sheet_with_preview(0, |_| previewed = true).unwrap();
    assert!(previewed, "the streamed path was taken");

    let h = s.style(0, 0);
    assert!(h.bold && h.italic);
    assert_eq!(h.size, Some(160));
    assert_eq!(h.align, Some(HAlign::Center));
    assert_eq!(h.fill, Some([0x12, 0x34, 0x56]));
    assert_eq!(
        s.merged(0, 2),
        Some((CellPos::new(0, 0), CellPos::new(0, 2)))
    );
    assert_eq!(s.style(PREVIEW_ROWS + 10, 1).align, Some(HAlign::Right));
    assert_eq!(s.style(5, 0), CellStyle::default());
}
