import { el, themeLogo } from './dom.js';

export interface MenuItem {
  id: string;
  label: string;
  description?: string;
  /** The group it shows in; groups keep the order their first item comes in. */
  section?: string;
}

/**
 * Forma's Start menu: entries as tiles with an icon, in labelled rows (Selected, Library, Play,
 * Settings, Power), and the selected one's description once, at the bottom. The D-pad moves
 * across the grid as it looks; the order of `items` is also the reading order.
 */
export class Menu {
  readonly root = el('div', { class: 'overlay menu', hidden: '' });
  #items: MenuItem[] = [];
  #tiles: HTMLElement[] = [];
  #about: HTMLElement = el('p', { class: 'about' });
  selected = 0;

  get open(): boolean {
    return !this.root.hidden;
  }

  get item(): MenuItem | undefined {
    return this.#items[this.selected];
  }

  get count(): number {
    return this.#items.length;
  }

  /** `logo`: drawn before the subtitle, in place of the name it starts with. */
  show(
    title: string,
    items: MenuItem[],
    subtitle?: string,
    logo?: { src: string; name: string }
  ): void {
    this.#items = items;
    this.#tiles = items.map((i) => {
      const tile = el(
        'li',
        { role: 'option', class: `tile tile-${i.id}` },
        el('span', { class: 'icon', 'aria-hidden': 'true' }),
        el('span', { class: 'name' }, i.label)
      );
      tile.querySelector('.icon')!.innerHTML = ICONS[i.id] ?? ICONS.default!;
      return tile;
    });
    const sections: { name: string; tiles: HTMLElement[] }[] = [];
    items.forEach((item, n) => {
      const name = item.section ?? '';
      const last = sections.at(-1);
      if (last?.name === name) last.tiles.push(this.#tiles[n]!);
      else sections.push({ name, tiles: [this.#tiles[n]!] });
    });
    this.#about = el('p', { class: 'about', 'aria-live': 'polite' });
    this.root.replaceChildren(
      el(
        'div',
        { class: 'panel' },
        el(
          'header',
          {},
          el('h2', {}, title),
          subtitle
            ? el(
                'p',
                { class: 'note' },
                logo ? themeLogo(logo.src, logo.name) : '',
                logo && subtitle.startsWith(logo.name)
                  ? subtitle.slice(logo.name.length).trim()
                  : subtitle
              )
            : null
        ),
        el(
          'div',
          { class: 'sections', role: 'listbox' },
          ...sections.map((s) =>
            el('section', {}, el('h3', {}, s.name), el('ul', { role: 'presentation' }, ...s.tiles))
          )
        ),
        this.#about
      )
    );
    this.root.hidden = false;
    this.select(0);
  }

  select(index: number): void {
    this.#tiles[this.selected]?.classList.remove('selected');
    this.selected = Math.max(0, Math.min(index, this.#items.length - 1));
    const tile = this.#tiles[this.selected];
    tile?.classList.add('selected');
    tile?.scrollIntoView({ block: 'nearest' });
    this.#about.textContent = this.item?.description ?? '';
  }

  /**
   * The tile in that direction on screen: up and down go to the nearest row that way and the
   * tile in it nearest across; left and right stay in the row. Off an edge, a fresh up or down wraps round; left and right stop. Without a layout
   * (nothing measured), it moves through the reading order.
   */
  move(direction: 'up' | 'down' | 'left' | 'right', repeat = false): void {
    const count = this.#items.length;
    if (count === 0) return;
    const boxes = this.#tiles.map((t) => t.getBoundingClientRect());
    const measured = boxes.some((b) => b.width > 0);
    const forward = direction === 'down' || direction === 'right';
    if (!measured) {
      const next = this.selected + (forward ? 1 : -1);
      const wraps = !repeat && (direction === 'up' || direction === 'down');
      return this.select(wraps ? (next + count) % count : Math.max(0, Math.min(next, count - 1)));
    }
    const centre = (b: DOMRect) => ({ x: b.left + b.width / 2, y: b.top + b.height / 2 });
    const from = centre(boxes[this.selected]!);
    const vertical = direction === 'up' || direction === 'down';
    const ahead = boxes
      .map((b, n) => ({ n, c: centre(b) }))
      .filter(({ n, c }) => {
        if (n === this.selected) return false;
        const along = vertical ? c.y - from.y : c.x - from.x;
        return (forward ? along : -along) > 2;
      });
    const nearestAcross = (row: typeof ahead) =>
      [...row].sort((a, b) => Math.abs(a.c.x - from.x) - Math.abs(b.c.x - from.x))[0]?.n ?? -1;
    const gap = (c: { y: number }) => Math.abs(c.y - from.y);
    const nearest = Math.min(...ahead.map((a) => gap(a.c)));
    const best = vertical
      ? // The nearest row that way, then the tile in it nearest across.
        nearestAcross(ahead.filter((a) => gap(a.c) - nearest < 4))
      : // Along the same row only: the next tile that way.
        nearestAcross(ahead.filter((a) => gap(a.c) < 4));
    if (best >= 0) return this.select(best);
    // Off the top or bottom: round to the other end, in the same column as near as can be.
    // Held down, it stops at the edge instead of racing round.
    if (vertical && !repeat) {
      const ends = boxes
        .map((b, n) => ({ n, c: centre(b) }))
        .sort((a, b) => (forward ? a.c.y - b.c.y : b.c.y - a.c.y));
      const edge = ends[0]!.c.y;
      const row = ends.filter((e) => Math.abs(e.c.y - edge) < 4);
      row.sort((a, b) => Math.abs(a.c.x - from.x) - Math.abs(b.c.x - from.x));
      this.select(row[0]!.n);
    }
  }

  close(): void {
    this.root.hidden = true;
  }
}

const svg = (body: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

/** A line icon per entry, drawn for Forma: one weight, rounded, on a 24 grid. */
const ICONS: Record<string, string> = {
  // A pad with sliders above: this game's own options.
  'game-options': svg(
    '<path d="M7 11h10a4 4 0 0 1 3.9 4.9l-.6 2.4a2 2 0 0 1-3.4.9L15 17H9l-1.9 2.2a2 2 0 0 1-3.4-.9l-.6-2.4A4 4 0 0 1 7 11z"/><path d="M8 14v2M7 15h2"/><circle cx="16" cy="14.5" r=".6" fill="currentColor"/><path d="M6 4h12M6 7.5h12"/><circle cx="9" cy="4" r="1.3" fill="var(--panel-solid, #0b111d)"/><circle cx="15" cy="7.5" r="1.3" fill="var(--panel-solid, #0b111d)"/>'
  ),
  // A console with a cartridge going in.
  'system-options': svg(
    '<rect x="3" y="10" width="18" height="9" rx="2"/><path d="M8 10V5h8v5"/><path d="M10 7.5h4"/><circle cx="7.5" cy="14.5" r="1"/><path d="M14 14.5h3.5"/>'
  ),
  // Lists stacked.
  // An open magazine.
  magazines: svg(
    '<path d="M3 5.5c3-1.3 6-1.3 9 .5v13c-3-1.8-6-1.8-9-.5z"/><path d="M21 5.5c-3-1.3-6-1.3-9 .5v13c3-1.8 6-1.8 9-.5z"/><path d="M5.5 9h4M5.5 12h4M14.5 9h4M14.5 12h4"/>'
  ),
  collections: svg(
    '<rect x="4" y="9" width="16" height="11" rx="2"/><path d="M6 6h12M8 3h8"/><path d="M8 13h8M8 16h5"/>'
  ),
  // Four panes.
  views: svg(
    '<rect x="4" y="4" width="7" height="7" rx="1.5"/><rect x="13" y="4" width="7" height="7" rx="1.5"/><rect x="4" y="13" width="7" height="7" rx="1.5"/><rect x="13" y="13" width="7" height="7" rx="1.5"/>'
  ),
  // Down into a tray.
  store: svg(
    '<path d="M12 3v11M7.5 9.5L12 14l4.5-4.5"/><path d="M4 15v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3"/>'
  ),
  // A cup.
  ra: svg(
    '<path d="M8 4h8v5a4 4 0 0 1-8 0z"/><path d="M8 6H5a3 3 0 0 0 3 4M16 6h3a3 3 0 0 1-3 4"/><path d="M12 13v3M9 20h6M10 16h4l1 4H9z"/>'
  ),
  // Two players, linked.
  netplay: svg(
    '<circle cx="7" cy="8" r="2.5"/><circle cx="17" cy="8" r="2.5"/><path d="M3 19a4 4 0 0 1 8 0M13 19a4 4 0 0 1 8 0"/><path d="M10 8h4" stroke-dasharray="1 1.6"/>'
  ),
  controllers: svg(
    '<path d="M7 7h10a4.5 4.5 0 0 1 4.4 5.5l-.8 3.6a2.2 2.2 0 0 1-3.8 1L15 15H9l-1.8 2.1a2.2 2.2 0 0 1-3.8-1l-.8-3.6A4.5 4.5 0 0 1 7 7z"/><path d="M8 10v3M6.5 11.5h3"/><circle cx="15.5" cy="10.5" r=".7" fill="currentColor"/><circle cx="17.5" cy="12.5" r=".7" fill="currentColor"/>'
  ),
  // A palette.
  interface: svg(
    '<path d="M12 3a9 9 0 1 0 0 18c1.2 0 1.8-.9 1.5-2-.4-1.4.4-2.5 1.9-2.5H18a3 3 0 0 0 3-3A9 9 0 0 0 12 3z"/><circle cx="7.5" cy="11" r="1.1" fill="currentColor"/><circle cx="10" cy="7" r="1.1" fill="currentColor"/><circle cx="15" cy="7.2" r="1.1" fill="currentColor"/>'
  ),
  music: svg(
    '<path d="M9 18V5l11-2v13"/><circle cx="6.5" cy="18" r="2.5"/><circle cx="17.5" cy="16" r="2.5"/>'
  ),
  // Sliders.
  'game-settings': svg(
    '<path d="M5 4v16M12 4v16M19 4v16"/><rect x="3" y="13" width="4" height="3" rx="1" fill="var(--panel-solid, #0b111d)"/><rect x="10" y="7" width="4" height="3" rx="1" fill="var(--panel-solid, #0b111d)"/><rect x="17" y="15" width="4" height="3" rx="1" fill="var(--panel-solid, #0b111d)"/>'
  ),
  // A cog.
  'system-settings': svg(
    '<circle cx="12" cy="12" r="3"/><path d="M12 2.8v2.4M12 18.8v2.4M2.8 12h2.4M18.8 12h2.4M5.5 5.5l1.7 1.7M16.8 16.8l1.7 1.7M5.5 18.5l1.7-1.7M16.8 7.2l1.7-1.7"/><circle cx="12" cy="12" r="6.5"/>'
  ),
  network: svg(
    '<path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.5a9.5 9.5 0 0 1 13 0M8.6 15.8a5 5 0 0 1 6.8 0"/><circle cx="12" cy="19" r="1.2" fill="currentColor"/>'
  ),
  // Over to the other frontend.
  emulationstation: svg('<path d="M4 8h13l-3.5-3.5M20 16H7l3.5 3.5"/>'),
  restart: svg('<path d="M20 12a8 8 0 1 1-2.4-5.7"/><path d="M20 4v4.5h-4.5"/>'),
  shutdown: svg('<path d="M12 3v8"/><path d="M7.1 6.3a7.5 7.5 0 1 0 9.8 0"/>'),
  default: svg('<circle cx="12" cy="12" r="7"/>')
};
