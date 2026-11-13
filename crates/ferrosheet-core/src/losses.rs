//! What saving an `.xlsx` in place would drop. The editing engine writes cells, styles,
//! number formats, merged cells, column widths, row heights, frozen panes, hyperlinks,
//! conditional formatting and names; the parts listed here it neither reads nor writes.

use std::io::Read;
use std::path::Path;

/// Something in an `.xlsx` that does not survive saving it with ferrosheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Loss {
    Comments,
    /// Drop-down lists and other input rules.
    Validation,
    /// Filter buttons.
    Filter,
    /// Excel tables ("Format as table"); the cells stay, the table goes.
    Tables,
    /// Charts, pictures and shapes.
    Drawings,
    PivotTables,
}

/// What saving over `path` would lose, in a fixed order. Empty for files that are not
/// `.xlsx` or cannot be read (saving reports those problems itself).
pub fn save_losses(path: &Path) -> Vec<Loss> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(mut zip) = zip::ZipArchive::new(std::io::BufReader::new(file)) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut sheets = Vec::new();
    for name in zip.file_names().flatten() {
        let n = name.to_ascii_lowercase();
        let loss = if n.starts_with("xl/comments") || n.starts_with("xl/threadedcomments/") {
            Some(Loss::Comments)
        } else if n.starts_with("xl/tables/") {
            Some(Loss::Tables)
        } else if n.starts_with("xl/charts/")
            || n.starts_with("xl/media/")
            || (n.starts_with("xl/drawings/") && n.ends_with(".xml"))
        {
            Some(Loss::Drawings)
        } else if n.starts_with("xl/pivottables/") {
            Some(Loss::PivotTables)
        } else {
            if n.starts_with("xl/worksheets/") && n.ends_with(".xml") {
                sheets.push(name.into_owned());
            }
            None
        };
        found.extend(loss);
    }
    for name in sheets {
        let Ok(mut f) = zip.by_name(&name) else {
            continue;
        };
        let mut xml = String::new();
        if f.read_to_string(&mut xml).is_err() {
            continue;
        }
        if xml.contains("<dataValidation") || xml.contains(":dataValidation") {
            found.push(Loss::Validation);
        }
        if xml.contains("<autoFilter") {
            found.push(Loss::Filter);
        }
    }
    found.sort();
    found.dedup();
    found
}
