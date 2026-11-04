import type { Backend, InputEvent, Unsubscribe } from '@ramraiders/forma-sdk';
import { listen } from '@tauri-apps/api/event';

/** The header the shell's calls carry its token in (`SHELL_HEADER` in forma-app). */
const SHELL_HEADER = 'x-forma-shell';

type RawInvoke = (
  command: string,
  args?: Record<string, unknown>,
  options?: { headers: Record<string, string> }
) => Promise<unknown>;

interface TauriInternals {
  invoke: RawInvoke;
}

/**
 * Tauri's own invoke, taken before any theme script runs, so a theme that replaces
 * `__TAURI_INTERNALS__.invoke` later never sees the shell's calls go through it. `null` outside
 * Forma (a theme opened in a plain browser) and in frames: the shell runs once, in the window.
 */
export function tauriInvoke(): RawInvoke | null {
  if (typeof window === 'undefined' || window.top !== window) return null;
  const internals = (window as unknown as { __TAURI_INTERNALS__?: TauriInternals })
    .__TAURI_INTERNALS__;
  return internals ? internals.invoke.bind(internals) : null;
}

/**
 * The SDK's backend as the shell uses it: every call carries the shell's token, which lets it
 * do what Forma's menu offers whatever the theme's permissions. Input comes from `shell:input`:
 * Start, Select, and everything while the shell has something open.
 */
export function shellBackend(invoke: RawInvoke, token: string): Backend {
  const headers = { [SHELL_HEADER]: token };
  return {
    kind: 'tauri',
    invoke: <T>(command: string, args?: Record<string, unknown>) =>
      invoke(command, args ?? {}, { headers }) as Promise<T>,
    listen<T>(event: string, handler: (payload: T) => void): Unsubscribe {
      const name = event === 'input' ? 'shell:input' : event;
      const pending = listen<T>(name, (e) => handler(e.payload));
      return () => void pending.then((stop) => stop());
    },
    mediaUrl: (gameId, kind, width) =>
      `forma://localhost/media/${gameId}/${kind}${width ? `?w=${Math.round(width)}` : ''}`
  };
}

export type { InputEvent };
