//! Integration test: needs a PDFium library and a test PDF.
//!
//!   npm run fetch:pdfium && npm run make:testpdf
//!   PDFIUM_LIB_DIR=src-tauri/resources/pdfium \
//!   TEST_PDF=testdata/200-pages.pdf cargo test -p pdf-core -- --nocapture
//!
//! Without those, the test prints a notice and passes, so plain `cargo test`
//! works for contributors who have not fetched PDFium yet.

use std::{path::PathBuf, time::Instant};

use pdf_core::PdfEngine;

#[test]
fn opens_and_renders_every_page() {
    let (Some(lib), Some(pdf)) = (
        std::env::var_os("PDFIUM_LIB_DIR"),
        std::env::var_os("TEST_PDF"),
    ) else {
        eprintln!("skipped: set PDFIUM_LIB_DIR and TEST_PDF to run this test");
        return;
    };

    let engine = PdfEngine::new(&[PathBuf::from(lib)]).expect("bind PDFium");

    let t = Instant::now();
    let info = engine.open(&PathBuf::from(pdf)).expect("open pdf");
    eprintln!("open: {} pages in {:?}", info.page_count, t.elapsed());
    assert!(info.page_count > 0);

    // First page must contain non-white pixels (i.e. something was drawn).
    let first = engine.render(info.id, 0, 800).expect("render page 0");
    assert_eq!(first.width, 800);
    assert_eq!(first.rgba.len(), (first.width * first.height * 4) as usize);
    assert!(
        first.rgba.chunks_exact(4).any(|px| px[0] < 250),
        "page 0 rendered blank"
    );

    let t = Instant::now();
    for i in 0..info.page_count {
        engine.render(info.id, i, 800).expect("render page");
    }
    let total = t.elapsed();
    eprintln!(
        "render all: {:?} total, {:?} per page",
        total,
        total / info.page_count as u32
    );

    assert!(engine.render(info.id, info.page_count, 800).is_err());
    engine.close(info.id);
    assert!(engine.render(info.id, 0, 800).is_err());
}
