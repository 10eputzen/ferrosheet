//! Cell formats from `.xlsx`/`.xls` files, turned into [`CellStyle`]s.

use std::collections::HashMap;

use calamine::{FillPattern, FontStyle, HorizontalAlignment, Style, StyleRange};

use crate::CellPos;
use crate::sheet::FileStyles;
use crate::{CellStyle, HAlign};

/// What ferrosheet shows of a file's cell format.
pub(crate) fn convert(style: &Style) -> CellStyle {
    let rgb = |c: &calamine::Color| [c.red, c.green, c.blue];
    let mut out = CellStyle::default();
    if let Some(font) = &style.font {
        out.bold = font.is_bold();
        out.italic = font.style == FontStyle::Italic;
        // black text is what the theme shows anyway (as for edited cells)
        out.color = font.color.as_ref().map(rgb).filter(|&c| c != [0, 0, 0]);
        out.size = font
            .size
            .filter(|s| s.is_finite() && *s > 0.0)
            .map(|s| (s * 10.0).round().min(4090.0) as u16)
            .filter(|&s| s != 110);
    }
    if let Some(fill) = &style.fill
        && fill.pattern == FillPattern::Solid
    {
        // patterns other than solid are left out rather than shown as a wrong colour
        out.fill = fill
            .foreground_color
            .as_ref()
            .map(rgb)
            .filter(|&c| c != [255, 255, 255]);
    }
    if let Some(a) = &style.alignment {
        out.align = match a.horizontal {
            HorizontalAlignment::Left => Some(HAlign::Left),
            HorizontalAlignment::Center | HorizontalAlignment::CenterContinuous => {
                Some(HAlign::Center)
            }
            HorizontalAlignment::Right => Some(HAlign::Right),
            _ => None,
        };
    }
    out
}

/// Collects distinct styles; id 0 is the default style.
pub(crate) struct Palette {
    ids: HashMap<CellStyle, u16>,
    styles: Vec<CellStyle>,
}

impl Palette {
    pub fn new() -> Palette {
        Palette {
            ids: HashMap::from([(CellStyle::default(), 0)]),
            styles: vec![CellStyle::default()],
        }
    }

    pub fn id(&mut self, style: CellStyle) -> u16 {
        if let Some(&id) = self.ids.get(&style) {
            return id;
        }
        let Ok(id) = u16::try_from(self.styles.len()) else {
            return 0; // more distinct looks than anyone can tell apart
        };
        self.styles.push(style);
        self.ids.insert(style, id);
        id
    }

    pub fn styles(&self) -> &[CellStyle] {
        &self.styles
    }

    /// Palette ids for a workbook's style records, in their order.
    pub fn ids_for(&mut self, records: &[Style]) -> Vec<u16> {
        records.iter().map(|s| self.id(convert(s))).collect()
    }

    pub fn finish(self, ids: Vec<u16>) -> FileStyles {
        FileStyles {
            palette: self.styles,
            ids,
        }
    }
}

/// Styles of a `rows` × `cols` sheet from calamine's per-cell styles.
pub(crate) fn from_range(range: &StyleRange, rows: u32, cols: u32) -> FileStyles {
    let mut palette = Palette::new();
    let mut ids = Vec::new();
    // cells with the same style share one record, so convert each record once
    let mut seen: HashMap<*const Style, u16> = HashMap::new();
    for (r, c, style) in range.cells() {
        if r >= rows || c >= cols {
            continue; // styled cells outside the values (e.g. formatted empty columns)
        }
        let id = *seen
            .entry(style as *const Style)
            .or_insert_with(|| palette.id(convert(style)));
        if id != 0 {
            if ids.is_empty() {
                ids = vec![0; rows as usize * cols as usize];
            }
            ids[r as usize * cols as usize + c as usize] = id;
        }
    }
    palette.finish(ids)
}

/// Merged ranges as (top left, bottom right).
pub(crate) fn merges(dims: Vec<calamine::Dimensions>) -> Vec<(CellPos, CellPos)> {
    dims.into_iter()
        .filter(|d| d.start != d.end)
        .map(|d| {
            (
                CellPos::new(d.start.0, d.start.1),
                CellPos::new(d.end.0, d.end.1),
            )
        })
        .collect()
}

/// Merged ranges of sheet `idx` of an `.xlsx`, found by scanning the sheet part for
/// `mergeCell` tags. Much cheaper than parsing the sheet's XML a second time, which
/// matters for big sheets. Empty when the part cannot be read.
pub(crate) fn xlsx_merges(path: &std::path::Path, idx: usize) -> Vec<(CellPos, CellPos)> {
    use std::io::Read;
    let Some((mut zip, parts)) = crate::frozen::sheet_parts(path) else {
        return Vec::new();
    };
    let Some(Some(part)) = parts.get(idx) else {
        return Vec::new();
    };
    let Ok(mut entry) = zip.by_name(part) else {
        return Vec::new();
    };
    let tag = regex::bytes::Regex::new(r"<(?:\w+:)?mergeCell\s[^>]*>").expect("valid regex");
    let mut out = Vec::new();
    let mut buf: Vec<u8> = Vec::with_capacity(1 << 20);
    let mut chunk = vec![0u8; 1 << 20];
    loop {
        let n = match entry.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        buf.extend_from_slice(&chunk[..n]);
        let mut done = 0;
        for m in tag.find_iter(&buf) {
            let range = std::str::from_utf8(m.as_bytes())
                .ok()
                .and_then(|t| crate::frozen::attr(t, "ref"))
                .and_then(|r| crate::parse_reference(&r));
            if let Some((None, a, b)) = range
                && a != b
            {
                out.push((a, b));
            }
            done = m.end();
        }
        // keep only an unfinished tag at the end for the next chunk
        let keep_from = match buf[done..].iter().rposition(|&b| b == b'<') {
            Some(p) if buf.len() - (done + p) < 4096 => done + p,
            _ => buf.len(),
        };
        buf.drain(..keep_from);
    }
    out
}
