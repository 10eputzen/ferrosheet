//! Case-insensitive search over the displayed cell text: plain text or a regular
//! expression.

use regex::{Regex, RegexBuilder};

use crate::{CellPos, Sheet};

/// What to look for. Both kinds ignore case.
#[derive(Clone, Debug)]
pub enum Query {
    Text(String),
    Regex(Regex),
}

impl Query {
    pub fn text(query: &str) -> Query {
        Query::Text(query.to_lowercase())
    }

    /// A regular expression; the error message says what is wrong with the pattern.
    pub fn regex(pattern: &str) -> Result<Query, String> {
        RegexBuilder::new(pattern)
            .case_insensitive(true)
            // keeps a pathological pattern from eating all memory
            .size_limit(1 << 20)
            .build()
            .map(Query::Regex)
            .map_err(|e| e.to_string())
    }

    fn matches(&self, lower: &str) -> bool {
        match self {
            Query::Text(q) => lower.contains(q.as_str()),
            // The index is lower-cased; a case-insensitive pattern matches it the same.
            Query::Regex(re) => re.is_match(lower),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Query::Text(q) => q.is_empty(),
            Query::Regex(re) => re.as_str().is_empty(),
        }
    }
}

/// All cells whose displayed text matches `query`, row by row.
pub fn find_all(sheet: &Sheet, query: &Query) -> Vec<CellPos> {
    if query.is_empty() || sheet.cols == 0 {
        return Vec::new();
    }
    let cols = sheet.cols as usize;
    sheet
        .lower()
        .iter()
        .enumerate()
        .filter(|(_, s)| query.matches(s))
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
            find_all(&s, &Query::text("MÜLL")),
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
        assert_eq!(find_all(&s, &Query::text("2,5")), [CellPos::new(0, 0)]);
        assert_eq!(find_all(&s, &Query::text("11.2023")), [CellPos::new(0, 1)]);
    }

    #[test]
    fn empty_query_no_hits() {
        let s = Sheet::from_cells("t".into(), 1, 1, vec![Text("a".into())], Locale::German);
        assert!(find_all(&s, &Query::text("")).is_empty());
        assert!(find_all(&Sheet::empty("e".into()), &Query::text("a")).is_empty());
    }

    #[test]
    fn regex_ignores_case_and_sees_display_text() {
        let cells = vec![
            Text("Müller".into()),
            Text("Meier".into()),
            Text("MÜLLER-X".into()),
            Number(2.5),
        ];
        let s = Sheet::from_cells("t".into(), 2, 2, cells, Locale::German);
        let q = Query::regex("^m(ü|ei)").unwrap();
        assert_eq!(
            find_all(&s, &q),
            [CellPos::new(0, 0), CellPos::new(0, 1), CellPos::new(1, 0)]
        );
        assert_eq!(
            find_all(&s, &Query::regex(r"^\d,\d$").unwrap()),
            [CellPos::new(1, 1)]
        );
        assert_eq!(
            find_all(&s, &Query::regex("ÜLLER$").unwrap()),
            [CellPos::new(0, 0)]
        );
    }

    #[test]
    fn invalid_regex_is_an_error_not_a_panic() {
        assert!(Query::regex("(abc").is_err());
        assert!(Query::regex("[").is_err());
        let s = Sheet::from_cells("t".into(), 1, 1, vec![Text("a".into())], Locale::German);
        assert!(find_all(&s, &Query::regex("").unwrap()).is_empty());
    }
}
