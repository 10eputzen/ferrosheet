//! The spreadsheet grid: headers, virtualised cell painting and mouse selection.
//! Only the visible cells are painted, so scrolling cost is independent of sheet size.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Rect, ScrollArea, Sense, Stroke, UiBuilder, Vec2,
    pos2, vec2,
};
use ferrosheet_core::{CellPos, CellValue, Selection, Sheet, column_name};

pub const ROW_H: f32 = 22.0;
const HEADER_H: f32 = 22.0;
const DEFAULT_W: f32 = 100.0;
const MIN_W: f32 = 20.0;
const MAX_AUTOFIT_W: f32 = 600.0;
const AUTOFIT_ROWS: u32 = 1_000;
const PAD: f32 = 4.0;
const RESIZE_GRAB: f32 = 4.0;

/// Per-sheet view state.
#[derive(Default)]
pub struct GridState {
    pub col_widths: Vec<f32>,
    pub selection: Selection,
    /// Set to make a cell visible on the next frame.
    pub scroll_to: Option<CellPos>,
    /// Visible body size of the last frame (for page up/down).
    pub view: Vec2,
    offset: Vec2,
    resizing: Option<u32>,
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
) {
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
    let total = vec2(xs[cols as usize], rows as f32 * ROW_H);

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
        area = area.scroll_offset(offset_to_show(target, &xs, st.offset, body.size()));
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let out = ui
        .scope_builder(UiBuilder::new().max_rect(body), |ui| {
            area.show_viewport(ui, |ui, vp| {
                let (rect, resp) = ui.allocate_exact_size(total, Sense::click_and_drag());
                let origin = rect.min;
                let painter = ui.painter_at(ui.clip_rect());
                let (r0, r1) = visible_range(vp.min.y, vp.max.y, rows);
                let c0 = xs.partition_point(|&x| x <= vp.min.x).saturating_sub(1) as u32;
                let c1 = (xs.partition_point(|&x| x < vp.max.x) as u32).min(cols);
                let cell_rect = |r: u32, c: u32| {
                    Rect::from_min_size(
                        origin + vec2(xs[c as usize], r as f32 * ROW_H),
                        vec2(st.col_widths[c as usize], ROW_H),
                    )
                };

                painter.rect_filled(vp.translate(origin.to_vec2()), 0.0, pal.cell_bg);

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
                let top = origin.y + r0 as f32 * ROW_H;
                let bottom = origin.y + r1 as f32 * ROW_H;
                let left = origin.x + xs[c0 as usize];
                let right = origin.x + xs[c1 as usize];
                for c in c0..=c1 {
                    let x = origin.x + xs[c as usize] - 0.5;
                    painter.line_segment([pos2(x, top), pos2(x, bottom)], pal.col_line);
                }
                for r in r0..=r1 {
                    let y = origin.y + r as f32 * ROW_H - 0.5;
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
                        let color = if matches!(value, CellValue::Error(_)) {
                            visuals.error_fg_color
                        } else {
                            pal.text
                        };
                        let rect = cell_rect(r, c);
                        let (anchor, pos) = if value.is_numeric() {
                            (Align2::RIGHT_CENTER, rect.right_center() - vec2(PAD, 0.0))
                        } else {
                            (Align2::LEFT_CENTER, rect.left_center() + vec2(PAD, 0.0))
                        };
                        // first line only, clipped to the cell
                        let line_text = text.lines().next().unwrap_or("");
                        painter
                            .with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()))
                            .text(pos, anchor, line_text, font.clone(), color);
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

                // mouse selection
                if resp.is_pointer_button_down_on()
                    && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                {
                    let cell = cell_at(p - origin, &xs, rows, cols);
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
    let (r0, r1) = visible_range(st.offset.y, st.offset.y + body.height(), rows);
    for r in r0..r1 {
        let y0 = row_hdr.min.y + r as f32 * ROW_H - st.offset.y;
        let rect = Rect::from_min_size(pos2(row_hdr.min.x, y0), vec2(row_hdr.width(), ROW_H));
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

    // column resize: grab the right edge of a header cell
    let edge_at = |x: f32| -> Option<u32> {
        let cx = x - col_hdr.min.x + st.offset.x;
        (1..=cols)
            .find(|&c| (xs[c as usize] - cx).abs() <= RESIZE_GRAB)
            .map(|c| c - 1)
    };
    if let Some(p) = col_resp.hover_pos()
        && (st.resizing.is_some() || edge_at(p.x).is_some())
    {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    if col_resp.double_clicked()
        && let Some(c) = col_resp.interact_pointer_pos().and_then(|p| edge_at(p.x))
    {
        st.col_widths[c as usize] = autofit(ui, sheet, c, &font);
    }
    if col_resp.drag_started() {
        st.resizing = col_resp.interact_pointer_pos().and_then(|p| edge_at(p.x));
    }
    if let Some(c) = st.resizing {
        if col_resp.dragged() {
            let w = &mut st.col_widths[c as usize];
            *w = (*w + col_resp.drag_delta().x).max(MIN_W);
        }
        if col_resp.drag_stopped() {
            st.resizing = None;
        }
    } else {
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

    ui.allocate_rect(full, Sense::hover());
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
            let r = (((p.y - strip.min.y + st.offset.y) / ROW_H) as u32).min(rows - 1);
            if pressed && !shift {
                st.selection.anchor = CellPos::new(r, 0);
            }
            st.selection.cursor = CellPos::new(r, last_col);
            st.selection.anchor.col = 0;
        }
    }
}

fn autofit(ui: &egui::Ui, sheet: &Sheet, c: u32, font: &FontId) -> f32 {
    let widest = (0..sheet.rows.min(AUTOFIT_ROWS))
        .map(|r| sheet.display(r, c))
        .filter(|t| !t.is_empty())
        .map(|t| {
            ui.painter()
                .layout_no_wrap(
                    t.lines().next().unwrap_or("").to_owned(),
                    font.clone(),
                    Color32::WHITE,
                )
                .size()
                .x
        })
        .fold(0.0, f32::max);
    (widest + 2.0 * PAD + 2.0).clamp(MIN_W, MAX_AUTOFIT_W)
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

fn visible_range(top: f32, bottom: f32, rows: u32) -> (u32, u32) {
    let r0 = ((top / ROW_H).floor().max(0.0) as u32).min(rows);
    let r1 = ((bottom / ROW_H).ceil().max(0.0) as u32 + 1).min(rows);
    (r0, r1)
}

fn cell_at(p: Vec2, xs: &[f32], rows: u32, cols: u32) -> CellPos {
    let r = ((p.y.max(0.0) / ROW_H) as u32).min(rows - 1);
    let c = (xs.partition_point(|&x| x <= p.x.max(0.0)).saturating_sub(1) as u32).min(cols - 1);
    CellPos::new(r, c)
}

/// Scroll offset that brings `target` fully into view, moving as little as possible.
fn offset_to_show(target: CellPos, xs: &[f32], offset: Vec2, view: Vec2) -> Vec2 {
    let ci = (target.col as usize).min(xs.len().saturating_sub(2));
    let (x0, x1) = (xs[ci], xs[ci + 1]);
    let (y0, y1) = (target.row as f32 * ROW_H, (target.row + 1) as f32 * ROW_H);
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
