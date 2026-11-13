//! What survives opening an `.xlsx`, editing one cell and saving it in place, and what
//! the warning before saving lists as lost.

use std::io::Read;
use std::path::{Path, PathBuf};

use ferrosheet_core::{EditSession, Locale, Loss, save_losses};
use rust_xlsxwriter::{
    Chart, ChartType, Color, ConditionalFormatCell, ConditionalFormatCellRule, DataValidation,
    Format, FormatBorder, Note, Table, Url, Workbook,
};

fn tmp(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    std::fs::create_dir_all(dir).unwrap();
    dir.join(name)
}

/// One part of a saved file as text.
fn part(path: &Path, name: &str) -> String {
    let file = std::fs::File::open(path).unwrap();
    let mut zip = zip::ZipArchive::new(file).unwrap();
    let mut s = String::new();
    zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
    s
}

/// A workbook with the formatting people use, plus the parts saving drops.
fn rich(path: &Path, lossy: bool) {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Data").unwrap();
    let bold = Format::new()
        .set_bold()
        .set_font_color(Color::RGB(0xC00000));
    let fill = Format::new().set_background_color(Color::RGB(0xFFC000));
    let border = Format::new().set_border(FormatBorder::Thin);
    let money = Format::new().set_num_format("#,##0.00 \"€\"");
    ws.write_with_format(0, 0, "Head", &bold).unwrap();
    ws.write_with_format(1, 0, "Fill", &fill).unwrap();
    ws.write_with_format(2, 0, "Border", &border).unwrap();
    ws.write_number_with_format(3, 0, 1234.5, &money).unwrap();
    ws.merge_range(5, 0, 5, 2, "Merged", &Format::new())
        .unwrap();
    ws.set_column_width(0, 30).unwrap();
    ws.set_row_height(6, 40).unwrap();
    ws.set_freeze_panes(1, 1).unwrap();
    ws.write_url(8, 0, Url::new("https://example.com")).unwrap();
    ws.write(15, 0, 1).unwrap();
    let cf = ConditionalFormatCell::new()
        .set_rule(ConditionalFormatCellRule::GreaterThan(0))
        .set_format(Format::new().set_bold());
    ws.add_conditional_format(15, 0, 15, 0, &cf).unwrap();
    if lossy {
        ws.insert_note(0, 1, &Note::new("a note")).unwrap();
        let dv = DataValidation::new()
            .allow_list_strings(&["a", "b"])
            .unwrap();
        ws.add_data_validation(7, 0, 7, 0, &dv).unwrap();
        ws.autofilter(10, 0, 12, 1).unwrap();
        ws.add_table(20, 0, 22, 1, &Table::new()).unwrap();
        let mut chart = Chart::new(ChartType::Column);
        chart.add_series().set_values("Data!$A$16:$A$16");
        ws.insert_chart(2, 4, &chart).unwrap();
    }
    wb.define_name("Amount", "=Data!$A$4").unwrap();
    wb.save(path).unwrap();
}

#[test]
fn saving_in_place_keeps_cell_formatting_and_layout() {
    let path = tmp("roundtrip-keep.xlsx");
    rich(&path, true);
    let mut s = EditSession::open_xlsx(&path, Locale::Invariant).unwrap();
    s.set_input(0, 30, 0, "edited").unwrap();
    s.save_xlsx(&path).unwrap();

    let sheet = part(&path, "xl/worksheets/sheet1.xml");
    let styles = part(&path, "xl/styles.xml");
    let book = part(&path, "xl/workbook.xml");
    for kept in [
        "<mergeCell ref=\"A6:C6\"/>",
        "width=\"30.7109375\" customWidth=\"1\"",
        "ht=\"39.75\" customHeight=\"1\"",
        "xSplit=\"1\" ySplit=\"1\"",
        "<hyperlink ref=\"A9\"",
        "<conditionalFormatting sqref=\"A16\">",
    ] {
        assert!(sheet.contains(kept), "{kept}");
    }
    for kept in [
        "formatCode=\"#,##0.00 &quot;€&quot;\"",
        "<b/>",
        "rgb=\"FFC00000\"",
        "rgb=\"FFFFC000\"",
        "style=\"thin\"",
    ] {
        assert!(styles.contains(kept), "{kept}");
    }
    assert!(book.contains("<definedName name=\"Amount\">Data!$A$4</definedName>"));

    // The rest is gone, which the warning before saving said.
    for lost in [
        "<dataValidation",
        "<autoFilter",
        "<tablePart",
        "<drawing",
        "<legacyDrawing",
    ] {
        assert!(!sheet.contains(lost), "{lost}");
    }
    assert!(save_losses(&path).is_empty());
}

#[test]
fn the_parts_saving_drops_are_found_before_saving() {
    let path = tmp("roundtrip-losses.xlsx");
    rich(&path, true);
    assert_eq!(
        save_losses(&path),
        [
            Loss::Comments,
            Loss::Validation,
            Loss::Filter,
            Loss::Tables,
            Loss::Drawings
        ]
    );
    let plain = tmp("roundtrip-plain.xlsx");
    rich(&plain, false);
    assert!(save_losses(&plain).is_empty());
    assert!(save_losses(&tmp("does-not-exist.xlsx")).is_empty());
}
