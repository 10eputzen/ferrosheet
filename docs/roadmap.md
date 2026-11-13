# Roadmap

## Phase 1 – read (in progress)

Open, view, search, copy.

## Platforms

All dependencies (calamine, egui/eframe, rfd) are cross-platform, so the code itself needs
almost no changes. The work is packaging.

| Platform | Effort | What it takes | Status |
| --- | --- | --- | --- |
| Windows x86_64 | – | exe, file association via `--register` | 🟡 phase 1 |
| Linux x86_64 | ~½ day | CI build on Ubuntu 22.04 (glibc 2.35), `.tar.gz`; later AppImage or `.deb` and a `.desktop` file with the MIME types for file association. File dialog uses the XDG portal, no GTK needed. | 🟡 build runs in CI |
| macOS (universal) | ~1–1½ days | universal binary (x86_64 + aarch64), `.app` bundle with `CFBundleDocumentTypes`, DMG. Finder "open with" sends an Apple event instead of argv; winit does not pass it through, so this needs a small macOS-specific handler. No Mac for hands-on testing – CI build only. | ❌ later |

### Code signing (optional, all platforms)

- **macOS:** unsigned apps are blocked by Gatekeeper on first start (workaround: right-click → Open).
  Signing and notarisation need an Apple Developer account (99 $/year) and ~½ day of setup.
- **Windows:** unsigned exes trigger a SmartScreen warning.

Decision: ship Linux and macOS unsigned together with the phase 1 release and explain the
first start in the README. Sign once the project is public and has users.

## Auto-update (before going public)

Silent, like Chrome or VS Code – no prompt, no click. Effort ~1–1½ days.

1. A few seconds after start (never blocking it), at most once a day, check the latest
   GitHub release.
2. Download the asset for the current platform in the background.
3. Verify the signature (own ed25519 key, public key compiled in); discard anything that fails.
4. Swap in the new binary on the next start; show "Updated to x.y.z" with a "What's new" link.
5. Keep the previous binary; if the new one fails to start, fall back automatically.
6. Opt-out in the settings; a beta channel can come later.

Needs a public repo (release assets of a private repo are not downloadable without a token).
Package managers (winget, Flathub/AppImage, Homebrew) come on top once the project is public.

## Formulas

- **Phase 1:** show the results stored in the file (Excel and LibreOffice save them with every
  formula), the formula itself in the formula bar. No calculation, so opening stays fast.
  Known gaps: volatile functions like `TODAY()` show the value from when the file was saved;
  files written without results (e.g. by Python scripts) have none. For those, the cell shows
  a hint instead of a silent 0 or blank.
- **Phase 2:** editing needs recalculation. Use [IronCalc](https://github.com/ironcalc/IronCalc)
  (Rust spreadsheet engine, MIT/Apache, reads and writes xlsx, several hundred Excel functions)
  instead of writing an engine. Loaded only when editing starts, so viewing stays as fast as now.
  Before committing to it: measure its effect on load time and binary size, check licence and
  function coverage against real files.

## Phase 2 – write

Done in 0.2: edit cells and formulas (IronCalc recalculation, built on the first edit),
undo/redo, bold, italic, text and fill colour, number formats. Save xlsx and csv in place,
other formats as a new `.xlsx`. Values and formulas are kept; other formatting from the file
may be lost.
Done in 0.3: office-style layout (menu bar File/Edit/View/Format, toolbar, formula bar below
it that edits the formula, not its result), right-click menus on cells and headers, sheet tabs
with new/rename/delete (undoable), grid zoom 50–300 %, recent files in *File → Recent* and on
the start screen.
Done in 0.5: paste tab-separated text (Ctrl+V, one undo step, a single value fills the
selection), insert and delete rows and columns with formulas following, and a warning before
overwriting an `.xlsx` whose content would be lost (see below).
Still open: replace, printing/PDF. See [feature parity](feature-parity.md).

### Saving an xlsx without losses

Checked in 0.5 (test `crates/ferrosheet-core/tests/roundtrip.rs`): the engine (IronCalc)
keeps values, formulas, fonts, colours, fills, borders, number formats, merged cells, column
widths, row heights, frozen panes, hyperlinks, conditional formatting and defined names. It
drops what it neither reads nor writes: comments, data validation (drop-down lists),
autofilters, Excel tables, charts/pictures/shapes, pivot tables, page setup and print
settings, and a custom theme (replaced by the default one). ferrosheet lists the first six
before overwriting a file that has them and offers to save a copy instead.

Full fix, not built yet (~2–3 days): after IronCalc writes the file, copy the parts it does
not know from the original (comments + VML, drawings + charts + media, tables, pivot caches,
`dataValidations`, `autoFilter`, `pageSetup`/`printOptions`/`headerFooter`, the theme)
into the new package, with their relationships and content types. Inserted or deleted
rows/columns shift the ranges those parts refer to, so they need the same shifting, or the
carry-over is limited to files whose structure did not change. Better long-term: contribute
the missing parts to IronCalc's importer/exporter upstream.

### Known gaps from 0.5

- Undoing a row/column deletion restores the cells, but formulas elsewhere that referred to
  them stay `#REF!`; IronCalc's undo does not restore them. Fix upstream or keep our own copy
  of the affected formulas.
- Column widths and row heights set by dragging in ferrosheet stay at their position when
  rows/columns are inserted or deleted.
- Paste takes values only (no formatting), and does not tile a copied block over a larger
  selection.
