//! All user-facing strings in one place, in German and English. The language follows
//! the system (German on a German system, English everywhere else); the environment
//! variable `FERROSHEET_LANG=de|en` overrides it.

use std::sync::OnceLock;

use ferrosheet_core::{LoadError, Locale, NumberFormat};

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
pub const HOMEPAGE: &str = "https://github.com/10eputzen/ferrosheet";

pub fn version(v: &str) -> String {
    match lang() {
        Lang::De => format!("Version {v}"),
        Lang::En => format!("Version {v}"),
    }
}

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
    regex: "Regex", "Regex";
    regex_hint: "Suchtext als regulären Ausdruck lesen, z. B. ^M(ü|ei)er$",
        "Treat the search as a regular expression, e.g. ^M(ue|ei)er$";
    invalid_regex: "Ungültiger Ausdruck", "Invalid expression";
    no_hits: "Keine Treffer", "No matches";
    search_enter: "Enter drücken zum Suchen", "Press Enter to search";
    prev_hit: "Vorheriger Treffer (Umschalt+F3)", "Previous match (Shift+F3)";
    next_hit: "Nächster Treffer (F3)", "Next match (F3)";
    close_search: "Suche schließen (Esc)", "Close search (Esc)";
    update_failed: "Aktualisierung fehlgeschlagen", "Update failed";
    save: "Speichern", "Save";
    save_hint: "Speichern (Strg+S), Speichern unter (Strg+Umschalt+S)",
        "Save (Ctrl+S), Save as (Ctrl+Shift+S)";
    saved: "Gespeichert", "Saved";
    save_failed: "Speichern fehlgeschlagen", "Saving failed";
    xlsx_filter: "Excel-Arbeitsmappe", "Excel workbook";
    preparing_edit: "Bearbeiten wird vorbereitet …", "Preparing to edit …";
    formula_bar_hint: "Klicken zum Bearbeiten. Oder in der Zelle einfach lostippen, F2 oder Doppelklick.",
        "Click to edit. Or just start typing on a cell, press F2 or double-click it.";
    editing_hint: "Bearbeiten – Enter übernimmt, Tab geht nach rechts, Esc bricht ab",
        "Editing – Enter saves, Tab moves right, Esc cancels";
    edit_failed: "Bearbeiten nicht möglich", "Editing not possible";
    unsaved_title: "Ungespeicherte Änderungen", "Unsaved changes";
    discard: "Verwerfen", "Discard";
    cancel: "Abbrechen", "Cancel";
    bold: "F", "B";
    bold_hint: "Fett (Strg+B)", "Bold (Ctrl+B)";
    italic: "K", "I";
    italic_hint: "Kursiv (Strg+I)", "Italic (Ctrl+I)";
    text_color: "Schriftfarbe", "Text colour";
    fill_color: "Füllfarbe", "Fill colour";
    no_color: "Keine", "None";
    number_format: "Zahlenformat", "Number format";
    menu_file: "Datei", "File";
    menu_edit: "Bearbeiten", "Edit";
    menu_view: "Ansicht", "View";
    menu_help: "Hilfe", "Help";
    about: "Über ferrosheet …", "About ferrosheet …";
    licence: "Freie Software, MIT- oder Apache-2.0-Lizenz", "Free software, MIT or Apache 2.0 licence";
    check_updates: "Nach Updates suchen", "Check for updates";
    checking: "Suche nach Updates …", "Checking for updates …";
    up_to_date: "Das ist die neueste Version.", "This is the latest version.";
    check_failed: "Suche fehlgeschlagen", "Check failed";
    update_off: "Die Update-Suche ist in dieser Version ausgeschaltet.", "Updates are turned off in this build.";
    menu_format: "Format", "Format";
    recent: "Zuletzt geöffnet", "Recent files";
    no_recent: "Noch keine Dateien", "No files yet";
    open_button: "Datei öffnen …", "Open file …";
    save_as: "Speichern unter …", "Save as …";
    exit: "Beenden", "Exit";
    undo: "Rückgängig", "Undo";
    redo: "Wiederholen", "Redo";
    cut: "Ausschneiden", "Cut";
    copy: "Kopieren", "Copy";
    copy_raw: "Rohwerte kopieren", "Copy raw values";
    clear_contents: "Inhalte löschen", "Clear contents";
    find: "Suchen …", "Find …";
    bold_name: "Fett", "Bold";
    italic_name: "Kursiv", "Italic";
    zoom_in: "Vergrößern", "Zoom in";
    zoom_out: "Verkleinern", "Zoom out";
    zoom_reset: "Originalgröße (100 %)", "Actual size (100 %)";
    zoom_hint: "Zoom – eintippen oder ziehen", "Zoom – type or drag";
    select_column: "Spalte auswählen", "Select column";
    select_row: "Zeile auswählen", "Select row";
    fit_width: "Optimale Spaltenbreite", "Autofit column width";
    fit_height: "Optimale Zeilenhöhe", "Autofit row height";
    new_sheet: "Neues Blatt", "New sheet";
    rename_sheet: "Umbenennen", "Rename";
    delete_sheet: "Blatt löschen", "Delete sheet";
    delete_sheet_title: "Blatt löschen?", "Delete sheet?";
    delete: "Löschen", "Delete";
    sheet_rename_hint: "Doppelklick zum Umbenennen, Rechtsklick für mehr",
        "Double-click to rename, right-click for more";
    formulas_loading: "Formeln werden noch geladen …", "Formulas are still loading …";
    formula_label: "fx", "fx";
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

/// Shortcut shown next to a menu entry, e.g. "Strg+S".
pub fn shortcut(keys: &str) -> String {
    match lang() {
        Lang::De => keys
            .replace("Ctrl", "Strg")
            .replace("Shift", "Umschalt")
            .replace("Del", "Entf"),
        Lang::En => keys.to_owned(),
    }
}

pub fn delete_sheet_question(name: &str) -> String {
    match lang() {
        Lang::De => {
            format!("Blatt „{name}“ mit allen Inhalten löschen? Rückgängig geht mit Strg+Z.")
        }
        Lang::En => {
            format!("Delete sheet \"{name}\" with everything on it? Ctrl+Z brings it back.")
        }
    }
}

pub fn cannot_edit(reason: &str) -> String {
    let reason = if reason.is_empty() { "?" } else { reason };
    match lang() {
        Lang::De => {
            format!("ferrosheet kann diese Datei nicht bearbeiten, nur anzeigen. Grund: {reason}")
        }
        Lang::En => format!("ferrosheet can show this file but not edit it. Reason: {reason}"),
    }
}

pub fn save_question(file: &str) -> String {
    match lang() {
        Lang::De => format!("Änderungen an „{file}“ speichern?"),
        Lang::En => format!("Save changes to \"{file}\"?"),
    }
}

pub fn number_format_name(f: NumberFormat) -> &'static str {
    match f {
        NumberFormat::General => pick("Standard", "General"),
        NumberFormat::Decimal2 => pick("Zahl (0,00)", "Number (0.00)"),
        NumberFormat::Currency => pick("Währung (€)", "Currency (€)"),
        NumberFormat::Percent => pick("Prozent", "Percent"),
        NumberFormat::Date => pick("Datum", "Date"),
    }
}

pub fn hit_counter(current: usize, total: usize) -> String {
    match lang() {
        Lang::De => format!("{current} von {total}"),
        Lang::En => format!("{current} of {total}"),
    }
}

pub fn restarting() -> &'static str {
    match lang() {
        Lang::De => "Starte neu …",
        Lang::En => "Restarting …",
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
