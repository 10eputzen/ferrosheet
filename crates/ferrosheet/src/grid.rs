//! The spreadsheet grid: headers, virtualised cell painting and mouse selection.
//! Only the visible cells are painted, so scrolling cost is independent of sheet size.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, ScrollArea, Sense, Stroke,
    UiBuilder, Vec2, pos2, vec2,
};
use ferrosheet_core::{CellPos, CellStyle, CellValue, Selection, Sheet, column_name};

use crate::rows::{ROW_H, RowHeights};

const HEADER_H: f32 = 22.0;
const DEFAULT_W: f32 = 100.0;
const MIN_W: f32 = 20.0;
const MAX_AUTOFIT_W: f32 = 600.0;
/// Lines measured when fitting a column (the ones with the most characters).
const AUTOFIT_CANDIDATES: usize = 64;
const PAD: f32 = 4.0;
const RESIZE_GRAB: f32 = 4.0;
const MAX_AUTOFIT_H: f32 = 400.0;
/// Top padding for text in rows taller than the default.
const TEXT_TOP: f32 = 3.0;

/// Per-sheet view state.
#[derive(Default)]
pub struct GridState {
    pub col_widths: Vec<f32>,
    pub selection: Selection,
    /// Set to make a cell visible on the next frame.
    pub scroll_to: Option<CellPos>,
    /// Visible body size of the last frame (for page up/down).
    pub view: Vec2,
    pub row_heights: RowHeights,
    /// The open cell editor.
    pub edit: Option<CellEdit>,
    offset: Vec2,
    resizing: Option<Resize>,
}

/// A cell being edited in place.
pub struct CellEdit {
    pub pos: CellPos,
    /// What the cell held when editing started.
    pub original: String,
    pub text: String,
    /// Focus the editor (cursor at the end) on the next frame.
    pub focus: bool,
}

/// Id of the in-cell editor, so the app can take its focus away when it closes.
pub fn edit_id() -> egui::Id {
    egui::Id::unique("ferrosheet_cell_edit")
}

/// What happened in the grid this frame that the app has to act on.
#[derive(Default)]
pub struct GridOut {
    pub double_clicked: bool,
    /// The editor lost focus without Enter, Tab or Esc (a click elsewhere).
    pub edit_left: bool,
}

/// Header edge being dragged.
#[derive(Clone, Copy)]
enum Resize {
    Col(u32),
    Row(u32),
}

impl GridState {
    pub fn visible_rows(&self) -> i64 {
        ((self.view.y / ROW_H) as i64 - 1).max(1)
    }
}

/// Grid colours. egui's defaults are tuned for widgets, not for dense tables:
/// text is too dim in dark mode and column lines vanish.
struct Palette {
    cell_bg: Color32,
    text: Color32,
    /// Vertical lines are stronger than horizontal ones so columns stand out.
    col_line: Stroke,
    row_line: Stroke,
    header_bg: Color32,
    header_text: Color32,
    header_border: Stroke,
}

impl Palette {
    fn new(v: &egui::Visuals) -> Self {
        let gray = Color32::from_gray;
        if v.dark_mode {
            Palette {
                cell_bg: gray(24),
                text: gray(235),
                col_line: Stroke::new(1.0, gray(78)),
                row_line: Stroke::new(1.0, gray(50)),
                header_bg: gray(40),
                header_text: gray(200),
                header_border: Stroke::new(1.0, gray(90)),
            }
        } else {
            Palette {
                cell_bg: Color32::WHITE,
                text: gray(20),
                col_line: Stroke::new(1.0, gray(190)),
                row_line: Stroke::new(1.0, gray(222)),
                header_bg: gray(236),
                header_text: gray(60),
                header_border: Stroke::new(1.0, gray(175)),
            }
        }
    }
}

/// What the header strips do when dragged.
enum Header {
    Cols,
    Rows,
}

pub fn show(
    ui: &mut egui::Ui,
    sheet: &Sheet,
    st: &mut GridState,
    hits: &[CellPos],
    current_hit: Option<CellPos>,
) -> GridOut {
    let mut grid_out = GridOut::default();
    let full = ui.available_rect_before_wrap();
    let row_hdr_w = (digits(sheet.rows.max(1000)) as f32 * 8.0 + 2.0 * PAD).max(40.0);
    let body = Rect::from_min_max(full.min + vec2(row_hdr_w, HEADER_H), full.max);

    // Virtual grid fills the window even for small sheets, like a real spreadsheet.
    let rows = sheet.rows.max((body.height() / ROW_H) as u32 + 1);
    let cols = sheet.cols.max((body.width() / DEFAULT_W) as u32 + 1);
    if st.col_widths.len() < cols as usize {
        st.col_widths.resize(cols as usize, DEFAULT_W);
    }
    let xs = prefix(&st.col_widths[..cols as usize]);
    let rh = &st.row_heights;
    let total = vec2(xs[cols as usize], rh.top(rows));

    let visuals = ui.visuals().clone();
    let pal = Palette::new(&visuals);
    let font = FontId::proportional(13.0);
    let sel_fill = visuals.selection.bg_fill.gamma_multiply(0.35);
    let sel_stroke = Stroke::new(2.0, visuals.selection.bg_fill);
    let hit_fill = Color32::from_rgba_unmultiplied(255, 220, 0, 70);
    let current_fill = Color32::from_rgba_unmultiplied(255, 140, 0, 140);

    // --- body ---------------------------------------------------------------
    let mut area = ScrollArea::both().id_salt("grid").auto_shrink(false);
    if let Some(target) = st.scroll_to.take() {
        area = area.scroll_offset(offset_to_show(target, &xs, rh, st.offset, body.size()));
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let out = ui
        .scope_builder(UiBuilder::new().max_rect(body), |ui| {
            area.show_viewport(ui, |ui, vp| {
                let (rect, resp) = ui.allocate_exact_size(total, Sense::click_and_drag());
                let origin = rect.min;
                let painter = ui.painter_at(ui.clip_rect());
                let (r0, r1) = visible_range(vp.min.y, vp.max.y, rows, rh);
                let c0 = xs.partition_point(|&x| x <= vp.min.x).saturating_sub(1) as u32;
                let c1 = (xs.partition_point(|&x| x < vp.max.x) as u32).min(cols);
                let cell_rect = |r: u32, c: u32| {
                    Rect::from_min_size(
                        origin + vec2(xs[c as usize], rh.top(r)),
                        vec2(st.col_widths[c as usize], rh.height(r)),
                    )
                };

                painter.rect_filled(vp.translate(origin.to_vec2()), 0.0, pal.cell_bg);

                // fill colours set by the user
                for r in r0..r1.min(sheet.rows) {
                    for c in c0..c1.min(sheet.cols) {
                        if let Some([red, g, b]) = sheet.style(r, c).fill {
                            painter.rect_filled(cell_rect(r, c), 0.0, Color32::from_rgb(red, g, b));
                        }
                    }
                }

                // search hits (sorted row-major, so the visible ones are contiguous)
                let first = hits.partition_point(|p| p.row < r0);
                for p in hits[first..].iter().take_while(|p| p.row < r1) {
                    if p.col >= c0 && p.col < c1 {
                        let fill = if Some(*p) == current_hit {
                            current_fill
                        } else {
                            hit_fill
                        };
                        painter.rect_filled(cell_rect(p.row, p.col), 0.0, fill);
                    }
                }

                // selection
                let (tl, br) = st.selection.bounds();
                let sel = cell_rect(tl.row.min(rows - 1), tl.col.min(cols - 1))
                    .union(cell_rect(br.row.min(rows - 1), br.col.min(cols - 1)));
                if st.selection.cell_count() > 1 {
                    painter.rect_filled(sel, 0.0, sel_fill);
                }

                // grid lines
                let top = origin.y + rh.top(r0);
                let bottom = origin.y + rh.top(r1);
                let left = origin.x + xs[c0 as usize];
                let right = origin.x + xs[c1 as usize];
                for c in c0..=c1 {
                    let x = origin.x + xs[c as usize] - 0.5;
                    painter.line_segment([pos2(x, top), pos2(x, bottom)], pal.col_line);
                }
                for r in r0..=r1 {
                    let y = origin.y + rh.top(r) - 0.5;
                    painter.line_segment([pos2(left, y), pos2(right, y)], pal.row_line);
                }

                // text
                for r in r0..r1.min(sheet.rows) {
                    for c in c0..c1.min(sheet.cols) {
                        let text = sheet.display(r, c);
                        if text.is_empty() {
                            continue;
                        }
                        let value = sheet.value(r, c);
                        let style = sheet.style(r, c);
                        let color = match style.color {
                            _ if matches!(value, CellValue::Error(_)) => visuals.error_fg_color,
                            Some([red, g, b]) => Color32::from_rgb(red, g, b),
                            None => pal.text,
                        };
                        let rect = cell_rect(r, c);
                        // Default rows show the first line centred; taller rows show
                        // all lines from the top, clipped to the cell.
                        let tall = rect.height() > ROW_H + 1.0;
                        let (anchor, pos) = match (value.is_numeric(), tall) {
                            (true, false) => {
                                (Align2::RIGHT_CENTER, rect.right_center() - vec2(PAD, 0.0))
                            }
                            (false, false) => {
                                (Align2::LEFT_CENTER, rect.left_center() + vec2(PAD, 0.0))
                            }
                            (true, true) => {
                                (Align2::RIGHT_TOP, rect.right_top() + vec2(-PAD, TEXT_TOP))
                            }
                            (false, true) => {
                                (Align2::LEFT_TOP, rect.left_top() + vec2(PAD, TEXT_TOP))
                            }
                        };
                        let line_text = if tall {
                            &*text
                        } else {
                            text.lines().next().unwrap_or("")
                        };
                        let clipped =
                            painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
                        cell_text(&clipped, pos, anchor, line_text, &font, color, style);
                    }
                }

                // cursor
                let cur = st.selection.cursor;
                if cur.row < rows && cur.col < cols {
                    painter.rect_stroke(
                        cell_rect(cur.row, cur.col),
                        0.0,
                        sel_stroke,
                        egui::StrokeKind::Inside,
                    );
                }

                // in-cell editor
                if let Some(e) = &mut st.edit
                    && e.pos.row < rows
                    && e.pos.col < cols
                {
                    let rect = cell_rect(e.pos.row, e.pos.col);
                    let edit = ui.put(
                        rect,
                        egui::TextEdit::singleline(&mut e.text)
                            .id(edit_id())
                            .font(font.clone())
                            .margin(vec2(PAD - 1.0, 2.0))
                            .desired_width(rect.width()),
                    );
                    let address = column_name(e.pos.col) + &(e.pos.row + 1).to_string();
                    edit.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::TextInput, true, &address)
                    });
                    // Focus after the editor was drawn: the key that opened it is not
                    // typed a second time.
                    if std::mem::take(&mut e.focus) {
                        edit.request_focus();
                        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), edit_id())
                            .unwrap_or_default();
                        let end = egui::text::CCursor::new(e.text.chars().count());
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(end)));
                        state.store(ui.ctx(), edit_id());
                    } else if edit.lost_focus() {
                        grid_out.edit_left = true;
                    }
                }
                if resp.double_clicked() {
                    grid_out.double_clicked = true;
                }

                // mouse selection
                if resp.is_pointer_button_down_on()
                    && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                {
                    let cell = cell_at(p - origin, &xs, rh, rows, cols);
                    if ui.input(|i| i.pointer.primary_pressed()) && !shift {
                        st.selection = Selection::single(cell);
                    } else {
                        st.selection.cursor = cell;
                    }
                    st.scroll_to = Some(cell);
                }
            })
        })
        .inner;
    st.offset = out.state.clamped_offset();
    st.view = body.size();

    // --- headers ------------------------------------------------------------
    let col_hdr = Rect::from_min_max(pos2(body.min.x, full.min.y), pos2(full.max.x, body.min.y));
    let row_hdr = Rect::from_min_max(pos2(full.min.x, body.min.y), pos2(body.min.x, full.max.y));
    let corner = Rect::from_min_max(full.min, body.min);
    let (tl, br) = st.selection.bounds();

    let painter = ui.painter_at(col_hdr);
    painter.rect_filled(col_hdr, 0.0, pal.header_bg);
    let (c0, c1) = (
        xs.partition_point(|&x| x <= st.offset.x).saturating_sub(1) as u32,
        (xs.partition_point(|&x| x < st.offset.x + body.width()) as u32).min(cols),
    );
    for c in c0..c1 {
        let x0 = col_hdr.min.x + xs[c as usize] - st.offset.x;
        let rect = Rect::from_min_size(
            pos2(x0, col_hdr.min.y),
            vec2(st.col_widths[c as usize], HEADER_H),
        );
        if (tl.col..=br.col).contains(&c) {
            painter.rect_filled(rect, 0.0, sel_fill);
        }
        painter.line_segment([rect.right_top(), rect.right_bottom()], pal.header_border);
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            column_name(c),
            font.clone(),
            pal.header_text,
        );
    }

    let painter = ui.painter_at(row_hdr);
    painter.rect_filled(row_hdr, 0.0, pal.header_bg);
    let rh = &st.row_heights;
    let (r0, r1) = visible_range(st.offset.y, st.offset.y + body.height(), rows, rh);
    for r in r0..r1 {
        let y0 = row_hdr.min.y + rh.top(r) - st.offset.y;
        let rect =
            Rect::from_min_size(pos2(row_hdr.min.x, y0), vec2(row_hdr.width(), rh.height(r)));
        if (tl.row..=br.row).contains(&r) {
            painter.rect_filled(rect, 0.0, sel_fill);
        }
        painter.line_segment([rect.left_bottom(), rect.right_bottom()], pal.header_border);
        painter.text(
            rect.right_center() - vec2(PAD, 0.0),
            Align2::RIGHT_CENTER,
            (r + 1).to_string(),
            font.clone(),
            pal.header_text,
        );
    }
    ui.painter_at(corner)
        .rect_filled(corner, 0.0, pal.header_bg);
    // borders between headers and body
    let p = ui.painter();
    p.line_segment(
        [
            pos2(full.min.x, body.min.y - 0.5),
            pos2(full.max.x, body.min.y - 0.5),
        ],
        pal.header_border,
    );
    p.line_segment(
        [
            pos2(body.min.x - 0.5, full.min.y),
            pos2(body.min.x - 0.5, full.max.y),
        ],
        pal.header_border,
    );

    // --- header interaction ---------------------------------------------------
    let corner_resp = ui.interact(corner, ui.scope_id().with("corner"), Sense::click());
    if corner_resp.clicked() {
        st.selection = Selection {
            anchor: CellPos::new(0, 0),
            cursor: CellPos::new(rows.max(sheet.rows) - 1, cols.max(sheet.cols) - 1),
        };
    }
    let col_resp = ui.interact(
        col_hdr,
        ui.scope_id().with("col_hdr"),
        Sense::click_and_drag(),
    );
    let row_resp = ui.interact(
        row_hdr,
        ui.scope_id().with("row_hdr"),
        Sense::click_and_drag(),
    );
    let last_row = sheet.rows.max(1) - 1;
    let last_col = sheet.cols.max(1) - 1;

    // Resize: grab the right edge of a column header or the bottom edge of a row header.
    // The edge is taken from where the button went down: once the drag threshold is
    // passed the pointer is already a few pixels away from it.
    let col_edge = |x: f32| -> Option<u32> {
        let cx = x - col_hdr.min.x + st.offset.x;
        (1..=cols)
            .find(|&c| (xs[c as usize] - cx).abs() <= RESIZE_GRAB)
            .map(|c| c - 1)
    };
    let row_edge = |y: f32| -> Option<u32> {
        let rh = &st.row_heights;
        let cy = y - row_hdr.min.y + st.offset.y;
        let r = rh.row_at(cy);
        if rh.top(r) + rh.height(r) - cy <= RESIZE_GRAB {
            Some(r)
        } else if cy - rh.top(r) <= RESIZE_GRAB && r > 0 {
            Some(r - 1)
        } else {
            None
        }
    };
    let col_hover = col_resp.hover_pos().and_then(|p| col_edge(p.x));
    let row_hover = row_resp.hover_pos().and_then(|p| row_edge(p.y));
    match (st.resizing, col_hover, row_hover) {
        (Some(Resize::Col(_)), _, _) | (None, Some(_), _) => {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal)
        }
        (Some(Resize::Row(_)), _, _) | (None, _, Some(_)) => {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical)
        }
        _ => {}
    }
    let press = ui.input(|i| i.pointer.press_origin());
    let col_press_edge = press
        .filter(|p| col_hdr.contains(*p))
        .and_then(|p| col_edge(p.x));
    let row_press_edge = press
        .filter(|p| row_hdr.contains(*p))
        .and_then(|p| row_edge(p.y));

    // The press origin is gone once the button is released, so the double-click
    // (reported on release) takes the edge from where the pointer is.
    if col_resp.double_clicked()
        && let Some(c) = col_resp.interact_pointer_pos().and_then(|p| col_edge(p.x))
    {
        st.col_widths[c as usize] = autofit(ui, sheet, c, &font);
    }
    if row_resp.double_clicked()
        && let Some(r) = row_resp.interact_pointer_pos().and_then(|p| row_edge(p.y))
    {
        let h = autofit_row(ui, sheet, r, &font);
        st.row_heights.set(r, h);
    }
    if col_resp.drag_started() {
        st.resizing = col_press_edge.map(Resize::Col);
    }
    if row_resp.drag_started() {
        st.resizing = row_press_edge.map(Resize::Row);
    }
    match st.resizing {
        Some(Resize::Col(c)) => {
            let w = &mut st.col_widths[c as usize];
            *w = (*w + col_resp.drag_delta().x).max(MIN_W);
        }
        Some(Resize::Row(r)) => {
            let h = st.row_heights.height(r) + row_resp.drag_delta().y;
            st.row_heights.set(r, h);
        }
        None => {}
    }
    if col_resp.drag_stopped() || row_resp.drag_stopped() {
        st.resizing = None;
    }
    // A press on an edge resizes; anywhere else on a header it selects.
    if st.resizing.is_none() && col_press_edge.is_none() {
        header_select(
            ui,
            &col_resp,
            Header::Cols,
            st,
            &xs,
            col_hdr,
            rows,
            cols,
            last_row,
            last_col,
            shift,
        );
    }
    if st.resizing.is_none() && row_press_edge.is_none() {
        header_select(
            ui,
            &row_resp,
            Header::Rows,
            st,
            &xs,
            row_hdr,
            rows,
            cols,
            last_row,
            last_col,
            shift,
        );
    }

    ui.allocate_rect(full, Sense::hover());
    grid_out
}

/// Cell text in the user's style. egui's default font has no bold face, so bold is the
/// text drawn twice, a fraction of a pixel apart.
fn cell_text(
    painter: &Painter,
    pos: Pos2,
    anchor: Align2,
    text: &str,
    font: &FontId,
    color: Color32,
    style: CellStyle,
) {
    if !style.bold && !style.italic {
        painter.text(pos, anchor, text, font.clone(), color);
        return;
    }
    let job = egui::text::LayoutJob::single_section(
        text.to_owned(),
        egui::TextFormat {
            font_id: font.clone(),
            color,
            italics: style.italic,
            ..Default::default()
        },
    );
    let galley = painter.layout_job(job);
    let at = anchor.anchor_size(pos, galley.size()).min;
    if style.bold {
        painter.galley(at + vec2(0.6, 0.0), galley.clone(), color);
    }
    painter.galley(at, galley, color);
}

/// Click or drag on a header strip selects whole columns or rows.
#[allow(clippy::too_many_arguments)]
fn header_select(
    ui: &egui::Ui,
    resp: &egui::Response,
    kind: Header,
    st: &mut GridState,
    xs: &[f32],
    strip: Rect,
    rows: u32,
    cols: u32,
    last_row: u32,
    last_col: u32,
    shift: bool,
) {
    if !resp.is_pointer_button_down_on() {
        return;
    }
    let Some(p) = ui.input(|i| i.pointer.interact_pos()) else {
        return;
    };
    let pressed = ui.input(|i| i.pointer.primary_pressed());
    match kind {
        Header::Cols => {
            let x = p.x - strip.min.x + st.offset.x;
            let c = (xs.partition_point(|&e| e <= x).saturating_sub(1) as u32).min(cols - 1);
            if pressed && !shift {
                st.selection.anchor = CellPos::new(0, c);
            }
            st.selection.cursor = CellPos::new(last_row, c);
            st.selection.anchor.row = 0;
        }
        Header::Rows => {
            let r = st
                .row_heights
                .row_at(p.y - strip.min.y + st.offset.y)
                .min(rows - 1);
            if pressed && !shift {
                st.selection.anchor = CellPos::new(r, 0);
            }
            st.selection.cursor = CellPos::new(r, last_col);
            st.selection.anchor.col = 0;
        }
    }
}

fn autofit(ui: &egui::Ui, sheet: &Sheet, c: u32, font: &FontId) -> f32 {
    // Counting characters is cheap, laying out text is not: look at every line of every
    // row, then measure only the ones with the most characters.
    let mut lines: Vec<(usize, String)> = Vec::new();
    for r in 0..sheet.rows {
        for l in sheet.display(r, c).lines() {
            let n = l.chars().count();
            if n > 0 {
                lines.push((n, l.to_owned()));
            }
        }
    }
    if lines.len() > AUTOFIT_CANDIDATES {
        lines.select_nth_unstable_by(AUTOFIT_CANDIDATES, |a, b| b.0.cmp(&a.0));
        lines.truncate(AUTOFIT_CANDIDATES);
    }
    let widest = lines
        .iter()
        .map(|(_, l)| {
            ui.painter()
                .layout_no_wrap(l.clone(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max);
    (widest + 2.0 * PAD + 2.0).clamp(MIN_W, MAX_AUTOFIT_W)
}

/// Height that shows every line of the tallest cell in row `r`.
fn autofit_row(ui: &egui::Ui, sheet: &Sheet, r: u32, font: &FontId) -> f32 {
    if r >= sheet.rows {
        return ROW_H;
    }
    let tallest = (0..sheet.cols)
        .map(|c| sheet.display(r, c))
        .filter(|t| t.contains('\n'))
        .map(|t| {
            ui.painter()
                .layout_no_wrap(t.into_owned(), font.clone(), Color32::WHITE)
                .size()
                .y
        })
        .fold(0.0, f32::max);
    (tallest + 2.0 * TEXT_TOP).clamp(ROW_H, MAX_AUTOFIT_H)
}

fn prefix(widths: &[f32]) -> Vec<f32> {
    let mut xs = Vec::with_capacity(widths.len() + 1);
    xs.push(0.0);
    let mut acc = 0.0;
    for w in widths {
        acc += w;
        xs.push(acc);
    }
    xs
}

fn visible_range(top: f32, bottom: f32, rows: u32, rh: &RowHeights) -> (u32, u32) {
    (rh.row_at(top).min(rows), (rh.row_at(bottom) + 1).min(rows))
}

fn cell_at(p: Vec2, xs: &[f32], rh: &RowHeights, rows: u32, cols: u32) -> CellPos {
    let r = rh.row_at(p.y).min(rows - 1);
    let c = (xs.partition_point(|&x| x <= p.x.max(0.0)).saturating_sub(1) as u32).min(cols - 1);
    CellPos::new(r, c)
}

/// Scroll offset that brings `target` fully into view, moving as little as possible.
fn offset_to_show(target: CellPos, xs: &[f32], rh: &RowHeights, offset: Vec2, view: Vec2) -> Vec2 {
    let ci = (target.col as usize).min(xs.len().saturating_sub(2));
    let (x0, x1) = (xs[ci], xs[ci + 1]);
    let y0 = rh.top(target.row);
    let y1 = y0 + rh.height(target.row);
    let fit = |lo: f32, hi: f32, off: f32, size: f32| {
        if lo < off {
            lo
        } else if hi > off + size {
            hi - size
        } else {
            off
        }
    };
    vec2(fit(x0, x1, offset.x, view.x), fit(y0, y1, offset.y, view.y))
}

fn digits(n: u32) -> u32 {
    n.checked_ilog10().unwrap_or(0) + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use ferrosheet_core::{CellValue, Locale};

    /// Grid with a 5×5 sheet; the state also records where the grid starts on screen.
    fn harness() -> Harness<'static, (GridState, egui::Pos2)> {
        let cells = (0..25).map(|i| CellValue::Number(i as f64)).collect();
        harness_for(Sheet::from_cells(
            "t".into(),
            5,
            5,
            cells,
            Locale::Invariant,
        ))
    }

    fn harness_for(sheet: Sheet) -> Harness<'static, (GridState, egui::Pos2)> {
        Harness::builder()
            .with_size(vec2(800.0, 600.0))
            .with_step_dt(1.0 / 60.0) // real frame time, so double-clicks register
            .build_ui_state(
                move |ui, (st, origin): &mut (GridState, egui::Pos2)| {
                    *origin = ui.available_rect_before_wrap().min;
                    show(ui, &sheet, st, &[], None);
                },
                (GridState::default(), egui::Pos2::ZERO),
            )
    }

    /// Press at `from`, move in small steps by `by`, release.
    fn drag(h: &mut Harness<'static, (GridState, egui::Pos2)>, from: egui::Pos2, by: Vec2) {
        h.hover_at(from);
        h.run();
        h.drag_at(from);
        h.run();
        for i in 1..=5 {
            h.hover_at(from + by * (i as f32 / 5.0));
            h.run();
        }
        h.drop_at(from + by);
        h.run();
    }

    fn double_click(h: &mut Harness<'static, (GridState, egui::Pos2)>, at: egui::Pos2) {
        h.hover_at(at);
        h.run();
        for pressed in [true, false, true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run();
    }

    /// One text column, `rows` rows; `texts` puts text into chosen rows.
    fn text_sheet(rows: u32, texts: &[(u32, &str)]) -> Sheet {
        let mut cells = vec![CellValue::Empty; rows as usize];
        for (r, t) in texts {
            cells[*r as usize] = CellValue::Text((*t).into());
        }
        Sheet::from_cells("t".into(), rows, 1, cells, Locale::Invariant)
    }

    const LONG: &str = "eine deutlich längere zweite Zeile mit viel Text darin";

    #[test]
    fn double_clicking_a_column_edge_fits_the_longest_line_of_any_row() {
        // The long line is the second line of a cell, far below the first 1000 rows.
        let mut h = harness_for(text_sheet(
            5000,
            &[
                (0, "kurz"),
                (
                    4500,
                    &format!(
                        "x
{LONG}"
                    ),
                ),
            ],
        ));
        h.run();
        let o = h.state().1;
        double_click(&mut h, o + vec2(40.0 + DEFAULT_W, HEADER_H / 2.0));
        let w = h.state().0.col_widths[0];
        let needed = h.ctx.fonts_mut(|f| {
            f.layout_no_wrap(
                LONG.into(),
                egui::TextStyle::Body.resolve(&egui::Style::default()),
                Color32::WHITE,
            )
            .size()
            .x
        });
        assert!(
            w >= needed + 2.0 * PAD && w < needed + 4.0 * PAD + 4.0,
            "width {w}, text {needed}"
        );
    }

    #[test]
    fn double_clicking_a_column_edge_shrinks_a_narrow_column() {
        let mut h = harness_for(text_sheet(3, &[(0, "ab")]));
        h.run();
        let o = h.state().1;
        double_click(&mut h, o + vec2(40.0 + DEFAULT_W, HEADER_H / 2.0));
        let w = h.state().0.col_widths[0];
        assert!(w < DEFAULT_W / 2.0, "width {w}");
    }

    #[test]
    fn double_clicking_a_row_edge_fits_all_lines_and_back_to_default() {
        let mut h = harness_for(text_sheet(
            3,
            &[
                (
                    0,
                    "eins
zwei
drei",
                ),
                (1, "eine Zeile"),
            ],
        ));
        h.run();
        let o = h.state().1;
        double_click(&mut h, o + vec2(20.0, HEADER_H + ROW_H));
        let tall = h.state().0.row_heights.height(0);
        assert!(tall > 2.0 * ROW_H, "height {tall}"); // three lines of text

        // a single-line row that was made taller goes back to the default height
        h.state_mut().0.row_heights.set(1, 80.0);
        h.run_steps(60); // a second apart, or egui counts a triple-click
        double_click(&mut h, o + vec2(20.0, HEADER_H + tall + 80.0));
        assert_eq!(h.state().0.row_heights.height(1), ROW_H);
    }

    // Row header is 40 px wide (4 digits), columns 100 px, rows 22 px.
    #[test]
    fn dragging_a_column_edge_changes_its_width_and_keeps_it() {
        let mut h = harness();
        h.run();
        let o = h.state().1;
        drag(
            &mut h,
            o + vec2(40.0 + DEFAULT_W, HEADER_H / 2.0),
            vec2(50.0, 0.0),
        );
        let w = h.state().0.col_widths[0];
        assert!(w > DEFAULT_W + 35.0, "width {w}");
        h.run();
        assert_eq!(h.state().0.col_widths[0], w);
        assert_eq!(h.state().0.col_widths[1], DEFAULT_W);
    }

    #[test]
    fn dragging_a_row_edge_changes_its_height() {
        let mut h = harness();
        h.run();
        let o = h.state().1;
        drag(&mut h, o + vec2(20.0, HEADER_H + ROW_H), vec2(0.0, 30.0));
        let rh = &h.state().0.row_heights;
        assert!(rh.height(0) > ROW_H + 20.0, "height {}", rh.height(0));
        assert_eq!(rh.height(1), ROW_H);
    }

    #[test]
    fn dragging_inside_a_column_header_selects_columns() {
        let mut h = harness();
        h.run();
        let o = h.state().1;
        drag(
            &mut h,
            o + vec2(40.0 + 50.0, HEADER_H / 2.0),
            vec2(DEFAULT_W, 0.0),
        );
        let st = &h.state().0;
        assert_eq!(st.col_widths[0], DEFAULT_W);
        let (tl, br) = st.selection.bounds();
        assert_eq!((tl.col, br.col, tl.row), (0, 1, 0));
    }
}
