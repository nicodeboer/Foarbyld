<script setup lang="ts">
import { ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";

// `html` comes from md-core, which sanitizes it (scripts, handlers, remote images).
const props = defineProps<{ html: string; zoom: number }>();
const emit = defineEmits<{ (e: "zoom-by", factor: number): void }>();

const scroller = ref<HTMLElement | null>(null);

watch(
  () => props.html,
  () => {
    if (scroller.value) scroller.value.scrollTop = 0;
  },
);

function onWheel(e: WheelEvent) {
  if (!e.ctrlKey) return;
  e.preventDefault();
  emit("zoom-by", e.deltaY < 0 ? 1.1 : 1 / 1.1);
}

/** Never let the webview navigate away from the app: open web links externally. */
function onClick(e: MouseEvent) {
  const a = (e.target as HTMLElement | null)?.closest("a");
  if (!a) return;
  e.preventDefault();
  const href = a.getAttribute("href") ?? "";
  if (/^(https?:|mailto:)/i.test(href)) void openUrl(href);
  else if (href.startsWith("#") && href.length > 1) {
    const id = decodeURIComponent(href.slice(1));
    scroller.value?.querySelector(`[id="${CSS.escape(id)}"]`)?.scrollIntoView();
  }
}

/**
 * Find the next match and select it. `window.find` is non-standard but is
 * implemented by WKWebView, WebKitGTK and WebView2, which covers every target.
 */
function find(query: string, caseSensitive: boolean, backwards: boolean): boolean {
  if (!query) return false;
  // window.find is missing from lib.dom.d.ts
  const f = (window as unknown as {
    find: (q: string, cs: boolean, back: boolean, wrap: boolean) => boolean;
  }).find;
  return f.call(window, query, caseSensitive, backwards, true);
}

defineExpose({ find });
</script>

<template>
  <div ref="scroller" class="scroller" @wheel="onWheel" @click="onClick">
    <!-- eslint-disable-next-line vue/no-v-html -->
    <article class="markdown" :style="{ fontSize: `${16 * zoom}px` }" v-html="html" />
  </div>
</template>

<style scoped>
.scroller {
  flex: 1;
  overflow: auto;
  background: var(--md-bg);
  color: var(--md-fg);
}
.markdown {
  max-width: 46em;
  margin: 0 auto;
  padding: 1.5em 1.25em 4em;
  line-height: 1.6;
  overflow-wrap: break-word;
  user-select: text;
  -webkit-user-select: text;
}
.markdown :deep(h1),
.markdown :deep(h2),
.markdown :deep(h3),
.markdown :deep(h4),
.markdown :deep(h5),
.markdown :deep(h6) {
  line-height: 1.25;
  margin: 1.5em 0 0.5em;
}
.markdown :deep(h1) { font-size: 2em; padding-bottom: 0.3em; border-bottom: 1px solid var(--border); }
.markdown :deep(h2) { font-size: 1.5em; padding-bottom: 0.3em; border-bottom: 1px solid var(--border); }
.markdown :deep(h3) { font-size: 1.25em; }
.markdown :deep(h4) { font-size: 1em; }
.markdown :deep(h5) { font-size: 0.875em; }
.markdown :deep(h6) { font-size: 0.85em; opacity: 0.75; }
.markdown > :deep(:first-child) { margin-top: 0; }
.markdown :deep(p),
.markdown :deep(ul),
.markdown :deep(ol),
.markdown :deep(blockquote),
.markdown :deep(pre),
.markdown :deep(table) {
  margin: 0 0 1em;
}
.markdown :deep(a) { color: var(--accent); }
.markdown :deep(ul),
.markdown :deep(ol) { padding-left: 2em; }
.markdown :deep(li > ul),
.markdown :deep(li > ol) { margin: 0.25em 0; }
.markdown :deep(li input[type="checkbox"]) { margin: 0 0.4em 0 -1.3em; }
.markdown :deep(blockquote) {
  padding: 0 1em;
  border-left: 0.25em solid var(--border);
  opacity: 0.85;
}
.markdown :deep(code) {
  font: 0.875em ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  padding: 0.15em 0.35em;
  background: var(--md-code-bg);
  border-radius: 4px;
}
.markdown :deep(pre) {
  padding: 0.9em 1em;
  overflow: auto;
  background: var(--md-code-bg);
  border-radius: 6px;
  line-height: 1.45;
}
.markdown :deep(pre code) { padding: 0; background: none; font-size: 0.85em; }
.markdown :deep(img) { max-width: 100%; height: auto; }
.markdown :deep(hr) { border: 0; border-top: 1px solid var(--border); margin: 1.5em 0; }
.markdown :deep(table) { border-collapse: collapse; display: block; overflow: auto; }
.markdown :deep(th),
.markdown :deep(td) { padding: 0.35em 0.8em; border: 1px solid var(--border); }
.markdown :deep(tr:nth-child(even)) { background: var(--md-code-bg); }
.markdown :deep(.footnote-definition) { font-size: 0.9em; }
</style>
