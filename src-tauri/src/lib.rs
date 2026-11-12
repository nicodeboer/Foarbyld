use std::path::{Path, PathBuf};

use std::sync::Arc;

use pdf_core::{DocInfo, PageText, PdfEngine, SearchMatch};
use tauri::{ipc::Response, Manager, State};

/// App state. The engine is a `Result` so the UI can show a helpful message
/// when PDFium is missing instead of the whole app failing to start.
struct AppState {
    engine: Result<PdfEngine, String>,
    initial_file: Option<String>,
}

impl AppState {
    fn engine(&self) -> Result<&PdfEngine, String> {
        self.engine.as_ref().map_err(|e| e.clone())
    }
}

/// Where to look for the PDFium shared library, in priority order.
fn pdfium_search_dirs(app: &tauri::App) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("PDFIUM_LIB_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Ok(res) = app.path().resource_dir() {
        dirs.push(res.join("resources").join("pdfium"));
        dirs.push(res.join("pdfium"));
    }
    // `tauri dev`: resources live next to Cargo.toml.
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join("pdfium"));
    dirs
}

/// First command-line argument that points to an existing file
/// (used for "Open with" / file associations).
fn initial_file_from_args() -> Option<String> {
    std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-') && Path::new(a).is_file())
}

// NOTE: `command(async)` runs on the thread pool. Plain sync commands run on
// the main thread in Tauri 2 and would freeze the UI while PDFium renders.

/// Opens a PDF or a raster image; the engine decides by extension / magic bytes.
#[tauri::command(async)]
fn open_document(state: State<'_, AppState>, path: String) -> Result<DocInfo, String> {
    state.engine()?.open(Path::new(&path)).map_err(|e| e.to_string())
}

/// Returns raw bytes: `[width: u32 LE][height: u32 LE][RGBA pixels...]`.
/// Raw binary responses avoid JSON/base64 overhead on the IPC channel.
#[tauri::command(async)]
fn render_page(
    state: State<'_, AppState>,
    doc: u64,
    page: usize,
    width: u32,
) -> Result<Response, String> {
    let r = state.engine()?.render(doc, page, width).map_err(|e| e.to_string())?;
    let mut buf = Vec::with_capacity(8 + r.rgba.len());
    buf.extend_from_slice(&r.width.to_le_bytes());
    buf.extend_from_slice(&r.height.to_le_bytes());
    buf.extend_from_slice(&r.rgba);
    Ok(Response::new(buf))
}

/// Characters + per-character boxes of one page (JSON; ~20 bytes per character).
#[tauri::command(async)]
fn page_text(state: State<'_, AppState>, doc: u64, page: usize) -> Result<Arc<PageText>, String> {
    state.engine()?.page_text(doc, page).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn search_text(
    state: State<'_, AppState>,
    doc: u64,
    query: String,
    case_sensitive: bool,
) -> Result<Vec<SearchMatch>, String> {
    state
        .engine()?
        .search(doc, &query, case_sensitive)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn close_pdf(state: State<'_, AppState>, doc: u64) -> Result<(), String> {
    state.engine()?.close(doc);
    Ok(())
}

/// Engine status for the UI: `null` when OK, otherwise an error message.
#[tauri::command]
fn engine_error(state: State<'_, AppState>) -> Option<String> {
    state.engine.as_ref().err().cloned()
}

#[tauri::command]
fn initial_file(state: State<'_, AppState>) -> Option<String> {
    state.initial_file.clone()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let engine = PdfEngine::new(&pdfium_search_dirs(app)).map_err(|e| e.to_string());
            app.manage(AppState {
                engine,
                initial_file: initial_file_from_args(),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_document,
            page_text,
            search_text,
            render_page,
            close_pdf,
            engine_error,
            initial_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running Foarbyld");
}
