# ADR 0002: PDFium as the PDF engine

- Status: Proposed
- Date: 2026-11-12

## Context

The core feature is fast, accurate PDF rendering, plus text selection,
search, annotations, form fields, page rotation, reordering and merging.
The engine must work on Windows and Linux (x64 and arm64), be maintained, and
have a license compatible with an open-source project that may want a
permissive license (see ADR 0003).

## Decision

Use **PDFium** (BSD-3-Clause / Apache-2.0), accessed from Rust through the
`pdfium-render` crate, using prebuilt PDFium binaries per platform. Rendered
pages are sent to the webview as bitmaps or tiles. Structural edits that
PDFium does not cover well (merge, split, reorder, metadata) may use `lopdf`.

## Alternatives considered

- **pdf.js** (Apache-2.0): runs in the webview, no native dependency. Rejected
  as the main engine because rendering performance and fidelity depend on the
  webview (WebKitGTK is the weak spot) and editing support is limited.
- **MuPDF** (AGPL / commercial): fast, accurate and feature-rich. Rejected
  because it would force the whole project to AGPL (or require a commercial
  license). Revisit if we decide to go AGPL anyway (see ADR 0003).
- **Poppler** (GPL): common on Linux, but weak on Windows and GPL-only.

## Consequences

- Chrome-grade rendering fidelity and good performance.
- We must bundle and update PDFium binaries per platform and architecture.
  Prebuilt binaries are available from the `pdfium-binaries` project; the
  build must pin versions and verify checksums.
- Annotation writing through PDFium is workable but limited; complex
  annotation types may need extra work in Rust.
- A native dependency adds packaging work for Flatpak, AppImage and Windows
  installers.
