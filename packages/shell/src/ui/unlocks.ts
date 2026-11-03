import type { RaSession } from '@ramraiders/forma-sdk';

import { el } from './dom.js';

/** A picture from RetroAchievements; one that cannot load (offline) leaves its plain box. */
function picture(src: string | null, cls: string, part?: string): HTMLElement {
  const box = el('span', { class: cls, ...(part ? { part } : {}) });
  if (!src) return box;
  const img = el('img', { class: cls, src, alt: '', ...(part ? { part } : {}) });
  img.addEventListener('error', () => img.replaceWith(box));
  return img;
}

/** How long the card stays: long enough to read four achievements. */
export const UNLOCKS_MS = 12_000;
/** Shown one by one; the rest are counted. */
const SHOWN = 4;

/**
 * After a game, what it earned on RetroAchievements: the game, each achievement with its badge,
 * the points, and how far through the game the user is now. A theme restyles it from outside
 * (`forma-shell::part(unlocks)`, `unlock`, `unlock-badge`), or draws its own (`"draws":
 * ["achievements"]` in its theme.json, listening for `ra:unlocked`).
 */
export class UnlocksView {
  readonly root = el('div', { class: 'unlocks', part: 'unlocks', role: 'status', hidden: '' });
  #timer: ReturnType<typeof setTimeout> | undefined;

  show(session: RaSession): void {
    const { game, unlocked, points } = session;
    const more = unlocked.length - SHOWN;
    this.root.replaceChildren(
      el(
        'header',
        {},
        picture(game.icon, 'icon'),
        el(
          'div',
          { class: 'words' },
          el(
            'span',
            { class: 'kicker' },
            unlocked.length === 1
              ? 'Achievement unlocked'
              : `${unlocked.length} achievements unlocked`
          ),
          el('span', { class: 'game' }, game.title)
        )
      ),
      el(
        'ul',
        {},
        ...unlocked
          .slice(0, SHOWN)
          .map((a) =>
            el(
              'li',
              { part: 'unlock' },
              picture(a.badge, 'badge', 'unlock-badge'),
              el(
                'span',
                { class: 'words' },
                el('span', { class: 'title' }, a.title),
                el('span', { class: 'description' }, a.description)
              ),
              el('span', { class: 'points' }, `${a.points}`)
            )
          ),
        ...(more > 0 ? [el('li', { class: 'more' }, `and ${more} more`)] : [])
      ),
      el(
        'footer',
        {},
        el('span', { class: 'total' }, `+${points} points`),
        el(
          'span',
          { class: 'progress' },
          `${game.earned} of ${game.achievementCount} · ${Math.round(game.completion * 100)}%`
        ),
        el('span', { class: 'bar', style: `--done: ${game.completion}` })
      )
    );
    this.root.hidden = false;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => this.hide(), UNLOCKS_MS);
  }

  hide(): void {
    clearTimeout(this.#timer);
    this.root.hidden = true;
  }
}
