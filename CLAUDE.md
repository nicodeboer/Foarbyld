# Foarbyld

Open-source, cross-platform document and image previewer (think macOS Preview) for
Windows and Linux (also built for macOS). Apache-2.0. Currently a prototype. ADR 0001
(Tauri) is accepted without a Linux performance gate: the owner dropped it and the
Benchmark code was removed. Read `docs/adr/` before changing anything architectural.

## Stack

Tauri 2 shell · Vue 3 + TypeScript (Vite) · Rust core · PDFium via `pdfium-render`.

```
crates/pdf-core/   PDFium engine on its own worker thread; no UI deps; unit/integration tested
crates/md-core/    Markdown -> sanitized HTML (pulldown-cmark + ammonia); no UI deps; unit tested (`cargo test -p md-core`)
src-tauri/         Tauri commands (open_document, open_markdown, render_page, page_text, search_text, close_pdf, engine_error, initial_file)
src/               Vue UI: App.vue (toolbar, find bar), components/PdfViewer.vue (virtual scroll, selection, highlights), components/MarkdownViewer.vue, components/ThumbnailSidebar.vue, lib/api.ts, lib/textLayer.ts (pure selection/hit-test helpers, vitest)
scripts/           fetch-pdfium.mjs (pinned version + SHA-256), make-test-pdf.mjs
docs/adr/          Architecture decision records (0001 Tauri, 0002 PDFium, 0003 Apache-2.0, 0004 Markdown)
```

## Commands

```sh
npm install
npm run fetch:pdfium       # pinned PDFium -> src-tauri/resources/pdfium (not in git)
npm run make:testpdf       # testdata/200-pages.pdf (not in git)
npm run tauri dev          # run the app
npm run typecheck          # vue-tsc
npm test                   # vitest (src/lib/*.test.ts)
npm run build              # typecheck + vite build

# Engine tests against real PDFium (skipped without the env vars)
PDFIUM_LIB_DIR=$PWD/src-tauri/resources/pdfium TEST_PDF=$PWD/testdata/200-pages.pdf \
  cargo test -p pdf-core -- --nocapture
```

Linux needs: `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libssl-dev patchelf`.

## Architecture rules and gotchas

- **PDFium is single-threaded and its handles are not `Send`.** All PDFium work runs on one
  worker thread in `pdf-core`; `PdfEngine` is a `Send + Sync` channel handle. Do not hold
  `PdfDocument` outside that thread.
- **Tauri sync commands run on the main thread.** Anything that calls PDFium must be
  `#[tauri::command(async)]`, or the UI freezes while rendering.
- **Render IPC is raw bytes**, not JSON: `[width u32 LE][height u32 LE][RGBA pixels]`
  (`render_page` in `src-tauri/src/lib.rs`, parsed in `src/lib/api.ts`). Keep both sides in sync.
- If PDFium is missing the app still starts; the UI shows the error from `engine_error`.
  Lookup order: `PDFIUM_LIB_DIR`, bundled resources dir, `src-tauri/resources/pdfium`, system.
- **Images** (`image_doc.rs`) are one-page documents in the same worker/id space as PDFs: 1 image px = 0.75 pt,
  so 100% zoom = 1:1. `DocInfo.kind` is `pdf` | `image`. Decoders are pure-Rust `image` features only (keep it that way for licensing/portability).
- **Text**: `page_text` returns one char + one box per PDFium char (`PageText`, boxes in top-left pt of the *displayed* page, rotation applied).
  Char index is a code point index (JS: `Array.from`), valid for both selection and search (`SearchMatch.start/len`). Search runs in Rust over cached page text.
  Selection is custom (no DOM text layer): hit-test, drag, highlight and copy all live in `PdfViewer.vue` + `lib/textLayer.ts`.
- Integration tests share one `PdfEngine` per process (`OnceLock`): PDFium cannot be initialised twice concurrently. On Apple Silicon with an x86_64 Rust toolchain
  add `--target aarch64-apple-darwin` (the fetched dylib is arm64).
- Viewer (`PdfViewer.vue`): only visible pages (+ overscan) have canvases; decoded bitmaps are
  cached by byte budget (256 MB). Canvas intrinsic size is set at draw time so a zoom shows the
  old bitmap scaled until the re-render lands.
- Bumping PDFium: change `TAG` and all SHA-256 values together in `scripts/fetch-pdfium.mjs`.
  Linux/Windows (x64, arm64) are release targets; macOS arm64 is released as an ad-hoc signed, not notarized .dmg (`release.yml`); macOS x64 is development only.
- **Markdown** (ADR 0004) is rendered in Rust (`md-core`) and shown as DOM in `MarkdownViewer.vue`, separate from the
  PDFium engine and its id space (`App.vue` holds `markdown` XOR `doc`). Output is sanitized by ammonia; local images are
  inlined as `data:` URIs, remote images become links (CSP is `'self' data:`). Do not loosen this without updating the ADR.
  Find uses `window.find`, so it works from Enter/Shift+Enter, not from the find field.
- Dependencies must stay Apache-2.0-compatible: no GPL/AGPL (this is why MuPDF was rejected, ADR 0002).

## Conventions

- Code, comments, commit messages and docs in English. Chat with the owner can be Dutch.
- Architectural decisions go in `docs/adr/NNNN-title.md` (status, context, decision,
  alternatives, consequences); never edit an accepted ADR, supersede it with a new one.
- App id is `io.github.nicodeboer.foarbyld`; keep it stable after the first release. Icons are placeholders.

## Status / next steps

Done: PDF open/render/zoom/scroll, drag & drop, Markdown viewer, images (image-rs), text selection + copy, find (Ctrl/Cmd+F), thumbnail sidebar, CI workflow (Ubuntu + Windows; not yet run).
Not yet: selection/find UI and the Markdown viewer never exercised in the real webview on Linux (Markdown find via `window.find` is untested on every platform); annotations; page reorder/merge; signing; packaging (Flatpak, winget, code signing).
