import { type Backend, connect, type InputEvent } from '@ramraiders/forma-sdk';
import { mockBackend } from '@ramraiders/forma-sdk/mock';
import { describe, expect, it } from 'vitest';

import { App } from './app.js';

/** Drives the theme like a gamepad would, against the SDK's mock library. */
async function started() {
  const mock = mockBackend({ playMs: 20 });
  // The pad's presses come from Forma as `input` events: the test sends its own.
  const pads = new Set<(e: InputEvent) => void>();
  const backend: Backend = {
    kind: mock.kind,
    invoke: (command, args) => mock.invoke(command, args),
    mediaUrl: (id, kind, width) => mock.mediaUrl(id, kind, width),
    listen: <T>(event: string, handler: (payload: T) => void) => {
      if (event !== 'input') return mock.listen(event, handler);
      const h = handler as (e: InputEvent) => void;
      pads.add(h);
      return () => pads.delete(h);
    }
  };
  const forma = await connect({ backend, keyboard: false });
  const root = document.createElement('main');
  await new App(forma, root).start();
  const press = async (action: InputEvent['action']) => {
    for (const pad of pads) pad({ action, button: null, gamepad: 0, pressed: true, repeat: false });
    for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 0));
  };
  const text = (selector: string) =>
    [...root.querySelectorAll(selector)].map((n) => n.textContent ?? '');
  return { forma, root, press, text };
}

describe('the theme', () => {
  it('lists systems, opens one, and goes back', async () => {
    const { press, text } = await started();
    expect(text('h1')).toEqual(['Systems']);
    const first = text('li.selected')[0];
    await press('confirm');
    expect(text('h1')[0]).not.toBe('Systems');
    expect(text('li').length).toBeGreaterThan(0);
    await press('back');
    expect(text('li.selected')[0]).toBe(first);
  });

  it('launches the selected game', async () => {
    const { forma, press } = await started();
    const ended = new Promise((resolve) => forma.events.on('game:end', resolve));
    await press('confirm');
    await press('confirm');
    await expect(ended).resolves.toBeDefined();
  });
});
