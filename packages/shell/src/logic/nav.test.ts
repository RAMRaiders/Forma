import { describe, expect, it } from 'vitest';

import { moveTiles, type Slot } from './nav.js';

/** A row, then a heading over seven tiles, then a row. */
const slots: Slot[] = [
  { tile: false, heading: false },
  ...Array.from({ length: 7 }, (_, i) => ({ tile: true, heading: i === 0 })),
  { tile: false, heading: false }
];

describe('moveTiles', () => {
  it('moves along a line, stopping at its ends', () => {
    expect(moveTiles(1, slots, 'right', 3)).toBe(2);
    expect(moveTiles(3, slots, 'right', 3)).toBe(3);
    expect(moveTiles(4, slots, 'left', 3)).toBe(4);
    expect(moveTiles(7, slots, 'right', 3)).toBe(7);
  });

  it('moves by a line, to the last tile of a short one, and off the grid to the list', () => {
    expect(moveTiles(2, slots, 'down', 3)).toBe(5);
    expect(moveTiles(6, slots, 'down', 3)).toBe(7);
    expect(moveTiles(7, slots, 'down', 3)).toBe(8);
    expect(moveTiles(5, slots, 'up', 3)).toBe(2);
    expect(moveTiles(2, slots, 'up', 3)).toBe(0);
    // A grid at the very top leaves the list to wrap.
    expect(moveTiles(0, slots.slice(1), 'up', 3)).toBeNull();
  });

  it('leaves a row that is not a tile to the list', () => {
    expect(moveTiles(0, slots, 'down', 3)).toBeNull();
  });
});
