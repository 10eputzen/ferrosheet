//! Writes a test workbook: `cargo run --release --example gen_big -- <path> [rows] [cols]`
//! Mixed text, numbers and dates; used for performance measurements.
use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "target/big.xlsx".into());
    let rows: u32 = args.next().map_or(Ok(100_000), |s| s.parse())?;
    let cols: u16 = args.next().map_or(Ok(30), |s| s.parse())?;
    let date_fmt = Format::new().set_num_format("dd.mm.yyyy");
    let base = ExcelDateTime::from_ymd(2020, 1, 1)?;
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet().set_name("Daten")?;
    for c in 0..cols {
        ws.write_string(0, c, format!("Spalte {}", c + 1))?;
    }
    for r in 1..rows {
        for c in 0..cols {
            match c % 3 {
                0 => ws.write_string(r, c, format!("Kunde {} abc", (r * 7 + c as u32) % 9973))?,
                1 => ws.write_number(r, c, r as f64 * 1.25 + c as f64)?,
                _ => ws.write_datetime_with_format(
                    r,
                    c,
                    &ExcelDateTime::from_serial_datetime(base.to_excel() + (r % 3650) as f64)?,
                    &date_fmt,
                )?,
            };
        }
    }
    let ws2 = wb.add_worksheet().set_name("Zweites Blatt")?;
    ws2.write_string(0, 0, "Hallo vom zweiten Blatt")?;
    ws2.write_formula(1, 0, "=1+2")?;
    wb.save(&path)?;
    println!("{path}: {rows} x {cols}");
    Ok(())
}
