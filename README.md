# Folio (working title)

An open-source document and image previewer for Windows and Linux, in the
spirit of macOS Preview. This repository is the **prototype that decides
[ADR 0001](docs/adr/0001-tauri-as-application-shell.md)**: can Tauri + a
PDFium-backed renderer scroll and zoom a large PDF smoothly on WebKitGTK?

Stack: Tauri 2 · Vue 3 + TypeScript · Rust · PDFium. See [docs/adr](docs/adr).

> "Folio" is a placeholder. Rename in `package.json`, `src-tauri/Cargo.toml`,
> `src-tauri/tauri.conf.json` (name, identifier) and `index.html`.

## Layout

```
crates/pdf-core/   PDFium engine on a worker thread, no UI dependencies (unit-testable)
src-tauri/         Tauri shell: commands open_pdf / render_page / close_pdf
src/               Vue UI: virtual-scrolling viewer, zoom, drag & drop, benchmark
scripts/           fetch-pdfium.mjs (pinned + SHA-256 verified), make-test-pdf.mjs
docs/adr/          Architecture decision records
```

Pages are rendered by PDFium in Rust and sent to the webview as raw RGBA bytes
(`[w u32][h u32][pixels]`, no JSON/base64), then drawn on a `<canvas>`.

## Develop

Prerequisites: Node 22+, Rust (stable), and on Linux:

```sh
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev \
  libayatana-appindicator3-dev libssl-dev patchelf build-essential
```

```sh
npm install
npm run fetch:pdfium        # downloads the pinned PDFium into src-tauri/resources/pdfium
npm run make:testpdf        # creates testdata/200-pages.pdf
npm run tauri dev
```

Engine tests (real PDFium, skipped when the env vars are not set):

```sh
PDFIUM_LIB_DIR=$PWD/src-tauri/resources/pdfium \
TEST_PDF=$PWD/testdata/200-pages.pdf \
cargo test -p pdf-core -- --nocapture
```

## The performance gate (ADR 0001)

On each target OS (Windows 11, Ubuntu LTS, Fedora, ideally also on a HiDPI
screen and on an arm64 device):

1. `npm run tauri build` (or `tauri dev --release`), open `testdata/200-pages.pdf`.
2. Click **Benchmark**. It scrolls at 4000 px/s, then zooms 100% → 200%.
3. Paste the JSON into the ADR (or an issue).

Suggested pass criteria: first page < 500 ms, frame time p95 < 25 ms, slow
frames (> 50 ms) < 1%, zoom settle < 1 s. If Linux fails these, supersede
ADR 0001 with Electron or Qt.

## Known limitations of the prototype

- PDF only (no images yet), no text selection, search or annotations.
- Rendering is serialised on one PDFium thread; bitmaps are converted BGRA → RGBA on the CPU.
- The page cache is limited by bytes (256 MB) but not yet by document count.
- Placeholder icon and app id (`org.example.folio`).

## License

Apache-2.0 (see ADR 0003). Bundled PDFium is BSD-3-Clause / Apache-2.0; its
license ships as `PDFIUM-LICENSE` next to the library.
