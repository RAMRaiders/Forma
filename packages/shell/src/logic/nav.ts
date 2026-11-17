/** Moving a selection with a gamepad. Pure, so it is tested without a screen. */

export type Move = 'up' | 'down' | 'left' | 'right' | 'pageUp' | 'pageDown';

/**
 * A vertical list. Up from the top wraps to the bottom (and back), as EmulationStation does,
 * but only on a fresh press: a held direction stops at the end instead of racing round.
 * Left/right jump a page, like the shoulder buttons.
 */
export function moveList(
  index: number,
  count: number,
  move: Move,
  pageSize: number,
  repeat: boolean
): number {
  if (count === 0) return 0;
  const last = count - 1;
  switch (move) {
    case 'up':
      if (index > 0) return index - 1;
      return repeat ? 0 : last;
    case 'down':
      if (index < last) return index + 1;
      return repeat ? last : 0;
    case 'left':
    case 'pageUp':
      return Math.max(0, index - pageSize);
    case 'right':
    case 'pageDown':
      return Math.min(last, index + pageSize);
  }
}

/** What `moveTiles` needs to know of a row: whether it is a tile, and starts a group. */
export interface Slot {
  tile: boolean;
  heading: boolean;
}

/**
 * Moving among tiles: rows of `cols` pictures, among the page's other rows. A run of tiles
 * (one group, no heading inside) is laid out `cols` across. Left and right move along it; up
 * and down move by a line, and off its top or bottom to the row before or after it. Returns
 * null when `index` is not a tile, or the move leaves it to the list's own rules.
 */
export function moveTiles(
  index: number,
  slots: Slot[],
  move: 'up' | 'down' | 'left' | 'right',
  cols: number
): number | null {
  if (!slots[index]?.tile) return null;
  let start = index;
  while (start > 0 && slots[start - 1]!.tile && !slots[start]!.heading) start--;
  let end = index;
  while (end + 1 < slots.length && slots[end + 1]!.tile && !slots[end + 1]!.heading) end++;
  const at = index - start;
  const line = Math.floor(at / cols);
  const lastLine = Math.floor((end - start) / cols);
  switch (move) {
    case 'left':
      return at % cols > 0 ? index - 1 : index;
    case 'right':
      return at % cols < cols - 1 && index < end ? index + 1 : index;
    // Off the grid: the row just before or after it, else the list's own wrapping.
    case 'up':
      if (line > 0) return index - cols;
      return start > 0 ? start - 1 : null;
    case 'down':
      if (line < lastLine) return Math.min(index + cols, end);
      return end + 1 < slots.length ? end + 1 : null;
  }
}
