//! Writes a small demo workbook: `cargo run --example gen_demo -- <path>`
//! Shows formulas (with cached results, as Excel stores them), dates, durations,
//! percentages, booleans, an error value, multi-line text and a second sheet.
use rust_xlsxwriter::{ExcelDateTime, Format, Formula, Workbook};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "target/demo.xlsx".into());
    let date = Format::new().set_num_format("dd.mm.yyyy");
    let euro = Format::new().set_num_format("#,##0.00 €");
    let pct = Format::new().set_num_format("0%");
    let time = Format::new().set_num_format("[hh]:mm");
    let bold = Format::new().set_bold();

    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Rechnung")?;
    for (c, h) in [
        "Artikel",
        "Menge",
        "Einzelpreis",
        "Summe",
        "Lieferdatum",
        "Aufwand",
        "Rabatt",
        "Bezahlt",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string_with_format(0, c as u16, *h, &bold)?;
    }
    let items = [
        ("Schrauben M4", 120.0, 0.08, 0.0, false),
        ("Kabelkanal 2 m", 15.0, 4.95, 0.1, true),
        ("Netzteil 24 V", 3.0, 39.9, 0.05, true),
        ("Montage vor Ort\n(inkl. Anfahrt)", 1.0, 180.0, 0.0, false),
    ];
    let base = ExcelDateTime::from_ymd(2026, 11, 2)?;
    for (i, (name, qty, price, discount, paid)) in items.iter().enumerate() {
        let r = i as u32 + 1;
        let row = r + 1; // Excel row number
        ws.write_string(r, 0, *name)?;
        ws.write_number(r, 1, *qty)?;
        ws.write_number_with_format(r, 2, *price, &euro)?;
        let sum = qty * price * (1.0 - discount);
        ws.write_formula_with_format(
            r,
            3,
            Formula::new(format!("=B{row}*C{row}*(1-G{row})")).set_result(sum.to_string()),
            &euro,
        )?;
        let day = ExcelDateTime::from_serial_datetime(base.to_excel() + i as f64 * 3.0)?;
        ws.write_datetime_with_format(r, 4, &day, &date)?;
        ws.write_number_with_format(r, 5, 0.75 + i as f64 * 0.5 / 24.0 * 3.0, &time)?;
        ws.write_number_with_format(r, 6, *discount, &pct)?;
        ws.write_boolean(r, 7, *paid)?;
    }
    let total: f64 = items.iter().map(|(_, q, p, d, _)| q * p * (1.0 - d)).sum();
    ws.write_string_with_format(6, 0, "Gesamt", &bold)?;
    ws.write_formula_with_format(
        6,
        3,
        Formula::new("=SUM(D2:D5)").set_result(total.to_string()),
        &euro,
    )?;
    ws.write_string(7, 0, "Durchschnitt")?;
    ws.write_formula_with_format(
        7,
        3,
        Formula::new("=AVERAGE(D2:D5)").set_result((total / 4.0).to_string()),
        &euro,
    )?;
    ws.write_string(8, 0, "Alles bezahlt?")?;
    ws.write_formula(
        8,
        3,
        Formula::new("=IF(COUNTIF(H2:H5,FALSE)=0,\"ja\",\"nein\")").set_result("nein"),
    )?;
    ws.write_string(9, 0, "Fehlerbeispiel")?;
    ws.write_formula(9, 3, Formula::new("=D7/0").set_result("#DIV/0!"))?;
    ws.set_column_width(0, 22)?;
    ws.set_column_width(3, 14)?;
    ws.set_column_width(4, 12)?;

    let ws2 = wb.add_worksheet().set_name("Übersicht")?;
    ws2.write_string_with_format(0, 0, "Bezug auf das erste Blatt", &bold)?;
    ws2.write_string(1, 0, "Rechnungssumme")?;
    ws2.write_formula_with_format(
        1,
        1,
        Formula::new("=Rechnung!D7").set_result(total.to_string()),
        &euro,
    )?;
    ws2.write_string(2, 0, "inkl. 19 % MwSt.")?;
    ws2.write_formula_with_format(
        2,
        1,
        Formula::new("=ROUND(B2*1.19,2)")
            .set_result(((total * 1.19 * 100.0).round() / 100.0).to_string()),
        &euro,
    )?;
    ws2.write_string(3, 0, "Heute")?;
    ws2.write_formula_with_format(3, 1, Formula::new("=TODAY()").set_result("46338"), &date)?;
    ws2.set_column_width(0, 26)?;

    wb.save(&path)?;
    println!("{path}");
    Ok(())
}
