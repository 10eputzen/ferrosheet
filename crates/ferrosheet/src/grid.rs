//! The spreadsheet grid: headers, virtualised cell painting and mouse selection.
//! Only the visible cells are painted, so scrolling cost is independent of sheet size.

use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Painter, Pos2, Rect, Response, ScrollArea, Sense,
    Stroke, UiBuilder, Vec2, pos2, vec2,
};
use ferrosheet_core::{
    CellPos, CellStyle, CellValue, Fill, HAlign, Lines, Selection, Sheet, column_name, find_refs,
    ref_slot, ref_text, splice,
};

#[cfg(test)]
use crate::rows::ROW_H;
use crate::rows::RowHeights;

pub const MIN_ZOOM: f32 = 0.5;
pub const MAX_ZOOM: f32 = 3.0;

const HEADER_H: f32 = 22.0;
const FONT_SIZE: f32 = 13.0;
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
/// Size of the fill handle at the bottom-right corner of the selection.
const HANDLE: f32 = 7.0;

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
    /// Fit these columns or rows to their content on the next frame.
    pub fit: Option<Fit>,
    /// Frozen panes: this many rows at the top and columns at the left stay in view.
    pub frozen: (u32, u32),
    offset: Vec2,
    /// Screen position of the top-left corner of A1 in the last frame.
    origin: Pos2,
    resizing: Option<Resize>,
    /// The fill handle is being dragged: the selection it started from and where the
    /// pointer would extend it to.
    filling: Option<(Selection, Option<Fill>)>,
    /// Zoom the widths and heights are stored at (0 = not drawn yet, i.e. 100 %).
    zoom: f32,
}

/// Sizes of deleted rows or columns at 100 % zoom, kept so undo can bring them back.
#[derive(Debug, Default)]
pub struct Removed {
    cols: Vec<f32>,
    rows: Vec<(u32, f32)>,
}

/// Columns or rows (first, last) to fit to their content.
#[derive(Clone, Copy, Debug)]
pub enum Fit {
    Cols(u32, u32),
    Rows(u32, u32),
}

/// A cell being edited in place.
pub struct CellEdit {
    pub pos: CellPos,
    /// What the cell held when editing started.
    pub original: String,
    pub text: String,
    /// Focus the editor (cursor at the end) on the next frame.
    pub focus: bool,
    /// Typing happens in the formula bar; the cell only mirrors the text.
    pub in_bar: bool,
    /// Characters of the reference the last click on a cell wrote into the formula
    /// (while pointing: the ones the reference being pointed at replaces).
    pub point: Option<(usize, usize)>,
    /// A click or drag on cells is writing a reference (point mode); where it started.
    pub pointing: Option<CellPos>,
    /// Where the text cursor goes when the editor takes the focus (default: the end).
    pub cursor: Option<usize>,
}

impl CellEdit {
    pub fn new(pos: CellPos, original: String, text: String) -> Self {
        CellEdit {
            pos,
            original,
            text,
            focus: true,
            in_bar: false,
            point: None,
            pointing: None,
            cursor: None,
        }
    }

    /// The text field typing happens in.
    pub fn editor_id(&self) -> egui::Id {
        if self.in_bar { bar_id() } else { edit_id() }
    }

    /// Where the reference of a clicked cell goes, if the cursor sits where one may.
    fn point_slot(&self, ctx: &egui::Context) -> Option<(usize, usize)> {
        let n = self.text.chars().count();
        let sel = egui::text_edit::TextEditState::load(ctx, self.editor_id())
            .and_then(|s| s.cursor.char_range())
            .map_or((n, n), |r| {
                let r = r.as_sorted_char_range();
                (r.start.0.min(n), r.end.0.min(n))
            });
        ref_slot(&self.text, sel, self.point)
    }

    /// Writes the reference to `a`..`b` over the slot and puts the cursor behind it.
    fn write_ref(&mut self, ctx: &egui::Context, a: CellPos, b: CellPos) {
        let Some((start, end)) = self.point else {
            return;
        };
        let end = splice(&mut self.text, start, end, &ref_text(a, b));
        self.point = Some((start, end));
        let id = self.editor_id();
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        let cursor = egui::text::CCursor::new(end);
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
        state.store(ctx, id);
    }
}

/// Colours of the references in a formula, in turn; readable on light and dark cells.
pub fn ref_color(i: usize, dark: bool) -> Color32 {
    const LIGHT: [[u8; 3]; 6] = [
        [0, 90, 200],
        [200, 30, 30],
        [120, 60, 180],
        [0, 130, 60],
        [200, 90, 0],
        [190, 0, 140],
    ];
    const DARK: [[u8; 3]; 6] = [
        [110, 170, 255],
        [255, 110, 100],
        [190, 140, 255],
        [100, 210, 120],
        [255, 170, 80],
        [255, 110, 210],
    ];
    let [r, g, b] = if dark { DARK } else { LIGHT }[i % 6];
    Color32::from_rgb(r, g, b)
}

/// An editor's text with each reference of a formula in its colour.
pub fn formula_job(text: &str, font: FontId, color: Color32, dark: bool) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let format = |color| egui::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    let refs = if text.starts_with('=') {
        find_refs(text)
    } else {
        Vec::new()
    };
    let byte: Vec<usize> = text
        .char_indices()
        .map(|(b, _)| b)
        .chain([text.len()])
        .collect();
    let mut at = 0;
    for r in refs {
        if r.start > at {
            job.append(&text[byte[at]..byte[r.start]], 0.0, format(color));
        }
        let color = ref_color(r.color, dark);
        job.append(&text[byte[r.start]..byte[r.end]], 0.0, format(color));
        at = r.end;
    }
    job.append(&text[byte[at]..], 0.0, format(color));
    job
}

/// Text field layouter that colours the references of a formula.
pub fn formula_layouter(
    font: FontId,
) -> impl FnMut(&egui::Ui, &dyn egui::TextBuffer, f32) -> std::sync::Arc<egui::Galley> {
    move |ui, buf, wrap| {
        let v = ui.visuals();
        let mut job = formula_job(buf.as_str(), font.clone(), v.text_color(), v.dark_mode);
        job.wrap.max_width = wrap;
        ui.fonts_mut(|f| f.layout_job(job))
    }
}

/// Id of the in-cell editor, so the app can take its focus away when it closes.
pub fn edit_id() -> egui::Id {
    egui::Id::unique("ferrosheet_cell_edit")
}

/// Id of the formula bar.
pub fn bar_id() -> egui::Id {
    egui::Id::unique("ferrosheet_formula_bar")
}

/// What happened in the grid this frame that the app has to act on.
#[derive(Default)]
pub struct GridOut {
    pub double_clicked: bool,
    /// The editor lost focus without Enter, Tab or Esc (a click elsewhere).
    pub edit_left: bool,
    /// The cell area and the header strips, for their right-click menus.
    pub body: Option<Response>,
    pub col_hdr: Option<Response>,
    pub row_hdr: Option<Response>,
    /// The fill handle was let go: continue the selection (the source) as far as this.
    pub fill: Option<(Selection, Fill)>,
}

/// Header edge being dragged.
#[derive(Clone, Copy)]
enum Resize {
    Col(u32),
    Row(u32),
}

impl GridState {
    /// Centre of a cell on screen, as of the last frame (for UI tests).
    #[cfg(test)]
    pub fn cell_center(&self, cell: CellPos) -> Pos2 {
        let x: f32 = self.col_widths[..cell.col as usize].iter().sum();
        let w = self.col_widths[cell.col as usize];
        let rh = &self.row_heights;
        self.origin + vec2(x + w / 2.0, rh.top(cell.row) + rh.height(cell.row) / 2.0)
    }

    /// The fill handle on screen (bottom-right corner of the selection), as of the last
    /// frame (for UI tests).
    #[cfg(test)]
    pub fn fill_handle(&self) -> Pos2 {
        let (_, br) = self.selection.bounds();
        let x: f32 = self.col_widths[..=br.col as usize].iter().sum();
        let rh = &self.row_heights;
        self.origin + vec2(x, rh.top(br.row) + rh.height(br.row))
    }

    pub fn visible_rows(&self) -> i64 {
        ((self.view.y / self.row_heights.base()) as i64 - 1).max(1)
    }

    /// Rows or columns were inserted before `at`: sizes move with their lines. `restore`
    /// gives the sizes of the new lines when a deletion is undone, otherwise they get
    /// the default size.
    pub fn insert_lines(&mut self, lines: Lines, at: u32, count: u32, restore: Option<Removed>) {
        let z = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        let restore = restore.unwrap_or_default();
        match lines {
            Lines::Cols => {
                let at = at as usize;
                if at <= self.col_widths.len() {
                    let widths =
                        (0..count as usize).map(|i| restore.cols.get(i).unwrap_or(&DEFAULT_W) * z);
                    self.col_widths.splice(at..at, widths);
                }
            }
            Lines::Rows => {
                let heights: Vec<_> = restore.rows.iter().map(|&(r, h)| (r, h * z)).collect();
                self.row_heights.insert(at, count, &heights);
            }
        }
    }

    /// Rows or columns from `at` were deleted: the sizes after them move up or left.
    /// Returns the deleted sizes for undo.
    pub fn delete_lines(&mut self, lines: Lines, at: u32, count: u32) -> Removed {
        let z = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        match lines {
            Lines::Cols => {
                let len = self.col_widths.len();
                let (from, to) = (
                    (at as usize).min(len),
                    (at as usize + count as usize).min(len),
                );
                Removed {
                    cols: self.col_widths.drain(from..to).map(|w| w / z).collect(),
                    rows: Vec::new(),
                }
            }
            Lines::Rows => Removed {
                cols: Vec::new(),
                rows: (self.row_heights.delete(at, count).into_iter())
                    .map(|(r, h)| (r, h / z))
                    .collect(),
            },
        }
    }

    /// Stores widths and heights for `zoom` (1.0 = 100 %).
    fn apply_zoom(&mut self, zoom: f32) {
        let old = if self.zoom > 0.0 { self.zoom } else { 1.0 };
        if (old - zoom).abs() > f32::EPSILON {
            let f = zoom / old;
            self.col_widths.iter_mut().for_each(|w| *w *= f);
            self.row_heights.rescale(f);
            self.offset *= f;
            self.scroll_to = self.scroll_to.or(Some(self.selection.cursor));
        }
        self.zoom = zoom;
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
    zoom: f32,
) -> GridOut {
    let mut grid_out = GridOut::default();
    st.apply_zoom(zoom);
    let header_h = HEADER_H * zoom;
    let default_w = DEFAULT_W * zoom;
    let full = ui.available_rect_before_wrap();
    let row_hdr_w = (digits(sheet.rows.max(1000)) as f32 * 8.0 * zoom + 2.0 * PAD).max(40.0);
    let body = Rect::from_min_max(full.min + vec2(row_hdr_w, header_h), full.max);

    // Virtual grid fills the window even for small sheets, like a real spreadsheet,
    // and reaches as far as the selection (Go to may select beyond the used range).
    let (_, sel_end) = st.selection.bounds();
    let rows = sheet
        .rows
        .max((body.height() / st.row_heights.base()) as u32 + 1)
        .max(sel_end.row.saturating_add(1));
    let cols = sheet
        .cols
        .max((body.width() / default_w) as u32 + 1)
        .max(sel_end.col.saturating_add(1));
    if st.col_widths.len() < cols as usize {
        st.col_widths.resize(cols as usize, default_w);
    }
    let font = FontId::proportional(FONT_SIZE * zoom);
    match st.fit.take() {
        Some(Fit::Cols(a, b)) => {
            for c in a..=b.min(cols - 1) {
                st.col_widths[c as usize] = autofit(ui, sheet, c, &font);
            }
        }
        Some(Fit::Rows(a, b)) => {
            for r in a..=b.min(rows - 1) {
                let h = autofit_row(ui, sheet, r, &font, st.row_heights.base());
                st.row_heights.set(r, h);
            }
        }
        None => {}
    }
    let xs = prefix(&st.col_widths[..cols as usize]);
    let rh = &st.row_heights;
    let total = vec2(xs[cols as usize], rh.top(rows));

    let visuals = ui.visuals().clone();
    let pal = Palette::new(&visuals);
    let sel_fill = visuals.selection.bg_fill.gamma_multiply(0.35);
    let sel_stroke = Stroke::new(2.0, visuals.selection.bg_fill);
    let hit_fill = Color32::from_rgba_unmultiplied(255, 220, 0, 70);
    let current_fill = Color32::from_rgba_unmultiplied(255, 140, 0, 140);

    // --- body ---------------------------------------------------------------
    // Frozen rows and columns, unless they would leave almost nothing to scroll.
    let frozen = {
        let (fr, fc) = (st.frozen.0.min(rows), st.frozen.1.min(cols));
        let fr = if rh.top(fr) > body.height() * 0.8 {
            0
        } else {
            fr
        };
        let fc = if xs[fc as usize] > body.width() * 0.8 {
            0
        } else {
            fc
        };
        (fr, fc)
    };
    let (fr, fc) = frozen;
    let (fh, fw) = (rh.top(fr), xs[fc as usize]);
    let mut area = ScrollArea::both().id_salt("grid").auto_shrink(false);
    if let Some(target) = st.scroll_to.take() {
        let off = offset_to_show(target, &xs, rh, st.offset, body.size(), frozen);
        area = area.scroll_offset(off);
    }
    let shift = ui.input(|i| i.modifiers.shift);
    let secondary = ui.input(|i| i.pointer.secondary_down() || i.pointer.secondary_released());
    let out = ui
        .scope_builder(UiBuilder::new().max_rect(body), |ui| {
            area.show_viewport(ui, |ui, vp| {
                let (rect, resp) = ui.allocate_exact_size(total, Sense::click_and_drag());
                let origin = rect.min;
                st.origin = origin;
                let painter = ui.painter_at(ui.clip_rect());
                // Frozen cells are drawn at the top and left of the view whatever the
                // scroll offset; everything here is in content coordinates + `origin`.
                let pane_shift = |r: u32, c: u32| {
                    vec2(
                        if c < fc { vp.min.x } else { 0.0 },
                        if r < fr { vp.min.y } else { 0.0 },
                    )
                };
                let cell_rect = |r: u32, c: u32| {
                    Rect::from_min_size(
                        origin + vec2(xs[c as usize], rh.top(r)) + pane_shift(r, c),
                        vec2(st.col_widths[c as usize], rh.height(r)),
                    )
                };
                let view = vp.translate(origin.to_vec2());
                let split = view.min + vec2(fw, fh);
                // The part of the view the pane holding cell (r, c) is drawn in.
                let pane_clip = |r: u32, c: u32| {
                    let (x0, x1) = if c < fc {
                        (view.min.x, split.x)
                    } else {
                        (split.x, view.max.x)
                    };
                    let (y0, y1) = if r < fr {
                        (view.min.y, split.y)
                    } else {
                        (split.y, view.max.y)
                    };
                    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
                };
                // The cell under a point on screen.
                let hit = |p: Pos2| {
                    let mut q = p - origin;
                    if p.x - view.min.x < fw {
                        q.x = p.x - view.min.x;
                    }
                    if p.y - view.min.y < fh {
                        q.y = p.y - view.min.y;
                    }
                    cell_at(q, &xs, rh, rows, cols)
                };
                let (r0, r1) = visible_range(vp.min.y + fh, vp.max.y, rows, rh);
                let r0 = r0.max(fr);
                let c0 = xs
                    .partition_point(|&x| x <= vp.min.x + fw)
                    .saturating_sub(1) as u32;
                let c0 = c0.max(fc);
                let c1 = (xs.partition_point(|&x| x < vp.max.x) as u32).min(cols);

                painter.rect_filled(view, 0.0, pal.cell_bg);
                let (tl, br) = st.selection.bounds();

                // One pane: fills, search hits, selection, grid lines and text of the
                // cells in rows ra..rb and columns ca..cb.
                let paint_pane = |ra: u32, rb: u32, ca: u32, cb: u32| {
                    if ra >= rb || ca >= cb {
                        return;
                    }
                    let painter = painter.with_clip_rect(pane_clip(ra, ca).intersect(view));

                    // merged ranges reaching into this pane; one that crosses the
                    // frozen edge is drawn as separate cells
                    let merges: Vec<(CellPos, CellPos)> = sheet
                        .merges()
                        .iter()
                        .filter(|(a, b)| {
                            a.row < rb
                                && b.row >= ra
                                && a.col < cb
                                && b.col >= ca
                                && b.row < rows
                                && b.col < cols
                                && (a.row < fr) == (b.row < fr)
                                && (a.col < fc) == (b.col < fc)
                        })
                        .copied()
                        .collect();
                    let in_merge = |r: u32, c: u32| {
                        merges.iter().any(|(a, b)| {
                            (a.row..=b.row).contains(&r) && (a.col..=b.col).contains(&c)
                        })
                    };

                    // fill colours from the file or set by the user
                    for r in ra..rb.min(sheet.rows) {
                        for c in ca..cb.min(sheet.cols) {
                            if in_merge(r, c) {
                                continue;
                            }
                            if let Some([red, g, b]) = sheet.style(r, c).fill {
                                let fill = Color32::from_rgb(red, g, b);
                                painter.rect_filled(cell_rect(r, c), 0.0, fill);
                            }
                        }
                    }

                    // search hits (sorted row-major, so the visible ones are contiguous)
                    let first = hits.partition_point(|p| p.row < ra);
                    for p in hits[first..].iter().take_while(|p| p.row < rb) {
                        if p.col >= ca && p.col < cb {
                            let fill = if Some(*p) == current_hit {
                                current_fill
                            } else {
                                hit_fill
                            };
                            painter.rect_filled(cell_rect(p.row, p.col), 0.0, fill);
                        }
                    }

                    // the part of the selection in this pane
                    let (sr0, sc0) = (tl.row.max(ra), tl.col.max(ca));
                    let (sr1, sc1) = (br.row.min(rb - 1), br.col.min(cb - 1));
                    if st.selection.cell_count() > 1 && sr0 <= sr1 && sc0 <= sc1 {
                        let part = cell_rect(sr0, sc0).union(cell_rect(sr1, sc1));
                        painter.rect_filled(part, 0.0, sel_fill);
                    }

                    // grid lines
                    let top = cell_rect(ra, ca).top();
                    let bottom = cell_rect(rb - 1, ca).bottom();
                    let left = cell_rect(ra, ca).left();
                    let right = cell_rect(ra, cb - 1).right();
                    for c in ca..=cb {
                        let x = left + xs[c as usize] - xs[ca as usize] - 0.5;
                        painter.line_segment([pos2(x, top), pos2(x, bottom)], pal.col_line);
                    }
                    for r in ra..=rb {
                        let y = top + rh.top(r) - rh.top(ra) - 0.5;
                        painter.line_segment([pos2(left, y), pos2(right, y)], pal.row_line);
                    }

                    // merged ranges: one cell in the fill of their top left cell, over
                    // the grid lines inside (the outer lines stay)
                    for (a, b) in &merges {
                        let rect = cell_rect(a.row, a.col).union(cell_rect(b.row, b.col));
                        let inner = Rect::from_min_max(rect.min, rect.max - vec2(1.0, 1.0));
                        let bg = match sheet.style(a.row, a.col).fill {
                            Some([red, g, b]) => Color32::from_rgb(red, g, b),
                            None => pal.cell_bg,
                        };
                        painter.rect_filled(inner, 0.0, bg);
                        let selected = st.selection.cell_count() > 1
                            && a.row <= br.row
                            && b.row >= tl.row
                            && a.col <= br.col
                            && b.col >= tl.col;
                        if selected {
                            painter.rect_filled(inner, 0.0, sel_fill);
                        }
                    }

                    // text
                    let draw_text = |r: u32, c: u32, rect: Rect| {
                        let text = sheet.display(r, c);
                        if text.is_empty() {
                            return;
                        }
                        let value = sheet.value(r, c);
                        let style = sheet.style(r, c);
                        let color = match style.color {
                            _ if matches!(value, CellValue::Error(_)) => visuals.error_fg_color,
                            Some([red, g, b]) => Color32::from_rgb(red, g, b),
                            // on a fill, text keeps the file's black even in dark mode
                            None if style.fill.is_some() => Color32::BLACK,
                            None => pal.text,
                        };
                        let font = match style.size {
                            Some(tenths) => FontId::new(
                                font.size * f32::from(tenths) / 110.0,
                                font.family.clone(),
                            ),
                            None => font.clone(),
                        };
                        // Default rows show the first line centred; taller rows show
                        // all lines from the top, clipped to the cell.
                        let tall = rect.height() > rh.base() + 1.0;
                        let h = match style.align {
                            Some(HAlign::Left) => egui::Align::Min,
                            Some(HAlign::Center) => egui::Align::Center,
                            Some(HAlign::Right) => egui::Align::Max,
                            None if value.is_numeric() => egui::Align::Max,
                            None => egui::Align::Min,
                        };
                        let x = match h {
                            egui::Align::Min => rect.left() + PAD,
                            egui::Align::Center => rect.center().x,
                            egui::Align::Max => rect.right() - PAD,
                        };
                        let (v, y) = if tall {
                            (egui::Align::Min, rect.top() + TEXT_TOP)
                        } else {
                            (egui::Align::Center, rect.center().y)
                        };
                        let line_text = if tall {
                            &*text
                        } else {
                            text.lines().next().unwrap_or("")
                        };
                        let clipped =
                            painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
                        let anchor = Align2([h, v]);
                        cell_text(&clipped, pos2(x, y), anchor, line_text, &font, color, style);
                    };
                    for r in ra..rb.min(sheet.rows) {
                        for c in ca..cb.min(sheet.cols) {
                            if !in_merge(r, c) {
                                draw_text(r, c, cell_rect(r, c));
                            }
                        }
                    }
                    for (a, b) in &merges {
                        let rect = cell_rect(a.row, a.col).union(cell_rect(b.row, b.col));
                        draw_text(a.row, a.col, rect);
                    }
                };
                paint_pane(r0, r1, c0, c1);
                paint_pane(0, fr, c0, c1);
                paint_pane(r0, r1, 0, fc);
                paint_pane(0, fr, 0, fc);
                // a stronger line where the frozen panes end
                if fr > 0 {
                    let y = split.y - 0.5;
                    let line = [pos2(view.min.x, y), pos2(view.max.x, y)];
                    painter.line_segment(line, pal.header_border);
                }
                if fc > 0 {
                    let x = split.x - 0.5;
                    let line = [pos2(x, view.min.y), pos2(x, view.max.y)];
                    painter.line_segment(line, pal.header_border);
                }
                let sel = cell_rect(tl.row.min(rows - 1), tl.col.min(cols - 1))
                    .union(cell_rect(br.row.min(rows - 1), br.col.min(cols - 1)));

                // cursor
                let cur = st.selection.cursor;
                if cur.row < rows && cur.col < cols {
                    let painter = painter.with_clip_rect(pane_clip(cur.row, cur.col));
                    // a merged cell is framed as a whole
                    let frame = match sheet.merged(cur.row, cur.col) {
                        Some((a, b))
                            if b.row < rows
                                && b.col < cols
                                && (a.row < fr) == (b.row < fr)
                                && (a.col < fc) == (b.col < fc) =>
                        {
                            cell_rect(a.row, a.col).union(cell_rect(b.row, b.col))
                        }
                        _ => cell_rect(cur.row, cur.col),
                    };
                    painter.rect_stroke(frame, 0.0, sel_stroke, egui::StrokeKind::Inside);
                }

                // fill handle, and the frame of where a drag on it extends the selection
                let handle = Rect::from_center_size(sel.right_bottom(), Vec2::splat(HANDLE * zoom));
                if st.edit.is_none() {
                    if let Some((src, Some(fill))) = &st.filling {
                        let (a, b) = fill.covering(src).bounds();
                        let frame = cell_rect(a.row.min(rows - 1), a.col.min(cols - 1))
                            .union(cell_rect(b.row.min(rows - 1), b.col.min(cols - 1)));
                        let stroke = Stroke::new(1.5, visuals.selection.bg_fill);
                        dashed_rect(&painter, frame, stroke);
                    }
                    painter.rect_filled(handle, 0.0, visuals.selection.bg_fill);
                    painter.rect_stroke(
                        handle,
                        0.0,
                        Stroke::new(1.0, pal.cell_bg),
                        egui::StrokeKind::Outside,
                    );
                }

                // references of the formula being typed, framed in their colours
                if let Some(e) = &st.edit
                    && e.text.starts_with('=')
                {
                    let here = sheet.name.to_lowercase();
                    for r in find_refs(&e.text) {
                        let other = r.sheet.is_some_and(|s| s.to_lowercase() != here);
                        if other || r.first.row >= rows || r.first.col >= cols {
                            continue;
                        }
                        let rect = cell_rect(r.first.row, r.first.col).union(cell_rect(
                            r.last.row.min(rows - 1),
                            r.last.col.min(cols - 1),
                        ));
                        let color = ref_color(r.color, visuals.dark_mode);
                        painter.rect_filled(rect, 0.0, color.gamma_multiply(0.12));
                        painter.rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(2.0, color),
                            egui::StrokeKind::Inside,
                        );
                    }
                }

                // in-cell editor
                if let Some(e) = &mut st.edit
                    && e.pos.row < rows
                    && e.pos.col < cols
                {
                    let rect = cell_rect(e.pos.row, e.pos.col);
                    if e.in_bar {
                        // Mirror what is typed in the formula bar.
                        painter.rect_filled(rect, 0.0, pal.cell_bg);
                        let clipped =
                            painter.with_clip_rect(rect.shrink(1.0).intersect(painter.clip_rect()));
                        let job = formula_job(&e.text, font.clone(), pal.text, visuals.dark_mode);
                        let galley = clipped.layout_job(job);
                        let at = Align2::LEFT_CENTER
                            .anchor_size(rect.left_center() + vec2(PAD, 0.0), galley.size())
                            .min;
                        clipped.galley(at, galley, pal.text);
                    }
                    // A strong frame marks the cell being edited.
                    let frame = Stroke::new(2.5, visuals.selection.stroke.color);
                    painter.rect_stroke(rect.expand(1.0), 0.0, frame, egui::StrokeKind::Outside);
                }
                if let Some(e) = &mut st.edit
                    && !e.in_bar
                    && e.pos.row < rows
                    && e.pos.col < cols
                {
                    let rect = cell_rect(e.pos.row, e.pos.col);
                    let mut layouter = formula_layouter(font.clone());
                    let edit = ui.put(
                        rect,
                        egui::TextEdit::singleline(&mut e.text)
                            .id(edit_id())
                            .font(font.clone())
                            .margin(vec2(PAD - 1.0, 2.0))
                            .desired_width(rect.width())
                            .layouter(&mut layouter),
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
                        let at = e.cursor.take().unwrap_or(e.text.chars().count());
                        let end = egui::text::CCursor::new(at);
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::one(end)));
                        state.store(ui.ctx(), edit_id());
                    } else if edit.lost_focus() && e.pointing.is_none() {
                        grid_out.edit_left = true;
                    }
                }
                if resp.double_clicked() {
                    grid_out.double_clicked = true;
                }

                // Right-click outside the selection selects the clicked cell first.
                if resp.secondary_clicked()
                    && let Some(p) = resp.interact_pointer_pos()
                {
                    let cell = hit(p);
                    if !st.selection.contains(cell) {
                        st.selection = Selection::single(cell);
                    }
                }

                // Fill handle: a drag from it continues the selection (series or copies).
                let grab = handle.expand(2.0);
                let on_handle = |p: Pos2| grab.contains(p);
                if st.edit.is_none()
                    && st.filling.is_none()
                    && resp.hovered()
                    && ui.input(|i| i.pointer.hover_pos()).is_some_and(on_handle)
                {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                }
                if st.edit.is_none()
                    && !secondary
                    && st.filling.is_none()
                    && ui.input(|i| i.pointer.primary_pressed())
                    && resp.is_pointer_button_down_on()
                    && ui
                        .input(|i| i.pointer.press_origin())
                        .is_some_and(on_handle)
                {
                    st.filling = Some((st.selection, None));
                }
                let filling = st.filling.is_some();
                if let Some((src, target)) = &mut st.filling {
                    ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
                    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                        let cell = hit(p);
                        *target = Fill::target(src, cell);
                        st.scroll_to = Some(cell);
                    }
                    if !ui.input(|i| i.pointer.primary_down()) {
                        grid_out.fill = target.map(|f| (*src, f));
                        st.filling = None;
                    }
                }

                // Point mode: while a formula is typed and the cursor sits where a
                // reference may go, a click or drag on cells writes their reference
                // into it instead of selecting them. The editor keeps the focus.
                let mut pointed = false;
                if !secondary && let Some(e) = &mut st.edit {
                    let ctx = ui.ctx().clone();
                    if e.pointing.is_none()
                        && !shift
                        && ui.input(|i| i.pointer.primary_pressed())
                        && resp.is_pointer_button_down_on()
                        && let Some(slot) = e.point_slot(&ctx)
                        && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                    {
                        e.point = Some(slot);
                        e.pointing = Some(hit(p));
                    }
                    if let Some(anchor) = e.pointing {
                        pointed = true;
                        if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                            let cell = hit(p);
                            e.write_ref(&ctx, anchor, cell);
                        }
                        if !ui.input(|i| i.pointer.primary_down()) {
                            e.pointing = None;
                            ctx.memory_mut(|m| m.request_focus(e.editor_id()));
                        }
                    }
                }

                // mouse selection
                if !pointed
                    && !filling
                    && !secondary
                    && resp.is_pointer_button_down_on()
                    && let Some(p) = ui.input(|i| i.pointer.interact_pos())
                {
                    let cell = hit(p);
                    if ui.input(|i| i.pointer.primary_pressed()) && !shift {
                        st.selection = Selection::single(cell);
                    } else {
                        st.selection.cursor = cell;
                    }
                    st.scroll_to = Some(cell);
                }
                resp
            })
        })
        .inner;
    grid_out.body = Some(out.inner);
    st.offset = out.state.clamped_offset();
    st.view = body.size();

    // --- headers ------------------------------------------------------------
    let col_hdr = Rect::from_min_max(pos2(body.min.x, full.min.y), pos2(full.max.x, body.min.y));
    let row_hdr = Rect::from_min_max(pos2(full.min.x, body.min.y), pos2(body.min.x, full.max.y));
    let corner = Rect::from_min_max(full.min, body.min);
    let (tl, br) = st.selection.bounds();

    // Header positions follow the panes: frozen lines do not scroll.
    let off = st.offset;
    let col_x = |c: u32| xs[c as usize] - if c < fc { 0.0 } else { off.x };
    let content_x = |x: f32| if x < fw { x } else { x + off.x };
    let content_y = |y: f32| if y < fh { y } else { y + off.y };

    let painter = ui.painter_at(col_hdr);
    painter.rect_filled(col_hdr, 0.0, pal.header_bg);
    let (c0, c1) = (
        (xs.partition_point(|&x| x <= off.x + fw).saturating_sub(1) as u32).max(fc),
        (xs.partition_point(|&x| x < off.x + body.width()) as u32).min(cols),
    );
    let scrolled = painter.with_clip_rect(col_hdr.with_min_x(col_hdr.min.x + fw));
    for (painter, cs) in [(&scrolled, c0..c1), (&painter, 0..fc)] {
        for c in cs {
            let x0 = col_hdr.min.x + col_x(c);
            let rect = Rect::from_min_size(
                pos2(x0, col_hdr.min.y),
                vec2(st.col_widths[c as usize], header_h),
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
    }

    let painter = ui.painter_at(row_hdr);
    painter.rect_filled(row_hdr, 0.0, pal.header_bg);
    let rh = &st.row_heights;
    let (r0, r1) = visible_range(off.y + fh, off.y + body.height(), rows, rh);
    let scrolled = painter.with_clip_rect(row_hdr.with_min_y(row_hdr.min.y + fh));
    for (painter, rs) in [(&scrolled, r0.max(fr)..r1), (&painter, 0..fr)] {
        for r in rs {
            let y0 = row_hdr.min.y + rh.top(r) - if r < fr { 0.0 } else { off.y };
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
        let cx = content_x(x - col_hdr.min.x);
        (1..=cols)
            .find(|&c| (xs[c as usize] - cx).abs() <= RESIZE_GRAB)
            .map(|c| c - 1)
    };
    let row_edge = |y: f32| -> Option<u32> {
        let rh = &st.row_heights;
        let cy = content_y(y - row_hdr.min.y);
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
        let h = autofit_row(ui, sheet, r, &font, st.row_heights.base());
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
    // Right-click on a header outside the selection selects that column or row.
    if col_resp.secondary_clicked()
        && let Some(p) = col_resp.interact_pointer_pos()
    {
        let x = content_x(p.x - col_hdr.min.x);
        let c = (xs.partition_point(|&e| e <= x).saturating_sub(1) as u32).min(cols - 1);
        if !(tl.col..=br.col).contains(&c) || tl.row != 0 || br.row < last_row {
            st.selection = Selection {
                anchor: CellPos::new(0, c),
                cursor: CellPos::new(last_row, c),
            };
        }
    }
    if row_resp.secondary_clicked()
        && let Some(p) = row_resp.interact_pointer_pos()
    {
        let r = st
            .row_heights
            .row_at(content_y(p.y - row_hdr.min.y))
            .min(rows - 1);
        if !(tl.row..=br.row).contains(&r) || tl.col != 0 || br.col < last_col {
            st.selection = Selection {
                anchor: CellPos::new(r, 0),
                cursor: CellPos::new(r, last_col),
            };
        }
    }
    // A press on an edge resizes; anywhere else on a header it selects.
    if secondary {
        // handled above
    } else if st.resizing.is_none() && col_press_edge.is_none() {
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
            vec2(fw, fh),
        );
    }
    if !secondary && st.resizing.is_none() && row_press_edge.is_none() {
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
            vec2(fw, fh),
        );
    }

    ui.allocate_rect(full, Sense::hover());
    grid_out.col_hdr = Some(col_resp);
    grid_out.row_hdr = Some(row_resp);
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

/// A rectangle outlined with dashes, like the frame Excel shows while filling.
fn dashed_rect(painter: &Painter, rect: Rect, stroke: Stroke) {
    let (dash, gap) = (4.0, 3.0);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    for w in corners.windows(2) {
        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], stroke, dash, gap));
    }
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
    pane: Vec2,
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
            let x = p.x - strip.min.x;
            let x = if x < pane.x { x } else { x + st.offset.x };
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
                .row_at({
                    let y = p.y - strip.min.y;
                    if y < pane.y { y } else { y + st.offset.y }
                })
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
fn autofit_row(ui: &egui::Ui, sheet: &Sheet, r: u32, font: &FontId, base: f32) -> f32 {
    if r >= sheet.rows {
        return base;
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
    (tallest + 2.0 * TEXT_TOP).clamp(base, MAX_AUTOFIT_H)
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
/// Frozen rows and columns never need scrolling; the others must clear the panes.
fn offset_to_show(
    target: CellPos,
    xs: &[f32],
    rh: &RowHeights,
    offset: Vec2,
    view: Vec2,
    (fr, fc): (u32, u32),
) -> Vec2 {
    let ci = (target.col as usize).min(xs.len().saturating_sub(2));
    let (fh, fw) = (rh.top(fr), xs[(fc as usize).min(xs.len() - 1)]);
    let (x0, x1) = (xs[ci] - fw, xs[ci + 1] - fw);
    let y0 = rh.top(target.row) - fh;
    let y1 = y0 + rh.height(target.row);
    let view = view - vec2(fw, fh);
    let fit = |lo: f32, hi: f32, off: f32, size: f32| {
        if lo < off {
            lo
        } else if hi > off + size {
            hi - size
        } else {
            off
        }
    };
    vec2(
        if target.col < fc {
            offset.x
        } else {
            fit(x0, x1, offset.x, view.x)
        },
        if target.row < fr {
            offset.y
        } else {
            fit(y0, y1, offset.y, view.y)
        },
    )
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
                    show(ui, &sheet, st, &[], None, 1.0);
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

    fn right_click(h: &mut Harness<'static, (GridState, egui::Pos2)>, at: egui::Pos2) {
        h.hover_at(at);
        h.run();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run();
    }

    /// Centre of cell (r, c) at 100 % with the 40 px row header.
    fn cell_center(o: egui::Pos2, r: u32, c: u32) -> egui::Pos2 {
        o + vec2(
            40.0 + c as f32 * DEFAULT_W + DEFAULT_W / 2.0,
            HEADER_H + r as f32 * ROW_H + ROW_H / 2.0,
        )
    }

    fn click(h: &mut Harness<'static, (GridState, egui::Pos2)>, at: egui::Pos2) {
        h.hover_at(at);
        h.run();
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
        }
        h.run();
    }

    #[test]
    fn frozen_rows_and_columns_stay_in_view_while_scrolling() {
        let cells = (0..200 * 30).map(|i| CellValue::Number(i as f64)).collect();
        let sheet = Sheet::from_cells("big".into(), 200, 30, cells, Locale::Invariant);
        let mut h = harness_for(sheet);
        h.run();
        let o = h.state().1;
        h.state_mut().0.frozen = (1, 1);
        h.state_mut().0.scroll_to = Some(CellPos::new(150, 20));
        h.run();
        h.run();
        let st = &h.state().0;
        assert!(st.offset.y > 100.0 * ROW_H && st.offset.x > 10.0 * DEFAULT_W);

        // the corner cell is still A1
        click(&mut h, cell_center(o, 0, 0));
        assert_eq!(h.state().0.selection.cursor, CellPos::new(0, 0));
        // the top row keeps row 1 but its columns scroll
        click(&mut h, cell_center(o, 0, 3));
        let cur = h.state().0.selection.cursor;
        assert!(cur.row == 0 && cur.col > 10, "{cur:?}");
        // the first column keeps column A but its rows scroll
        click(&mut h, cell_center(o, 3, 0));
        let cur = h.state().0.selection.cursor;
        assert!(cur.col == 0 && cur.row > 100, "{cur:?}");
        // a row header click in the scrolled part selects a scrolled row
        let row_hdr = o + vec2(20.0, HEADER_H + 3.5 * ROW_H);
        click(&mut h, row_hdr);
        assert_eq!(h.state().0.selection.anchor.row, cur.row);
    }

    #[test]
    fn scrolling_to_a_cell_keeps_it_clear_of_the_frozen_panes() {
        let frozen = (2, 1);
        let xs: Vec<f32> = (0..=30).map(|c| c as f32 * DEFAULT_W).collect();
        let rh = RowHeights::default();
        let view = vec2(500.0, 300.0);
        // a cell above the scrolled part: brought down below the two frozen rows
        let off = offset_to_show(CellPos::new(5, 3), &xs, &rh, vec2(0.0, 400.0), view, frozen);
        assert_eq!(off.y, 3.0 * ROW_H);
        // a frozen row never scrolls
        let off = offset_to_show(CellPos::new(1, 3), &xs, &rh, vec2(0.0, 400.0), view, frozen);
        assert_eq!(off.y, 400.0);
        // a column left of the scrolled part comes in right of the frozen column
        let off = offset_to_show(CellPos::new(5, 4), &xs, &rh, vec2(900.0, 0.0), view, frozen);
        assert_eq!(off.x, 3.0 * DEFAULT_W);
    }

    #[test]
    fn right_click_outside_the_selection_selects_the_cell_inside_keeps_it() {
        let mut h = harness();
        h.run();
        let o = h.state().1;
        right_click(&mut h, cell_center(o, 2, 1));
        assert_eq!(h.state().0.selection, Selection::single(CellPos::new(2, 1)));

        let range = Selection {
            anchor: CellPos::new(0, 0),
            cursor: CellPos::new(3, 3),
        };
        h.state_mut().0.selection = range;
        right_click(&mut h, cell_center(o, 1, 2));
        assert_eq!(h.state().0.selection, range);
    }

    #[test]
    fn right_click_on_a_column_header_selects_the_column() {
        let mut h = harness();
        h.run();
        let o = h.state().1;
        right_click(&mut h, o + vec2(40.0 + DEFAULT_W * 2.5, HEADER_H / 2.0));
        let (tl, br) = h.state().0.selection.bounds();
        assert_eq!((tl, br), (CellPos::new(0, 2), CellPos::new(4, 2)));
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
