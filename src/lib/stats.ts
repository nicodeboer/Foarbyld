export function percentile(values: number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.max(0, Math.ceil((p / 100) * sorted.length) - 1));
  return sorted[idx];
}

export function mean(values: number[]): number {
  return values.length ? values.reduce((a, b) => a + b, 0) / values.length : 0;
}

const r1 = (n: number) => Math.round(n * 10) / 10;

export interface BenchmarkResult {
  timestamp: string;
  userAgent: string;
  devicePixelRatio: number;
  pageCount: number;
  /** Time from "open" until the first page was drawn. */
  firstPageMs: number | null;
  scroll: {
    distancePx: number;
    durationMs: number;
    frames: number;
    frameMsP50: number;
    frameMsP95: number;
    frameMsP99: number;
    /** frames slower than 25 ms (missed at least one 60 Hz vsync) */
    slowFrames25: number;
    /** frames slower than 50 ms (clearly visible stutter) */
    slowFrames50: number;
    avgFps: number;
  };
  render: {
    count: number;
    msMean: number;
    msP95: number;
  };
  /** Time until all visible pages were re-rendered after zooming 100% -> 200%. */
  zoomSettleMs: number;
  /** Time until all visible pages were drawn after the scroll test finished. */
  scrollSettleMs: number;
}

export function summarizeFrames(frameMs: number[], distancePx: number, durationMs: number) {
  // The first frame includes setup time; ignore it.
  const f = frameMs.slice(1);
  return {
    distancePx: Math.round(distancePx),
    durationMs: Math.round(durationMs),
    frames: f.length,
    frameMsP50: r1(percentile(f, 50)),
    frameMsP95: r1(percentile(f, 95)),
    frameMsP99: r1(percentile(f, 99)),
    slowFrames25: f.filter((x) => x > 25).length,
    slowFrames50: f.filter((x) => x > 50).length,
    avgFps: r1(f.length / (durationMs / 1000)),
  };
}

export function summarizeRenders(ms: number[]) {
  return { count: ms.length, msMean: r1(mean(ms)), msP95: r1(percentile(ms, 95)) };
}
