//! Measures load and search time: `cargo run --release --example bench_load -- <file>`
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: bench_load <file>")?;
    let t = Instant::now();
    let mut wb =
        ferrosheet_core::Workbook::open(path.as_ref(), ferrosheet_core::Locale::Invariant)?;
    let open = t.elapsed();
    let t = Instant::now();
    let sheet = wb.load_sheet(0)?;
    let load = t.elapsed();
    let t = Instant::now();
    let hits = ferrosheet_core::find_all(&sheet, "abc");
    let first_search = t.elapsed();
    let t = Instant::now();
    ferrosheet_core::find_all(&sheet, "kunde 42");
    let next_search = t.elapsed();
    println!("{} x {} cells", sheet.rows, sheet.cols);
    println!("open        {:>7.1} ms", open.as_secs_f64() * 1e3);
    println!("load_sheet  {:>7.1} ms", load.as_secs_f64() * 1e3);
    println!(
        "search 1st  {:>7.1} ms ({} hits, builds lower-case index)",
        first_search.as_secs_f64() * 1e3,
        hits.len()
    );
    println!("search      {:>7.1} ms", next_search.as_secs_f64() * 1e3);
    Ok(())
}
