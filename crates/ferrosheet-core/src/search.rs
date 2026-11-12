//! Case-insensitive search over the displayed cell text.

use crate::{CellPos, Sheet};

/// All cells whose displayed text contains `query`, row by row.
pub fn find_all(sheet: &Sheet, query: &str) -> Vec<CellPos> {
    if query.is_empty() || sheet.cols == 0 {
        return Vec::new();
    }
    let q = query.to_lowercase();
    let cols = sheet.cols as usize;
    sheet
        .lower()
        .iter()
        .enumerate()
        .filter(|(_, s)| s.contains(q.as_str()))
        .map(|(i, _)| CellPos::new((i / cols) as u32, (i % cols) as u32))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CellValue::*;
    use crate::Locale;

    #[test]
    fn finds_case_insensitive() {
        let cells = vec![
            Text("Müller".into()),
            Text("x".into()),
            Text("müller".into()),
            Text("MÜLLER-X".into()),
        ];
        let s = Sheet::from_cells("t".into(), 2, 2, cells, Locale::German);
        assert_eq!(
            find_all(&s, "MÜLL"),
            [CellPos::new(0, 0), CellPos::new(1, 0), CellPos::new(1, 1)]
        );
    }

    #[test]
    fn searches_display_text() {
        let s = Sheet::from_cells(
            "t".into(),
            1,
            2,
            vec![Number(2.5), DateTime(45231.0)],
            Locale::German,
        );
        assert_eq!(find_all(&s, "2,5"), [CellPos::new(0, 0)]);
        assert_eq!(find_all(&s, "11.2023"), [CellPos::new(0, 1)]);
    }

    #[test]
    fn empty_query_no_hits() {
        let s = Sheet::from_cells("t".into(), 1, 1, vec![Text("a".into())], Locale::German);
        assert!(find_all(&s, "").is_empty());
        assert!(find_all(&Sheet::empty("e".into()), "a").is_empty());
    }
}
