import { describe, expect, it, vi } from 'vitest';
import type { Backend } from './backend.js';
import { connect } from './client.js';
import { FormaError } from './errors.js';
import type { InputEvent } from './types.js';
import { SDK_VERSION } from './version.js';

/** Commands that reject, with what Tauri's IPC rejects with: the app's plain error object. */
function fakeBackend(host = SDK_VERSION, rejections: Record<string, object> = {}) {
  const calls: string[] = [];
  const listeners = new Map<string, (p: unknown) => void>();
  const backend: Backend = {
    kind: 'mock',
    invoke<T>(command: string): Promise<T> {
      calls.push(command);
      if (command === 'sdk_api_version') return Promise.resolve(host as T);
      const rejection = rejections[command];
      if (rejection) return Promise.reject(rejection);
      return Promise.resolve(null as T);
    },
    listen<T>(event: string, handler: (p: T) => void) {
      listeners.set(event, handler as (p: unknown) => void);
      return () => listeners.delete(event);
    },
    mediaUrl: (id, kind) => `media/${id}/${kind}`
  };
  return { backend, calls, listeners };
}

describe('connect', () => {
  it('checks the host version, then reports the theme ready', async () => {
    const { backend, calls } = fakeBackend('1.14.0');
    const sdk = await connect({ backend });
    expect(sdk.hostVersion).toBe('1.14.0');
    expect(calls).toEqual(['sdk_api_version', 'theme_ready']);
  });

  it('can leave reporting ready to the theme, for once its first screen is drawn', async () => {
    const { backend, calls } = fakeBackend('1.14.0');
    const sdk = await connect({ backend, deferReady: true });
    expect(calls).toEqual(['sdk_api_version']);
    await sdk.ready();
    expect(calls).toEqual(['sdk_api_version', 'theme_ready']);
  });

  it('tells Forma’s menu what is on screen', async () => {
    const { backend, calls } = fakeBackend('1.14.0');
    const sdk = await connect({ backend });
    await sdk.shell.setContext({ view: 'systems', game: { id: 1, name: 'Zelda' } });
    expect(calls).toContain('shell_set_context');
  });

  it('refuses a host with another major version, before reporting ready', async () => {
    const { backend, calls } = fakeBackend('2.0.0');
    await expect(connect({ backend })).rejects.toMatchObject({ code: 'incompatible' });
    expect(calls).not.toContain('theme_ready');
  });

  it('falls back to the mock outside Forma', async () => {
    vi.spyOn(console, 'info').mockImplementation(() => {});
    const sdk = await connect();
    expect(sdk.backendKind).toBe('mock');
    expect((await sdk.library.systems()).length).toBeGreaterThan(0);
  });
});

describe('calls', () => {
  it('reject with a FormaError carrying the host code', async () => {
    const { backend } = fakeBackend(SDK_VERSION, {
      system_quit: { code: 'permissionDenied', message: 'needs system.power' }
    });
    const sdk = await connect({ backend });
    const error: unknown = await sdk.system.quit().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(FormaError);
    expect(error).toMatchObject({ code: 'permissionDenied', message: 'needs system.power' });
  });

  it('wrap anything else, and codes this SDK does not know, as internal', () => {
    expect(FormaError.from({ code: 'fromTheFuture', message: 'x' })).toMatchObject({
      code: 'internal',
      message: 'x'
    });
    expect(FormaError.from(new Error('boom'))).toMatchObject({
      code: 'internal',
      message: 'boom'
    });
    expect(FormaError.from('text')).toMatchObject({ code: 'internal', message: 'text' });
  });

  it('send the options, system settings and store commands with their arguments', async () => {
    const sent: [string, unknown][] = [];
    const backend: Backend = {
      ...fakeBackend().backend,
      invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
        sent.push([command, args]);
        return Promise.resolve((command === 'sdk_api_version' ? SDK_VERSION : null) as T);
      }
    };
    const sdk = await connect({ backend });
    sent.length = 0;
    const nes = { level: 'system', system: 'nes' } as const;
    await sdk.options.get(nes);
    await sdk.options.set(nes, 'ratio', null);
    await sdk.system.info();
    await sdk.system.settings();
    await sdk.system.set('system.timezone', 'Europe/Lisbon');
    await sdk.store.list();
    await sdk.store.refresh();
    await sdk.store.install('nes-alter-ego');
    await sdk.store.remove('nes-alter-ego');
    await sdk.store.update();
    expect(sent).toEqual([
      ['options_get', { scope: nes }],
      ['options_set', { scope: nes, key: 'ratio', value: null }],
      ['system_info', undefined],
      ['system_settings', undefined],
      ['system_settings_set', { key: 'system.timezone', value: 'Europe/Lisbon' }],
      ['store_list', undefined],
      ['store_refresh', undefined],
      ['store_install', { name: 'nes-alter-ego' }],
      ['store_remove', { name: 'nes-alter-ego' }],
      ['store_update', undefined]
    ]);
  });

  it('build media URLs through the backend', async () => {
    const { backend } = fakeBackend();
    const sdk = await connect({ backend });
    expect(sdk.library.mediaUrl(3)).toBe('media/3/image');
    expect(sdk.library.mediaUrl(3, 'marquee')).toBe('media/3/marquee');
  });
});

describe('input', () => {
  it('subscribes to gamepad events once, and stops when the last handler goes', async () => {
    const { backend, listeners } = fakeBackend();
    const sdk = await connect({ backend, keyboard: false });
    const seen: string[] = [];
    const offAll = sdk.input.on((e) => seen.push(`all:${e.action}`));
    const offConfirm = sdk.input.onPress('confirm', (e) => seen.push(`confirm:${e.repeat}`));

    const send = (e: Partial<InputEvent>) =>
      listeners.get('input')!({ button: null, gamepad: 0, pressed: true, repeat: false, ...e });
    send({ action: 'confirm' });
    send({ action: 'confirm', pressed: false });
    send({ action: 'down' });
    expect(seen).toEqual(['all:confirm', 'confirm:false', 'all:confirm', 'all:down']);

    offAll();
    expect(listeners.has('input')).toBe(true);
    offConfirm();
    expect(listeners.has('input')).toBe(false);
  });
});
