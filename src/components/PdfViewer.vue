<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, shallowReactive, shallowRef, watch } from "vue";
import { fetchPageText, renderPage, type DocInfo, type SearchMatch } from "../lib/api";
import { summarizeFrames, summarizeRenders, type BenchmarkResult } from "../lib/stats";
import {
  caretAt,
  isEmptySelection,
  lineRange,
  lineRects,
  ordered,
  overGlyph,
  textOf,
  wordRange,
  type PageText,
  type Rect,
  type Selection,
} from "../lib/textLayer";

const props = withDefaults(
  defineProps<{ doc: DocInfo | null; zoom: number; matches?: SearchMatch[]; currentMatch?: number }>(),
  { matches: () => [], currentMatch: -1 },
);
const emit = defineEmits<{
  (e: "first-render"): void;
  (e: "zoom-by", factor: number): void;
  (e: "error", message: string): void;
}>();

const GAP = 12; // CSS px between pages
const PT_TO_CSS = 96 / 72; // PDF points -> CSS px at zoom 1.0
const CACHE_BYTES = 256 * 1024 * 1024; // decoded page bitmaps kept in memory

const scroller = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportW = ref(0);
const viewportH = ref(0);

// ---- layout (all in CSS px) -------------------------------------------------
const layout = computed(() => {
  const doc = props.doc;
  const tops: number[] = [];
  const widths: number[] = [];
  const heights: number[] = [];
  let y = GAP;
  let maxW = 0;
  if (doc) {
    const s = props.zoom * PT_TO_CSS;
    for (const p of doc.pages) {
      const w = p.width * s;
      const h = p.height * s;
      tops.push(y);
      widths.push(w);
      heights.push(h);
      y += h + GAP;
      maxW = Math.max(maxW, w);
    }
  }
  return { tops, widths, heights, total: y, maxW };
});

const contentWidth = computed(() => Math.max(viewportW.value, layout.value.maxW + 2 * GAP));

// ---- virtual scrolling -------------------------------------------------------
const range = computed(() => {
  const { tops, heights } = layout.value;
  const n = tops.length;
  if (!n) return { first: 0, last: -1 };
  const over = viewportH.value * 0.5;
  const y0 = scrollTop.value - over;
  const y1 = scrollTop.value + viewportH.value + over;
  let lo = 0;
  let hi = n - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (tops[mid] + heights[mid] < y0) lo = mid + 1;
    else hi = mid;
  }
  let last = lo;
  while (last + 1 < n && tops[last + 1] <= y1) last++;
  return { first: lo, last };
});
const first = computed(() => range.value.first);
const last = computed(() => range.value.last);
const visiblePages = computed(() =>
  Array.from({ length: Math.max(0, last.value - first.value + 1) }, (_, k) => first.value + k),
);

function pageStyle(i: number) {
  const l = layout.value;
  return {
    top: `${l.tops[i]}px`,
    left: `${Math.max(0, (contentWidth.value - l.widths[i]) / 2)}px`,
    width: `${l.widths[i]}px`,
    height: `${l.heights[i]}px`,
  };
}

// ---- rendering ---------------------------------------------------------------
const canvases = new Map<number, HTMLCanvasElement>();
const drawnWidth = new Map<number, number>(); // page -> pixel width currently on the canvas
const cache = new Map<string, ImageBitmap>(); // insertion order = LRU order
let cacheBytes = 0;
const renderMs: number[] = [];
let pumping = false;
let timer: number | undefined;
let timerActive = false;
let firstRenderDone = false;

const pixelWidth = (i: number) => Math.max(1, Math.round(layout.value.widths[i] * window.devicePixelRatio));

function setCanvas(i: number, el: HTMLCanvasElement | null) {
  // Function refs are re-invoked on every render (old with null, new with the
  // element). Only react when the element really changed.
  if (el && canvases.get(i) !== el) {
    canvases.set(i, el);
    drawnWidth.delete(i);
    schedule(0);
  }
}

function cacheGet(key: string): ImageBitmap | undefined {
  const b = cache.get(key);
  if (b) {
    cache.delete(key);
    cache.set(key, b);
  }
  return b;
}

function cachePut(key: string, bmp: ImageBitmap) {
  cache.set(key, bmp);
  cacheBytes += bmp.width * bmp.height * 4;
  for (const [k, v] of cache) {
    if (cacheBytes <= CACHE_BYTES || cache.size <= 1) break;
    cache.delete(k);
    cacheBytes -= v.width * v.height * 4;
    v.close();
  }
}

function clearCache() {
  for (const v of cache.values()) v.close();
  cache.clear();
  cacheBytes = 0;
}

function schedule(delay: number) {
  clearTimeout(timer);
  timerActive = true;
  timer = window.setTimeout(() => {
    timerActive = false;
    void pump();
  }, delay);
}

/** Next page to render: visible, has a canvas, not yet drawn at the right size; closest to viewport centre first. */
function nextWanted(): number | null {
  if (!props.doc) return null;
  const center = scrollTop.value + viewportH.value / 2;
  const { tops, heights } = layout.value;
  let best = -1;
  let bestDist = Infinity;
  for (let i = first.value; i <= last.value; i++) {
    const c = canvases.get(i);
    if (!c || !c.isConnected) continue;
    if (drawnWidth.get(i) === pixelWidth(i)) continue;
    const d = Math.abs(tops[i] + heights[i] / 2 - center);
    if (d < bestDist) {
      best = i;
      bestDist = d;
    }
  }
  return best < 0 ? null : best;
}

async function pump() {
  if (pumping) return;
  pumping = true;
  try {
    for (;;) {
      const i = nextWanted();
      if (i === null || !props.doc) break;
      await renderOne(props.doc, i);
    }
  } finally {
    pumping = false;
  }
}

async function renderOne(doc: DocInfo, i: number) {
  const w = pixelWidth(i);
  const key = `${doc.id}:${i}@${w}`;
  try {
    let bmp = cacheGet(key);
    if (!bmp) {
      const t0 = performance.now();
      const r = await renderPage(doc.id, i, w);
      bmp = await createImageBitmap(new ImageData(r.pixels, r.width, r.height));
      renderMs.push(performance.now() - t0);
      cachePut(key, bmp);
    }
    if (props.doc?.id !== doc.id) return; // document was closed meanwhile
    const c = canvases.get(i);
    if (c && c.isConnected && pixelWidth(i) === w) {
      c.width = bmp.width;
      c.height = bmp.height;
      c.getContext("2d")?.drawImage(bmp, 0, 0);
      drawnWidth.set(i, w);
      if (!firstRenderDone) {
        firstRenderDone = true;
        emit("first-render");
      }
    }
  } catch (err) {
    drawnWidth.set(i, w); // do not retry forever
    emit("error", String(err));
  }
}

// ---- events ------------------------------------------------------------------
let raf = 0;
function onScroll() {
  if (raf) return;
  raf = requestAnimationFrame(() => {
    raf = 0;
    if (scroller.value) scrollTop.value = scroller.value.scrollTop;
  });
}

function onWheel(e: WheelEvent) {
  if (!e.ctrlKey) return;
  e.preventDefault();
  emit("zoom-by", e.deltaY < 0 ? 1.1 : 1 / 1.1);
}

let observer: ResizeObserver | undefined;
onMounted(() => {
  window.addEventListener("keydown", onKeyDown);
  const el = scroller.value!;
  observer = new ResizeObserver(() => {
    viewportW.value = el.clientWidth;
    viewportH.value = el.clientHeight;
  });
  observer.observe(el);
  viewportW.value = el.clientWidth;
  viewportH.value = el.clientHeight;
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeyDown);
  endDrag();
  clearTimeout(prefetchTimer);
  observer?.disconnect();
  clearTimeout(timer);
  clearCache();
});

// New document: reset everything and start from the top.
watch(
  () => props.doc?.id,
  async () => {
    clearCache();
    canvases.clear();
    drawnWidth.clear();
    renderMs.length = 0;
    firstRenderDone = false;
    await nextTick();
    if (scroller.value) scroller.value.scrollTop = 0;
    scrollTop.value = 0;
    schedule(0);
  },
);

// Zoom: keep the viewport centre anchored, re-render after a short debounce.
watch(
  () => props.zoom,
  async (nz, oz) => {
    const el = scroller.value;
    if (!el) return;
    const atTop = el.scrollTop === 0; // e.g. fit-width right after opening: stay at the top
    const centre = el.scrollTop + el.clientHeight / 2;
    await nextTick();
    el.scrollTop = atTop ? 0 : centre * (nz / oz) - el.clientHeight / 2;
    scrollTop.value = el.scrollTop;
    schedule(120);
  },
);

watch([first, last], () => {
  for (const k of canvases.keys()) {
    if (k < first.value || k > last.value) {
      canvases.delete(k);
      drawnWidth.delete(k);
    }
  }
  schedule(0);
});

// ---- text: loading, selection, search highlights -------------------------------
const texts = shallowReactive(new Map<number, PageText>()); // page -> text of the current doc
const inflight = new Map<number, Promise<PageText | null>>();
const selection = shallowRef<Selection | null>(null);
const overText = ref(false); // pointer is over a glyph -> text cursor
const hasText = computed(() => props.doc?.kind === "pdf");
const scale = computed(() => props.zoom * PT_TO_CSS); // PDF points -> CSS px

/** Page text, fetched once. Resolves null when the doc changed meanwhile or the call failed. */
function ensureText(page: number): Promise<PageText | null> {
  const doc = props.doc;
  if (!doc || doc.kind !== "pdf") return Promise.resolve(null);
  const hit = texts.get(page);
  if (hit) return Promise.resolve(hit);
  let p = inflight.get(page);
  if (!p) {
    p = fetchPageText(doc.id, page)
      .then((t) => {
        if (props.doc?.id !== doc.id) return null;
        texts.set(page, t);
        return t;
      })
      .catch(() => null)
      .finally(() => inflight.delete(page));
    inflight.set(page, p);
  }
  return p;
}

let prefetchTimer: number | undefined;
function prefetchVisibleText() {
  clearTimeout(prefetchTimer);
  // Slightly delayed so the first render is not queued behind text extraction.
  prefetchTimer = window.setTimeout(() => {
    for (let i = first.value; i <= last.value; i++) void ensureText(i);
  }, 120);
}

const matchesByPage = computed(() => {
  const m = new Map<number, number[]>();
  props.matches.forEach((hit, gi) => {
    const list = m.get(hit.page);
    if (list) list.push(gi);
    else m.set(hit.page, [gi]);
  });
  return m;
});

interface Highlight extends Rect {
  kind: "sel" | "match" | "current";
}

/** Highlights for the visible pages, in CSS px relative to the page. */
const highlights = computed(() => {
  const out = new Map<number, Highlight[]>();
  const s = scale.value;
  const add = (page: number, kind: Highlight["kind"], rects: Rect[]) => {
    let list = out.get(page);
    if (!list) out.set(page, (list = []));
    for (const r of rects) list.push({ kind, x: r.x * s, y: r.y * s, w: r.w * s, h: r.h * s });
  };
  const sel = selection.value;
  const range = isEmptySelection(sel) ? null : ordered(sel!);
  for (let page = first.value; page <= last.value; page++) {
    const t = texts.get(page);
    if (!t) continue;
    for (const gi of matchesByPage.value.get(page) ?? []) {
      const m = props.matches[gi];
      add(page, gi === props.currentMatch ? "current" : "match", lineRects(t, m.start, m.start + m.len));
    }
    if (range && page >= range.start.page && page <= range.end.page) {
      const a = page === range.start.page ? range.start.index : 0;
      const b = page === range.end.page ? range.end.index : t.chars.length;
      add(page, "sel", lineRects(t, a, b));
    }
  }
  return out;
});

function highlightStyle(h: Highlight) {
  return { left: `${h.x}px`, top: `${h.y}px`, width: `${h.w}px`, height: `${h.h}px` };
}

/** Page under a viewport point (the nearest one when between pages) and the point in PDF points on it. */
function locate(clientX: number, clientY: number): { page: number; x: number; y: number } | null {
  const el = scroller.value;
  const doc = props.doc;
  if (!el || !doc) return null;
  const r = el.getBoundingClientRect();
  const cx = clientX - r.left + el.scrollLeft;
  const cy = clientY - r.top + el.scrollTop;
  const { tops, heights, widths } = layout.value;
  let lo = 0;
  let hi = tops.length - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (tops[mid] + heights[mid] + GAP / 2 < cy) lo = mid + 1;
    else hi = mid;
  }
  const left = Math.max(0, (contentWidth.value - widths[lo]) / 2);
  return { page: lo, x: (cx - left) / scale.value, y: (cy - tops[lo]) / scale.value };
}

let dragging = false;
let lastPointer = { x: 0, y: 0 };
let dragSeq = 0;
let autoScroll = 0;

async function onMouseDown(e: MouseEvent) {
  if (e.button !== 0 || !hasText.value) return;
  if (!(e.target as HTMLElement).closest(".content")) return; // scrollbar
  const p = locate(e.clientX, e.clientY);
  if (!p) return;
  e.preventDefault(); // no native selection / image drag
  const t = await ensureText(p.page);
  if (!t) return;
  const caret = caretAt(t, p.x, p.y);
  if (caret === null) {
    selection.value = null;
    return;
  }
  if (e.detail >= 2) {
    const [a, b] = e.detail === 2 ? wordRange(t, caret) : lineRange(t, caret);
    selection.value = { anchor: { page: p.page, index: a }, focus: { page: p.page, index: b } };
    return;
  }
  const here = { page: p.page, index: caret };
  selection.value = e.shiftKey && selection.value ? { anchor: selection.value.anchor, focus: here } : { anchor: here, focus: here };
  dragging = true;
  lastPointer = { x: e.clientX, y: e.clientY };
  window.addEventListener("mousemove", onDragMove);
  window.addEventListener("mouseup", endDrag);
  autoScroll = window.setInterval(autoScrollStep, 30);
}

async function extendTo(clientX: number, clientY: number) {
  const p = locate(clientX, clientY);
  const seq = ++dragSeq;
  if (!p) return;
  const t = await ensureText(p.page);
  if (!t || seq !== dragSeq || !dragging || !selection.value) return;
  const caret = caretAt(t, p.x, p.y);
  if (caret === null) return;
  selection.value = { anchor: selection.value.anchor, focus: { page: p.page, index: caret } };
}

function onDragMove(e: MouseEvent) {
  lastPointer = { x: e.clientX, y: e.clientY };
  void extendTo(e.clientX, e.clientY);
}

/** Keeps scrolling while the pointer is held beyond the top/bottom edge. */
function autoScrollStep() {
  const el = scroller.value;
  if (!el || !dragging) return;
  const r = el.getBoundingClientRect();
  const edge = 24;
  const dy = lastPointer.y < r.top + edge ? lastPointer.y - (r.top + edge) : lastPointer.y > r.bottom - edge ? lastPointer.y - (r.bottom - edge) : 0;
  if (!dy) return;
  el.scrollTop += Math.sign(dy) * Math.min(60, Math.abs(dy) / 2 + 4);
  scrollTop.value = el.scrollTop;
  void extendTo(lastPointer.x, lastPointer.y);
}

function endDrag() {
  dragging = false;
  clearInterval(autoScroll);
  window.removeEventListener("mousemove", onDragMove);
  window.removeEventListener("mouseup", endDrag);
  if (isEmptySelection(selection.value)) selection.value = null;
}

let hoverRaf = 0;
function onHover(e: MouseEvent) {
  if (dragging || !hasText.value || hoverRaf) return;
  const { clientX, clientY } = e;
  hoverRaf = requestAnimationFrame(() => {
    hoverRaf = 0;
    const p = locate(clientX, clientY);
    const t = p && texts.get(p.page);
    overText.value = !!t && overGlyph(t, p!.x, p!.y);
  });
}

async function selectedText(): Promise<string> {
  const sel = selection.value;
  if (!sel || isEmptySelection(sel)) return "";
  const { start, end } = ordered(sel);
  const parts: string[] = [];
  for (let page = start.page; page <= end.page; page++) {
    const t = await ensureText(page);
    if (!t) continue;
    parts.push(textOf(t, page === start.page ? start.index : 0, page === end.page ? end.index : t.chars.length));
  }
  return parts.join("\n");
}

async function selectAll() {
  const doc = props.doc;
  if (!doc || !hasText.value) return;
  const t = await ensureText(doc.pageCount - 1);
  if (!t) return;
  selection.value = {
    anchor: { page: 0, index: 0 },
    focus: { page: doc.pageCount - 1, index: t.chars.length },
  };
}

function onKeyDown(e: KeyboardEvent) {
  const tag = (e.target as HTMLElement | null)?.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA") return; // let fields handle their own shortcuts
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === "c" && !isEmptySelection(selection.value)) {
    e.preventDefault();
    void selectedText().then(async (s) => {
      if (s) await navigator.clipboard?.writeText(s).catch(() => undefined);
    });
  } else if (mod && e.key.toLowerCase() === "a" && hasText.value) {
    e.preventDefault();
    void selectAll();
  } else if (e.key === "Escape" && selection.value) {
    selection.value = null;
  }
}

/** Scroll so the match is visible (centred), unless it already is. */
async function revealMatch(index: number) {
  const m = props.matches[index];
  const el = scroller.value;
  if (!m || !el) return;
  const t = await ensureText(m.page);
  const r = t && lineRects(t, m.start, m.start + m.len)[0];
  if (!r || props.matches[index] !== m) return; // superseded while loading
  const s = scale.value;
  const top = layout.value.tops[m.page] + r.y * s;
  const bottom = top + r.h * s;
  if (top < el.scrollTop + 8 || bottom > el.scrollTop + el.clientHeight - 8) {
    el.scrollTop = Math.max(0, top - (el.clientHeight - r.h * s) / 2);
    scrollTop.value = el.scrollTop;
  }
  const left = Math.max(0, (contentWidth.value - layout.value.widths[m.page]) / 2) + r.x * s;
  if (left < el.scrollLeft || left + r.w * s > el.scrollLeft + el.clientWidth) {
    el.scrollLeft = Math.max(0, left - el.clientWidth / 2);
  }
}

watch([() => props.matches, () => props.currentMatch], () => {
  if (props.currentMatch >= 0) void revealMatch(props.currentMatch);
});

watch(
  () => props.doc?.id,
  () => {
    texts.clear();
    inflight.clear();
    selection.value = null;
    overText.value = false;
  },
);
watch([first, last], prefetchVisibleText);

// ---- exposed API (toolbar + benchmark) -----------------------------------------
function fitWidthZoom(): number {
  const doc = props.doc;
  if (!doc) return 1;
  const maxPt = Math.max(...doc.pages.map((p) => p.width));
  return (viewportW.value - 2 * GAP) / (maxPt * PT_TO_CSS);
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** Resolves (with the wait in ms) once every visible page is drawn; -1 on timeout. */
async function waitIdle(timeoutMs = 30000): Promise<number> {
  const t0 = performance.now();
  while (performance.now() - t0 < timeoutMs) {
    if (!pumping && !timerActive && nextWanted() === null) return performance.now() - t0;
    await sleep(16);
  }
  return -1;
}

/**
 * Objective performance gate for ADR 0001: scrolls the document at a fixed
 * speed while recording frame times, then stresses zooming.
 */
async function runBenchmark(firstPageMs: number | null): Promise<BenchmarkResult> {
  const el = scroller.value;
  const doc = props.doc;
  if (!el || !doc) throw new Error("No document open");

  el.scrollTop = 0;
  scrollTop.value = 0;
  await waitIdle();
  renderMs.length = 0;

  const speed = 4000; // px/s: a fast flick through the document
  const distance = Math.max(0, Math.min(layout.value.total - viewportH.value, 120_000));
  const duration = (distance / speed) * 1000;
  const frames: number[] = [];
  const t0 = performance.now();
  let prev = t0;
  await new Promise<void>((resolve) => {
    const step = (now: number) => {
      frames.push(now - prev);
      prev = now;
      const y = Math.min(distance, ((now - t0) / 1000) * speed);
      el.scrollTop = y;
      scrollTop.value = y;
      if (now - t0 < duration) requestAnimationFrame(step);
      else resolve();
    };
    requestAnimationFrame(step);
  });
  const scroll = summarizeFrames(frames, distance, performance.now() - t0);
  const render = summarizeRenders(renderMs);
  const scrollSettleMs = Math.round(await waitIdle());

  // Zoom stress: 100% -> 200%, measure time until the viewport is crisp again.
  const startZoom = props.zoom;
  emit("zoom-by", 2 / startZoom);
  const z0 = performance.now();
  await nextTick();
  await sleep(0);
  const w = await waitIdle();
  const zoomSettleMs = Math.round(w < 0 ? -1 : performance.now() - z0);
  emit("zoom-by", startZoom / 2);

  return {
    timestamp: new Date().toISOString(),
    userAgent: navigator.userAgent,
    devicePixelRatio: window.devicePixelRatio,
    pageCount: doc.pageCount,
    firstPageMs: firstPageMs === null ? null : Math.round(firstPageMs),
    scroll,
    render,
    zoomSettleMs,
    scrollSettleMs,
  };
}

defineExpose({ fitWidthZoom, runBenchmark });
</script>

<template>
  <div
    ref="scroller"
    class="scroller"
    :class="{ 'over-text': overText }"
    @scroll.passive="onScroll"
    @wheel="onWheel"
    @mousedown="onMouseDown"
    @mousemove="onHover"
  >
    <div v-if="doc" class="content" :style="{ height: layout.total + 'px', width: contentWidth + 'px' }">
      <canvas
        v-for="i in visiblePages"
        :key="`${doc.id}:${i}`"
        :ref="(el) => setCanvas(i, el as HTMLCanvasElement | null)"
        class="page"
        :style="pageStyle(i)"
      />
      <div v-for="i in visiblePages" :key="`hl:${doc.id}:${i}`" class="layer" :style="pageStyle(i)">
        <i v-for="(h, k) in highlights.get(i)" :key="k" :class="['hl', h.kind]" :style="highlightStyle(h)" />
      </div>
    </div>
  </div>
</template>

<style scoped>
.scroller {
  position: relative;
  flex: 1;
  overflow: auto;
}
.scroller.over-text {
  cursor: text;
}
.content {
  position: relative;
  user-select: none;
  -webkit-user-select: none;
}
.layer {
  position: absolute;
  overflow: hidden;
  pointer-events: none;
}
.hl {
  position: absolute;
  mix-blend-mode: multiply; /* keep the glyphs readable under the highlight */
}
.hl.sel {
  background: rgb(60 130 255 / 0.4);
}
.hl.match {
  background: rgb(255 214 0 / 0.55);
}
.hl.current {
  background: rgb(255 140 0 / 0.65);
}
.page {
  position: absolute;
  background: #fff;
  box-shadow: 0 1px 4px rgb(0 0 0 / 0.35);
}
</style>
