import { describe, expect, it } from 'vitest';
import { keyToInput } from './keyboard.js';

describe('keyToInput', () => {
  it('maps the keys a sofa UI needs to the same actions as a gamepad', () => {
    expect(keyToInput('Enter', true, false)).toEqual({
      action: 'confirm',
      button: null,
      gamepad: -1,
      pressed: true,
      repeat: false
    });
    expect(keyToInput('Escape', true, false)?.action).toBe('back');
    expect(keyToInput('Backspace', true, false)?.action).toBe('back');
    expect(keyToInput('ArrowDown', true, true)).toMatchObject({ action: 'down', repeat: true });
    expect(keyToInput('a', true, false)).toBeNull();
  });
});
