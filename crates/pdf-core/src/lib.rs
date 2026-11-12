//! PDF engine for Foarbyld.
//!
//! A thin, UI-independent wrapper around PDFium (via `pdfium-render`).
//! The app shell (Tauri) only calls the functions in this crate, which keeps
//! the engine testable without a webview or any GTK/WebView2 dependency.
//!
//! PDFium is not thread-safe and its handles are not `Send`. All PDFium work
//! therefore runs on one dedicated worker thread; [`PdfEngine`] is a cheap,
//! `Send + Sync` handle that talks to it over a channel.

mod image_doc;
mod text;

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread,
};

use image_doc::{ImageDoc, IMAGE_EXTENSIONS};
use pdfium_render::prelude::*;
use serde::Serialize;

pub use image_doc::IMAGE_EXTENSIONS as SUPPORTED_IMAGE_EXTENSIONS;
pub use text::{find_in_text, PageText, SearchMatch};

/// Hard cap so a one-letter query on a huge document cannot flood the UI.
const MAX_MATCHES: usize = 20_000;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("PDFium library not found ({0}). Run `npm run fetch:pdfium` or set PDFIUM_LIB_DIR.")]
    LibraryNotFound(String),
    #[error("PDFium error: {0}")]
    Pdfium(String),
    #[error("cannot open image: {0}")]
    Image(String),
    #[error("unknown document id {0}")]
    UnknownDocument(u64),
    #[error("page {0} is out of range")]
    PageOutOfRange(usize),
    #[error("the PDF worker thread stopped unexpectedly")]
    WorkerStopped,
}

impl From<PdfiumError> for PdfError {
    fn from(e: PdfiumError) -> Self {
        PdfError::Pdfium(format!("{e:?}"))
    }
}

/// Page size in PDF points (1/72 inch).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PageSize {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DocKind {
    Pdf,
    Image,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    pub id: u64,
    pub kind: DocKind,
    pub page_count: usize,
    pub pages: Vec<PageSize>,
}

/// A rendered page as tightly packed RGBA8 pixels.
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

enum Request {
    Open {
        path: PathBuf,
        reply: Sender<Result<DocInfo, PdfError>>,
    },
    Render {
        id: u64,
        page: usize,
        width: u32,
        reply: Sender<Result<RenderedPage, PdfError>>,
    },
    Text {
        id: u64,
        page: usize,
        reply: Sender<Result<Arc<PageText>, PdfError>>,
    },
    Search {
        id: u64,
        query: String,
        case_sensitive: bool,
        reply: Sender<Result<Vec<SearchMatch>, PdfError>>,
    },
    Close {
        id: u64,
    },
}

/// Handle to the PDF worker thread. Dropping it stops the worker.
pub struct PdfEngine {
    tx: Sender<Request>,
}

impl PdfEngine {
    /// Start the worker and bind to PDFium, looking in each directory of
    /// `search_dirs` first and falling back to the system library path.
    pub fn new(search_dirs: &[PathBuf]) -> Result<Self, PdfError> {
        let dirs = search_dirs.to_vec();
        let (tx, rx) = mpsc::channel::<Request>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), PdfError>>();

        thread::Builder::new()
            .name("pdfium".into())
            .spawn(move || {
                // Bindings are not Send, so they must be created on this thread.
                match bind(&dirs) {
                    Ok(pdfium) => {
                        let _ = ready_tx.send(Ok(()));
                        worker(&pdfium, rx);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| PdfError::Pdfium(e.to_string()))?;

        ready_rx.recv().map_err(|_| PdfError::WorkerStopped)??;
        Ok(Self { tx })
    }

    pub fn open(&self, path: &Path) -> Result<DocInfo, PdfError> {
        let path = path.to_path_buf();
        self.call(|reply| Request::Open { path, reply })
    }

    /// Render one page so that the bitmap is `width_px` wide.
    ///
    /// Requests are processed one at a time. Parallel rendering would need
    /// multiple PDFium instances (processes); not needed for the prototype.
    pub fn render(&self, id: u64, page: usize, width_px: u32) -> Result<RenderedPage, PdfError> {
        self.call(|reply| Request::Render {
            id,
            page,
            width: width_px,
            reply,
        })
    }

    /// Characters and their boxes for one page (empty for images).
    pub fn page_text(&self, id: u64, page: usize) -> Result<Arc<PageText>, PdfError> {
        self.call(|reply| Request::Text { id, page, reply })
    }

    /// Search the whole document. Blocks the worker (and so rendering) until
    /// every page's text has been extracted; extracted text is cached.
    pub fn search(&self, id: u64, query: &str, case_sensitive: bool) -> Result<Vec<SearchMatch>, PdfError> {
        let query = query.to_owned();
        self.call(|reply| Request::Search { id, query, case_sensitive, reply })
    }

    pub fn close(&self, id: u64) {
        let _ = self.tx.send(Request::Close { id });
    }

    fn call<T>(
        &self,
        make: impl FnOnce(Sender<Result<T, PdfError>>) -> Request,
    ) -> Result<T, PdfError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(make(reply_tx))
            .map_err(|_| PdfError::WorkerStopped)?;
        reply_rx.recv().map_err(|_| PdfError::WorkerStopped)?
    }
}

fn bind(search_dirs: &[PathBuf]) -> Result<Pdfium, PdfError> {
    for dir in search_dirs {
        let name = Pdfium::pdfium_platform_library_name_at_path(dir);
        if let Ok(bindings) = Pdfium::bind_to_library(name) {
            return Ok(Pdfium::new(bindings));
        }
    }
    Pdfium::bind_to_system_library()
        .map(Pdfium::new)
        .map_err(|e| PdfError::LibraryNotFound(format!("{e:?}")))
}

enum Doc<'a> {
    Pdf(PdfDocument<'a>),
    Image(ImageDoc),
}

struct Worker<'a> {
    pdfium: &'a Pdfium,
    docs: HashMap<u64, Doc<'a>>,
    texts: HashMap<(u64, usize), Arc<PageText>>,
    next_id: u64,
}

fn worker(pdfium: &Pdfium, rx: Receiver<Request>) {
    let mut w = Worker { pdfium, docs: HashMap::new(), texts: HashMap::new(), next_id: 1 };
    for request in rx {
        match request {
            Request::Open { path, reply } => {
                let _ = reply.send(w.open(&path));
            }
            Request::Render { id, page, width, reply } => {
                let _ = reply.send(w.render(id, page, width));
            }
            Request::Text { id, page, reply } => {
                let _ = reply.send(w.text(id, page));
            }
            Request::Search { id, query, case_sensitive, reply } => {
                let _ = reply.send(w.search(id, &query, case_sensitive));
            }
            Request::Close { id } => {
                w.docs.remove(&id);
                w.texts.retain(|(doc, _), _| *doc != id);
            }
        }
    }
}

impl<'a> Worker<'a> {
    fn open(&mut self, path: &Path) -> Result<DocInfo, PdfError> {
        let (doc, kind, pages) = if is_pdf(path) {
            let (doc, pages) = open_pdf(self.pdfium, path)?;
            (Doc::Pdf(doc), DocKind::Pdf, pages)
        } else {
            let img = ImageDoc::open(path)?;
            let size = img.page_size();
            (Doc::Image(img), DocKind::Image, vec![size])
        };
        let id = self.next_id;
        self.next_id += 1;
        self.docs.insert(id, doc);
        Ok(DocInfo { id, kind, page_count: pages.len(), pages })
    }

    fn render(&self, id: u64, page: usize, width: u32) -> Result<RenderedPage, PdfError> {
        match self.docs.get(&id) {
            Some(Doc::Pdf(doc)) => render_page(doc, page, width),
            Some(Doc::Image(img)) if page == 0 => Ok(img.render(width)),
            Some(Doc::Image(_)) => Err(PdfError::PageOutOfRange(page)),
            None => Err(PdfError::UnknownDocument(id)),
        }
    }

    fn text(&mut self, id: u64, page: usize) -> Result<Arc<PageText>, PdfError> {
        if let Some(t) = self.texts.get(&(id, page)) {
            return Ok(t.clone());
        }
        let text = match self.docs.get(&id) {
            Some(Doc::Pdf(doc)) => extract_text(doc, page)?,
            Some(Doc::Image(_)) if page == 0 => PageText::default(),
            Some(Doc::Image(_)) => return Err(PdfError::PageOutOfRange(page)),
            None => return Err(PdfError::UnknownDocument(id)),
        };
        let text = Arc::new(text);
        self.texts.insert((id, page), text.clone());
        Ok(text)
    }

    fn search(&mut self, id: u64, query: &str, case_sensitive: bool) -> Result<Vec<SearchMatch>, PdfError> {
        let page_count = match self.docs.get(&id) {
            Some(Doc::Pdf(doc)) => usize::from(doc.pages().len()),
            Some(Doc::Image(_)) => return Ok(Vec::new()),
            None => return Err(PdfError::UnknownDocument(id)),
        };
        let mut out = Vec::new();
        for page in 0..page_count {
            let text = self.text(id, page)?;
            for (start, len) in find_in_text(&text.text, query, case_sensitive) {
                out.push(SearchMatch { page, start, len });
                if out.len() >= MAX_MATCHES {
                    return Ok(out);
                }
            }
        }
        Ok(out)
    }
}

/// PDF by extension, or by the `%PDF` magic when the extension says nothing.
fn is_pdf(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("pdf") => true,
        Some(e) if IMAGE_EXTENSIONS.contains(&e) => false,
        _ => {
            let mut magic = [0u8; 4];
            std::fs::File::open(path)
                .and_then(|mut f| f.read_exact(&mut magic))
                .map(|_| &magic == b"%PDF")
                .unwrap_or(false)
        }
    }
}

fn open_pdf<'a>(
    pdfium: &'a Pdfium,
    path: &Path,
) -> Result<(PdfDocument<'a>, Vec<PageSize>), PdfError> {
    let doc = pdfium.load_pdf_from_file(path, None)?;
    let pages = doc
        .pages()
        .iter()
        .map(|p| PageSize {
            width: p.width().value,
            height: p.height().value,
        })
        .collect();
    Ok((doc, pages))
}

fn get_page<'a>(doc: &'a PdfDocument<'_>, page_index: usize) -> Result<PdfPage<'a>, PdfError> {
    let index = u16::try_from(page_index).map_err(|_| PdfError::PageOutOfRange(page_index))?;
    if usize::from(index) >= usize::from(doc.pages().len()) {
        return Err(PdfError::PageOutOfRange(page_index));
    }
    Ok(doc.pages().get(index)?)
}

fn render_page(doc: &PdfDocument<'_>, page_index: usize, width_px: u32) -> Result<RenderedPage, PdfError> {
    let page = get_page(doc, page_index)?;
    let config = PdfRenderConfig::new().set_target_width(width_px.max(1) as Pixels);
    let bitmap = page.render_with_config(&config)?;
    Ok(RenderedPage {
        width: bitmap.width() as u32,
        height: bitmap.height() as u32,
        rgba: bitmap.as_rgba_bytes(),
    })
}

/// Map a rectangle from PDF user space (origin bottom-left, y up, unrotated)
/// to the displayed page (origin top-left, y down, page rotation applied).
/// `w`/`h` are the *unrotated* page size; `rot` is the clockwise rotation in degrees.
fn to_display(left: f32, bottom: f32, right: f32, top: f32, w: f32, h: f32, rot: u16) -> [f32; 4] {
    // Unrotated top-left space.
    let (x0, y0, x1, y1) = (left, h - top, right, h - bottom);
    let (a, b, c, d) = match rot {
        90 => (h - y1, x0, h - y0, x1),
        180 => (w - x1, h - y1, w - x0, h - y0),
        270 => (y0, w - x1, y1, w - x0),
        _ => (x0, y0, x1, y1),
    };
    [a.min(c), b.min(d), (c - a).abs(), (d - b).abs()]
}

fn extract_text(doc: &PdfDocument<'_>, page_index: usize) -> Result<PageText, PdfError> {
    let page = get_page(doc, page_index)?;
    let rot: u16 = match page.rotation() {
        Ok(PdfPageRenderRotation::Degrees90) => 90,
        Ok(PdfPageRenderRotation::Degrees180) => 180,
        Ok(PdfPageRenderRotation::Degrees270) => 270,
        _ => 0,
    };
    // width()/height() are of the displayed page; swap back for 90/270.
    let (dw, dh) = (page.width().value, page.height().value);
    let (w, h) = if rot == 90 || rot == 270 { (dh, dw) } else { (dw, dh) };

    let page_text = page.text()?;
    let mut text = String::new();
    let mut boxes = Vec::new();
    for ch in page_text.chars().iter() {
        text.push(ch.unicode_char().unwrap_or(' '));
        let b = match (ch.is_generated(), ch.loose_bounds()) {
            (Ok(false), Ok(r)) => to_display(
                r.left().value,
                r.bottom().value,
                r.right().value,
                r.top().value,
                w,
                h,
                rot,
            ),
            _ => [0.0; 4],
        };
        boxes.extend_from_slice(&b);
    }
    Ok(PageText { text, boxes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PdfEngine>();
    }

    #[test]
    fn display_mapping_handles_rotation() {
        // Unrotated 100x200 page, box at bottom-left corner: x 10..30, y 20..50 (y up).
        let r = |rot| to_display(10.0, 20.0, 30.0, 50.0, 100.0, 200.0, rot);
        assert_eq!(r(0), [10.0, 150.0, 20.0, 30.0]);
        assert_eq!(r(180), [70.0, 20.0, 20.0, 30.0]);
        // 90 cw: displayed page is 200 wide, 100 tall.
        assert_eq!(r(90), [20.0, 10.0, 30.0, 20.0]);
        assert_eq!(r(270), [150.0, 70.0, 30.0, 20.0]);
    }

    #[test]
    fn pdf_detection_by_extension_and_magic() {
        assert!(is_pdf(Path::new("a.PDF")));
        assert!(!is_pdf(Path::new("a.png")));
        assert!(!is_pdf(Path::new("/nonexistent/noext")));
    }
}
