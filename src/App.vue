<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, shallowRef, watch } from "vue";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import MarkdownViewer from "./components/MarkdownViewer.vue";
import PdfViewer from "./components/PdfViewer.vue";
import ThumbnailSidebar from "./components/ThumbnailSidebar.vue";
import {
  closePdf,
  engineError,
  initialFile,
  IMAGE_EXTENSIONS,
  isMarkdownFile,
  isSupportedFile,
  MARKDOWN_EXTENSIONS,
  MAX_MATCHES,
  openDocument,
  openMarkdown,
  PDF_EXTENSIONS,
  searchText,
  type DocInfo,
  type SearchMatch,
} from "./lib/api";

const doc = shallowRef<DocInfo | null>(null);
const fileName = ref("");
const zoom = ref(1);
const error = ref<string | null>(null);
const dragging = ref(false);
const firstPageMs = ref<number | null>(null);
const sidebarOpen = ref(true);
const currentPage = ref(0);
const viewer = ref<InstanceType<typeof PdfViewer> | null>(null);
const mdViewer = ref<InstanceType<typeof MarkdownViewer> | null>(null);
/** Rendered HTML of the open Markdown file; mutually exclusive with `doc`. */
const markdown = ref<string | null>(null);

let openStartedAt = 0;

const ZOOM_MIN = 0.25;
const ZOOM_MAX = 5;
const setZoom = (z: number) => (zoom.value = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z)));
const zoomBy = (f: number) => setZoom(zoom.value * f);
const hasContent = () => doc.value !== null || markdown.value !== null;
const fitWidth = () => setZoom(viewer.value && doc.value ? viewer.value.fitWidthZoom() : 1);

async function openPath(path: string) {
  error.value = null;
  firstPageMs.value = null;
  const previous = doc.value;
  resetFind();
  try {
    if (isMarkdownFile(path)) {
      markdown.value = await openMarkdown(path);
      doc.value = null;
      fileName.value = path.split(/[\\/]/).pop() ?? path;
      if (previous) void closePdf(previous.id);
      setZoom(1);
      return;
    }
    openStartedAt = performance.now();
    const info = await openDocument(path);
    markdown.value = null;
    doc.value = info;
    fileName.value = path.split(/[\\/]/).pop() ?? path;
    if (previous) void closePdf(previous.id);
    // Default to fit-width so the first view is sensible on any window size.
    requestAnimationFrame(fitWidth);
  } catch (e) {
    error.value = String(e);
  }
}

async function chooseFile() {
  const picked = await open({
    multiple: false,
    filters: [
      {
        name: "Documents and images",
        extensions: [...PDF_EXTENSIONS, ...MARKDOWN_EXTENSIONS, ...IMAGE_EXTENSIONS],
      },
      { name: "PDF", extensions: PDF_EXTENSIONS },
      { name: "Markdown", extensions: MARKDOWN_EXTENSIONS },
      { name: "Images", extensions: IMAGE_EXTENSIONS },
    ],
  });
  if (typeof picked === "string") await openPath(picked);
}

function onFirstRender() {
  firstPageMs.value = performance.now() - openStartedAt;
}

// ---- find in document ---------------------------------------------------------
const findOpen = ref(false);
const query = ref("");
const caseSensitive = ref(false);
const matches = shallowRef<SearchMatch[]>([]);
const current = ref(-1);
const searching = ref(false);
const mdFindMiss = ref(false); // last Markdown find found nothing
const findInput = ref<HTMLInputElement | null>(null);
let searchSeq = 0;
let searchTimer: number | undefined;
let searchPending = false; // typed since the last search ran (debounce running)

const canSearch = () => doc.value?.kind === "pdf" || markdown.value !== null;

function resetFind() {
  searchPending = false;
  searchSeq++;
  clearTimeout(searchTimer);
  matches.value = [];
  current.value = -1;
  searching.value = false;
  mdFindMiss.value = false;
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
  mdFindMiss.value = false;
  if (markdown.value !== null) return; // Markdown is searched on Enter, see step()
  searchPending = true;
  clearTimeout(searchTimer);
  searchTimer = window.setTimeout(runSearch, 200);
});

function step(delta: number) {
  if (markdown.value !== null) {
    // window.find selects the match in the page, which moves focus out of the
    // input; onWindowKey keeps Enter / Shift+Enter working from there.
    mdFindMiss.value = !(mdViewer.value?.find(query.value, caseSensitive.value, delta < 0) ?? false);
    return;
  }
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
    if (markdown.value !== null) step(e.shiftKey ? -1 : 1);
    else if (searchPending) void runSearch(); // Enter right after typing: do not wait for the debounce
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
  } else if (findOpen.value && markdown.value !== null && (e.target as HTMLElement | null)?.tagName !== "INPUT") {
    if (e.key === "Enter") {
      e.preventDefault();
      step(e.shiftKey ? -1 : 1);
    } else if (e.key === "Escape") {
      closeFind();
    }
  }
}

const findStatus = () => {
  if (markdown.value !== null) return mdFindMiss.value ? "No results" : "";
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
      <button :disabled="!hasContent()" title="Zoom out" @click="zoomBy(1 / 1.25)">−</button>
      <span class="zoom">{{ Math.round(zoom * 100) }}%</span>
      <button :disabled="!hasContent()" title="Zoom in" @click="zoomBy(1.25)">+</button>
      <button :disabled="!hasContent()" @click="fitWidth">Fit width</button>
      <button
        :disabled="!doc || doc.kind !== 'pdf'"
        :class="{ active: sidebarOpen }"
        title="Show or hide page thumbnails"
        @click="sidebarOpen = !sidebarOpen"
      >
        Pages
      </button>
      <button :disabled="!canSearch()" title="Find (Ctrl/Cmd+F)" @click="openFind">Find</button>
      <span class="spacer" />
      <span v-if="markdown !== null" class="info">{{ fileName }}</span>
      <span v-else-if="doc" class="info">
        {{ fileName }} ·
        <template v-if="doc.kind === 'image'">
          {{ Math.round(doc.pages[0].width / 0.75) }} × {{ Math.round(doc.pages[0].height / 0.75) }} px
        </template>
        <template v-else>page {{ currentPage + 1 }} / {{ doc.pageCount }}</template>
        <template v-if="firstPageMs !== null"> · first page {{ Math.round(firstPageMs) }} ms</template>
      </span>
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
      <ThumbnailSidebar
        v-if="doc && doc.kind === 'pdf' && sidebarOpen"
        :doc="doc"
        :current-page="currentPage"
        @select="(i: number) => viewer?.scrollToPage(i)"
      />
      <PdfViewer
        ref="viewer"
        :doc="doc"
        :zoom="zoom"
        :matches="matches"
        :current-match="current"
        @first-render="onFirstRender"
        @zoom-by="zoomBy"
        @page-change="(i: number) => (currentPage = i)"
        @error="(m: string) => (error = m)"
      />
      <MarkdownViewer v-if="markdown !== null" ref="mdViewer" :html="markdown" :zoom="zoom" @zoom-by="zoomBy" />
      <div v-if="!doc && markdown === null" class="empty">
        Drop a PDF, Markdown file or image here or press “Open…”
      </div>
    </main>

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
button.active {
  border-color: var(--accent);
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
</style>
