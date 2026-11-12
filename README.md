# ferrosheet

A fast, lightweight spreadsheet viewer written in Rust. Opens `.xls`, `.xlsx`, `.xlsm`, `.xlsb`
and `.ods`, shows them in under a second and gets out of the way.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/screenshot-light.png">
  <img alt="ferrosheet showing an invoice workbook with formulas, dates and a selected range" src="docs/screenshot-dark.png">
</picture>

## Why

I don't have a Microsoft licence any more, and I have no appetite for an office suite that
takes ages to start just so I can look at a file someone emailed me. I only want to open my
Excel files, find something, copy a few cells. With an AI subscription at hand the obvious
question was: why not port that small piece to Rust?

## What it does

- Opens xls, xlsx, xlsm, xlsb and ods, including multiple sheets
- Numbers, text, dates, durations, booleans and error values; formulas show their stored
  result in the cell and the formula itself in the formula bar
- Search across the current sheet or all sheets
- Select cells, rows, columns or everything; copy as tab-separated text that pastes into
  Excel, Google Sheets or an email
- Resize columns and rows by dragging, double-click an edge to fit the content
- Light and dark mode, following the system
- Damaged files end in an error message, never in a crash

It is a viewer. Editing and saving as `.xlsx` are planned for phase 2, see the
[roadmap](docs/roadmap.md) and the honest [comparison with LibreOffice Calc](docs/feature-parity.md).
The interface is in German for now.

## Performance

Measured on a desktop PC with Windows 11, release build:

| | |
| --- | --- |
| Window visible | < 300 ms |
| Open a 100 000 × 30 `.xlsx` (11.7 MB) | 1.4 s |
| Search 3 million cells, first / again | 190 ms / 37 ms |
| Download size | single 6.9 MB `.exe`, no installer |

Reproduce with `cargo run --release -p ferrosheet-core --example gen_big` and
`cargo run --release -p ferrosheet-core --example bench_load -- target/big.xlsx`.

## Install

Download the latest build from [Releases](https://github.com/10eputzen/ferrosheet/releases).

- **Windows:** unzip and run `ferrosheet.exe`. The exe is not signed yet, so SmartScreen warns
  on first start: *More info → Run anyway*. To open spreadsheets with a double-click, run
  `ferrosheet.exe --register` once and pick ferrosheet in the settings page that opens.
  `--unregister` removes it again. Both only touch the current user's registry.
- **Linux (x86_64):** `tar xzf ferrosheet-linux-x86_64.tar.gz && ./ferrosheet file.xlsx`.
  Built on Ubuntu 22.04, needs an X11 or Wayland desktop with OpenGL.
- **macOS:** not yet, see the [roadmap](docs/roadmap.md#platforms).

From source, with Rust installed:

```bash
cargo install --path crates/ferrosheet
```

## Keyboard

| Keys | Action |
| --- | --- |
| Ctrl+O | Open a file (or drop it on the window) |
| Arrows, Enter, Tab | Move; Shift extends the selection (Shift+Enter/Tab moves back) |
| Ctrl+Arrows, Home/End, Ctrl+Home/End | Jump to the edge of the sheet or row |
| Page Up / Page Down | Scroll a screen |
| Ctrl+A | Select everything |
| Ctrl+C | Copy as shown |
| Ctrl+Shift+C | Copy raw values (dot as decimal separator, ISO dates) |
| Ctrl+F, F3, Shift+F3, Esc | Search, next hit, previous hit, close |
| Ctrl+Page Up / Page Down | Previous / next sheet |

Mouse: click a column or row header to select it, drag a header edge to resize,
double-click an edge to fit.

## How it is built

- [`ferrosheet-core`](crates/ferrosheet-core): reading files via [calamine](https://github.com/tafia/calamine),
  value formatting, selection, search and copy. No UI dependency.
- [`ferrosheet`](crates/ferrosheet): the app, [egui](https://github.com/emilk/egui)/eframe. Loading
  runs on a background thread; the grid only paints the visible cells, so a million rows
  scroll as smoothly as ten.

## Tests

```bash
cargo test --workspace
```

- Unit tests for formatting, selection, search and copy
- Robustness tests that truncate and corrupt every fixture and assert the reader never panics
- UI tests with [egui_kittest](https://github.com/emilk/egui/tree/main/crates/egui_kittest) that
  drive the whole app: open a workbook, press keys, copy, search, switch sheets, resize columns
- CI runs format, clippy and tests on Windows and Linux and reports test coverage

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at
your option. Test fixtures `date.*` come from the calamine test suite (MIT).
