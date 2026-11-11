/**
 * The theme's page held still while the screensaver shows (Dim leaves it in sight, the others
 * hide it; either way the Pi should not draw it): its CSS animations paused where they are,
 * its `<video>`s paused and played again after. The shell's own lie in its shadow root, out of
 * reach of the page's styles, and keep moving. Themes hear `screensaver` for their timers.
 */
const RULE =
  'html.forma-frozen *, html.forma-frozen *::before, html.forma-frozen *::after ' +
  '{ animation-play-state: paused !important; }';

let style: HTMLStyleElement | null = null;
let paused: HTMLVideoElement[] = [];

export function freezeTheme(on: boolean, doc: Document = document): void {
  const root = doc.documentElement;
  if (on) {
    if (!style || !style.isConnected) {
      style = doc.createElement('style');
      style.textContent = RULE;
      root.prepend(style);
    }
    root.classList.add('forma-frozen');
    paused = [...doc.querySelectorAll('video')].filter((v) => !v.paused && !v.ended);
    for (const v of paused) v.pause();
    return;
  }
  root.classList.remove('forma-frozen');
  for (const v of paused) if (v.isConnected) void v.play().catch(() => {});
  paused = [];
}
