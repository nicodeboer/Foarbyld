import { describe, expect, it } from "vitest";
import { caretAt, lineRange, lineRects, ordered, overGlyph, textOf, toPageText, wordRange } from "./textLayer";

// "ab cd\n" laid out on one line, 10pt per glyph, 10pt tall; the space and \n are generated (no box).
const t = toPageText({
  text: "ab cd\n",
  boxes: [0, 0, 10, 10, 10, 0, 10, 10, 0, 0, 0, 0, 25, 0, 10, 10, 35, 0, 10, 10, 0, 0, 0, 0],
});

describe("textLayer", () => {
  it("splits text into code points", () => {
    expect(toPageText({ text: "a😀b", boxes: [] }).chars).toEqual(["a", "😀", "b"]);
  });

  it("places the caret before/after a glyph by its midpoint", () => {
    expect(caretAt(t, 2, 5)).toBe(0);
    expect(caretAt(t, 8, 5)).toBe(1);
    expect(caretAt(t, 52, 5)).toBe(5); // past the end of the line -> after "d" (index 4 + 1)
    expect(caretAt(toPageText({ text: "", boxes: [] }), 1, 1)).toBeNull();
  });

  it("sticks to the pointer's own line", () => {
    const two = toPageText({ text: "ab", boxes: [0, 0, 10, 10, 0, 20, 10, 10] });
    expect(caretAt(two, 9, 24)).toBe(2); // second line, right half
    expect(caretAt(two, 9, 8)).toBe(1);
  });

  it("detects the pointer over a glyph", () => {
    expect(overGlyph(t, 5, 5)).toBe(true);
    expect(overGlyph(t, 22, 5)).toBe(false); // the gap (generated space)
  });

  it("selects words and lines", () => {
    expect(wordRange(t, 1)).toEqual([0, 2]);
    expect(wordRange(t, 4)).toEqual([3, 5]);
    expect(wordRange(t, 2)).toEqual([0, 2]); // caret right after a word still picks it
    expect(lineRange(t, 1)).toEqual([0, 5]);
  });

  it("merges glyph boxes of one line into one rectangle", () => {
    expect(lineRects(t, 0, 6)).toEqual([{ x: 0, y: 0, w: 45, h: 10 }]);
    const two = toPageText({ text: "ab", boxes: [0, 0, 10, 10, 0, 20, 10, 10] });
    expect(lineRects(two, 0, 2)).toHaveLength(2);
    expect(lineRects(t, 3, 3)).toEqual([]);
  });

  it("orders a selection regardless of drag direction", () => {
    const a = { page: 2, index: 5 };
    const b = { page: 1, index: 9 };
    expect(ordered({ anchor: a, focus: b })).toEqual({ start: b, end: a });
  });

  it("produces clipboard text", () => {
    const h = toPageText({ text: "co\u0002\r\nop", boxes: [] });
    expect(textOf(h, 0, 7)).toBe("co-\nop");
  });
});
