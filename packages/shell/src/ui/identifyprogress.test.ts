// @vitest-environment jsdom
import type { Forma, IdentifyStatus } from '@ramraiders/forma-sdk';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { IdentifyProgress } from './identifyprogress.js';
import { LINGER_MS } from './progresscard.js';

function fake() {
  let tell: (status: IdentifyStatus) => void = () => {};
  const forma = {
    events: {
      on: (_event: string, handler: (status: IdentifyStatus) => void) => {
        tell = handler;
        return () => {};
      }
    },
    library: {
      systems: () => Promise.resolve([{ name: 'nes', fullname: 'Nintendo Entertainment System' }])
    }
  } as unknown as Forma;
  const card = new IdentifyProgress(forma);
  const text = (sel: string) => card.root.querySelector(sel)?.textContent;
  return { card, text, tell: (status: IdentifyStatus) => tell(status) };
}

afterEach(() => vi.useRealTimers());

describe('IdentifyProgress', () => {
  it('shows the system, how far and the file, then that it is done, then goes', async () => {
    vi.useFakeTimers();
    const { card, text, tell } = fake();
    expect(card.root.hidden).toBe(true);
    tell({
      pass: 'whole',
      running: true,
      system: 'nes',
      done: 3,
      total: 12,
      current: 'sub/Blaster Master (USA).zip'
    });
    await vi.waitFor(() =>
      expect(text('.title')).toBe('Identifying Nintendo Entertainment System')
    );
    expect(card.root.hidden).toBe(false);
    expect(text('.count')).toBe('3 / 12');
    expect(text('.rom')).toBe('Blaster Master (USA).zip');
    expect(card.root.querySelector<HTMLElement>('.bar i')!.style.transform).toBe('scaleX(0.25)');

    tell({ pass: 'whole', running: false, system: '', done: 12, total: 12, current: null });
    await vi.waitFor(() => expect(text('.title')).toBe('Games identified'));
    expect(text('.count')).toBe('12 files');
    vi.advanceTimersByTime(LINGER_MS);
    expect(card.root.hidden).toBe(true);
  });

  it('tells about how long the system has left, once there is enough to tell', async () => {
    vi.useFakeTimers();
    const { text, tell } = fake();
    const at = (done: number) =>
      tell({
        pass: 'whole',
        running: true,
        system: 'nes',
        done,
        total: 1000,
        current: `${done}.nes`
      });
    at(0);
    await vi.waitFor(() => expect(text('.count')).toBe('0 / 1000'));
    vi.advanceTimersByTime(20_000);
    at(10);
    await vi.waitFor(() => expect(text('.count')).toBe('10 / 1000'));
    // 20 files a minute: 980 to go is 49 minutes.
    vi.advanceTimersByTime(40_000);
    at(20);
    await vi.waitFor(() => expect(text('.count')).toBe('20 / 1000 · ~49 min left'));
    // Another system starts over.
    tell({ pass: 'whole', running: true, system: 'snes', done: 0, total: 5000, current: 'a.sfc' });
    await vi.waitFor(() => expect(text('.count')).toBe('0 / 5000'));
    vi.advanceTimersByTime(60_000);
    tell({ pass: 'whole', running: true, system: 'snes', done: 20, total: 5000, current: 'b.sfc' });
    await vi.waitFor(() => expect(text('.count')).toBe('20 / 5000 · ~4 h 9 min left'));
  });

  it('says when it is finding achievements, the quick pass', async () => {
    const { text, tell } = fake();
    tell({
      pass: 'achievements',
      running: true,
      system: 'nes',
      done: 1,
      total: 4,
      current: 'A.cue'
    });
    await vi.waitFor(() =>
      expect(text('.title')).toBe('Finding achievements: Nintendo Entertainment System')
    );
    expect(text('.count')).toBe('1 / 4');
  });

  it('an earlier file still waiting for the names never covers the end', async () => {
    const { text, tell } = fake();
    tell({ pass: 'whole', running: true, system: 'nes', done: 0, total: 1, current: 'A.nes' });
    tell({ pass: 'whole', running: false, system: '', done: 1, total: 1, current: null });
    await new Promise((r) => setTimeout(r, 0));
    expect(text('.title')).toBe('Games identified');
    expect(text('.count')).toBe('1 file');
  });
});
