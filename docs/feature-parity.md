# Feature parity with LibreOffice Calc (rough)

ferrosheet is a viewer first. This table shows where it stands against LibreOffice Calc,
so nobody expects an office suite. ✅ done · 🟡 planned · ❌ out of scope

| Area | LibreOffice Calc | ferrosheet |
| --- | --- | --- |
| **Open** xls, xlsx, xlsm, xlsb, ods | ✅ | ✅ |
| Open csv | ✅ | ❌ (any text editor does it) |
| Start-up and load time | seconds | ✅ window < 300 ms, 100k×30 cells in ~1.4 s |
| Install size | ~350 MB | ✅ single 6.9 MB exe |
| Multiple sheets, switch with Ctrl+PgUp/PgDn | ✅ | ✅ |
| Cell values: numbers, text, dates, durations, booleans, errors | ✅ | ✅ |
| Formulas: show the formula of a cell | ✅ | ✅ (formula bar) |
| Formulas: recalculate | ✅ | 🟡 phase 2 via IronCalc; phase 1 shows the results stored in the file |
| Search, case-insensitive, current or all sheets | ✅ | ✅ |
| Search with regex, replace | ✅ | ❌ / 🟡 replace with phase 2 |
| Select cells, rows, columns, all | ✅ | ✅ |
| Copy as tab-separated text (pastes into Excel, Sheets, mail) | ✅ | ✅ plus raw values with Ctrl+Shift+C |
| Column width: drag, double-click autofit | ✅ | ✅ |
| Light and dark mode | ✅ | ✅ |
| Drag & drop, open via file association | ✅ | ✅ / 🟡 Windows registration |
| Cell formatting (fonts, colours, borders) | ✅ | ❌ values only |
| Number formats from the file (currency, custom) | ✅ | 🟡 German default formats only |
| Merged cells | ✅ | ❌ |
| Freeze panes, hidden rows and columns | ✅ | ❌ |
| Sort and filter | ✅ | ❌ (maybe later, view-only) |
| **Edit** cells | ✅ | 🟡 phase 2 |
| Save | all formats | 🟡 phase 2, xlsx only, no formatting |
| Charts, pivot tables, conditional formatting | ✅ | ❌ |
| Images, comments, hyperlinks | ✅ | ❌ |
| Macros, scripting | ✅ | ❌ |
| Print, PDF export | ✅ | ❌ |
| Password-protected files | ✅ | ❌ |
| Collaboration, change tracking | partly | ❌ |

**Rough parity:** for "open, read, find, copy" ferrosheet covers the everyday path completely
and is far faster. Measured against everything Calc can do it is maybe 5 %, on purpose.
