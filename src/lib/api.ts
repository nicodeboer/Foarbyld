import { invoke } from "@tauri-apps/api/core";
import { toPageText, type PageText, type PageTextWire } from "./textLayer";

export interface PageSize {
  /** PDF points (1/72 inch) */
  width: number;
  height: number;
}

export type DocKind = "pdf" | "image";

export interface DocInfo {
  id: number;
  kind: DocKind;
  pageCount: number;
  pages: PageSize[];
}

export interface RenderedPage {
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
}

export const PDF_EXTENSIONS = ["pdf"];
export const IMAGE_EXTENSIONS = ["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];
export const MARKDOWN_EXTENSIONS = ["md", "markdown"];
export const DOCUMENT_EXTENSIONS = [...PDF_EXTENSIONS, ...IMAGE_EXTENSIONS, ...MARKDOWN_EXTENSIONS];

const extensionOf = (path: string) => path.split(".").pop()?.toLowerCase() ?? "";

export const isMarkdownFile = (path: string) => MARKDOWN_EXTENSIONS.includes(extensionOf(path));

export const isSupportedFile = (path: string) => DOCUMENT_EXTENSIONS.includes(extensionOf(path));

export const openDocument = (path: string) => invoke<DocInfo>("open_document", { path });

/** Sanitized HTML for a Markdown file (rendered in Rust, independent of PDFium). */
export const openMarkdown = (path: string) => invoke<string>("open_markdown", { path });

export const closePdf = (doc: number) => invoke<void>("close_pdf", { doc });

export const engineError = () => invoke<string | null>("engine_error");

export const initialFile = () => invoke<string | null>("initial_file");

/** Wire format from Rust: [width u32 LE][height u32 LE][RGBA...] */
export async function renderPage(doc: number, page: number, width: number): Promise<RenderedPage> {
  const buf = await invoke<ArrayBuffer>("render_page", { doc, page, width });
  const view = new DataView(buf);
  const w = view.getUint32(0, true);
  const h = view.getUint32(4, true);
  return { width: w, height: h, pixels: new Uint8ClampedArray(buf, 8, w * h * 4) };
}

/** A search hit: `len` characters from character index `start` on `page` (0-based). */
export interface SearchMatch {
  page: number;
  start: number;
  len: number;
}

/** Engine stops at this many matches (keep in sync with MAX_MATCHES in pdf-core). */
export const MAX_MATCHES = 20_000;

export const fetchPageText = async (doc: number, page: number): Promise<PageText> =>
  toPageText(await invoke<PageTextWire>("page_text", { doc, page }));

export const searchText = (doc: number, query: string, caseSensitive: boolean) =>
  invoke<SearchMatch[]>("search_text", { doc, query, caseSensitive });
