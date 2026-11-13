# Feature parity with LibreOffice Calc (rough)

ferrosheet is a viewer first. This table shows where it stands against LibreOffice Calc,
so nobody expects an office suite. ✅ done · 🟡 planned · ❌ out of scope

Spreadsheet functions (SUM, VLOOKUP, …) have their own list: [function parity](feature-parity-functions.md).

| Feature | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| **Files** | | |
| Open xls, xlsx, xlsm, xlsb, ods | ✅ | ✅ |
| Open csv, tsv | ✅ (import dialog) | ✅ delimiter, encoding and decimals detected, no dialog; codes with leading zeros stay text |
| Open other formats (Numbers, dbf, html, xml, Lotus) | ✅ | ❌ |
| Save | all formats | 🟡 xlsx and csv in place, other formats as `.xlsx`. An xlsx keeps values, formulas, cell formatting, number formats, merged cells, widths, heights, frozen panes, links, conditional formatting and names; comments, drop-downs, filters, tables, charts/pictures, pivot tables and print settings are lost, and ferrosheet warns before overwriting such a file |
| Export PDF, html | ✅ | ❌ |
| Password-protected files | ✅ | ❌ |
| Templates | ✅ | ❌ |
| Drag & drop, open via file association | ✅ | ✅ / 🟡 Windows registration |
| Start-up and load time | seconds | ✅ window < 300 ms, 100k×30 cells: first rows after 11 ms, all in ~0.7 s |
| Install size | ~350 MB | ✅ single 8 MB exe |
| Automatic updates | via system or download | ✅ downloaded in the background, restart in an instant; manual check in *Help → About* |
| **Viewing** | | |
| Multiple sheets, switch with Ctrl+PgUp/PgDn | ✅ | ✅ |
| Cell values: numbers, text, dates, durations, booleans, errors | ✅ | ✅ |
| Number formats from the file (currency, custom) | ✅ | 🟡 default formats only (German or neutral, by system language) |
| Cell formatting from the file (fonts, colours, borders) | ✅ | ❌ values only |
| Merged cells | ✅ | ❌ |
| Freeze panes, hidden rows and columns | ✅ | ❌ |
| Column width and row height: drag, double-click autofit | ✅ | ✅ |
| Zoom | ✅ | ✅ grid only, 50–300 %, Ctrl+Plus/Minus/0 or Ctrl+wheel |
| Add, rename, delete sheets | ✅ | ✅ "+" tab, double-click or right-click a tab |
| Recent files | ✅ | ✅ *File → Recent* and the start screen |
| Split window, several windows on one file | ✅ | ❌ |
| Light and dark mode | ✅ | ✅ |
| Interface language | many | German, English |
| **Finding and selecting** | | |
| Search, current or all sheets, match case | ✅ | ✅ |
| Search with regex | ✅ | ✅ toggle next to "All sheets", invalid pattern shows a hint |
| Replace | ✅ | ✅ Ctrl+H or *Edit → Replace…*: one match or all at once (one undo step, says how many cells), also inside formulas; regex groups with `$1` |
| Name box, go to a cell or range (`B200`, `Sheet2!A1`, `A1:C10`) | ✅ | ✅ Ctrl+G or F5; named ranges ❌ |
| Select cells, rows, columns, all | ✅ | ✅ |
| Copy as tab-separated text (pastes into Excel, Sheets, mail) | ✅ | ✅ plus raw values with Ctrl+Shift+C |
| Sort and filter | ✅ | ❌ (maybe later, view-only) |
| Sum, average and count of the selection in the status bar | ✅ | ✅ |
| **Editing** | | |
| Edit cells | ✅ | ✅ |
| Undo, redo | ✅ | ✅ |
| Paste | ✅ | ✅ tab-separated text from Excel, Sheets or ferrosheet (Ctrl+V, *Edit → Paste*); one value fills the selection; one undo step; values only, no formatting |
| Insert and delete rows, columns, sheets | ✅ | ✅ right-click a row/column header or *Edit*; formulas follow. Undoing a delete brings the cells back, but formulas that pointed into them stay `#REF!` (engine limitation) |
| Format cells: bold, italic, colour, number format | ✅ | ✅ shown for what is set in ferrosheet |
| Format cells: fonts, borders, alignment, styles | ✅ | ❌ |
| Merge cells | ✅ | ❌ |
| Autofill (drag a series) | ✅ | ❌ |
| AutoSum (Σ, Alt+=), today's date (Ctrl+;) | ✅ | ✅ the range can be changed by clicking cells before Enter; on German keyboards also Ctrl+. |
| Data validation, drop-down lists | ✅ | ❌ |
| Cell and sheet protection | ✅ | ❌ |
| **Formulas and analysis** | | |
| Show the formula of a cell | ✅ | ✅ (formula bar) |
| Recalculate | ✅ | ✅ via IronCalc once editing starts, see [function parity](feature-parity-functions.md); before that the results stored in the file |
| Enter formulas | ✅ | ✅ in English (`=SUM(A1:A3)`) |
| Pick references by clicking or dragging while typing a formula, coloured references | ✅ | ✅ in the cell and the formula bar |
| Named ranges | ✅ | ❌ |
| Goal seek, solver, scenarios, what-if tables | ✅ | ❌ |
| Pivot tables | ✅ | ❌ |
| Conditional formatting | ✅ | ❌ |
| Charts | ✅ | ❌ |
| Statistics dialogs (regression, sampling, …) | ✅ | ❌ |
| Links to other files and external data | ✅ | ❌ |
| **Everything else** | | |
| Images, shapes, comments, hyperlinks | ✅ | ❌ |
| Forms and controls (buttons, check boxes) | ✅ | ❌ |
| Macros, scripting | ✅ | ❌ |
| Print, page layout | ✅ | ❌ |
| Spell check | ✅ | ❌ |
| Collaboration, change tracking | partly | ❌ |
| Settings dialog | ✅ | 🟡 later |

**Rough parity:** for "open, read, find, copy" ferrosheet covers the everyday path completely
and is far faster. Measured against everything Calc can do it is maybe 5 %, on purpose.
