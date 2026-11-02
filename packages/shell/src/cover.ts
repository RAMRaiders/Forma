import { el } from './ui/dom.js';

/**
 * Forma's entry cover: its logo while the theme loads, so what follows Forma's start-up screen
 * (or a theme switch) is Forma, never a blank page or the theme's half-drawn first frame. It
 * matches the last frame of the start-up screen, and fades once the theme says it is ready.
 */
export class Cover {
  readonly root = el(
    'div',
    { class: 'cover', role: 'status', 'aria-label': 'Starting' },
    el('img', { class: 'mark', src: 'forma://localhost/system/forma-logo.svg', alt: '' }),
    el('p', { class: 'word' }, 'FORMA'),
    el('p', { class: 'theme' }),
    el('div', { class: 'bar', 'aria-hidden': 'true' }, el('span', {}))
  );
  #gone = false;

  /** The theme about to show, under the logo. */
  set theme(line: string) {
    const p = this.root.querySelector('.theme');
    if (p) p.textContent = line;
  }

  /** Fades away; safe to call more than once. */
  hide(): void {
    if (this.#gone) return;
    this.#gone = true;
    this.root.classList.add('gone');
    const remove = () => this.root.remove();
    this.root.addEventListener('transitionend', remove, { once: true });
    // No transition to end (reduced motion, a hidden page): gone anyway.
    setTimeout(remove, 800);
  }
}
