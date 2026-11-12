import { invoke } from "@tauri-apps/api/core";

export interface PageSize {
  /** PDF points (1/72 inch) */
  width: number;
  height: number;
}

export interface DocInfo {
  id: number;
  pageCount: number;
  pages: PageSize[];
}

export interface RenderedPage {
  width: number;
  height: number;
  pixels: Uint8ClampedArray;
}

export const openPdf = (path: string) => invoke<DocInfo>("open_pdf", { path });

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
