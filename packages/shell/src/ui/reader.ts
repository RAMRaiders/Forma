import {
  type Forma,
  FormaError,
  type Game,
  type InputEvent,
  type Magazine,
  type MediaKind
} from '@ramraiders/forma-sdk';

import { el } from './dom.js';

/** Zoom steps, from fitted to the screen: A or R2 goes up one, X or L2 down one. */
const ZOOMS = [1, 1.5, 2, 3, 4] as const;
/** The widest page Forma makes. */
const MAX_WIDTH = 2560;

/** What the reader shows: pages (a magazine, a manual) or pictures (a game's media). */
export interface Document {
  title: string;
  count: number;
  /** Where to open, from 0. */
  start?: number;
  /** Page `at`, about `width` pixels wide. */
  url(at: number, width: number): string;
  /** Names instead of numbers (media kinds); with them, "Fan art" rather than "3 / 8". */
  labels?: string[];
  /** The game whose video page `at` is, played by Forma instead of drawn. */
  video?(at: number): number | null;
  /** The user is on page `at`. */
  onPage?(at: number): void;
  /** Gone, after a video played: the theme may show its own again. */
  onClose?(playedVideo: boolean): void;
}

/** Media kinds in words, in the order they are shown; the manual is read, not shown. */
const MEDIA: [MediaKind, string][] = [
  ['video', 'Video'],
  ['image', 'Screenshot'],
  ['titleshot', 'Title screen'],
  ['thumbnail', 'Box'],
  ['boxart', 'Box art'],
  ['boxback', 'Box back'],
  ['boxside', 'Box spine'],
  ['boxfull', 'Whole box'],
  ['cartridge', 'Cartridge'],
  ['fanart', 'Fan art'],
  ['marquee', 'Marquee'],
  ['wheel', 'Logo'],
  ['banner', 'Banner'],
  ['flyer', 'Flyer'],
  ['cabinet', 'Cabinet'],
  ['map', 'Map'],
  ['mix', 'Mix'],
  ['bezel', 'Bezel']
];

/** The media a game has to show, in order, with their names. */
export function mediaToShow(game: Pick<Game, 'media'>): [MediaKind, string][] {
  return MEDIA.filter(([kind]) => game.media[kind]);
}

/**
 * Forma's reader, over everything: a magazine, a game's manual or its media, one page fitted to
 * the screen. Left and right (or L1 and R1) turn pages; A or R2 zooms in, X or L2 out, and,
 * zoomed, the pad moves over the page (L1 and R1 still turn). B closes.
 */
export class Reader {
  readonly root = el('div', { class: 'reader', hidden: '', role: 'dialog' });
  readonly #sheet = el('div', { class: 'sheet' });
  readonly #page = el('img', { class: 'page', alt: '' });
  readonly #status = el('p', { class: 'where', role: 'status' });
  #doc: Document | null = null;
  #at = 0;
  #zoom = 0;
  #playing = false;
  #played = false;
  #seeThrough: HTMLStyleElement | null = null;
  /** The next page, loading while this one is read. */
  #ahead: HTMLImageElement | null = null;

  constructor(
    private readonly forma: Forma,
    private readonly notify: (message: string) => void
  ) {
    this.#sheet.append(this.#page);
    this.root.append(this.#sheet, this.#status);
    this.#page.addEventListener('error', () => {
      if (this.open && !this.#playing) {
        this.#status.textContent = `${this.#label()} · this page cannot be shown`;
      }
    });
    // Zoomed, its size comes from the picture's shape, known once it has loaded.
    this.#page.addEventListener('load', () => {
      if (this.#zoom > 0) this.#size();
    });
  }

  get open(): boolean {
    return !this.root.hidden;
  }

  /** A magazine, where it was left. */
  async show(magazine: Magazine): Promise<void> {
    let count: number;
    try {
      count = await this.forma.magazines.pages(magazine.id);
    } catch (error) {
      this.notify(FormaError.from(error).message);
      return;
    }
    this.showDocument({
      title: magazine.title,
      count,
      start: magazine.page ?? 0,
      url: (at, width) => this.forma.magazines.pageUrl(magazine, at, width),
      onPage: (at) => void this.forma.magazines.setPage(magazine.id, at).catch(() => {})
    });
  }

  /** A game's manual, from its first page. */
  async showManual(game: { id: number; name: string }): Promise<void> {
    const manual = await this.forma.library.manual(game.id).catch(() => null);
    if (!manual || manual.pages === 0) return this.notify('This manual cannot be read');
    this.showDocument({
      title: `${game.name} manual`,
      count: manual.pages,
      url: (at, width) => this.forma.library.manualPageUrl(manual, at, width)
    });
  }

  /** A game's pictures and video, one by one. `restore` when its video played. */
  showMedia(game: Pick<Game, 'id' | 'name' | 'media'>, restore?: () => void): void {
    const shown = mediaToShow(game);
    if (shown.length === 0) return this.notify('No pictures or video for this game');
    this.showDocument({
      title: game.name,
      count: shown.length,
      labels: shown.map(([, label]) => label),
      url: (at, width) => this.forma.library.mediaUrl(game.id, shown[at]![0], width),
      video: (at) => (shown[at]?.[0] === 'video' ? game.id : null),
      onClose: (played) => {
        if (played) restore?.();
      }
    });
  }

  showDocument(doc: Document): void {
    this.#doc = doc;
    this.#at = Math.min(Math.max(0, doc.start ?? 0), doc.count - 1);
    this.#zoom = 0;
    this.#played = false;
    this.root.hidden = false;
    this.#draw();
  }

  close(): void {
    if (this.#doc) this.#doc.onPage?.(this.#at);
    const doc = this.#doc;
    this.#stopVideo();
    this.root.hidden = true;
    this.#page.removeAttribute('src');
    this.#ahead = null;
    this.#doc = null;
    doc?.onClose?.(this.#played);
  }

  input(e: InputEvent): void {
    if (!e.pressed || !this.#doc) return;
    const turn = (step: number) => {
      const next = Math.min(this.#doc!.count - 1, Math.max(0, this.#at + step));
      if (next === this.#at) return;
      this.#at = next;
      this.#draw();
      this.#doc!.onPage?.(this.#at);
    };
    const zoomed = this.#zoom > 0 && !this.#playing;
    const pan = (x: number, y: number) => {
      this.#sheet.scrollLeft += x * this.#sheet.clientWidth * 0.4;
      this.#sheet.scrollTop += y * this.#sheet.clientHeight * 0.4;
    };
    switch (e.action) {
      case 'pageDown':
        return turn(1);
      case 'pageUp':
        return turn(-1);
      case 'right':
        return zoomed ? pan(1, 0) : turn(1);
      case 'left':
        return zoomed ? pan(-1, 0) : turn(-1);
      case 'up':
        return zoomed ? pan(0, -1) : undefined;
      case 'down':
        return zoomed ? pan(0, 1) : undefined;
      case 'confirm':
      case 'r2':
        if (!e.repeat) this.#zoomTo(this.#zoom + 1);
        return;
      case 'west':
      case 'l2':
        if (!e.repeat) this.#zoomTo(this.#zoom - 1);
        return;
      case 'back':
        if (!e.repeat) this.close();
        return;
      default:
    }
  }

  #label(): string {
    const doc = this.#doc;
    if (!doc) return '';
    const where = doc.labels?.[this.#at] ?? `${this.#at + 1} / ${doc.count}`;
    return doc.labels && doc.count > 1
      ? `${doc.title} · ${where} (${this.#at + 1} / ${doc.count})`
      : `${doc.title} · ${where}`;
  }

  #hint(): string {
    if (this.#playing) return '';
    if (this.#zoom === 0) return ' · A: zoom';
    const out = this.#zoom === ZOOMS.length - 1 ? '' : 'A: in · ';
    return ` · ${ZOOMS[this.#zoom]}× · ${out}X: out`;
  }

  #zoomTo(level: number): void {
    if (this.#playing) return;
    const next = Math.min(ZOOMS.length - 1, Math.max(0, level));
    if (next === this.#zoom) return;
    // The spot in the middle of the screen stays there.
    const s = this.#sheet;
    const cx = s.scrollWidth ? (s.scrollLeft + s.clientWidth / 2) / s.scrollWidth : 0.5;
    const cy = s.scrollHeight ? (s.scrollTop + s.clientHeight / 2) / s.scrollHeight : 0;
    this.#zoom = next;
    // Back to 1×: the whole page fitted to the screen again, from where it starts.
    if (next === 0) return this.#draw(true);
    this.#draw(false);
    s.scrollLeft = cx * s.scrollWidth - s.clientWidth / 2;
    s.scrollTop = cy * s.scrollHeight - s.clientHeight / 2;
  }

  /** The page's width on screen, fitted: the whole page in sight. */
  #fitted(): number {
    const w = this.#sheet.clientWidth || globalThis.innerWidth || 1280;
    const h = this.#sheet.clientHeight || globalThis.innerHeight || 720;
    const { naturalWidth: nw, naturalHeight: nh } = this.#page;
    // Before it has loaded, a page's usual shape.
    return nw && nh ? Math.min(w, (h * nw) / nh) : Math.min(w, h * 0.75);
  }

  /** Zoomed, the picture's size; fitted, CSS fits it. */
  #size(): void {
    const scale = ZOOMS[this.#zoom] ?? 1;
    this.#page.style.width = this.#zoom > 0 ? `${Math.round(this.#fitted() * scale)}px` : '';
  }

  /** The width to ask for: as wide as it is drawn, on this screen. */
  #width(): number {
    const dpr = globalThis.devicePixelRatio || 1;
    return Math.min(MAX_WIDTH, Math.round(this.#fitted() * (ZOOMS[this.#zoom] ?? 1) * dpr));
  }

  #draw(fresh = true): void {
    const doc = this.#doc;
    if (!doc) return;
    const game = doc.video?.(this.#at) ?? null;
    if (game !== null) return this.#showVideo(game);
    this.#stopVideo();
    const width = this.#width();
    this.root.classList.toggle('zoomed', this.#zoom > 0);
    this.#size();
    this.#page.hidden = false;
    this.#page.src = doc.url(this.#at, width);
    if (fresh) {
      this.#sheet.scrollTop = 0;
      this.#sheet.scrollLeft = 0;
    }
    this.#status.textContent = `${this.#label()}${this.#hint()}`;
    const next = this.#at + 1;
    if (next < doc.count && (doc.video?.(next) ?? null) === null) {
      this.#ahead = new Image();
      this.#ahead.src = doc.url(next, width);
    }
  }

  /** A video: Forma plays it under the page, which lets it show through. */
  #showVideo(gameId: number): void {
    this.#zoom = 0;
    this.#playing = true;
    this.#played = true;
    this.root.classList.remove('zoomed');
    this.root.classList.add('video');
    this.#page.hidden = true;
    this.#page.removeAttribute('src');
    this.#status.textContent = this.#label();
    if (!this.#seeThrough) {
      this.#seeThrough = document.createElement('style');
      this.#seeThrough.textContent =
        'html, body { visibility: hidden !important; background: transparent !important; }';
      document.documentElement.append(this.#seeThrough);
    }
    void this.forma.video.play(gameId).catch(() => {
      if (this.#playing) this.#status.textContent = `${this.#label()} · it cannot be played`;
    });
  }

  #stopVideo(): void {
    if (!this.#playing) return;
    this.#playing = false;
    this.root.classList.remove('video');
    this.#seeThrough?.remove();
    this.#seeThrough = null;
    void this.forma.video.stop().catch(() => {});
  }
}
