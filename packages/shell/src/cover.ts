import { el } from './ui/dom.js';

/** What Forma puts on the shell's script tag for the cover (`CoverInfo` in forma-app). */
export interface CoverInfo {
  /** Batocera's boot image first: the first page since Forma started. */
  boot: boolean;
  batocera: string;
  forma: string;
  theme: string;
  /** Screen margins for a TV's overscan, percent per side. */
  margins: number;
}

/** How long Batocera's boot image stays before Forma's screen. */
const BOOT_MS = 2000;
/** After the boot image, Forma's screen stays at least this long, however fast the theme is. */
const FORMA_MS = 1500;

const SYSTEM = 'forma://localhost/system/';

/** The cover's details from the shell's own script tag; defaults if there are none. */
export function coverInfo(script: HTMLOrSVGScriptElement | null): CoverInfo {
  const data = script instanceof HTMLElement ? script.dataset : {};
  return {
    boot: data.boot === '1',
    batocera: data.batocera ?? '',
    forma: data.forma ?? '',
    theme: data.theme ?? '',
    margins: Math.min(10, Math.max(0, Number(data.margins) || 0))
  };
}

/**
 * Forma's entry screen, over the theme page while it loads, whatever the theme. On the first
 * page since Forma started, Batocera's boot image; then Forma's logo and a loader, with
 * Batocera's, Forma's and the theme's versions in a corner. It goes, at once, when the theme
 * reports ready (when Forma starts, not before the boot image and Forma's screen have had their
 * time). Nothing moves but the loader.
 */
export class Cover {
  readonly root: HTMLElement;
  #ready = false;
  #booting: boolean;
  #gone = false;
  /** Called once the cover has gone over a ready theme: Forma's UI is up. */
  onStarted: () => void = () => {};

  constructor(info: CoverInfo) {
    const versions = el(
      'ul',
      { class: 'versions' },
      info.batocera ? line(el('span', { class: 'batocera' }), 'Batocera', info.batocera) : null,
      line(el('img', { src: `${SYSTEM}forma-logo.svg`, alt: '' }), 'Forma', info.forma),
      info.theme ? line(el('span', { class: 'dot' }), 'Theme', info.theme) : null
    );
    const forma = el(
      'div',
      { class: 'forma' },
      el('img', { class: 'mark', src: `${SYSTEM}forma-logo.svg`, alt: 'Forma' }),
      el('div', { class: 'bar', role: 'progressbar', 'aria-label': 'Loading' }, el('span', {})),
      versions
    );
    // Keep clear of the edges a TV cuts off, as themes do with the same setting.
    versions.style.setProperty('--margin', `${info.margins}vh`);
    this.root = el('div', { class: 'cover' }, forma);

    this.#booting = info.boot;
    if (info.boot) {
      const wide = typeof window === 'undefined' || window.innerWidth / window.innerHeight > 1.5;
      const image = el('img', {
        class: 'boot',
        src: `${SYSTEM}${wide ? 'boot-logo.png' : 'boot-logo-4x3.png'}`,
        alt: 'Batocera'
      });
      // No 4:3 image on this box: the wide one, cropped to fit.
      if (!wide) image.addEventListener('error', () => (image.src = `${SYSTEM}boot-logo.png`));
      this.root.append(image);
      setTimeout(() => image.remove(), BOOT_MS);
      setTimeout(() => {
        this.#booting = false;
        if (this.#ready) this.hide();
      }, BOOT_MS + FORMA_MS);
    }
  }

  /** The theme is ready: the cover goes, once the boot image has had its time. */
  ready(): void {
    this.#ready = true;
    if (!this.#booting) this.hide();
  }

  /** Gone, whatever is showing; safe to call more than once. */
  hide(): void {
    if (this.#gone) return;
    this.#gone = true;
    this.root.remove();
    if (this.#ready) this.onStarted();
  }
}

function line(logo: HTMLElement, name: string, version: string): HTMLElement {
  return el(
    'li',
    {},
    logo,
    el('span', { class: 'name' }, name),
    el('span', { class: 'version' }, version)
  );
}
