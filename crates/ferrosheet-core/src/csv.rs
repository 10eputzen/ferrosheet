//! Reading delimited text files (`.csv`, `.tsv`). Everything is detected, nothing is
//! asked: the encoding (UTF-8, UTF-16 with BOM, else Windows-1252 as Excel writes it),
//! the delimiter (`;` `,` tab `|`) and the decimal separator.
//!
//! Only plain numbers become numbers, so they align right and copy as values. Anything
//! that could lose information stays text exactly as written: dates, percentages,
//! currency, and digit strings with a leading zero or more than 15 digits (postcodes,
//! IDs, phone and account numbers), which Excel is known to mangle.

use crate::{CellValue, Locale, Sheet};

/// How a text file was encoded, so saving writes it back the same way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

/// Everything needed to write a text file back as it was read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextFormat {
    pub delimiter: u8,
    pub decimal_comma: bool,
    pub encoding: Encoding,
}

impl TextFormat {
    /// Detects the format of a decoded file.
    pub fn detect(text: &str, encoding: Encoding) -> TextFormat {
        let delimiter = sniff_delimiter(text);
        TextFormat {
            delimiter,
            decimal_comma: decimal_comma(text, delimiter),
            encoding,
        }
    }
}

/// A parsed file: row-major cells, padded to the widest row.
pub struct Table {
    pub rows: u32,
    pub cols: u32,
    pub cells: Vec<CellValue>,
}

/// Turns the raw file into text and says which encoding it was.
pub fn decode(bytes: Vec<u8>) -> (String, Encoding) {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return (
            String::from_utf8_lossy(rest).into_owned(),
            Encoding::Utf8Bom,
        );
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return (utf16(rest, u16::from_le_bytes), Encoding::Utf16Le);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return (utf16(rest, u16::from_be_bytes), Encoding::Utf16Be);
    }
    match String::from_utf8(bytes) {
        Ok(s) => (s, Encoding::Utf8),
        Err(e) => (windows_1252(e.as_bytes()), Encoding::Windows1252),
    }
}

/// Writes a sheet as delimited text in `format`. Numbers use the file's decimal
/// separator without grouping; everything else is written as displayed.
pub fn write(sheet: &Sheet, format: TextFormat) -> Result<Vec<u8>, String> {
    let mut w = ::csv::WriterBuilder::new()
        .delimiter(format.delimiter)
        .terminator(::csv::Terminator::CRLF)
        .from_writer(Vec::new());
    for r in 0..sheet.rows {
        let row = (0..sheet.cols).map(|c| match sheet.value(r, c) {
            CellValue::Number(n) => {
                let s = CellValue::Number(*n).format(Locale::Invariant);
                if format.decimal_comma {
                    s.replace('.', ",")
                } else {
                    s
                }
            }
            CellValue::Text(t) => t.to_string(),
            _ => sheet.display(r, c).into_owned(),
        });
        w.write_record(row).map_err(|e| e.to_string())?;
    }
    let text =
        String::from_utf8(w.into_inner().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(encode(&text, format.encoding))
}

fn encode(text: &str, encoding: Encoding) -> Vec<u8> {
    match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => [b"\xEF\xBB\xBF".as_slice(), text.as_bytes()].concat(),
        Encoding::Utf16Le => [0xFF, 0xFE]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
            .collect(),
        Encoding::Utf16Be => [0xFE, 0xFF]
            .into_iter()
            .chain(text.encode_utf16().flat_map(u16::to_be_bytes))
            .collect(),
        Encoding::Windows1252 => text.chars().map(to_windows_1252).collect(),
    }
}

/// `?` for characters Windows-1252 cannot hold.
fn to_windows_1252(ch: char) -> u8 {
    match u32::from(ch) {
        n @ (0..=0x7F | 0xA0..=0xFF) => n as u8,
        _ => WINDOWS_1252_HIGH
            .iter()
            .position(|&h| h == ch)
            .map_or(b'?', |i| 0x80 + i as u8),
    }
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> String {
    let words = bytes.as_chunks::<2>().0.iter().map(|&c| word(c));
    char::decode_utf16(words)
        .map(|c| c.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Windows-1252: Latin-1 plus typographic characters in 0x80–0x9F.
const WINDOWS_1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
];

fn windows_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => WINDOWS_1252_HIGH[usize::from(b - 0x80)],
            _ => char::from(b),
        })
        .collect()
}

/// The delimiter that splits the first lines most evenly, `,` if none does.
fn sniff_delimiter(text: &str) -> u8 {
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).take(30).collect();
    let mut best = (b',', 0u64);
    for d in *b";,\t|" {
        let counts: Vec<usize> = lines.iter().map(|l| count_outside_quotes(l, d)).collect();
        let min = counts.iter().copied().min().unwrap_or(0) as u64;
        let even = counts.windows(2).all(|w| w[0] == w[1]);
        // Same count on every line beats more on some; ties go to the earlier candidate.
        let score = if even { min * 1_000 } else { min };
        if score > best.1 {
            best = (d, score);
        }
    }
    best.0
}

fn count_outside_quotes(line: &str, d: u8) -> usize {
    let mut quoted = false;
    line.bytes()
        .filter(|&b| {
            if b == b'"' {
                quoted = !quoted;
            }
            !quoted && b == d
        })
        .count()
}

/// Decimal comma or point. `,` as delimiter means point, `;` comma (that is how Excel
/// writes CSV in Germany); otherwise whichever separates digits more often.
fn decimal_comma(text: &str, delimiter: u8) -> bool {
    match delimiter {
        b',' => return false,
        b';' => return true,
        _ => {}
    }
    let sample = &text.as_bytes()[..text.len().min(64 * 1024)];
    // `d<sep>d…` not followed by another `<sep>`, so `01.02.2026` does not count.
    let decimals = |sep: u8| {
        (1..sample.len().saturating_sub(1))
            .filter(|&i| {
                sample[i] == sep && sample[i - 1].is_ascii_digit() && {
                    let rest = &sample[i + 1..];
                    let n = rest.iter().take_while(|b| b.is_ascii_digit()).count();
                    n > 0 && rest.get(n) != Some(&sep)
                }
            })
            .count()
    };
    decimals(b',') > decimals(b'.')
}

pub fn parse(text: &str) -> Result<Table, String> {
    let delimiter = sniff_delimiter(text);
    let comma = decimal_comma(text, delimiter);
    let mut reader = ::csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .delimiter(delimiter)
        .from_reader(text.as_bytes());
    let mut rows: Vec<Vec<CellValue>> = Vec::new();
    let mut cols = 0;
    for record in reader.records() {
        let record = record.map_err(|e| e.to_string())?;
        let row: Vec<CellValue> = record.iter().map(|f| value(f, comma)).collect();
        cols = cols.max(row.len());
        rows.push(row);
    }
    // A trailing delimiter on every line leaves an empty last column: drop it.
    while cols > 0
        && rows
            .iter()
            .all(|r| r.get(cols - 1).is_none_or(|v| *v == CellValue::Empty))
    {
        cols -= 1;
    }
    let mut cells = Vec::with_capacity(rows.len() * cols);
    for mut row in rows.iter_mut().map(std::mem::take) {
        row.resize(cols, CellValue::Empty);
        cells.extend(row);
    }
    Ok(Table {
        rows: if cols == 0 { 0 } else { rows.len() as u32 },
        cols: cols as u32,
        cells,
    })
}

fn value(field: &str, decimal_comma: bool) -> CellValue {
    let t = field.trim();
    if t.is_empty() {
        return CellValue::Empty;
    }
    match number(t, decimal_comma) {
        Some(n) => CellValue::Number(n),
        None => CellValue::Text(field.into()),
    }
}

/// A plain number such as `-1.234,5` (decimal comma) or `1,234.5` / `1e3` (point).
fn number(t: &str, decimal_comma: bool) -> Option<f64> {
    let (dec, group) = if decimal_comma {
        (',', '.')
    } else {
        ('.', ',')
    };
    let digits = t.trim_start_matches(['-', '+']);
    let int_part = digits.split([dec, 'e', 'E']).next().unwrap_or("");
    // Leading zeros and long digit strings are codes, not amounts.
    let plain_int = int_part.replace(group, "");
    if (plain_int.len() > 1 && plain_int.starts_with('0'))
        || digits.chars().filter(char::is_ascii_digit).count() > 15
    {
        return None;
    }
    // Thousands separators only in groups of three.
    if int_part.contains(group) {
        let mut groups = int_part.split(group);
        let first = groups.next()?;
        if first.is_empty() || first.len() > 3 || !groups.all(|g| g.len() == 3) {
            return None;
        }
    }
    if !digits.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    let normal: String = t
        .chars()
        .filter(|&c| c != group)
        .map(|c| if c == dec { '.' } else { c })
        .collect();
    normal.parse::<f64>().ok().filter(|n| n.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(v: &CellValue) -> &str {
        match v {
            CellValue::Text(s) => s,
            _ => panic!("not text: {v:?}"),
        }
    }

    #[test]
    fn german_excel_export() {
        let t =
            parse("Name;Betrag;PLZ;Datum\nMüller;1.234,50;01067;01.02.2026\nSchmidt;-3,5;80331;\n")
                .unwrap();
        assert_eq!((t.rows, t.cols), (3, 4));
        assert_eq!(t.cells[5], CellValue::Number(1234.5));
        assert_eq!(text(&t.cells[6]), "01067");
        assert_eq!(text(&t.cells[7]), "01.02.2026");
        assert_eq!(t.cells[9], CellValue::Number(-3.5));
        assert_eq!(t.cells[10], CellValue::Number(80331.0));
        assert_eq!(t.cells[11], CellValue::Empty);
    }

    #[test]
    fn comma_separated_with_quotes_and_line_breaks() {
        let t = parse("a,b,c\n\"x, y\",\"1,234.5\",\"two\nlines\"\n3,4.25,1e3\n").unwrap();
        assert_eq!((t.rows, t.cols), (3, 3));
        assert_eq!(text(&t.cells[3]), "x, y");
        assert_eq!(t.cells[4], CellValue::Number(1234.5));
        assert_eq!(text(&t.cells[5]), "two\nlines");
        assert_eq!(t.cells[7], CellValue::Number(4.25));
        assert_eq!(t.cells[8], CellValue::Number(1000.0));
    }

    #[test]
    fn tabs_ragged_rows_and_trailing_delimiters() {
        let t = parse("a\tb\t\n1\t\n1\t2\t\n").unwrap();
        assert_eq!((t.rows, t.cols), (3, 2));
        assert_eq!(t.cells[3], CellValue::Empty);
    }

    #[test]
    fn codes_and_odd_numbers_stay_text() {
        for s in [
            "007",
            "4711123456789012345",
            "12 %",
            "12,50 €",
            "1.2.3",
            "1.23,4",
            "+-1",
        ] {
            assert!(matches!(value(s, true), CellValue::Text(_)), "{s}");
        }
        assert_eq!(value("0,5", true), CellValue::Number(0.5));
        assert_eq!(value("0", true), CellValue::Number(0.0));
        assert_eq!(value("12.000", true), CellValue::Number(12000.0));
    }

    #[test]
    fn encodings() {
        assert_eq!(decode(b"\xEF\xBB\xBFa;\xC3\xA4".to_vec()).0, "a;ä");
        assert_eq!(decode(b"Gr\xF6\xDFe \x80".to_vec()).0, "Größe €");
        assert_eq!(decode(b"\xFF\xFEa\0\xE4\0".to_vec()).0, "aä");
    }

    #[test]
    fn delimiter_detection() {
        assert_eq!(sniff_delimiter("a;b;c\n1;2,5;3\n"), b';');
        assert_eq!(sniff_delimiter("a,b,c\n1,2,3\n"), b',');
        assert_eq!(sniff_delimiter("a\tb\n\"x;y\"\t2\n"), b'\t');
        assert_eq!(sniff_delimiter("one column\n"), b',');
    }

    #[test]
    fn writes_back_in_the_format_it_was_read() {
        let cases = [
            (
                b"Name;Betrag;PLZ\r\nM\xFCller;2,5;01234\r\n\x80;1000;x\r\n".to_vec(),
                Encoding::Windows1252,
            ),
            (
                b"\xEF\xBB\xBFa,b\r\n\"x,y\",1.5\r\n".to_vec(),
                Encoding::Utf8Bom,
            ),
        ];
        for (bytes, enc) in cases {
            let (text, found) = decode(bytes.clone());
            assert_eq!(found, enc);
            let format = TextFormat::detect(&text, found);
            let t = parse(&text).unwrap();
            let sheet = Sheet::from_cells("t".into(), t.rows, t.cols, t.cells, Locale::Invariant);
            assert_eq!(write(&sheet, format).unwrap(), bytes);
        }
    }
}
