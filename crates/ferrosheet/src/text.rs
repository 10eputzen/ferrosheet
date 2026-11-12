//! All user-facing strings in one place (German for now).

pub const APP: &str = "ferrosheet";
pub const OPEN: &str = "Öffnen …";
pub const OPEN_HINT: &str = "Excel-Datei hierher ziehen oder mit Strg+O öffnen";
pub const FILE_FILTER: &str = "Tabellen";
pub const LOADING: &str = "Lade …";
pub const LOAD_FAILED: &str = "Datei konnte nicht geöffnet werden";
pub const OK: &str = "OK";
pub const EMPTY_SHEET: &str = "Dieses Blatt ist leer.";
pub const SEARCH: &str = "Suchen";
pub const ALL_SHEETS: &str = "Alle Blätter";
pub const NO_HITS: &str = "Keine Treffer";
pub const SEARCH_ENTER: &str = "Enter drücken zum Suchen";
pub const PREV_HIT: &str = "Vorheriger Treffer (Umschalt+F3)";
pub const NEXT_HIT: &str = "Nächster Treffer (F3)";
pub const CLOSE_SEARCH: &str = "Suche schließen (Esc)";

pub fn hit_counter(current: usize, total: usize) -> String {
    format!("{current} von {total}")
}

pub fn copied(cells: u64) -> String {
    match cells {
        1 => "1 Zelle kopiert".into(),
        n => format!("{n} Zellen kopiert"),
    }
}

pub fn selected(cells: u64) -> String {
    format!("{cells} Zellen markiert")
}
