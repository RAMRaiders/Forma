import type { InputEvent } from '@ramraiders/forma-sdk';
import { FormaError } from '@ramraiders/forma-sdk';

import { moveList } from '../logic/nav.js';
import { el, themeLogo } from './dom.js';
import { type Ask, Keyboard } from './keyboard.js';

export interface Row {
  label: string;
  detail?: string;
  /** A, or nothing for a row that only shows something. */
  run?: () => unknown;
  /** Left/right, for switches and choices. */
  adjust?: (step: -1 | 1) => unknown;
  /** Y: a second action on the row (e.g. remove). */
  alt?: { label: string; run: () => unknown };
  /** Starts a group of rows, with this heading. */
  heading?: string;
  /** A picture under the row: a choice between pictures. */
  image?: string | null;
  /** A logo drawn in place of the label's name (`name`), followed by `text`. */
  logo?: { src: string; name: string; text?: string };
  /** A small tag beside the label: about the row, apart from its value. `kept`: nothing new. */
  badge?: { text: string; kept?: boolean };
  /** A small picture before the label (see `.icon-*` in style.css). */
  icon?:
    | 'scrape'
    | 'edit'
    | 'emulator'
    | 'manual'
    | 'media'
    | 'filter'
    | 'theme'
    | 'delete'
    | 'collections'
    | 'netplay'
    | 'play';
  /** It destroys something: shown apart, in red. */
  danger?: boolean;
}

/** A page left for a deeper one, to come back to where it was. */
interface Level {
  title: string;
  build: () => Row[] | Promise<Row[]>;
  note: string | null;
  file: string | null;
  selected: number;
}

/**
 * A settings page over the screen: a title, a note, rows. A runs a row, left/right adjusts it,
 * B goes back. Rows come from `build`, called again after every change, so they always show
 * what Forma says now; the selection stays where it was. A row can open a deeper page (`push`);
 * B then comes back to the row it was opened from.
 */
export class Page {
  readonly keyboard = new Keyboard();
  readonly root = el('div', { class: 'overlay switcher page', hidden: '' });
  #title = '';
  #build: () => Row[] | Promise<Row[]> = () => [];
  #rows: Row[] = [];
  #selected = 0;
  #busy: string | null = null;
  #note: string | null = null;
  /** A small line under the title, e.g. the game's file name. */
  #file: string | null = null;
  #parents: Level[] = [];
  /** Asked to open, its rows not read yet: it is the shell's already, and shows once whole. */
  #opening = false;

  constructor(
    readonly notify: (message: string) => void,
    /** After every change on screen, so the button hints can follow. */
    private readonly changed: () => void
  ) {}

  get open(): boolean {
    return this.#opening || !this.root.hidden;
  }

  /** The row chosen on the page on screen. */
  get selected(): number {
    return this.#selected;
  }

  /** `file`: a small line under the title, e.g. the game's ROM file name. `at`: the row chosen. */
  async show(
    title: string,
    build: () => Row[] | Promise<Row[]>,
    note?: string,
    file?: string,
    at = 0
  ): Promise<void> {
    this.#parents = [];
    await this.#open(title, build, note, at, file);
  }

  /** A deeper page, starting on row `at`; B comes back here. */
  async push(
    title: string,
    build: () => Row[] | Promise<Row[]>,
    note?: string,
    at = 0
  ): Promise<void> {
    this.#parents.push({
      title: this.#title,
      build: this.#build,
      note: this.#note,
      file: this.#file,
      selected: this.#selected
    });
    return this.#open(title, build, note, at);
  }

  /** Back to the page this one was opened from, or closed when there is none. */
  async pop(): Promise<void> {
    const parent = this.#parents.pop();
    if (!parent) return this.close();
    this.#title = parent.title;
    this.#build = parent.build;
    this.#note = parent.note;
    this.#file = parent.file;
    this.#selected = parent.selected;
    this.#rows = [];
    await this.refresh();
  }

  async #open(
    title: string,
    build: () => Row[] | Promise<Row[]>,
    note?: string,
    at = 0,
    file?: string
  ): Promise<void> {
    this.#title = title;
    this.#file = file ?? null;
    this.#build = build;
    this.#note = note ?? null;
    this.#selected = at;
    this.#rows = [];
    if (!this.root.hidden) return this.refresh();
    // Shut until now: it comes up with its rows, never empty or with what it showed last time.
    this.#opening = true;
    await this.#read();
    // Closed while it read: it stays shut.
    if (!this.#opening || this.#build !== build) return;
    this.#opening = false;
    this.root.hidden = false;
    this.#render();
  }

  close(): void {
    this.#parents = [];
    this.#opening = false;
    this.root.hidden = true;
    // Nothing of this page is there to flash by when the next one opens.
    this.root.replaceChildren();
    this.changed();
  }

  /** Whether `build` is on screen, or a page under it that B comes back to. */
  has(build: () => Row[] | Promise<Row[]>): boolean {
    return this.open && (this.#build === build || this.#parents.some((l) => l.build === build));
  }

  /** Whether `build` is the page on screen. */
  showing(build: () => Row[] | Promise<Row[]>): boolean {
    return this.open && this.#build === build && !this.#busy;
  }

  /** Builds the rows again from what Forma says now. */
  async refresh(): Promise<void> {
    await this.#read();
    // Still coming up (`#open` shows it), or shut while it read: nothing to draw.
    if (!this.root.hidden) this.#render();
  }

  async #read(): Promise<void> {
    // The selection follows its row: playing something adds rows above it, and A again must not
    // land on whatever moved into its place (Stop, say). Rows can share a label (two `Video
    // mode`s in different groups), so the one at the same place wins.
    const was = this.#rows[this.#selected]?.label;
    const at = this.#selected;
    try {
      this.#rows = await this.#build();
      if (this.#rows[at]?.label !== was) {
        const same = this.#rows.findIndex((r) => r.label === was);
        if (same >= 0) this.#selected = same;
      }
    } catch (error) {
      this.#rows = [];
      this.#note = FormaError.from(error).message;
    }
  }

  /** Runs a change with the page showing `label`, then reads everything again. */
  async run(label: string, work: () => Promise<string | void>): Promise<void> {
    this.#busy = label;
    this.#render();
    try {
      const done = await work();
      if (done) this.notify(done);
    } catch (error) {
      this.notify(FormaError.from(error).message);
    } finally {
      this.#busy = null;
      await this.refresh();
    }
  }

  /** While `run` works: what it is doing now (a download's progress). */
  progress(label: string): void {
    if (this.#busy === null) return;
    this.#busy = label;
    // Only the words: drawing the page again would start its loader over at every step.
    const shown = this.root.querySelector('.busy .note');
    if (shown) shown.textContent = label;
    else this.#render();
  }

  /** The on-screen keyboard; the page redraws after it, typed or cancelled. */
  async ask(ask: Ask): Promise<string | null> {
    const typed = await this.keyboard.ask(ask);
    this.#render();
    return typed;
  }

  hints(): [string, string][] {
    if (this.keyboard.open) return this.keyboard.hints();
    if (this.#busy) return [];
    const row = this.#rows[this.#selected];
    return [
      ...(row?.run ? [['A', 'Choose'] as [string, string]] : []),
      ...(row?.adjust ? [['◀ ▶', 'Change'] as [string, string]] : []),
      ...(row?.alt ? [['Y', row.alt.label] as [string, string]] : []),
      ['B', 'Back']
    ];
  }

  input(e: InputEvent): void {
    if (this.keyboard.open) return this.keyboard.input(e);
    if (!e.pressed || this.#busy) return;
    if (e.action === 'up' || e.action === 'down') {
      return this.#move(moveList(this.#selected, this.#rows.length, e.action, 6, e.repeat));
    }
    const row = this.#rows[this.#selected];
    if ((e.action === 'left' || e.action === 'right') && row?.adjust) {
      void row.adjust(e.action === 'left' ? -1 : 1);
      return;
    }
    if (e.repeat) return;
    if (e.action === 'confirm') void row?.run?.();
    else if (e.action === 'north') void row?.alt?.run();
    else if (e.action === 'back') void this.pop();
  }

  /**
   * Moving only moves the highlight: rebuilding the page on every step started its panel scrolled
   * to the top and jumped back to the selection, a flicker on every press.
   */
  #move(index: number): void {
    const items = this.root.querySelectorAll('li[role=option]');
    items[this.#selected]?.classList.remove('selected');
    this.#selected = index;
    items[index]?.classList.add('selected');
    if (items[index]) reveal(items[index]);
    this.changed();
  }

  #render(): void {
    this.#selected = Math.min(this.#selected, Math.max(0, this.#rows.length - 1));
    // A redraw (after a change) stays where the panel was scrolled to.
    const scrolled = this.root.querySelector('.panel')?.scrollTop ?? 0;
    const items: HTMLElement[] = [];
    this.#rows.forEach((row, i) => {
      if (row.heading)
        items.push(el('li', { class: 'heading', role: 'presentation' }, row.heading));
      items.push(
        el(
          'li',
          {
            role: 'option',
            class: [
              i === this.#selected ? 'selected' : '',
              row.run || row.adjust ? '' : 'info',
              row.danger ? 'danger' : ''
            ]
              .filter(Boolean)
              .join(' ')
          },
          row.logo
            ? el(
                'span',
                { class: 'name' },
                themeLogo(row.logo.src, row.logo.name),
                row.logo.text ? el('span', { class: 'logo-text' }, row.logo.text) : null
              )
            : el(
                'span',
                { class: 'name' },
                row.icon
                  ? el('span', { class: `icon icon-${row.icon}`, 'aria-hidden': 'true' })
                  : null,
                row.label
              ),
          row.badge
            ? el('span', { class: row.badge.kept ? 'badge current' : 'badge' }, row.badge.text)
            : null,
          row.adjust ? el('span', { class: 'adjust', 'aria-hidden': 'true' }, '◀ ▶') : '',
          el('span', { class: 'description' }, row.detail ?? ''),
          row.image ? el('img', { class: 'choice', src: row.image, alt: '' }) : null
        )
      );
    });
    this.root.replaceChildren(
      el(
        'div',
        { class: 'panel' },
        el('h2', {}, this.#title, this.#file ? el('small', { class: 'file' }, this.#file) : null),
        this.#busy
          ? el(
              'div',
              { class: 'busy', role: 'status' },
              el('p', { class: 'note' }, this.#busy),
              el('div', { class: 'loader', 'aria-hidden': 'true' }, el('span', {}))
            )
          : this.#note
            ? el('p', { class: 'note' }, this.#note)
            : null,
        el('ul', { role: 'listbox' }, ...(this.#busy ? [] : items))
      ),
      this.keyboard.root
    );
    const panel = this.root.querySelector<HTMLElement>('.panel');
    if (panel) {
      panel.scrollTop = scrolled;
      // Rows stop below the title, however many lines a long name takes.
      const title = panel.querySelector<HTMLElement>('h2')?.offsetHeight ?? 0;
      if (title) panel.style.setProperty('--title', `${title}px`);
    }
    const selected = this.root.querySelector('li.selected');
    if (selected) reveal(selected);
    this.changed();
  }
}

/** Scrolls a row into sight, with the heading of its group when it is the group's first. */
function reveal(row: Element): void {
  const before = row.previousElementSibling;
  if (before?.classList.contains('heading')) before.scrollIntoView({ block: 'nearest' });
  row.scrollIntoView({ block: 'nearest' });
}

/** A Batocera switch shown and flipped: `'1'` is on. */
export const onOff = (value: string | null): string => (value === '1' ? 'On' : 'Off');
