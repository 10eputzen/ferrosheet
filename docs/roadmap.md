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

## Phase 2 – write

Edit cells, save as `.xlsx` only, formatting is not preserved. See [feature parity](feature-parity.md).
