import { type Forma, FormaError, type InputEvent, type Magazine } from '@ramraiders/forma-sdk';

import { el } from './dom.js';

/**
 * Forma's magazine reader, over everything: one page fitted to the screen. Left and right (or
 * L1 and R1) turn pages, A zooms to the page's width and up and down scroll it, B closes. The
 * page the user is on is kept, to open there next time.
 */
export class Reader {
  readonly root = el('div', { class: 'reader', hidden: '', role: 'dialog' });
  readonly #page = el('img', { class: 'page', alt: '' });
  readonly #status = el('p', { class: 'where', role: 'status' });
  #magazine: Magazine | null = null;
  #count = 0;
  #at = 0;
  #zoomed = false;
  /** The next page, loading while this one is read. */
  #ahead: HTMLImageElement | null = null;

  constructor(
    private readonly forma: Forma,
    private readonly notify: (message: string) => void
  ) {
    this.root.append(el('div', { class: 'sheet' }, this.#page), this.#status);
    this.#page.addEventListener('error', () => {
      if (this.open) this.#status.textContent = `${this.#label()} · this page cannot be shown`;
    });
  }

  get open(): boolean {
    return !this.root.hidden;
  }

  async show(magazine: Magazine): Promise<void> {
    try {
      this.#count = await this.forma.magazines.pages(magazine.id);
    } catch (error) {
      this.notify(FormaError.from(error).message);
      return;
    }
    this.#magazine = magazine;
    this.#at = Math.min(Math.max(0, magazine.page ?? 0), this.#count - 1);
    this.#zoomed = false;
    this.root.hidden = false;
    this.#draw();
  }

  close(): void {
    if (this.#magazine) {
      void this.forma.magazines.setPage(this.#magazine.id, this.#at).catch(() => {});
    }
    this.root.hidden = true;
    this.#page.removeAttribute('src');
    this.#ahead = null;
  }

  input(e: InputEvent): void {
    if (!e.pressed) return;
    const turn = (step: number) => {
      const next = Math.min(this.#count - 1, Math.max(0, this.#at + step));
      if (next === this.#at) return;
      this.#at = next;
      this.#draw();
      void this.forma.magazines.setPage(this.#magazine!.id, this.#at).catch(() => {});
    };
    switch (e.action) {
      case 'right':
      case 'pageDown':
        return turn(1);
      case 'left':
      case 'pageUp':
        return turn(-1);
      case 'up':
      case 'down':
        if (this.#zoomed) {
          const sheet = this.root.firstElementChild as HTMLElement;
          sheet.scrollBy({ top: (e.action === 'down' ? 1 : -1) * sheet.clientHeight * 0.4 });
        }
        return;
      case 'confirm':
        if (e.repeat) return;
        this.#zoomed = !this.#zoomed;
        return this.#draw();
      case 'back':
        if (!e.repeat) this.close();
        return;
      default:
    }
  }

  #label(): string {
    return `${this.#magazine?.title ?? ''} · ${this.#at + 1} / ${this.#count}`;
  }

  /** The width to ask for: the screen's, zoomed; about a page's at the screen's height, fitted. */
  #width(): number {
    const dpr = globalThis.devicePixelRatio || 1;
    const w = globalThis.innerWidth || 1280;
    const h = globalThis.innerHeight || 720;
    return Math.round((this.#zoomed ? w : Math.min(w, h * 0.75)) * dpr);
  }

  #draw(): void {
    const m = this.#magazine;
    if (!m) return;
    const width = this.#width();
    this.root.classList.toggle('zoomed', this.#zoomed);
    this.#page.src = this.forma.magazines.pageUrl(m, this.#at, width);
    (this.root.firstElementChild as HTMLElement).scrollTop = 0;
    this.#status.textContent = `${this.#label()}${this.#zoomed ? ' · A: whole page' : ' · A: zoom'}`;
    if (this.#at + 1 < this.#count) {
      this.#ahead = new Image();
      this.#ahead.src = this.forma.magazines.pageUrl(m, this.#at + 1, width);
    }
  }
}
