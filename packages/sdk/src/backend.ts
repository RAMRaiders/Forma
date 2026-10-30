import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type { Unsubscribe } from './types.js';

/** How the SDK reaches RomDex: Tauri inside the app, or the mock in a plain browser. */
export interface Backend {
  readonly kind: 'tauri' | 'mock';
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Unsubscribe;
  /** The URL that serves one kind of a game's media. */
  mediaUrl(gameId: number, kind: string): string;
}

export function tauriBackend(): Backend {
  return {
    kind: 'tauri',
    invoke: <T>(command: string, args?: Record<string, unknown>) => invoke<T>(command, args),
    listen<T>(event: string, handler: (payload: T) => void): Unsubscribe {
      // Tauri subscribes asynchronously; unsubscribing before it settles must still work.
      const pending = listen<T>(event, (e) => handler(e.payload));
      return () => {
        void pending.then((unlisten) => unlisten());
      };
    },
    mediaUrl: (gameId, kind) => `romdex://localhost/media/${gameId}/${kind}`
  };
}

export function inTauri(): boolean {
  return isTauri();
}
