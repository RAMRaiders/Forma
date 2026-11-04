import { el, themeLogo } from './ui/dom.js';

/** What Forma puts on the shell's script tag for the cover (`CoverInfo` in forma-app). */
export interface CoverInfo {
  batocera: string;
  forma: string;
  theme: string;
  /** The theme's logo, drawn in place of its name, then its version. */
  themeLogo: string;
  themeVersion: string;
  /** Screen margins for a TV's overscan, percent per side. */
  margins: number;
  /**
   * At boot on Batocera, Forma's boot picture is under the see-through window: the cover lets it
   * show, and draws over it only what moves (the loader and the glow).
   */
  seeThrough: boolean;
}

/** The cover fades this long when the theme is ready. */
const FADE_MS = 300;

const SYSTEM = 'forma://localhost/system/';

/** The cover's details from the shell's own script tag; defaults if there are none. */
export function coverInfo(script: HTMLOrSVGScriptElement | null): CoverInfo {
  const data = script instanceof HTMLElement ? script.dataset : {};
  return {
    batocera: data.batocera ?? '',
    forma: data.forma ?? '',
    theme: data.theme ?? '',
    themeLogo: data.themeLogo ?? '',
    themeVersion: data.themeVersion ?? '',
    margins: Math.min(10, Math.max(0, Number(data.margins) || 0)),
    seeThrough: data.seeThrough === '1'
  };
}

/**
 * Forma's entry screen, over the theme page while it loads, whatever the theme: Forma's logo
 * over a breathing glow, a loader, and Batocera's, Forma's and the theme's versions in a
 * corner, as Forma's boot picture has them (forma-app's splash.rs), so the boot goes on from
 * that picture without a jump. It fades out as soon as the theme reports ready.
 */
export class Cover {
  readonly root: HTMLElement;
  #ready = false;
  #gone = false;
  /** Called once the cover has gone over a ready theme: Forma's UI is up. */
  onStarted: () => void = () => {};
  /** Called as the cover starts to go: the page under it may show. */
  onReveal: () => void = () => {};

  constructor(info: CoverInfo) {
    const versions = el(
      'ul',
      { class: 'versions' },
      info.batocera ? line(el('span', { class: 'batocera' }), 'Batocera', info.batocera) : null,
      line(el('img', { src: `${SYSTEM}forma-logo.svg`, alt: '' }), 'Forma', info.forma),
      info.theme ? line(el('span', { class: 'dot' }), 'Theme', themeLine(info)) : null
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
    this.root = el(
      'div',
      { class: info.seeThrough ? 'cover see-through' : 'cover' },
      el('div', { class: 'glow' }),
      forma
    );
  }

  /** The theme is ready: the cover fades out over it. */
  ready(): void {
    this.#ready = true;
    this.hide(true);
  }

  /** Gone, whatever is showing; safe to call more than once. `fade`: fading out first. */
  hide(fade = false): void {
    if (this.#gone) return;
    this.#gone = true;
    this.onReveal();
    // Forma's UI is up the moment the theme is: the fade is only for show.
    if (this.#ready) this.onStarted();
    if (!fade) return this.root.remove();
    // The page is shown first, and the cover fades off it only once it has been drawn: a Pi
    // takes a moment to draw a page that was hidden, and the screen went dark meanwhile.
    const frame = (then: () => void) =>
      typeof requestAnimationFrame === 'function' ? requestAnimationFrame(() => then()) : then();
    frame(() =>
      frame(() => {
        this.root.classList.add('leaving');
        setTimeout(() => this.root.remove(), FADE_MS);
      })
    );
  }
}

/** The theme as its logo and version, or its name and version without a logo. */
function themeLine(info: CoverInfo): Node | string {
  if (!info.themeLogo) return info.theme;
  const name = info.theme.slice(0, info.theme.length - info.themeVersion.length).trim();
  return el(
    'span',
    { class: 'with-logo' },
    themeLogo(info.themeLogo, name || info.theme),
    info.themeVersion
  );
}

function line(logo: HTMLElement, name: string, version: Node | string): HTMLElement {
  return el(
    'li',
    {},
    logo,
    el('span', { class: 'name' }, name),
    el('span', { class: 'version' }, version)
  );
}
