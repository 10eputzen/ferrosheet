//! Damaged files must end in an error message, never in a crash.
//! Each fixture is truncated and corrupted in many deterministic ways; opening and
//! loading every sheet (values and formulas) must return `Ok` or `Err` without panicking.

use std::path::{Path, PathBuf};

use ferrosheet_core::{Locale, Workbook};

const FIXTURES: [&str; 4] = ["date.xls", "date.xlsb", "date.ods", "demo.xlsx"];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Opens `bytes` as a file with the given extension and loads everything.
fn load_all(bytes: &[u8], ext: &str, tag: &str) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("robustness");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{tag}.{ext}"));
    std::fs::write(&path, bytes).unwrap();
    let Ok(mut wb) = Workbook::open(&path, Locale::German) else {
        return;
    };
    for idx in 0..wb.sheet_names.len() {
        if let Ok(sheet) = wb.load_sheet(idx) {
            // touch every display string so lazy formatting paths run too
            for r in 0..sheet.rows.min(200) {
                for c in 0..sheet.cols.min(50) {
                    let _ = sheet.display(r, c);
                }
            }
        }
        let _ = wb.load_formulas(idx);
    }
}

/// Small deterministic generator, so a failing case can be reproduced.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn truncated_files_never_panic() {
    for name in FIXTURES {
        let bytes = std::fs::read(fixture(name)).unwrap();
        let ext = name.rsplit('.').next().unwrap();
        let n = bytes.len();
        let mut cuts: Vec<usize> = vec![0, 1, 4, 8, 64, 512, n / 4, n / 2, 3 * n / 4, n - 1];
        cuts.extend((0..n).step_by((n / 40).max(1)));
        for cut in cuts.into_iter().filter(|&c| c < n) {
            load_all(&bytes[..cut], ext, &format!("{name}-cut{cut}"));
        }
    }
}

#[test]
fn corrupted_bytes_never_panic() {
    let mut rng = Lcg(0x5EED);
    for name in FIXTURES {
        let original = std::fs::read(fixture(name)).unwrap();
        let ext = name.rsplit('.').next().unwrap();
        for round in 0..60 {
            let mut bytes = original.clone();
            for _ in 0..(1 + rng.next() % 8) {
                let i = (rng.next() as usize) % bytes.len();
                bytes[i] = rng.next() as u8;
            }
            load_all(&bytes, ext, &format!("{name}-flip{round}"));
        }
    }
}

#[test]
fn wrong_extension_never_panics() {
    let xls = std::fs::read(fixture("date.xls")).unwrap();
    let ods = std::fs::read(fixture("date.ods")).unwrap();
    for (bytes, ext) in [(&xls, "xlsx"), (&xls, "ods"), (&ods, "xls"), (&ods, "xlsb")] {
        load_all(bytes, ext, &format!("wrong-{ext}"));
    }
    load_all(b"Name;Wert\nA;1\n", "xlsx", "csv-as-xlsx");
}
