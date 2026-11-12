//! All user-facing strings in one place, in German and English. The language follows
//! the system (German on a German system, English everywhere else); the environment
//! variable `FERROSHEET_LANG=de|en` overrides it.

use std::sync::OnceLock;

use ferrosheet_core::{LoadError, Locale};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    De,
    En,
}

pub fn lang() -> Lang {
    static LANG: OnceLock<Lang> = OnceLock::new();
    *LANG.get_or_init(|| {
        // Tests always run in English, whatever the machine's language.
        if cfg!(test) {
            return Lang::En;
        }
        let tag = std::env::var("FERROSHEET_LANG")
            .ok()
            .or_else(sys_locale::get_locale)
            .unwrap_or_default();
        if tag.to_ascii_lowercase().starts_with("de") {
            Lang::De
        } else {
            Lang::En
        }
    })
}

/// How cell values are shown: German style on German systems, neutral otherwise.
pub fn locale() -> Locale {
    match lang() {
        Lang::De => Locale::German,
        Lang::En => Locale::Invariant,
    }
}

fn pick(de: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::De => de,
        Lang::En => en,
    }
}

macro_rules! texts {
    ($($(#[$attr:meta])* $name:ident: $de:expr, $en:expr;)*) => {
        $($(#[$attr])* pub fn $name() -> &'static str { pick($de, $en) })*
    };
}

pub const APP: &str = "ferrosheet";

texts! {
    open: "Öffnen …", "Open …";
    open_hint: "Excel-Datei hierher ziehen oder mit Strg+O öffnen",
        "Drop a spreadsheet here or press Ctrl+O";
    file_filter: "Tabellen", "Spreadsheets";
    loading: "Lade …", "Loading …";
    load_failed: "Datei konnte nicht geöffnet werden", "Could not open the file";
    ok: "OK", "OK";
    empty_sheet: "Dieses Blatt ist leer.", "This sheet is empty.";
    search: "Suchen", "Search";
    all_sheets: "Alle Blätter", "All sheets";
    no_hits: "Keine Treffer", "No matches";
    search_enter: "Enter drücken zum Suchen", "Press Enter to search";
    prev_hit: "Vorheriger Treffer (Umschalt+F3)", "Previous match (Shift+F3)";
    next_hit: "Nächster Treffer (F3)", "Next match (F3)";
    close_search: "Suche schließen (Esc)", "Close search (Esc)";
    update_failed: "Aktualisierung fehlgeschlagen", "Update failed";
    #[cfg(windows)]
    file_type: "Tabelle", "Spreadsheet";
    #[cfg(windows)]
    app_description: "Schneller Betrachter für Excel- und ODS-Tabellen",
        "Fast viewer for Excel and ODS spreadsheets";
    #[cfg(windows)]
    registered: "ferrosheet ist jetzt unter „Öffnen mit“ eingetragen. In den Einstellungen, die sich gerade öffnen, kannst du es als Standard-App festlegen.",
        "ferrosheet is now listed under \"Open with\". In the settings page that just opened you can make it the default app.";
    #[cfg(windows)]
    unregistered: "ferrosheet ist nicht mehr mit Tabellen verknüpft.",
        "ferrosheet is no longer associated with spreadsheets.";
    #[cfg(windows)]
    register_failed: "Verknüpfung fehlgeschlagen", "Association failed";
}

pub fn hit_counter(current: usize, total: usize) -> String {
    match lang() {
        Lang::De => format!("{current} von {total}"),
        Lang::En => format!("{current} of {total}"),
    }
}

pub fn update_ready(version: &str) -> String {
    match lang() {
        Lang::De => format!("Version {version} bereit – neu starten"),
        Lang::En => format!("Version {version} ready – restart"),
    }
}

pub fn copied(cells: u64) -> String {
    match (lang(), cells) {
        (Lang::De, 1) => "1 Zelle kopiert".into(),
        (Lang::De, n) => format!("{n} Zellen kopiert"),
        (Lang::En, 1) => "1 cell copied".into(),
        (Lang::En, n) => format!("{n} cells copied"),
    }
}

pub fn selected(cells: u64) -> String {
    match lang() {
        Lang::De => format!("{cells} Zellen markiert"),
        Lang::En => format!("{cells} cells selected"),
    }
}

pub fn load_error(e: &LoadError) -> String {
    match (lang(), e) {
        (Lang::De, LoadError::Io(e)) => format!("Datei konnte nicht gelesen werden: {e}"),
        (Lang::De, LoadError::Format(e)) => format!("Datei ist keine lesbare Tabelle: {e}"),
        (Lang::De, LoadError::Damaged) => "Die Datei ist beschädigt.".into(),
        (Lang::En, e) => {
            let s = e.to_string();
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect())
                .unwrap_or_default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tests_run_in_english_with_neutral_values() {
        assert_eq!(lang(), Lang::En);
        assert_eq!(locale(), Locale::Invariant);
        assert_eq!(copied(1), "1 cell copied");
        assert_eq!(load_error(&LoadError::Damaged), "The file is damaged");
    }
}
