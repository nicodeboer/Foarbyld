// Pure helpers for text selection and highlighting. No Vue, no DOM: unit-testable.
//
// Coordinates are PDF points with the origin at the top-left of the displayed
// page. A *caret* is a position between characters: 0 = before the first char,
// n = after the last.

export interface PageText {
  /** One entry per PDFium character (code points, not UTF-16 units). */
  chars: string[];
  /** 4 numbers per character: x, y, width, height. All zero = no glyph (generated space/newline). */
  boxes: ArrayLike<number>;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Caret {
  page: number;
  index: number;
}

export interface Selection {
  anchor: Caret;
  focus: Caret;
}

/** Wire format of the `page_text` command. */
export interface PageTextWire {
  text: string;
  boxes: number[];
}

export const toPageText = (w: PageTextWire): PageText => ({ chars: Array.from(w.text), boxes: w.boxes });

export const hasBox = (t: PageText, i: number) => t.boxes[i * 4 + 2] > 0 || t.boxes[i * 4 + 3] > 0;

export const boxOf = (t: PageText, i: number): Rect => ({
  x: t.boxes[i * 4],
  y: t.boxes[i * 4 + 1],
  w: t.boxes[i * 4 + 2],
  h: t.boxes[i * 4 + 3],
});

export function compareCarets(a: Caret, b: Caret): number {
  return a.page - b.page || a.index - b.index;
}

/** Selection as an ordered [start, end) pair of carets. */
export function ordered(sel: Selection): { start: Caret; end: Caret } {
  return compareCarets(sel.anchor, sel.focus) <= 0
    ? { start: sel.anchor, end: sel.focus }
    : { start: sel.focus, end: sel.anchor };
}

export const isEmptySelection = (sel: Selection | null): boolean =>
  !sel || compareCarets(sel.anchor, sel.focus) === 0;

/** The glyph box closest to (x, y), with the distance to it (0 = inside). Vertical distance counts 4x so the pointer sticks to its own line. */
export function nearestChar(t: PageText, x: number, y: number): { index: number; dist: number } | null {
  let best = -1;
  let bestDist = Infinity;
  for (let i = 0; i < t.chars.length; i++) {
    if (!hasBox(t, i)) continue;
    const bx = t.boxes[i * 4];
    const by = t.boxes[i * 4 + 1];
    const dx = Math.max(bx - x, 0, x - (bx + t.boxes[i * 4 + 2]));
    const dy = Math.max(by - y, 0, y - (by + t.boxes[i * 4 + 3]));
    const d = dy * 4 + dx;
    if (d < bestDist) {
      best = i;
      bestDist = d;
      if (d === 0) break;
    }
  }
  return best < 0 ? null : { index: best, dist: bestDist };
}

/** Caret position for a pointer at (x, y); null when the page has no text. */
export function caretAt(t: PageText, x: number, y: number): number | null {
  const n = nearestChar(t, x, y);
  if (!n) return null;
  const b = boxOf(t, n.index);
  return x > b.x + b.w / 2 ? n.index + 1 : n.index;
}

/** True when the pointer is directly over a glyph. */
export function overGlyph(t: PageText, x: number, y: number): boolean {
  return nearestChar(t, x, y)?.dist === 0;
}

const isWordChar = (c: string) => /[\p{L}\p{N}_]/u.test(c);

/** Range of the word at a caret (double click). Non-word characters select just themselves. */
export function wordRange(t: PageText, caret: number): [number, number] {
  const n = t.chars.length;
  if (n === 0) return [0, 0];
  let i = Math.min(caret, n - 1);
  if (!isWordChar(t.chars[i]) && i > 0 && isWordChar(t.chars[i - 1]) && caret === i) i -= 1;
  if (!isWordChar(t.chars[i])) return [i, i + 1];
  let a = i;
  let b = i + 1;
  while (a > 0 && isWordChar(t.chars[a - 1])) a--;
  while (b < n && isWordChar(t.chars[b])) b++;
  return [a, b];
}

/** Range of the visual line at a caret (triple click): up to the surrounding line breaks. */
export function lineRange(t: PageText, caret: number): [number, number] {
  const n = t.chars.length;
  const isBreak = (c: string) => c === "\n" || c === "\r";
  let a = Math.min(caret, n);
  let b = a;
  while (a > 0 && !isBreak(t.chars[a - 1])) a--;
  while (b < n && !isBreak(t.chars[b])) b++;
  return [a, b];
}

/**
 * Highlight rectangles for characters [a, b): glyph boxes on the same line
 * are merged into one rectangle so a selected paragraph is a few divs, not thousands.
 */
export function lineRects(t: PageText, a: number, b: number): Rect[] {
  const out: Rect[] = [];
  let cur: Rect | null = null;
  const end = Math.min(b, t.chars.length);
  for (let i = Math.max(0, a); i < end; i++) {
    if (!hasBox(t, i)) continue;
    const r = boxOf(t, i);
    if (cur) {
      const overlap = Math.min(cur.y + cur.h, r.y + r.h) - Math.max(cur.y, r.y);
      const gap = r.x - (cur.x + cur.w);
      if (overlap > 0.5 * Math.min(cur.h, r.h) && gap < Math.max(cur.h, r.h) && gap > -cur.w) {
        const x1 = Math.max(cur.x + cur.w, r.x + r.w);
        const y0 = Math.min(cur.y, r.y);
        const y1 = Math.max(cur.y + cur.h, r.y + r.h);
        cur = { x: Math.min(cur.x, r.x), y: y0, w: 0, h: y1 - y0 };
        cur.w = x1 - cur.x;
        out[out.length - 1] = cur;
        continue;
      }
    }
    cur = { ...r };
    out.push(cur);
  }
  return out;
}

/** Clipboard text for chars [a, b): PDFium's hyphen marker becomes "-", CR is dropped. */
export function textOf(t: PageText, a: number, b: number): string {
  return t.chars
    .slice(Math.max(0, a), Math.max(0, b))
    .join("")
    .replace(/\u0002/g, "-")
    .replace(/\r/g, "");
}
