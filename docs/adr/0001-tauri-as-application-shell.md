# ADR 0001: Tauri 2 as the application shell

- Status: Accepted
- Date: 2026-11-12

## Context

We are building an open-source document and image previewer (PDF and common
image formats, with annotation, page management and signing) that runs on
Windows and Linux. The UI team knows TypeScript and Vue. Binaries should be
small and start fast. The app must integrate with the OS (file associations,
drag and drop, sandboxed file access on Flatpak).

## Decision

Use **Tauri 2** with a **Vue 3 + TypeScript** frontend and a **Rust** core.
Heavy work (PDF rendering, image decoding, PDF editing) runs in Rust. The
webview only displays bitmaps/tiles and handles UI and interaction.

## Alternatives considered

- **Electron**: consistent Chromium rendering on every platform, large
  ecosystem. Rejected for now because of size (about 100 MB) and memory use.
  This is the fallback if the prototype fails (see below).
- **Qt 6 (QML or PySide)**: best native feel, LGPL, QtPdf is built on PDFium.
  Rejected because it moves the UI away from the Vue/TypeScript skills in the
  team and makes web-style contributions harder.
- **Flutter desktop**: good rendering control, but a weaker Linux integration
  story and another language (Dart) for contributors.

## Consequences

- Small binaries and low memory use.
- Rust is required for the core; contributors need basic Rust.
- On Linux, Tauri uses **WebKitGTK**, which is slower and less predictable
  than Chromium for canvas-heavy work (zoom, scroll, annotation overlays).
  This is the main risk.

## Validation (prototype gate)

The original plan was to validate the choice before committing further: a
prototype opening a 200-page PDF on Windows 11 and on Ubuntu LTS and Fedora
(WebKitGTK), judged on smooth scrolling and zooming, time to first page under
500 ms, and reasonable memory use. The prototype included a Benchmark button
for this.

**Update (2026-11-12):** the owner dropped the gate. Linux performance is not a
priority for this project, and the owner has seen the prototype work well
enough, so the numbers will not be collected. The Benchmark button and its
code were removed and this ADR is accepted without formal measurements. If
WebKitGTK performance turns out to be a problem in practice, supersede this
ADR with Electron or Qt.
