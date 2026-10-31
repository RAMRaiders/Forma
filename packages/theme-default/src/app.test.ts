// @vitest-environment jsdom
/**
 * The whole theme, driven like a gamepad would, against the SDK's mock (the real engine's
 * output for the fixture library). jsdom has no layout, so this checks behaviour and wiring,
 * not looks.
 */
import { connect, type InputEvent } from '@forma/sdk';
import { mockBackend } from '../../sdk/src/mock/backend.js';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { App } from './app.js';

beforeEach(() => {
  // jsdom implements neither.
  Element.prototype.scrollIntoView = vi.fn();
  document.body.innerHTML = '<div id="app"></div>';
});

async function start() {
  const backend = mockBackend({ playMs: 50 });
  let send: (e: InputEvent) => void = () => {};
  const listen = backend.listen.bind(backend);
  backend.listen = <T>(event: string, handler: (p: T) => void) => {
    if (event === 'input') send = handler as (e: InputEvent) => void;
    return listen(event, handler);
  };
  const forma = await connect({ backend, keyboard: false });
  const root = document.querySelector<HTMLElement>('#app')!;
  await new App(forma, root).start();

  const press = async (action: InputEvent['action'], repeat = false) => {
    send({ action, button: null, gamepad: 0, pressed: true, repeat });
    // Let the SDK calls the press started settle.
    await vi.waitFor(() => Promise.resolve());
    await new Promise((r) => setTimeout(r, 0));
  };
  const text = (selector: string) =>
    [...root.querySelectorAll(selector)].map((n) => n.textContent?.trim() ?? '');
  return { root, press, text, forma };
}

describe('the default theme', () => {
  it('opens on the boot view as a grid of systems', async () => {
    const { text } = await start();
    expect(text('h1')).toEqual(['Systems']);
    expect(text('.card .label')).toEqual([
      'Nintendo Entertainment System',
      'Super Famicom',
      'Mega Drive',
      'PlayStation',
      'Homebrew'
    ]);
    expect(text('.card.selected .label')).toEqual(['Nintendo Entertainment System']);
    expect(text('.hint')).toEqual(['AOpen', 'SelectViews']);
  });

  it('opens a system into its games, shows details, and goes back to where it was', async () => {
    const { press, text } = await start();
    await press('right');
    expect(text('.card.selected .label')).toEqual(['Super Famicom']);
    await press('left');
    await press('confirm');

    expect(text('.trail .crumb')).toEqual(['Nintendo Entertainment System']);
    await vi.waitFor(() => expect(text('.row')).toContain('★ Super Mario Bros.'));
    expect(text('.row.selected')).toEqual(['Homebrew Demo']);
    await press('down');
    expect(text('.row.selected')).toEqual(['★ Super Mario Bros.']);
    // Quick presses count as a held direction: the panel settles once movement pauses.
    await vi.waitFor(() => expect(text('.detail h2')).toEqual(['★ Super Mario Bros.']));
    expect(text('.detail .facts')[0]).toContain('1985');
    expect(text('.hint')).toContain('YFavourite');

    await press('up');
    await press('up'); // a fresh press at the top wraps to the bottom
    expect(text('.row.selected')).toEqual(['The Legend of Zelda']);

    await press('back');
    expect(text('.card.selected .label')).toEqual(['Nintendo Entertainment System']);
  });

  it('toggles a favourite in place', async () => {
    const { press, text } = await start();
    await press('confirm');
    await vi.waitFor(() => expect(text('.row.selected')).toEqual(['Homebrew Demo']));
    await press('north');
    expect(text('.row.selected')).toEqual(['★ Homebrew Demo']);
    expect(text('.toast')).toEqual(['Added Homebrew Demo to favourites']);
  });

  it('launches a game, covers the screen while it runs, and comes back', async () => {
    const { press, root } = await start();
    await press('confirm');
    await press('confirm');
    const playing = root.querySelector<HTMLElement>('.playing')!;
    await vi.waitFor(() => expect(playing.hidden).toBe(false));
    expect(playing.textContent).toBe('Starting Homebrew Demo…');
    await vi.waitFor(() => expect(playing.hidden).toBe(true));
  });

  it('switches view and sets the home view from the switcher', async () => {
    const { press, text, forma } = await start();
    await press('select');
    expect(text('.switcher li.selected .name')).toEqual(['Systems']);
    await press('down');
    await press('north');
    await vi.waitFor(async () =>
      expect((await forma.nav.defaultView()).id).toBe('manufacturer-system')
    );
    await press('confirm');
    await vi.waitFor(() => expect(text('h1')).toEqual(['By manufacturer']));
    expect(text('.card .label')).toEqual(['Nintendo', 'Sega', 'Sony', 'Unknown']);
  });

  it('opens a menu view into its branches', async () => {
    const { press, text } = await start();
    await press('select');
    for (let i = 0; i < 5; i++) await press('down');
    expect(text('.switcher li.selected .name')).toEqual(['Home']);
    await press('confirm');
    await vi.waitFor(() =>
      expect(text('.card .label')).toEqual([
        'Favourites',
        'Recently played',
        'Systems',
        'Manufacturers',
        'Genres'
      ])
    );
    await press('confirm');
    await vi.waitFor(() => expect(text('.row')).toEqual(['★ Super Mario Bros.']));
  });
});
