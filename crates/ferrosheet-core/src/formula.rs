//! Cell references in a formula being typed: where they are (to colour them and frame
//! the cells they point at) and where a clicked cell may be inserted ("point mode").
//! All positions count characters, the way text editors place their cursor.

use crate::selection::CellPos;
use crate::workbook::column_name;

const MAX_COL: u32 = 16_384;
const MAX_ROW: u32 = 1_048_576;

/// A reference such as `A1`, `$B$2:C10` or `'Other sheet'!A1` in a formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormulaRef {
    /// Character range of the reference in the formula, sheet name included.
    pub start: usize,
    pub end: usize,
    /// The sheet named in front of it, if any.
    pub sheet: Option<String>,
    /// Top-left and bottom-right cell (the same for a single cell).
    pub first: CellPos,
    pub last: CellPos,
    /// Colour slot: references to the same cells share one, new ones count up.
    pub color: usize,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.' || c == '$'
}

/// `$A$1`-style cell at `i`; returns the cell and where it ends.
fn cell_at(cs: &[char], mut i: usize) -> Option<(CellPos, usize)> {
    if cs.get(i) == Some(&'$') {
        i += 1;
    }
    let mut col = 0u32;
    let start = i;
    while let Some(c) = cs.get(i).filter(|c| c.is_ascii_alphabetic()) {
        // saturating: a long word is no column, but must not overflow
        col = col
            .saturating_mul(26)
            .saturating_add(c.to_ascii_uppercase() as u32 - 'A' as u32 + 1);
        i += 1;
    }
    if i == start || i - start > 3 || col > MAX_COL {
        return None;
    }
    if cs.get(i) == Some(&'$') {
        i += 1;
    }
    let mut row = 0u32;
    let start = i;
    while let Some(d) = cs.get(i).and_then(|c| c.to_digit(10)) {
        row = row.saturating_mul(10).saturating_add(d);
        i += 1;
    }
    if i == start || row == 0 || row > MAX_ROW {
        return None;
    }
    Some((CellPos::new(row - 1, col - 1), i))
}

/// A cell or range at `i`, ending where no name or call could go on.
fn range_at(cs: &[char], i: usize) -> Option<(CellPos, CellPos, usize)> {
    let (a, mut end) = cell_at(cs, i)?;
    let mut b = a;
    if cs.get(end) == Some(&':')
        && let Some((c, e)) = cell_at(cs, end + 1)
    {
        b = c;
        end = e;
    }
    match cs.get(end) {
        Some(&c) if is_word(c) || c == '(' || c == '!' => None,
        _ => Some((
            CellPos::new(a.row.min(b.row), a.col.min(b.col)),
            CellPos::new(a.row.max(b.row), a.col.max(b.col)),
            end,
        )),
    }
}

/// All cell references in `formula`, in order. Text in double quotes is skipped.
pub fn find_refs(formula: &str) -> Vec<FormulaRef> {
    let cs: Vec<char> = formula.chars().collect();
    let mut refs: Vec<FormulaRef> = Vec::new();
    let mut seen: Vec<(Option<String>, CellPos, CellPos)> = Vec::new();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c == '"' {
            i += 1;
            while i < cs.len() && cs[i] != '"' {
                i += 1;
            }
            i += 1;
            continue;
        }
        if i > 0 && is_word(cs[i - 1]) {
            i += 1;
            continue;
        }
        // Sheet name: 'quoted, with ''escaped'' quotes' or a plain word, then '!'.
        let mut sheet = None;
        let mut at = i;
        if c == '\'' {
            let mut name = String::new();
            let mut j = i + 1;
            while j < cs.len() {
                if cs[j] == '\'' {
                    if cs.get(j + 1) == Some(&'\'') {
                        name.push('\'');
                        j += 2;
                        continue;
                    }
                    break;
                }
                name.push(cs[j]);
                j += 1;
            }
            if cs.get(j + 1) == Some(&'!') {
                sheet = Some(name);
                at = j + 2;
            } else {
                i = j + 1;
                continue;
            }
        } else if is_word(c) {
            let mut j = i;
            while j < cs.len() && is_word(cs[j]) {
                j += 1;
            }
            if cs.get(j) == Some(&'!') {
                sheet = Some(cs[i..j].iter().collect());
                at = j + 1;
            }
        } else {
            i += 1;
            continue;
        }
        match range_at(&cs, at) {
            Some((first, last, end)) => {
                let key = (sheet.as_ref().map(|s| s.to_lowercase()), first, last);
                let color = match seen.iter().position(|k| *k == key) {
                    Some(n) => n,
                    None => {
                        seen.push(key);
                        seen.len() - 1
                    }
                };
                refs.push(FormulaRef {
                    start: i,
                    end,
                    sheet,
                    first,
                    last,
                    color,
                });
                i = end;
            }
            None => {
                // Skip the whole word, so `LOG10` is not read as a cell further in.
                i = at.max(i + 1);
                while i < cs.len() && is_word(cs[i]) {
                    i += 1;
                }
            }
        }
    }
    refs
}

/// Whether a reference may be inserted at character `pos`: the text is a formula, the
/// cursor is not inside quotes and follows `=`, an opening bracket, a separator or an
/// operator.
pub fn accepts_ref(formula: &str, pos: usize) -> bool {
    let cs: Vec<char> = formula.chars().collect();
    if cs.first() != Some(&'=') || pos == 0 || pos > cs.len() {
        return false;
    }
    if cs[..pos].iter().filter(|&&c| c == '"').count() % 2 == 1 {
        return false;
    }
    if cs.get(pos).is_some_and(|&c| is_word(c)) {
        return false;
    }
    let before = cs[..pos].iter().rev().find(|c| !c.is_whitespace());
    matches!(
        before,
        Some('=' | '(' | ',' | ';' | '+' | '-' | '*' | '/' | '^' | '&' | '<' | '>' | '%')
    )
}

/// Where a clicked cell goes: the characters (start, end) it replaces. `sel` is the
/// editor's selection, `last` the reference inserted by the previous click; while the
/// cursor still sits right behind it, the next click replaces it.
pub fn ref_slot(
    formula: &str,
    sel: (usize, usize),
    last: Option<(usize, usize)>,
) -> Option<(usize, usize)> {
    if let Some((s, e)) = last
        && sel == (e, e)
        && find_refs(formula)
            .iter()
            .any(|r| r.start == s && r.end == e)
    {
        return Some((s, e));
    }
    accepts_ref(formula, sel.0).then_some(sel)
}

/// A cell or range typed into the name box, such as `B200`, `a1:c10`, `$A$1` or
/// `'My sheet'!A1`: the sheet named in front (if any), top-left and bottom-right cell.
/// `None` when the text is anything else.
pub fn parse_reference(input: &str) -> Option<(Option<String>, CellPos, CellPos)> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    let formula = format!("={input}");
    let refs = find_refs(&formula);
    match refs.as_slice() {
        [r] if r.start == 1 && r.end == formula.chars().count() => {
            Some((r.sheet.clone(), r.first, r.last))
        }
        _ => None,
    }
}

/// `A1` for one cell, `A1:B5` for a range (any two corners).
pub fn ref_text(a: CellPos, b: CellPos) -> String {
    let (tl, br) = (
        CellPos::new(a.row.min(b.row), a.col.min(b.col)),
        CellPos::new(a.row.max(b.row), a.col.max(b.col)),
    );
    let one = |p: CellPos| format!("{}{}", column_name(p.col), p.row + 1);
    if tl == br {
        one(tl)
    } else {
        format!("{}:{}", one(tl), one(br))
    }
}

/// `text` with the characters `start..end` replaced by `with`; returns where the
/// replacement ends.
pub fn splice(text: &mut String, start: usize, end: usize, with: &str) -> usize {
    let byte = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(b, _)| b);
    let (bs, be) = (byte(start), byte(end));
    text.replace_range(bs..be, with);
    start + with.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(row: u32, col: u32) -> CellPos {
        CellPos::new(row, col)
    }

    fn spans(f: &str) -> Vec<(String, usize)> {
        let cs: Vec<char> = f.chars().collect();
        find_refs(f)
            .iter()
            .map(|r| (cs[r.start..r.end].iter().collect(), r.color))
            .collect()
    }

    #[test]
    fn finds_cells_and_ranges() {
        let r = find_refs("=A1+$B$2*SUM(C3:D10)");
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].first, r[0].last), (p(0, 0), p(0, 0)));
        assert_eq!((r[1].first, r[1].last), (p(1, 1), p(1, 1)));
        assert_eq!((r[2].first, r[2].last), (p(2, 2), p(9, 3)));
        assert_eq!((r[2].start, r[2].end), (13, 19));
    }

    #[test]
    fn reversed_range_is_normalised() {
        let r = find_refs("=B5:A1");
        assert_eq!((r[0].first, r[0].last), (p(0, 0), p(4, 1)));
    }

    #[test]
    fn sheet_names_plain_and_quoted() {
        let r = find_refs("=Sheet2!A1+'My ''best'' sheet'!B2:C3");
        assert_eq!(r[0].sheet.as_deref(), Some("Sheet2"));
        assert_eq!((r[0].start, r[0].end), (1, 10));
        assert_eq!(r[1].sheet.as_deref(), Some("My 'best' sheet"));
        assert_eq!(r[1].first, p(1, 1));
    }

    #[test]
    fn ignores_text_functions_and_names() {
        assert!(find_refs("=\"A1\"&\"B2:C3\"").is_empty());
        assert!(find_refs("=LOG10(100)+SUM()").is_empty());
        assert!(find_refs("=TRUE").is_empty());
        assert!(find_refs("=1E5+A0").is_empty());
        assert!(find_refs("=XFE1+A1048577").is_empty());
        assert_eq!(spans("=\"x\"&A1"), [("A1".into(), 0)]);
    }

    #[test]
    fn same_reference_same_colour() {
        assert_eq!(
            spans("=A1+B2+a1+B2:C3+Sheet1!A1"),
            [
                ("A1".into(), 0),
                ("B2".into(), 1),
                ("a1".into(), 0),
                ("B2:C3".into(), 2),
                ("Sheet1!A1".into(), 3),
            ]
        );
    }

    #[test]
    fn accepts_after_operators_and_separators() {
        for f in [
            "=", "=A1+", "=SUM(", "=SUM(A1,", "=SUM(A1;", "=A1 * ", "=A1<",
        ] {
            assert!(accepts_ref(f, f.chars().count()), "{f}");
        }
        assert!(accepts_ref("=SUM()", 5));
    }

    #[test]
    fn refuses_values_names_and_text() {
        assert!(!accepts_ref("12", 2));
        assert!(!accepts_ref("+", 1));
        assert!(!accepts_ref("=SU", 3));
        assert!(!accepts_ref("=A1", 3));
        assert!(!accepts_ref("=\"a+", 4));
        assert!(!accepts_ref("=+SUM", 2)); // a word follows the cursor
        assert!(!accepts_ref("=A1", 0));
    }

    #[test]
    fn next_click_replaces_the_last_reference() {
        assert_eq!(ref_slot("=A1", (3, 3), Some((1, 3))), Some((1, 3)));
        // an operator typed after it: a new reference goes behind the operator
        assert_eq!(ref_slot("=A1+", (4, 4), Some((1, 3))), Some((4, 4)));
        assert_eq!(ref_slot("=", (1, 1), None), Some((1, 1)));
        assert_eq!(ref_slot("=SU", (3, 3), None), None);
    }

    #[test]
    fn writes_references() {
        assert_eq!(ref_text(p(0, 0), p(0, 0)), "A1");
        assert_eq!(ref_text(p(3, 2), p(1, 1)), "B2:C4");
        let mut t = "=A1+x".to_string();
        assert_eq!(splice(&mut t, 4, 5, "B2:C4"), 9);
        assert_eq!(t, "=A1+B2:C4");
        let mut t = "=ä+".to_string();
        splice(&mut t, 3, 3, "A1");
        assert_eq!(t, "=ä+A1");
    }

    #[test]
    fn name_box_input_is_a_cell_or_range() {
        assert_eq!(parse_reference("B200"), Some((None, p(199, 1), p(199, 1))));
        assert_eq!(parse_reference(" c10:a1 "), Some((None, p(0, 0), p(9, 2))));
        assert_eq!(parse_reference("$A$1"), Some((None, p(0, 0), p(0, 0))));
        assert_eq!(
            parse_reference("Sheet2!A1"),
            Some((Some("Sheet2".into()), p(0, 0), p(0, 0)))
        );
        assert_eq!(
            parse_reference("'My sheet'!B2:B3"),
            Some((Some("My sheet".into()), p(1, 1), p(2, 1)))
        );
        for bad in [
            "", "hello", "nonsense", "A0", "A1+B2", "SUM(A1)", "A1:", "ZZZZ1",
        ] {
            assert_eq!(parse_reference(bad), None, "{bad}");
        }
    }
}
