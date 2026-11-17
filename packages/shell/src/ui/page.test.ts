// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Page, type Row } from './page.js';

beforeEach(() => {
  // jsdom does not implement it.
  Element.prototype.scrollIntoView = vi.fn();
});

/** Rows a test hands over when it chooses, as Forma answers after a while. */
function later(): { build: () => Promise<Row[]>; answer: (rows: Row[]) => void } {
  let answer: (rows: Row[]) => void = () => {};
  const rows = new Promise<Row[]>((resolve) => (answer = resolve));
  return { build: () => rows, answer };
}

const names = (page: Page) =>
  [...page.root.querySelectorAll('li[role=option] .name')].map((n) => n.textContent);

describe('a page', () => {
  it('opens whole: nothing of the page before it shows while its rows are read', async () => {
    const page = new Page(vi.fn(), vi.fn());
    await page.show('Delete Sonic?', () => [{ label: 'Keep it' }]);
    expect(names(page)).toEqual(['Keep it']);
    page.close();
    expect(page.root.textContent).toBe('');

    const next = later();
    const shown = page.show('Streets of Rage', next.build);
    // The shell's already (input is its own), with nothing on screen yet.
    expect(page.open).toBe(true);
    expect(page.root.hidden).toBe(true);
    next.answer([{ label: 'Scrape' }]);
    await shown;
    expect(page.root.hidden).toBe(false);
    expect(page.root.querySelector('h2')?.textContent).toBe('Streets of Rage');
    expect(names(page)).toEqual(['Scrape']);
  });

  it('stays shut when it is closed before its rows come', async () => {
    const page = new Page(vi.fn(), vi.fn());
    const slow = later();
    const shown = page.show('Games', slow.build);
    page.close();
    slow.answer([{ label: 'Scrape' }]);
    await shown;
    expect(page.open).toBe(false);
    expect(page.root.textContent).toBe('');
  });

  it('lays tiles out in a grid, moved through in every direction, among its other rows', async () => {
    const page = new Page(vi.fn(), vi.fn());
    const press = (action: 'up' | 'down' | 'left' | 'right') =>
      page.input({ action, button: null, gamepad: 0, pressed: true, repeat: false });
    const chosen = () => page.root.querySelector('li.selected .name')?.textContent;
    await page.show('Box', () => [
      { label: 'Name' },
      ...['Keep', 'One', 'Two', 'Three', 'Four', 'Five'].map((label) => ({
        label,
        tile: true,
        image: label === 'One' ? 'one.png' : null,
        run: vi.fn()
      })),
      { label: 'Save', run: vi.fn() }
    ]);
    // One grid, its tiles four across.
    expect(page.root.querySelectorAll('ul.tiles li.tile')).toHaveLength(6);
    expect(page.root.querySelector('li.tile img.picture')?.getAttribute('src')).toBe('one.png');
    expect(page.root.querySelectorAll('li.tile .picture.none')).toHaveLength(5);
    press('down');
    expect(chosen()).toBe('Keep');
    press('right');
    press('right');
    expect(chosen()).toBe('Two');
    press('down');
    // The line below has two: the last of them.
    expect(chosen()).toBe('Five');
    press('right');
    expect(chosen()).toBe('Five');
    press('down');
    expect(chosen()).toBe('Save');
    press('up');
    expect(chosen()).toBe('Five');
    // Five sits under One.
    press('up');
    expect(chosen()).toBe('One');
    press('up');
    expect(chosen()).toBe('Name');
  });

  it('gives itself back on B while it works, if the work may be cancelled', async () => {
    const notify = vi.fn();
    const page = new Page(notify, vi.fn());
    const back = () =>
      page.input({ action: 'back', button: null, gamepad: 0, pressed: true, repeat: false });
    await page.show('Sonic', () => [{ label: 'Scrape' }]);
    let answer: () => void = () => {};
    const slow = new Promise<void>((resolve) => (answer = resolve));
    const pushed = vi.fn();
    const running = page.run(
      'Asking every scraper…',
      async (signal) => {
        await slow;
        if (!signal.aborted) pushed();
      },
      { cancellable: true }
    );
    expect(page.root.querySelector('.busy')).not.toBeNull();
    expect(page.hints()).toEqual([['B', 'Cancel']]);
    back();
    await running;
    expect(notify).toHaveBeenCalledWith('Cancelled');
    expect(page.root.querySelector('.busy')).toBeNull();
    expect(names(page)).toEqual(['Scrape']);
    // Still open: B cancelled the work, not the page.
    expect(page.open).toBe(true);
    answer();
    await slow;
    expect(pushed).not.toHaveBeenCalled();

    // Work that may not be cancelled holds B.
    let finish: () => void = () => {};
    const save = page.run('Saving…', () => new Promise<void>((r) => (finish = r)));
    expect(page.hints()).toEqual([]);
    back();
    expect(page.root.querySelector('.busy')).not.toBeNull();
    finish();
    await save;
    expect(page.open).toBe(true);
  });

  it('keeps the pictures on screen when it draws itself again', async () => {
    const page = new Page(vi.fn(), vi.fn());
    let second: string | null = null;
    await page.show('Box', () => [
      { label: 'One', tile: true, image: 'one.png' },
      { label: 'Two', tile: true, image: second }
    ]);
    const before = page.root.querySelector('img[src="one.png"]');
    second = 'two.png';
    await page.refresh();
    // The same element: it does not go blank while a copy decodes.
    expect(page.root.querySelector('img[src="one.png"]')).toBe(before);
    expect(page.root.querySelector('img[src="two.png"]')).not.toBeNull();
  });
});
