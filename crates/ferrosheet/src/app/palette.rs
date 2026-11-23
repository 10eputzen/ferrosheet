//! Command palette (Ctrl+Shift+P): every menu command, sheet and recent file in one
//! list, filtered by what is typed, run with Enter. Typing a few letters is faster than
//! hunting through menus, and the palette shows each command's shortcut too.

use std::path::PathBuf;

use eframe::egui::{self, Key, Modifiers};
use ferrosheet_core::{CopyFormat, NumberFormat, StyleChange};

use super::{App, Cmd};
use crate::text;

/// Rows shown at once; the rest is reached by typing more.
const SHOWN: usize = 12;

#[derive(Default)]
pub(super) struct Palette {
    pub(super) query: String,
    /// Index into the filtered list.
    selected: usize,
    /// Focus the text field next frame.
    focus: bool,
}

impl Palette {
    pub(super) fn new() -> Palette {
        Palette {
            focus: true,
            ..Palette::default()
        }
    }
}

/// One line of the palette.
pub(super) struct Entry {
    /// Where the command lives (menu name), shown dimmed before the label.
    pub(super) group: &'static str,
    pub(super) label: String,
    pub(super) keys: &'static str,
    pub(super) cmd: Cmd,
}

/// How well `query` matches `text`: every character of the query in order, case
/// ignored. Runs of consecutive characters and hits at the start of a word count
/// more, gaps less. `None` when it does not match at all.
pub(super) fn score(query: &str, text: &str) -> Option<i32> {
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut score = 0;
    let mut at = 0;
    let mut last: Option<usize> = None;
    for q in query.to_lowercase().chars().filter(|c| !c.is_whitespace()) {
        let i = at + text[at..].iter().position(|&t| t == q)?;
        let word_start = i == 0 || !text[i - 1].is_alphanumeric();
        score += match last {
            Some(l) if l + 1 == i => 8,
            _ if word_start => 6,
            Some(l) => 1 - ((i - l) as i32).min(5),
            None => 1 - (i as i32).min(5),
        };
        last = Some(i);
        at = i + 1;
    }
    Some(score)
}

/// The entries matching `query`, best first; all of them in their order for an empty one.
pub(super) fn filter(entries: Vec<Entry>, query: &str) -> Vec<Entry> {
    if query.trim().is_empty() {
        return entries;
    }
    let mut scored: Vec<(i32, usize, Entry)> = entries
        .into_iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let label = score(query, &e.label);
            let full = score(query, &format!("{} {}", e.group, e.label));
            // a match in the label itself beats one that needs the menu name
            let s = label.map(|s| s + 4).max(full)?;
            Some((s, i, e))
        })
        .collect();
    scored.sort_by_key(|(s, i, _)| (-s, *i));
    scored.into_iter().map(|(_, _, e)| e).collect()
}

impl App {
    /// Everything the palette offers right now.
    fn palette_entries(&mut self) -> Vec<Entry> {
        let view = self.active_view().is_some();
        let (file, edit, data, view_g, format, help) = (
            text::menu_file(),
            text::menu_edit(),
            text::menu_data(),
            text::menu_view(),
            text::menu_format(),
            text::menu_help(),
        );
        let mut out = Vec::new();
        let mut add = |group, label: &str, keys, cmd| {
            out.push(Entry {
                group,
                label: label.to_owned(),
                keys,
                cmd,
            })
        };
        add(file, text::open(), "Ctrl+O", Cmd::Open);
        if self.dirty() {
            add(file, text::save(), "Ctrl+S", Cmd::Save);
        }
        if self.doc.is_some() {
            add(file, text::save_as(), "Ctrl+Shift+S", Cmd::SaveAs);
        }
        if view {
            add(edit, text::undo(), "Ctrl+Z", Cmd::Undo);
            add(edit, text::redo(), "Ctrl+Y", Cmd::Redo);
            add(edit, text::cut(), "Ctrl+X", Cmd::Cut);
            add(edit, text::copy(), "Ctrl+C", Cmd::Copy(CopyFormat::Display));
            let raw = Cmd::Copy(CopyFormat::Invariant);
            add(edit, text::copy_raw(), "Ctrl+Shift+C", raw);
            add(edit, text::paste(), "Ctrl+V", Cmd::Paste);
            add(edit, text::fill_down(), "Ctrl+D", Cmd::FillDown);
            add(edit, text::fill_right(), "Ctrl+R", Cmd::FillRight);
            add(edit, text::clear_contents(), "Del", Cmd::Clear);
            add(edit, text::insert_rows_above(), "", Cmd::InsertRows(false));
            add(edit, text::insert_rows_below(), "", Cmd::InsertRows(true));
            add(edit, text::delete_rows(), "", Cmd::DeleteRows);
            add(edit, text::insert_cols_left(), "", Cmd::InsertCols(false));
            add(edit, text::insert_cols_right(), "", Cmd::InsertCols(true));
            add(edit, text::delete_cols(), "", Cmd::DeleteCols);
            add(edit, text::select_column(), "", Cmd::SelectCols);
            add(edit, text::select_row(), "", Cmd::SelectRows);
            add(edit, text::go_to(), "Ctrl+G", Cmd::GoTo);
        }
        if self.doc.is_some() {
            add(edit, text::find(), "Ctrl+F", Cmd::Find);
            add(edit, text::replace_menu(), "Ctrl+H", Cmd::Replace);
            let (up, down) = (self.zoom_step(true), self.zoom_step(false));
            add(view_g, text::zoom_in(), "Ctrl++", Cmd::Zoom(up));
            add(view_g, text::zoom_out(), "Ctrl+-", Cmd::Zoom(down));
            add(view_g, text::zoom_reset(), "Ctrl+0", Cmd::Zoom(1.0));
        }
        if view {
            add(view_g, text::fit_width(), "", Cmd::FitCols);
            add(view_g, text::fit_height(), "", Cmd::FitRows);
            let freeze = text::freeze();
            add(
                freeze,
                text::freeze_top_row(),
                "",
                Cmd::Freeze(Some((1, 0))),
            );
            add(
                freeze,
                text::freeze_first_col(),
                "",
                Cmd::Freeze(Some((0, 1))),
            );
            add(freeze, text::freeze_at_cursor(), "", Cmd::Freeze(None));
            add(freeze, text::unfreeze(), "", Cmd::Freeze(Some((0, 0))));
            add(data, text::sort_ascending(), "", Cmd::Sort(false));
            add(data, text::sort_descending(), "", Cmd::Sort(true));
            add(format, text::bold_name(), "Ctrl+B", Cmd::Bold);
            add(format, text::italic_name(), "Ctrl+I", Cmd::Italic);
            for f in NumberFormat::ALL {
                let label = format!("{}: {}", text::number_format(), text::number_format_name(f));
                add(format, &label, "", Cmd::Style(StyleChange::Number(f)));
            }
            add(edit, text::new_sheet(), "", Cmd::NewSheet);
        }
        let names = self
            .doc
            .as_ref()
            .map(|d| d.names.clone())
            .unwrap_or_default();
        for (i, name) in names.iter().enumerate() {
            add(text::sheet(), name, "", Cmd::Switch(i));
        }
        let recent: Vec<PathBuf> = self.recent.existing().cloned().collect();
        for p in recent {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            add(text::recent(), &name, "", Cmd::OpenRecent(p));
        }
        #[cfg(windows)]
        add(file, text::set_default(), "", Cmd::Register);
        add(help, text::about(), "", Cmd::About);
        add(file, text::exit(), "", Cmd::Exit);
        out
    }

    /// Shows the palette while it is open; returns the command picked.
    pub(super) fn palette_modal(&mut self, ctx: &egui::Context) -> Option<Cmd> {
        self.palette.as_ref()?;
        let entries = self.palette_entries();
        let p = self.palette.as_mut()?;
        let mut found = filter(entries, &p.query);
        let mut picked = None;
        let modal = egui::Modal::new(egui::Id::unique("palette")).area(
            egui::Modal::default_area(egui::Id::unique("palette_area"))
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0)),
        );
        let resp = modal.show(ctx, |ui| {
            ui.set_width(460.0);
            // Arrows pick a line; the text field must not see them.
            let (down, up, enter) = ui.input_mut(|i| {
                (
                    i.consume_key(Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(Modifiers::NONE, Key::Enter),
                )
            });
            let n = found.len();
            if down && n > 0 {
                p.selected = (p.selected + 1) % n;
            }
            if up && n > 0 {
                p.selected = (p.selected + n - 1) % n;
            }
            let field = ui.add(
                egui::TextEdit::singleline(&mut p.query)
                    .id(palette_id())
                    .hint_text(text::palette_hint())
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut p.focus) {
                field.request_focus();
            }
            if field.changed() {
                p.selected = 0;
            }
            p.selected = p.selected.min(n.saturating_sub(1));
            ui.add_space(4.0);
            if n == 0 {
                ui.weak(text::no_commands());
            }
            // Keep the selected line in the window of shown lines.
            let first = p.selected.saturating_sub(SHOWN - 1);
            for (i, e) in found.iter().enumerate().skip(first).take(SHOWN) {
                let label = format!("{} › {}", e.group, e.label);
                let mut b = egui::Button::new(label)
                    .selected(i == p.selected)
                    .frame_when_inactive(false)
                    .min_size(egui::vec2(ui.available_width(), 0.0));
                if !e.keys.is_empty() {
                    b = b.shortcut_text(text::shortcut(e.keys));
                }
                if ui.add(b).clicked() {
                    picked = Some(i);
                }
            }
            if enter && n > 0 {
                picked = Some(p.selected);
            }
        });
        if resp.should_close() || picked.is_some() {
            self.palette = None;
            ctx.memory_mut(|m| m.surrender_focus(palette_id()));
        }
        picked.map(|i| found.swap_remove(i).cmd)
    }
}

fn palette_id() -> egui::Id {
    egui::Id::unique("ferrosheet_palette")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(group: &'static str, label: &str) -> Entry {
        Entry {
            group,
            label: label.into(),
            keys: "",
            cmd: Cmd::About,
        }
    }

    #[test]
    fn letters_in_order_match_and_word_starts_win() {
        assert!(score("fd", "Fill down").is_some());
        assert!(score("df", "Fill down").is_none());
        assert!(score("sort", "Sort ascending (A–Z)") > score("sort", "Insert rows"));
        assert!(score("", "anything").is_some());
    }

    #[test]
    fn the_label_ranks_before_the_menu_name_and_empty_shows_all() {
        let all = || {
            vec![
                entry("Edit", "Copy"),
                entry("Format", "Bold"),
                entry("Edit", "Paste"),
            ]
        };
        let labels = |v: Vec<Entry>| v.into_iter().map(|e| e.label).collect::<Vec<_>>();
        assert_eq!(labels(filter(all(), "")), ["Copy", "Bold", "Paste"]);
        assert_eq!(labels(filter(all(), "past")), ["Paste"]);
        // "edit" is only in the menu name, and still finds both edit commands
        assert_eq!(labels(filter(all(), "edit")), ["Copy", "Paste"]);
    }
}
