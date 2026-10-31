import { describe, expect, it, vi } from 'vitest';

const unlisten = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn((command: string, args?: unknown) => Promise.resolve({ command, args })),
  isTauri: () => true
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((_event: string, handler: (e: { payload: unknown }) => void) => {
    handler({ payload: 'hello' });
    return Promise.resolve(unlisten);
  })
}));

const { inTauri, tauriBackend } = await import('./backend.js');

describe('tauriBackend', () => {
  it('passes commands through and unwraps event payloads', async () => {
    const backend = tauriBackend();
    expect(inTauri()).toBe(true);
    expect(await backend.invoke('library_game', { id: 1 })).toEqual({
      command: 'library_game',
      args: { id: 1 }
    });

    const payloads: unknown[] = [];
    const off = backend.listen('library:changed', (p) => payloads.push(p));
    expect(payloads).toEqual(['hello']);
    off();
    await Promise.resolve();
    expect(unlisten).toHaveBeenCalled();
  });

  it('serves media from the forma protocol by game id', () => {
    expect(tauriBackend().mediaUrl(42, 'video')).toBe('forma://localhost/media/42/video');
    expect(tauriBackend().mediaUrl(42, 'image', 479.6)).toBe(
      'forma://localhost/media/42/image?w=480'
    );
  });
});
