//! PDF engine for Folio.
//!
//! A thin, UI-independent wrapper around PDFium (via `pdfium-render`).
//! The app shell (Tauri) only calls the functions in this crate, which keeps
//! the engine testable without a webview or any GTK/WebView2 dependency.
//!
//! PDFium is not thread-safe and its handles are not `Send`. All PDFium work
//! therefore runs on one dedicated worker thread; [`PdfEngine`] is a cheap,
//! `Send + Sync` handle that talks to it over a channel.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use pdfium_render::prelude::*;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("PDFium library not found ({0}). Run `npm run fetch:pdfium` or set PDFIUM_LIB_DIR.")]
    LibraryNotFound(String),
    #[error("PDFium error: {0}")]
    Pdfium(String),
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocInfo {
    pub id: u64,
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

fn worker(pdfium: &Pdfium, rx: Receiver<Request>) {
    let mut docs: HashMap<u64, PdfDocument<'_>> = HashMap::new();
    let mut next_id = 1u64;

    for request in rx {
        match request {
            Request::Open { path, reply } => {
                let result = open_doc(pdfium, &path).map(|(doc, pages)| {
                    let id = next_id;
                    next_id += 1;
                    docs.insert(id, doc);
                    DocInfo {
                        id,
                        page_count: pages.len(),
                        pages,
                    }
                });
                let _ = reply.send(result);
            }
            Request::Render {
                id,
                page,
                width,
                reply,
            } => {
                let result = match docs.get(&id) {
                    Some(doc) => render_page(doc, page, width),
                    None => Err(PdfError::UnknownDocument(id)),
                };
                let _ = reply.send(result);
            }
            Request::Close { id } => {
                docs.remove(&id);
            }
        }
    }
}

fn open_doc<'a>(
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

fn render_page(doc: &PdfDocument<'_>, page_index: usize, width_px: u32) -> Result<RenderedPage, PdfError> {
    let index = u16::try_from(page_index).map_err(|_| PdfError::PageOutOfRange(page_index))?;
    if usize::from(index) >= usize::from(doc.pages().len()) {
        return Err(PdfError::PageOutOfRange(page_index));
    }
    let page = doc.pages().get(index)?;
    let config = PdfRenderConfig::new().set_target_width(width_px.max(1) as Pixels);
    let bitmap = page.render_with_config(&config)?;
    Ok(RenderedPage {
        width: bitmap.width() as u32,
        height: bitmap.height() as u32,
        rgba: bitmap.as_rgba_bytes(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PdfEngine>();
    }
}
