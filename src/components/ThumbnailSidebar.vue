<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { renderPage, type DocInfo } from "../lib/api";

const props = defineProps<{ doc: DocInfo; currentPage: number }>();
const emit = defineEmits<{ (e: "select", page: number): void }>();

const THUMB_W = 120; // CSS px
const GAP = 14; // vertical space between thumbnails (includes the page number)
const PAD = 10;
const CACHE_BYTES = 64 * 1024 * 1024;

const scroller = ref<HTMLElement | null>(null);
const scrollTop = ref(0);
const viewportH = ref(0);

const layout = computed(() => {
  const tops: number[] = [];
  const heights: number[] = [];
  let y = PAD;
  for (const p of props.doc.pages) {
    const h = Math.max(20, Math.round((THUMB_W * p.height) / p.width));
    tops.push(y);
    heights.push(h);
    y += h + GAP + 14; // 14 = label line
  }
  return { tops, heights, total: y + PAD };
});

const range = computed(() => {
  const { tops, heights } = layout.value;
  const n = tops.length;
  if (!n) return { first: 0, last: -1 };
  const y0 = scrollTop.value - viewportH.value * 0.5;
  const y1 = scrollTop.value + viewportH.value * 1.5;
  let lo = 0;
  let hi = n - 1;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (tops[mid] + heights[mid] + GAP < y0) lo = mid + 1;
    else hi = mid;
  }
  let last = lo;
  while (last + 1 < n && tops[last + 1] <= y1) last++;
  return { first: lo, last };
});
const visible = computed(() =>
  Array.from({ length: Math.max(0, range.value.last - range.value.first + 1) }, (_, k) => range.value.first + k),
);

// ---- thumbnail rendering --------------------------------------------------------
const canvases = new Map<number, HTMLCanvasElement>();
const drawn = new Set<number>();
const cache = new Map<string, ImageBitmap>(); // LRU by insertion order
let cacheBytes = 0;
let pumping = false;
let timer: number | undefined;

const pixelWidth = () => Math.round(THUMB_W * window.devicePixelRatio);

function setCanvas(i: number, el: HTMLCanvasElement | null) {
  if (el && canvases.get(i) !== el) {
    canvases.set(i, el);
    drawn.delete(i);
    schedule();
  }
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

/** Small delay: lets the main viewer's own render requests go first while scrolling. */
function schedule() {
  clearTimeout(timer);
  timer = window.setTimeout(() => void pump(), 80);
}

function nextWanted(): number | null {
  const { tops, heights } = layout.value;
  const centre = scrollTop.value + viewportH.value / 2;
  let best = -1;
  let bestDist = Infinity;
  for (let i = range.value.first; i <= range.value.last; i++) {
    const c = canvases.get(i);
    if (!c || !c.isConnected || drawn.has(i)) continue;
    const d = Math.abs(tops[i] + heights[i] / 2 - centre);
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
      if (i === null) break;
      await renderOne(i);
    }
  } finally {
    pumping = false;
  }
}

async function renderOne(i: number) {
  const doc = props.doc;
  const w = pixelWidth();
  const key = `${doc.id}:${i}@${w}`;
  try {
    let bmp = cache.get(key);
    if (bmp) {
      cache.delete(key);
      cache.set(key, bmp);
    } else {
      const r = await renderPage(doc.id, i, w);
      bmp = await createImageBitmap(new ImageData(r.pixels, r.width, r.height));
      cachePut(key, bmp);
    }
    const c = canvases.get(i);
    if (props.doc.id === doc.id && c?.isConnected) {
      c.width = bmp.width;
      c.height = bmp.height;
      c.getContext("2d")?.drawImage(bmp, 0, 0);
    }
  } catch {
    // Document closed or page failed: leave the placeholder, do not retry in a loop.
  }
  drawn.add(i);
}

// ---- events ---------------------------------------------------------------------
function onScroll() {
  if (scroller.value) scrollTop.value = scroller.value.scrollTop;
}

let observer: ResizeObserver | undefined;
onMounted(() => {
  const el = scroller.value!;
  observer = new ResizeObserver(() => (viewportH.value = el.clientHeight));
  observer.observe(el);
  viewportH.value = el.clientHeight;
  void revealCurrent();
});
onBeforeUnmount(() => {
  observer?.disconnect();
  clearTimeout(timer);
  clearCache();
});

watch(
  () => props.doc.id,
  async () => {
    clearCache();
    canvases.clear();
    drawn.clear();
    await nextTick();
    if (scroller.value) scroller.value.scrollTop = 0;
    scrollTop.value = 0;
    schedule();
  },
);

watch([() => range.value.first, () => range.value.last], () => {
  for (const k of [...canvases.keys()]) {
    if (k < range.value.first || k > range.value.last) {
      canvases.delete(k);
      drawn.delete(k);
    }
  }
  schedule();
});

/** Keep the current page's thumbnail in view. */
async function revealCurrent() {
  const el = scroller.value;
  if (!el) return;
  await nextTick();
  const { tops, heights } = layout.value;
  const i = props.currentPage;
  if (i < 0 || i >= tops.length) return;
  const top = tops[i];
  const bottom = top + heights[i] + 14;
  if (top < el.scrollTop || bottom > el.scrollTop + el.clientHeight) {
    el.scrollTop = Math.max(0, top - (el.clientHeight - heights[i]) / 2);
    scrollTop.value = el.scrollTop;
  }
}
watch(() => props.currentPage, revealCurrent);
</script>

<template>
  <aside ref="scroller" class="sidebar" aria-label="Page thumbnails" @scroll.passive="onScroll">
    <div class="list" :style="{ height: layout.total + 'px' }">
      <button
        v-for="i in visible"
        :key="`${doc.id}:${i}`"
        class="thumb"
        :class="{ current: i === currentPage }"
        :style="{ top: layout.tops[i] + 'px' }"
        :aria-label="`Page ${i + 1}`"
        :aria-current="i === currentPage ? 'page' : undefined"
        @click="emit('select', i)"
      >
        <canvas
          :ref="(el) => setCanvas(i, el as HTMLCanvasElement | null)"
          :style="{ width: THUMB_W + 'px', height: layout.heights[i] + 'px' }"
        />
        <span class="label">{{ i + 1 }}</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.sidebar {
  position: relative;
  flex: none;
  width: 160px;
  overflow-y: auto;
  overflow-x: hidden;
  background: var(--bar);
  border-right: 1px solid var(--border);
}
.list {
  position: relative;
}
.thumb {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 4px;
  border: 2px solid transparent;
  border-radius: 6px;
  background: transparent;
}
.thumb:hover:not(:disabled) {
  border-color: var(--border);
}
.thumb.current,
.thumb.current:hover:not(:disabled) {
  border-color: var(--accent);
}
canvas {
  display: block;
  background: #fff;
  box-shadow: 0 1px 3px rgb(0 0 0 / 0.35);
}
.label {
  font-size: 11px;
  line-height: 14px;
  font-variant-numeric: tabular-nums;
  opacity: 0.75;
}
</style>
