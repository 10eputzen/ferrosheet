//! Frozen panes stored in an `.xlsx`: the rows and columns a sheet keeps in view while
//! scrolling. calamine does not read them, so this looks into the file itself.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use regex::Regex;

/// The sheet view sits before the cell data, so the start of a sheet is enough.
const HEAD_BYTES: u64 = 64 * 1024;

/// Frozen (rows, columns) per sheet of an `.xlsx`/`.xlsm` in workbook order; `(0, 0)`
/// where nothing is frozen. Empty for other files or when the file cannot be read.
pub fn frozen_panes(path: &Path) -> Vec<(u32, u32)> {
    let xlsx = path.extension().is_some_and(|e| {
        matches!(
            e.to_string_lossy().to_ascii_lowercase().as_str(),
            "xlsx" | "xlsm"
        )
    });
    if !xlsx {
        return Vec::new();
    }
    read(path).unwrap_or_default()
}

fn read(path: &Path) -> Option<Vec<(u32, u32)>> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    let text = |zip: &mut zip::ZipArchive<_>, name: &str, limit: u64| -> Option<String> {
        let entry = zip.by_name(name).ok()?;
        let mut s = String::new();
        entry.take(limit).read_to_string(&mut s).ok()?;
        Some(s)
    };
    let workbook = text(&mut zip, "xl/workbook.xml", 4 << 20)?;
    let rels = text(&mut zip, "xl/_rels/workbook.xml.rels", 4 << 20)?;

    let attr = |tag: &str, name: &str| -> Option<String> {
        let re = Regex::new(&format!(r#"(?:^|\s){name}\s*=\s*"([^"]*)""#)).ok()?;
        re.captures(tag).map(|c| c[1].to_owned())
    };
    let targets: HashMap<String, String> = Regex::new(r"<(?:\w+:)?Relationship\b[^>]*>")
        .ok()?
        .find_iter(&rels)
        .filter_map(|m| Some((attr(m.as_str(), "Id")?, attr(m.as_str(), "Target")?)))
        .collect();
    let sheets: Vec<String> = Regex::new(r"<(?:\w+:)?sheet\b[^>]*>")
        .ok()?
        .find_iter(&workbook)
        .map(|m| attr(m.as_str(), r"r:id").unwrap_or_default())
        .collect();

    let pane = Regex::new(r"<(?:\w+:)?pane\b[^>]*>").ok()?;
    let out = sheets
        .iter()
        .map(|id| {
            let Some(target) = targets.get(id) else {
                return (0, 0);
            };
            let name = match target.strip_prefix('/') {
                Some(abs) => abs.to_owned(),
                None => format!("xl/{target}"),
            };
            let Some(head) = text(&mut zip, &name, HEAD_BYTES) else {
                return (0, 0);
            };
            let Some(tag) = pane.find(&head).map(|m| m.as_str().to_owned()) else {
                return (0, 0);
            };
            let state = attr(&tag, "state").unwrap_or_default();
            if state != "frozen" && state != "frozenSplit" {
                return (0, 0);
            }
            let split = |n: &str| {
                attr(&tag, n)
                    .and_then(|v| v.parse::<f64>().ok())
                    .map_or(0, |v| v.max(0.0) as u32)
            };
            (split("ySplit"), split("xSplit"))
        })
        .collect();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::Workbook;

    #[test]
    fn frozen_rows_and_columns_are_read_per_sheet() {
        let dir = std::env::temp_dir().join("ferrosheet-frozen-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("frozen.xlsx");
        let mut wb = Workbook::new();
        let a = wb.add_worksheet();
        a.write(0, 0, "head").unwrap();
        a.set_freeze_panes(1, 0).unwrap();
        let b = wb.add_worksheet();
        b.write(0, 0, 1).unwrap();
        let c = wb.add_worksheet();
        c.set_freeze_panes(2, 3).unwrap();
        wb.save(&path).unwrap();
        assert_eq!(frozen_panes(&path), [(1, 0), (0, 0), (2, 3)]);
        assert!(frozen_panes(Path::new("x.csv")).is_empty());
    }
}
