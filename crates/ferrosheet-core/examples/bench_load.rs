//! Measures load and search time: `cargo run --release --example bench_load -- <file>`
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: bench_load <file>")?;
    let t = Instant::now();
    let mut wb =
        ferrosheet_core::Workbook::open(path.as_ref(), ferrosheet_core::Locale::Invariant)?;
    let open = t.elapsed();
    let t = Instant::now();
    let mut preview = None;
    let sheet = wb.load_sheet_with_preview(0, |_| preview = Some(t.elapsed()))?;
    let load = t.elapsed();
    let t = Instant::now();
    let mut n = 0usize;
    for r in 0..sheet.rows {
        for c in 0..sheet.cols {
            n += sheet
                .value(r, c)
                .format(ferrosheet_core::Locale::Invariant)
                .len();
        }
    }
    let format = t.elapsed();
    std::hint::black_box(n);
    let t = Instant::now();
    let hits = ferrosheet_core::find_all(&sheet, &ferrosheet_core::Query::text("abc"));
    let first_search = t.elapsed();
    let t = Instant::now();
    ferrosheet_core::find_all(&sheet, &ferrosheet_core::Query::text("kunde 42"));
    let next_search = t.elapsed();
    println!("{} x {} cells", sheet.rows, sheet.cols);
    println!("open        {:>7.1} ms", open.as_secs_f64() * 1e3);
    if let Some(p) = preview {
        println!("first rows  {:>7.1} ms", p.as_secs_f64() * 1e3);
    }
    println!("load_sheet  {:>7.1} ms", load.as_secs_f64() * 1e3);
    println!(
        "format all  {:>7.1} ms (search index, built in the background)",
        format.as_secs_f64() * 1e3
    );
    println!(
        "search 1st  {:>7.1} ms ({} hits, builds lower-case index)",
        first_search.as_secs_f64() * 1e3,
        hits.len()
    );
    println!("search      {:>7.1} ms", next_search.as_secs_f64() * 1e3);
    Ok(())
}
