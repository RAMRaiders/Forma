import { el } from './dom.js';

/** Frames slower than this are counted as dropped at 60 Hz (one missed vsync and more). */
const DROPPED_MS = 1000 / 60 + 8;
/** Seconds of frames the numbers cover. */
const WINDOW_S = 3;

/**
 * Frame timing for the Batocera spike (#39): shown when the page is opened with `#perf`, which
 * RomDex adds when started with `ROMDEX_PERF=1`. Scroll through a long list and read it off.
 */
export function startPerfOverlay(): void {
  const panel = el('div', { class: 'perf', 'aria-hidden': 'true' });
  document.body.append(panel);
  const frames: number[] = [];
  let last = performance.now();
  let dropped = 0;
  let lastPaint = 0;

  const tick = (now: number) => {
    const ms = now - last;
    last = now;
    frames.push(ms);
    if (ms > DROPPED_MS) dropped += 1;
    let total = frames.reduce((a, b) => a + b, 0);
    while (total > WINDOW_S * 1000 && frames.length > 1) total -= frames.shift()!;

    if (now - lastPaint > 250) {
      lastPaint = now;
      const sorted = [...frames].sort((a, b) => a - b);
      const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? 0;
      const fps = (frames.length / total) * 1000;
      panel.textContent = `${fps.toFixed(0)} fps · p95 ${p95.toFixed(1)} ms · worst ${Math.max(...frames).toFixed(0)} ms · dropped ${dropped}`;
    }
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);
}
