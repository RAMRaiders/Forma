import { el } from './dom.js';

/** How long the last word (done, stopped) stays before the card goes. */
export const LINGER_MS = 5000;

/**
 * Work Forma does in the background (a scrape, games being identified), at the top centre
 * over any theme: what it is, how far, and what it is on, over a bar. Clear of the music card
 * (top right) and of the breadcrumbs (top left). Several stack there, each its own.
 */
export class ProgressCard {
  readonly root: HTMLElement;
  readonly #title = el('span', { class: 'title' });
  readonly #count = el('span', { class: 'count' });
  readonly #detail = el('span', { class: 'rom' });
  readonly #fill = el('i');
  #linger: ReturnType<typeof setTimeout> | null = null;

  /** `name`: its class and part, for themes (`scrape-progress`, `identify-progress`). */
  constructor(name: string) {
    this.root = el('div', { class: `progress-card ${name}`, part: name, hidden: '' });
    this.root.setAttribute('role', 'status');
    this.root.append(
      this.#title,
      this.#count,
      this.#detail,
      el('div', { class: 'bar', 'aria-hidden': 'true' }, this.#fill)
    );
  }

  /**
   * Shows it: `share` of the bar filled (0 to 1). `last`: the work ended, and the card goes
   * after a moment.
   */
  show(title: string, count: string, detail: string, share: number, last = false): void {
    if (this.#linger) clearTimeout(this.#linger);
    this.#linger = null;
    this.#fill.style.transform = `scaleX(${Math.max(0, Math.min(1, share))})`;
    this.#title.textContent = title;
    this.#count.textContent = count;
    this.#detail.textContent = detail;
    if (last) this.#linger = setTimeout(() => (this.root.hidden = true), LINGER_MS);
    this.root.hidden = false;
  }
}
