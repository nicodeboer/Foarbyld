<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import PdfViewer from "./components/PdfViewer.vue";
import {
  closePdf,
  engineError,
  initialFile,
  IMAGE_EXTENSIONS,
  isSupportedFile,
  MAX_MATCHES,
  openDocument,
  PDF_EXTENSIONS,
  searchText,
  type DocInfo,
  type SearchMatch,
} from "./lib/api";
import type { BenchmarkResult } from "./lib/stats";

const doc = shallowRef<DocInfo | null>(null);
const fileName = ref("");
const zoom = ref(1);
const error = ref<string | null>(null);
const dragging = ref(false);
const firstPageMs = ref<number | null>(null);
const bench = ref<string | null>(null);
const benchRunning = ref(false);
const viewer = ref<InstanceType<typeof PdfViewer> | null>(null);

let openStartedAt = 0;

const ZOOM_MIN = 0.25;
const ZOOM_MAX = 5;
const setZoom = (z: number) => (zoom.value = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z)));
const zoomBy = (f: number) => setZoom(zoom.value * f);

async function openPath(path: string) {
  error.value = null;
  firstPageMs.value = null;
  const previous = doc.value;
  resetFind();
  try {
    openStartedAt = performance.now();
    const info = await openDocument(path);
    doc.value = info;
    fileName.value = path.split(/[\\/]/).pop() ?? path;
    if (previous) void closePdf(previous.id);
    // Default to fit-width so the first view is sensible on any window size.
    requestAnimationFrame(() => viewer.value && setZoom(viewer.value.fitWidthZoom()));
  } catch (e) {
    error.value = String(e);
  }
}

async function chooseFile() {
  const picked = await open({
    multiple: false,
    filters: [
      { name: "Documents and images", extensions: [...PDF_EXTENSIONS, ...IMAGE_EXTENSIONS] },
      { name: "PDF", extensions: PDF_EXTENSIONS },
      { name: "Images", extensions: IMAGE_EXTENSIONS },
    ],
  });
  if (typeof picked === "string") await openPath(picked);
}

function onFirstRender() {
  firstPageMs.value = performance.now() - openStartedAt;
}

async function runBenchmark() {
  if (!viewer.value || !doc.value || benchRunning.value) return;
  benchRunning.value = true;
  try {
    const result: BenchmarkResult = await viewer.value.runBenchmark(firstPageMs.value);
    bench.value = JSON.stringify(result, null, 2);
  } catch (e) {
    error.value = String(e);
  } finally {
    benchRunning.value = false;
  }
}

async function copyBench() {
  if (bench.value) await navigator.clipboard?.writeText(bench.value).catch(() => undefined);
}

// ---- find in document ---------------------------------------------------------
const findOpen = ref(false);
const query = ref("");
const caseSensitive = ref(false);
const matches = shallowRef<SearchMatch[]>([]);
const current = ref(-1);
const searching = ref(false);
const findInput = ref<HTMLInputElement | null>(null);
let searchSeq = 0;
let searchTimer: number | undefined;
let searchPending = false; // typed since the last search ran (debounce running)

const canSearch = () => doc.value?.kind === "pdf";

function resetFind() {
  searchPending = false;
  searchSeq++;
  clearTimeout(searchTimer);
  matches.value = [];
  current.value = -1;
  searching.value = false;
}

async function runSearch() {
  searchPending = false;
  clearTimeout(searchTimer);
  const d = doc.value;
  const seq = ++searchSeq;
  if (!d || !canSearch() || !query.value.trim()) {
    resetFind();
    return;
  }
  searching.value = true;
  try {
    const found = await searchText(d.id, query.value, caseSensitive.value);
    if (seq !== searchSeq) return; // a newer query or document replaced this one
    matches.value = found;
    current.value = found.length ? 0 : -1;
  } catch (e) {
    if (seq === searchSeq) error.value = String(e);
  } finally {
    if (seq === searchSeq) searching.value = false;
  }
}

watch([query, caseSensitive], () => {
  searchPending = true;
  clearTimeout(searchTimer);
  searchTimer = window.setTimeout(runSearch, 200);
});

function step(delta: number) {
  const n = matches.value.length;
  if (n) current.value = (current.value + delta + n) % n;
}

async function openFind() {
  if (!canSearch()) return;
  findOpen.value = true;
  await nextTick();
  findInput.value?.focus();
  findInput.value?.select();
}

function closeFind() {
  findOpen.value = false;
  resetFind();
}

function onFindKey(e: KeyboardEvent) {
  if (e.key === "Enter") {
    e.preventDefault();
    if (searchPending) void runSearch(); // Enter right after typing: do not wait for the debounce
    else step(e.shiftKey ? -1 : 1);
  } else if (e.key === "Escape") {
    closeFind();
  }
}

function onWindowKey(e: KeyboardEvent) {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
    e.preventDefault();
    void openFind();
  } else if (e.key === "F3" && findOpen.value) {
    e.preventDefault();
    step(e.shiftKey ? -1 : 1);
  }
}

const findStatus = () => {
  if (searching.value) return "Searching…";
  if (!query.value.trim()) return "";
  if (!matches.value.length) return "No results";
  const n = matches.value.length;
  return `${current.value + 1} / ${n >= MAX_MATCHES ? `${n}+` : n}`;
};

let unlisten: (() => void) | undefined;
onMounted(async () => {
  window.addEventListener("keydown", onWindowKey);
  unlisten = await getCurrentWebview().onDragDropEvent((event) => {
    const p = event.payload;
    if (p.type === "over" || p.type === "enter") dragging.value = true;
    else if (p.type === "leave") dragging.value = false;
    else if (p.type === "drop") {
      dragging.value = false;
      const file = p.paths.find(isSupportedFile);
      if (file) void openPath(file);
    }
  });

  const missing = await engineError();
  if (missing) {
    error.value = missing;
    return;
  }
  const initial = await initialFile();
  if (initial) await openPath(initial);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onWindowKey);
  clearTimeout(searchTimer);
  unlisten?.();
});
</script>

<template>
  <div class="app" :class="{ dragging }">
    <header class="toolbar">
      <button @click="chooseFile">Open…</button>
      <span class="sep" />
      <button :disabled="!doc" title="Zoom out" @click="zoomBy(1 / 1.25)">−</button>
      <span class="zoom">{{ Math.round(zoom * 100) }}%</span>
      <button :disabled="!doc" title="Zoom in" @click="zoomBy(1.25)">+</button>
      <button :disabled="!doc" @click="viewer && setZoom(viewer.fitWidthZoom())">Fit width</button>
      <button :disabled="!doc || doc.kind !== 'pdf'" title="Find (Ctrl/Cmd+F)" @click="openFind">Find</button>
      <span class="spacer" />
      <span v-if="doc" class="info">
        {{ fileName }} ·
        <template v-if="doc.kind === 'image'">
          {{ Math.round(doc.pages[0].width / 0.75) }} × {{ Math.round(doc.pages[0].height / 0.75) }} px
        </template>
        <template v-else>{{ doc.pageCount }} pages</template>
        <template v-if="firstPageMs !== null"> · first page {{ Math.round(firstPageMs) }} ms</template>
      </span>
      <button :disabled="!doc || benchRunning" @click="runBenchmark">
        {{ benchRunning ? "Running…" : "Benchmark" }}
      </button>
    </header>

    <div v-if="findOpen" class="findbar">
      <input
        ref="findInput"
        v-model="query"
        type="search"
        placeholder="Find in document"
        spellcheck="false"
        @keydown="onFindKey"
      />
      <label class="case"><input v-model="caseSensitive" type="checkbox" /> Aa</label>
      <span class="status">{{ findStatus() }}</span>
      <button :disabled="!matches.length" title="Previous (Shift+Enter)" @click="step(-1)">↑</button>
      <button :disabled="!matches.length" title="Next (Enter)" @click="step(1)">↓</button>
      <span class="spacer" />
      <button title="Close (Esc)" @click="closeFind">✕</button>
    </div>

    <div v-if="error" class="error" role="alert">
      {{ error }}
      <button @click="error = null">Dismiss</button>
    </div>

    <main class="main">
      <PdfViewer
        ref="viewer"
        :doc="doc"
        :zoom="zoom"
        :matches="matches"
        :current-match="current"
        @first-render="onFirstRender"
        @zoom-by="zoomBy"
        @error="(m: string) => (error = m)"
      />
      <div v-if="!doc" class="empty">Drop a PDF or image here or press “Open…”</div>
    </main>

    <div v-if="bench" class="modal" @click.self="bench = null">
      <div class="dialog">
        <h3>Benchmark result</h3>
        <p>Attach this JSON to the performance gate in ADR 0001 (one result per OS).</p>
        <textarea readonly :value="bench" rows="18" @focus="($event.target as HTMLTextAreaElement).select()" />
        <div class="actions">
          <button @click="copyBench">Copy</button>
          <button @click="bench = null">Close</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
}
.app.dragging .main {
  outline: 3px dashed var(--accent);
  outline-offset: -6px;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  background: var(--bar);
  border-bottom: 1px solid var(--border);
}
.sep {
  width: 1px;
  height: 20px;
  background: var(--border);
  margin: 0 4px;
}
.spacer {
  flex: 1;
}
.zoom {
  min-width: 3.5em;
  text-align: center;
  font-variant-numeric: tabular-nums;
}
.info {
  opacity: 0.7;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.findbar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 10px;
  background: var(--bar);
  border-bottom: 1px solid var(--border);
}
.findbar input[type="search"] {
  width: 260px;
  padding: 3px 6px;
}
.findbar .case {
  display: flex;
  align-items: center;
  gap: 3px;
  font-size: 12px;
  user-select: none;
}
.findbar .status {
  min-width: 5.5em;
  opacity: 0.7;
  font-variant-numeric: tabular-nums;
}
.error {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: #b3261e;
  color: #fff;
}
.error button {
  color: #fff;
  border-color: rgb(255 255 255 / 0.6);
}
.main {
  position: relative;
  flex: 1;
  display: flex;
  min-height: 0;
}
.empty {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  opacity: 0.55;
  pointer-events: none;
}
.modal {
  position: fixed;
  inset: 0;
  display: grid;
  place-items: center;
  background: rgb(0 0 0 / 0.45);
}
.dialog {
  width: min(640px, 92vw);
  padding: 16px;
  background: var(--bar);
  border: 1px solid var(--border);
  border-radius: 10px;
}
.dialog h3 {
  margin: 0 0 6px;
}
.dialog textarea {
  width: 100%;
  font: 12px ui-monospace, Menlo, Consolas, monospace;
  background: var(--bg);
  color: var(--fg);
  border: 1px solid var(--border);
  border-radius: 6px;
}
.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: 10px;
}
</style>
