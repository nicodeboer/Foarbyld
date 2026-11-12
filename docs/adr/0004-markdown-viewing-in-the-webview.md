# ADR 0004: Markdown viewing in the webview

- Status: Accepted
- Date: 2026-11-12

## Context

We want Foarbyld to open Markdown files (`.md`, `.markdown`) as well as PDFs
and images. The PDF and image path (ADR 0002) renders pages to bitmaps in Rust
and draws them on a `<canvas>`, with a custom text layer for selection and
search. That makes sense for fixed-layout pages, but Markdown is flowing text:
it should reflow with the window, scale with zoom and support native text
selection, copy and links.

## Decision

Render Markdown to HTML in Rust and display it in the webview as real DOM.

- New crate `crates/md-core` (no UI dependencies, unit tested):
  `pulldown-cmark` parses (tables, footnotes, strikethrough, task lists, smart
  punctuation) and `ammonia` sanitizes the HTML.
- A Tauri command `open_markdown(path)` returns the sanitized HTML. It does not
  use the PDFium worker, so Markdown works even if PDFium failed to load.
- `MarkdownViewer.vue` shows the HTML. Zoom scales the font size; selection and
  copy are native. Find uses `window.find`, which WKWebView, WebKitGTK and
  WebView2 all implement.
- Security: raw HTML in the Markdown is sanitized (no scripts or event
  handlers). Local images are inlined as `data:` URIs (png, jpg, gif, webp,
  bmp, svg, at most 20 MB) because the CSP only allows `'self'` and `data:`.
  Remote images become plain links, so a document cannot make the viewer
  request anything. Link clicks never navigate the webview: `http(s)` and
  `mailto` open in the system browser through `tauri-plugin-opener`.
- `md` and `markdown` are registered as file associations.

## Alternatives considered

- **Parse in the frontend (markdown-it, marked)**: one less Rust crate, but
  sanitizing would then depend on a JS library and the DOM, and file reading
  and image inlining would need extra Tauri permissions.
- **Render to PDF and reuse the PDFium viewer**: one code path, but no reflow,
  poor text for small windows, and a pagination step we do not need.
- **A DOM text layer for selection in a shared viewer**: rejected, the custom
  selection code is specific to bitmap pages.

## Consequences

- Two viewers exist side by side; `App.vue` picks one per file. Thumbnails,
  page info and the PDF find bar do not apply to Markdown.
- Find in Markdown selects the match in the page, which moves focus out of the
  find field. Enter and Shift+Enter keep working from the page; there is no
  match count.
- Not supported yet: syntax highlighting in code blocks, heading anchors
  (`#section` links), relative links to other files, relative images inside raw
  HTML blocks, math and diagrams, live reload on file change.
- New dependencies, all MIT or Apache-2.0: `pulldown-cmark`, `ammonia`
  (with `html5ever`), `base64`, `tauri-plugin-opener`.
