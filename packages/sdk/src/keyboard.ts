import type { Action, InputEvent } from './types.js';

/** Keys a sofa UI understands, so a theme handles keyboard and gamepad as one stream. */
const KEYS: Record<string, Action> = {
  ArrowUp: 'up',
  ArrowDown: 'down',
  ArrowLeft: 'left',
  ArrowRight: 'right',
  Enter: 'confirm',
  Escape: 'back',
  Backspace: 'back',
  PageUp: 'pageUp',
  PageDown: 'pageDown',
  Tab: 'select',
  F1: 'start'
};

/** The input event for a key, or `null` for a key RomDex does not use. */
export function keyToInput(key: string, pressed: boolean, repeat: boolean): InputEvent | null {
  const action = KEYS[key];
  if (!action) return null;
  return { action, button: null, gamepad: -1, pressed, repeat };
}
