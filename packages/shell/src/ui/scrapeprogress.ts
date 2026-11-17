import type { Forma, ScraperStatus } from '@ramraiders/forma-sdk';
import { el } from './dom.js';

/** How long the last word (done, stopped) stays before the corner empties. */
const LINGER_MS = 5000;

/**
 * A background scrape (systems or everything), in the bottom-left corner over any theme: how
 * far it is and which ROM it is on. Clear of the music card (top right) and of the hints
 * (bottom right). One game's own scrape has its page; it shows nothing here.
 */
export class ScrapeProgress {
  readonly root = el('div', { class: 'scrape-progress', part: 'scrape-progress', hidden: '' });
  readonly #title = el('span', { class: 'title' });
  readonly #count = el('span', { class: 'count' });
  readonly #rom = el('span', { class: 'rom' });
  readonly #fill = el('i');
  #linger: ReturnType<typeof setTimeout> | null = null;

  constructor(forma: Forma) {
    this.root.setAttribute('role', 'status');
    this.root.append(
      this.#title,
      this.#count,
      this.#rom,
      el('div', { class: 'bar', 'aria-hidden': 'true' }, this.#fill)
    );
    forma.events.on('scraper:progress', (status) => this.show(status));
    forma.scraper
      .status()
      .then((status) => {
        if (status.running) this.show(status);
      })
      .catch(() => {});
  }

  show(status: ScraperStatus): void {
    if (status.single) return;
    if (this.#linger) clearTimeout(this.#linger);
    this.#linger = null;
    const share = status.total ? Math.min(1, status.done / status.total) : 0;
    this.#fill.style.transform = `scaleX(${share})`;
    this.#count.textContent = `${status.done} / ${status.total}`;
    if (status.running) {
      this.#title.textContent = `Scraping ${status.scope ?? ''}`.trim();
      this.#rom.textContent = status.current ?? 'Starting…';
    } else {
      this.#title.textContent = status.stopped ? 'Scraping stopped' : 'Scraping done';
      this.#rom.textContent = `${status.found} found · ${status.notFound} not found`;
      this.#linger = setTimeout(() => (this.root.hidden = true), LINGER_MS);
    }
    this.root.hidden = false;
  }
}
