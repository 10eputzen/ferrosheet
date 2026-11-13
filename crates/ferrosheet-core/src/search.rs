//! Search over the displayed cell text: plain text or a regular expression, ignoring
//! case unless asked not to. Also replaces matches in a cell's input.

use regex::{NoExpand, Regex, RegexBuilder};

use crate::{CellPos, Sheet};

/// What to look for.
#[derive(Clone, Debug)]
pub enum Query {
    /// Plain text, lower-cased when case does not matter.
    Text { text: String, case: bool },
    /// The pattern; built case-insensitive when case does not matter.
    Regex { re: Regex, case: bool },
}

impl Query {
    /// Plain text, ignoring case.
    pub fn text(query: &str) -> Query {
        Query::text_case(query, false)
    }

    /// Plain text; `case` makes upper and lower case count.
    pub fn text_case(query: &str, case: bool) -> Query {
        let text = if case {
            query.to_owned()
        } else {
            query.to_lowercase()
        };
        Query::Text { text, case }
    }

    /// A regular expression ignoring case; the error says what is wrong with it.
    pub fn regex(pattern: &str) -> Result<Query, String> {
        Query::regex_case(pattern, false)
    }

    /// A regular expression; `case` makes upper and lower case count.
    pub fn regex_case(pattern: &str, case: bool) -> Result<Query, String> {
        RegexBuilder::new(pattern)
            .case_insensitive(!case)
            // keeps a pathological pattern from eating all memory
            .size_limit(1 << 20)
            .build()
            .map(|re| Query::Regex { re, case })
            .map_err(|e| e.to_string())
    }

    fn case(&self) -> bool {
        match self {
            Query::Text { case, .. } | Query::Regex { case, .. } => *case,
        }
    }

    /// `s` is lower-cased when the query ignores case.
    fn matches(&self, s: &str) -> bool {
        match self {
            Query::Text { text, .. } => s.contains(text.as_str()),
            // A case-insensitive pattern matches the lower-cased text the same.
            Query::Regex { re, .. } => re.is_match(s),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Query::Text { text, .. } => text.is_empty(),
            Query::Regex { re, .. } => re.as_str().is_empty(),
        }
    }

    /// `s` with every match replaced by `with` (a pattern may use `$1` for groups);
    /// `None` when nothing matches.
    pub fn replace(&self, s: &str, with: &str) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let out = match self {
            Query::Regex { re, .. } => re.replace_all(s, with),
            Query::Text { text, case } => {
                let re = RegexBuilder::new(&regex::escape(text))
                    .case_insensitive(!case)
                    .build()
                    .ok()?;
                // `re` borrows nothing from `s`, so the result is owned either way
                return match re.replace_all(s, NoExpand(with)) {
                    std::borrow::Cow::Owned(o) => Some(o),
                    std::borrow::Cow::Borrowed(_) => None,
                };
            }
        };
        match out {
            std::borrow::Cow::Owned(o) => Some(o),
            std::borrow::Cow::Borrowed(_) => None,
        }
    }
}

/// All cells whose displayed text matches `query`, row by row.
pub fn find_all(sheet: &Sheet, query: &Query) -> Vec<CellPos> {
    if query.is_empty() || sheet.cols == 0 {
        return Vec::new();
    }
    let cols = sheet.cols as usize;
    let pos = |i: usize| CellPos::new((i / cols) as u32, (i % cols) as u32);
    if query.case() {
        return (0..sheet.rows as usize * cols)
            .filter(|&i| {
                let p = pos(i);
                query.matches(&sheet.display(p.row, p.col))
            })
            .map(pos)
            .collect();
    }
    sheet
        .lower()
        .iter()
        .enumerate()
        .filter(|(_, s)| query.matches(s))
        .map(|(i, _)| pos(i))
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

    #[test]
    fn match_case_only_finds_the_exact_spelling() {
        let cells = vec![Text("Müller".into()), Text("müller".into())];
        let s = Sheet::from_cells("t".into(), 1, 2, cells, Locale::German);
        assert_eq!(
            find_all(&s, &Query::text_case("Mü", true)),
            [CellPos::new(0, 0)]
        );
        assert_eq!(find_all(&s, &Query::text_case("Mü", false)).len(), 2);
        let q = Query::regex_case("^m", true).unwrap();
        assert_eq!(find_all(&s, &q), [CellPos::new(0, 1)]);
    }

    #[test]
    fn replace_keeps_the_rest_and_says_when_nothing_matched() {
        let q = Query::text("ab");
        assert_eq!(q.replace("xAbyab", "Z").as_deref(), Some("xZyZ"));
        assert_eq!(q.replace("none", "Z"), None);
        // plain text never expands `$1`
        assert_eq!(q.replace("ab", "$1").as_deref(), Some("$1"));
        let q = Query::text_case("ab", true);
        assert_eq!(q.replace("Abab", "Z").as_deref(), Some("AbZ"));
        let q = Query::regex(r"(\d+)-(\d+)").unwrap();
        assert_eq!(
            q.replace("10-20 and 3-4", "${2}-$1").as_deref(),
            Some("20-10 and 4-3")
        );
        assert_eq!(Query::text("").replace("abc", "x"), None);
    }
}
