//! Prints the first rows of every sheet: `cargo run --example dump -- file.xls`
fn main() {
    let path = std::env::args().nth(1).expect("usage: dump <file>");
    let mut wb = ferrosheet_core::Workbook::open(path.as_ref(), ferrosheet_core::Locale::Invariant)
        .unwrap_or_else(|e| panic!("{e}"));
    for i in 0..wb.sheet_names.len() {
        let s = wb.load_sheet(i).unwrap_or_else(|e| panic!("{e}"));
        println!("== {} ({}x{})", s.name, s.rows, s.cols);
        for r in 0..s.rows.min(10) {
            let row: Vec<_> = (0..s.cols.min(8))
                .map(|c| {
                    format!(
                        "{}={:?}",
                        ferrosheet_core::cell_address(r, c),
                        s.display(r, c)
                    )
                })
                .collect();
            println!("{}", row.join("  "));
        }
    }
}
