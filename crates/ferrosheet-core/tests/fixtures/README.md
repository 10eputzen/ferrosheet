# Test fixtures

`date.xls`, `date.xlsb` and `date.ods` are copied unchanged from the
[calamine](https://github.com/tafia/calamine) test suite (`tests/`), MIT licence.

`demo.xlsx` is the demo workbook, written by
`cargo run -p ferrosheet-core --example gen_demo -- crates/ferrosheet-core/tests/fixtures/demo.xlsx`.
`styles.xlsx` holds cell formats (fills, bold and coloured text, a 14 pt title merged over
A1:B1) as another spreadsheet library writes them; `tests/styles.rs` checks them cell by cell.

The robustness tests truncate and corrupt it; other `.xlsx` test files are generated at
test time with `rust_xlsxwriter`, so their content is defined in the tests.
