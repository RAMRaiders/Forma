import type { Forma, ScraperStatus } from '@ramraiders/forma-sdk';
import { ProgressCard } from './progresscard.js';

/**
 * A background scrape (systems or everything): how far it is and which ROM it is on. One game's
 * own scrape has its page; it shows nothing here.
 */
export class ScrapeProgress {
  readonly #card = new ProgressCard('scrape-progress');
  readonly root = this.#card.root;

  constructor(forma: Forma) {
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
    const share = status.total ? status.done / status.total : 0;
    const count = `${status.done} / ${status.total}`;
    if (status.running) {
      const title = `Scraping ${status.scope ?? ''}`.trim();
      this.#card.show(title, count, status.current ?? 'Starting…', share);
    } else {
      this.#card.show(
        status.stopped ? 'Scraping stopped' : 'Scraping done',
        count,
        `${status.found} found · ${status.notFound} not found`,
        share,
        true
      );
    }
  }
}
