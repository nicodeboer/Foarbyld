//! Integration test: needs a PDFium library and a test PDF.
//!
//!   npm run fetch:pdfium && npm run make:testpdf
//!   PDFIUM_LIB_DIR=src-tauri/resources/pdfium \
//!   TEST_PDF=testdata/200-pages.pdf cargo test -p pdf-core -- --nocapture
//!
//! Without those, the test prints a notice and passes, so plain `cargo test`
//! works for contributors who have not fetched PDFium yet.

use std::{path::PathBuf, sync::OnceLock, time::Instant};

use pdf_core::PdfEngine;

/// PDFium must be initialised once per process, so all tests share one engine.
fn shared_engine() -> Option<&'static PdfEngine> {
    static ENGINE: OnceLock<Option<PdfEngine>> = OnceLock::new();
    ENGINE
        .get_or_init(|| {
            let lib = std::env::var_os("PDFIUM_LIB_DIR")?;
            Some(PdfEngine::new(&[PathBuf::from(lib)]).expect("bind PDFium"))
        })
        .as_ref()
}

#[test]
fn opens_and_renders_every_page() {
    let (Some(engine), Some(pdf)) = (shared_engine(), std::env::var_os("TEST_PDF")) else {
        eprintln!("skipped: set PDFIUM_LIB_DIR and TEST_PDF to run this test");
        return;
    };

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

fn engine_and_pdf() -> Option<(&'static PdfEngine, PathBuf)> {
    Some((shared_engine()?, PathBuf::from(std::env::var_os("TEST_PDF")?)))
}

#[test]
fn extracts_text_with_boxes_and_searches() {
    let Some((engine, pdf)) = engine_and_pdf() else {
        eprintln!("skipped: set PDFIUM_LIB_DIR and TEST_PDF to run this test");
        return;
    };
    let info = engine.open(&pdf).unwrap();
    assert_eq!(info.kind, pdf_core::DocKind::Pdf);

    let t = engine.page_text(info.id, 0).unwrap();
    let n = t.text.chars().count();
    assert!(t.text.contains("Folio test page 1 of"), "got: {:?}", &t.text[..60.min(t.text.len())]);
    assert_eq!(t.boxes.len(), n * 4, "one box per character");

    // The title line is the first thing on the page: its glyph boxes lie inside the page.
    let page = info.pages[0];
    let first = &t.boxes[..4];
    assert!(first[2] > 0.0 && first[3] > 0.0);
    assert!(first[0] >= 0.0 && first[0] + first[2] <= page.width);
    assert!(first[1] >= 0.0 && first[1] + first[3] <= page.height);
    // Title (18 pt) is above the body text (9 pt): top-left origin means smaller y.
    let body_start = t.text.find("Line 1:").unwrap();
    let body_idx = t.text[..body_start].chars().count();
    assert!(first[1] < t.boxes[body_idx * 4 + 1]);

    // "page 7." appears 60 times on page 7 only in the body, plus the title "page 7 of".
    let hits = engine.search(info.id, "FOLIO TEST PAGE 7 OF", false).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 6);
    assert_eq!(hits[0].len, "Folio test page 7 of".len());
    assert!(engine.search(info.id, "FOLIO TEST PAGE 7 OF", true).unwrap().is_empty());

    let all = engine.search(info.id, "quick brown fox", false).unwrap();
    assert_eq!(all.len(), info.page_count * 60);

    assert!(engine.page_text(info.id, info.page_count).is_err());
    engine.close(info.id);
    assert!(engine.search(info.id, "x", false).is_err());
}

#[test]
fn opens_and_renders_images() {
    let Some(engine) = shared_engine() else {
        eprintln!("skipped: set PDFIUM_LIB_DIR to run this test");
        return;
    };
    let dir = std::env::temp_dir().join(format!("folio-it-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("pic.png");
    image::RgbaImage::from_pixel(320, 160, image::Rgba([200, 30, 30, 255]))
        .save(&path)
        .unwrap();

    let info = engine.open(&path).unwrap();
    assert_eq!(info.kind, pdf_core::DocKind::Image);
    assert_eq!(info.page_count, 1);
    assert_eq!((info.pages[0].width, info.pages[0].height), (240.0, 120.0));
    let r = engine.render(info.id, 0, 160).unwrap();
    assert_eq!((r.width, r.height), (160, 80));
    assert_eq!(&r.rgba[..4], &[200, 30, 30, 255]);
    assert!(engine.render(info.id, 1, 160).is_err());
    assert!(engine.page_text(info.id, 0).unwrap().text.is_empty());
    assert!(engine.search(info.id, "x", false).unwrap().is_empty());

    // Garbage with an image extension is an error, not a crash.
    let bad = dir.join("bad.png");
    std::fs::write(&bad, b"not an image").unwrap();
    assert!(engine.open(&bad).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opens_every_supported_image_format() {
    let Some(engine) = shared_engine() else {
        eprintln!("skipped: set PDFIUM_LIB_DIR to run this test");
        return;
    };
    let dir = std::env::temp_dir().join(format!("folio-fmt-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let img = image::RgbImage::from_pixel(64, 32, image::Rgb([20, 120, 220]));
    for ext in pdf_core::SUPPORTED_IMAGE_EXTENSIONS {
        let path = dir.join(format!("pic.{ext}"));
        // Encoders for some formats need specific pixel layouts; RGB8 is accepted by all of these.
        if img.save(&path).is_err() {
            eprintln!("note: cannot encode {ext} in test, skipped");
            continue;
        }
        let info = engine.open(&path).unwrap_or_else(|e| panic!("open .{ext}: {e}"));
        assert_eq!(info.kind, pdf_core::DocKind::Image, ".{ext}");
        let r = engine.render(info.id, 0, 64).unwrap();
        assert_eq!((r.width, r.height), (64, 32), ".{ext}");
    }
    std::fs::remove_dir_all(&dir).ok();
}
