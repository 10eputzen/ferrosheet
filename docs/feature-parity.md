# Feature parity with LibreOffice Calc (rough)

ferrosheet is a viewer first. These tables show where it stands against LibreOffice Calc,
so nobody expects an office suite. ✅ done · 🟡 planned · ❌ out of scope

Spreadsheet functions (SUM, VLOOKUP, …) have their own list: [function parity](feature-parity-functions.md).

## Files

| Feature | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| Open xls, xlsx, xlsm, xlsb, ods | ✅ | ✅ |
| Open csv, tsv | ✅ (import dialog) | ✅ delimiter, encoding and decimals detected, no dialog; codes with leading zeros stay text |
| Open other formats (Numbers, dbf, html, xml, Lotus) | ✅ | ❌ |
| Save | all formats | 🟡 phase 2, xlsx and csv |
| Export PDF, html | ✅ | ❌ |
| Password-protected files | ✅ | ❌ |
| Templates | ✅ | ❌ |
| Drag & drop, open via file association | ✅ | ✅ / 🟡 Windows registration |
| Start-up and load time | seconds | ✅ window < 300 ms, 100k×30 cells: first rows after 11 ms, all in ~0.7 s |
| Install size | ~350 MB | ✅ single 8 MB exe |
| Automatic updates | via system or download | ✅ downloaded in the background, restart in an instant |

## Viewing

| Feature | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| Multiple sheets, switch with Ctrl+PgUp/PgDn | ✅ | ✅ |
| Cell values: numbers, text, dates, durations, booleans, errors | ✅ | ✅ |
| Number formats from the file (currency, custom) | ✅ | 🟡 default formats only (German or neutral, by system language) |
| Cell formatting from the file (fonts, colours, borders) | ✅ | ❌ values only |
| Merged cells | ✅ | ❌ |
| Freeze panes, hidden rows and columns | ✅ | ❌ |
| Column width and row height: drag, double-click autofit | ✅ | ✅ |
| Zoom | ✅ | ❌ |
| Split window, several windows on one file | ✅ | ❌ |
| Light and dark mode | ✅ | ✅ |
| Interface language | many | German, English |

## Finding and selecting

| Feature | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| Search, case-insensitive, current or all sheets | ✅ | ✅ |
| Search with regex | ✅ | ✅ toggle next to "All sheets", invalid pattern shows a hint |
| Replace | ✅ | 🟡 phase 2 |
| Go to cell or named range | ✅ | ❌ |
| Select cells, rows, columns, all | ✅ | ✅ |
| Copy as tab-separated text (pastes into Excel, Sheets, mail) | ✅ | ✅ plus raw values with Ctrl+Shift+C |
| Sort and filter | ✅ | ❌ (maybe later, view-only) |

## Editing

| Feature | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| Edit cells | ✅ | 🟡 phase 2 |
| Undo, redo | ✅ | 🟡 phase 2 |
| Paste | ✅ | 🟡 phase 2 |
| Insert and delete rows, columns, sheets | ✅ | 🟡 phase 2 |
| Format cells: bold, italic, colour, number format | ✅ | 🟡 phase 2 |
| Format cells: fonts, borders, alignment, styles | ✅ | ❌ |
| Merge cells | ✅ | ❌ |
| Autofill (drag a series) | ✅ | ❌ |
| Data validation, drop-down lists | ✅ | ❌ |
| Cell and sheet protection | ✅ | ❌ |

## Formulas and analysis

| Feature | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| Show the formula of a cell | ✅ | ✅ (formula bar) |
| Recalculate | ✅ | 🟡 phase 2 via IronCalc, see [function parity](feature-parity-functions.md); phase 1 shows the results stored in the file |
| Enter formulas | ✅ | 🟡 phase 2 |
| Named ranges | ✅ | ❌ |
| Goal seek, solver, scenarios, what-if tables | ✅ | ❌ |
| Pivot tables | ✅ | ❌ |
| Conditional formatting | ✅ | ❌ |
| Charts | ✅ | ❌ |
| Statistics dialogs (regression, sampling, …) | ✅ | ❌ |
| Links to other files and external data | ✅ | ❌ |

## Everything else

| Feature | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| Images, shapes, comments, hyperlinks | ✅ | ❌ |
| Forms and controls (buttons, check boxes) | ✅ | ❌ |
| Macros, scripting | ✅ | ❌ |
| Print, page layout | ✅ | ❌ |
| Spell check | ✅ | ❌ |
| Collaboration, change tracking | partly | ❌ |
| Settings dialog | ✅ | 🟡 later |

**Rough parity:** for "open, read, find, copy" ferrosheet covers the everyday path completely
and is far faster. Measured against everything Calc can do it is maybe 5 %, on purpose.
