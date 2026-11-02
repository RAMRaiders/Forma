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
