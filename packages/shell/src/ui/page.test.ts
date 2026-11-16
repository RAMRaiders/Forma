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
});
