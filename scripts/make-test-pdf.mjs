// Generates a synthetic PDF for the performance gate (ADR 0001).
// Usage: node scripts/make-test-pdf.mjs [pages] [outfile]
// Each page has ~60 text lines and ~300 vector rectangles so rendering is not trivial.
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

const pages = Number(process.argv[2] ?? 200);
const out = process.argv[3] ?? "testdata/200-pages.pdf";

const objects = []; // index = object number - 1
const add = (body) => objects.push(body) && objects.length;

const catalog = add(""); // placeholder, filled below
const pagesRoot = add("");
const font = add("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");

const pageIds = [];
for (let p = 1; p <= pages; p++) {
  let c = "BT /F1 18 Tf 50 800 Td (Folio test page " + p + " of " + pages + ") Tj ET\n";
  c += "BT /F1 9 Tf 11 TL 50 780 Td\n";
  for (let l = 0; l < 60; l++) {
    c += "(Line " + (l + 1) + ": The quick brown fox jumps over the lazy dog, page " + p + ".) Tj T*\n";
  }
  c += "ET\n";
  for (let i = 0; i < 300; i++) {
    const x = 50 + (i % 30) * 16;
    const y = 80 + Math.floor(i / 30) * 14;
    const g = ((i * 37 + p * 11) % 100) / 100;
    c += g.toFixed(2) + " " + (1 - g).toFixed(2) + " 0.6 rg " + x + " " + y + " 12 10 re f\n";
  }
  const content = add("<< /Length " + Buffer.byteLength(c, "latin1") + " >>\nstream\n" + c + "endstream");
  const page = add(
    "<< /Type /Page /Parent " + pagesRoot + " 0 R /MediaBox [0 0 595 842] " +
      "/Resources << /Font << /F1 " + font + " 0 R >> >> /Contents " + content + " 0 R >>"
  );
  pageIds.push(page);
}

objects[catalog - 1] = "<< /Type /Catalog /Pages " + pagesRoot + " 0 R >>";
objects[pagesRoot - 1] =
  "<< /Type /Pages /Count " + pages + " /Kids [" + pageIds.map((i) => i + " 0 R").join(" ") + "] >>";

let pdf = "%PDF-1.4\n";
const offsets = [];
objects.forEach((body, i) => {
  offsets.push(Buffer.byteLength(pdf, "latin1"));
  pdf += i + 1 + " 0 obj\n" + body + "\nendobj\n";
});
const xref = Buffer.byteLength(pdf, "latin1");
pdf += "xref\n0 " + (objects.length + 1) + "\n0000000000 65535 f \n";
for (const o of offsets) pdf += String(o).padStart(10, "0") + " 00000 n \n";
pdf += "trailer\n<< /Size " + (objects.length + 1) + " /Root " + catalog + " 0 R >>\nstartxref\n" + xref + "\n%%EOF\n";

mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, pdf, "latin1");
console.log("wrote " + out + " (" + pages + " pages, " + (Buffer.byteLength(pdf, "latin1") / 1e6).toFixed(1) + " MB)");
