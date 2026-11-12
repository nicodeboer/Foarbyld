# ADR 0001: Tauri 2 as the application shell

- Status: Proposed
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

Before committing further, build a prototype that opens a 200-page PDF and
checks on Windows 11 and on Ubuntu LTS and Fedora (WebKitGTK):

- Smooth scrolling and zooming (target: no visible stutter at 60 Hz).
- Time to first page under 500 ms for a typical 5 MB PDF.
- Memory use stays reasonable while scrolling the whole document.

If Linux performance is unacceptable, supersede this ADR with Electron or Qt.
