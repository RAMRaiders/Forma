// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';

import { freezeTheme } from './freeze.js';

describe('freezeTheme', () => {
  it('pauses the page’s animations and playing videos, and plays them again', () => {
    document.body.innerHTML = '<video id="on"></video><video id="off"></video>';
    const [on, off] = [...document.querySelectorAll('video')] as HTMLVideoElement[];
    Object.defineProperty(on, 'paused', { get: () => false, configurable: true });
    const pause = vi.spyOn(on!, 'pause').mockImplementation(() => {});
    const play = vi.spyOn(on!, 'play').mockResolvedValue();
    const offPlay = vi.spyOn(off!, 'play').mockResolvedValue();
    freezeTheme(true);
    expect(document.documentElement.classList.contains('forma-frozen')).toBe(true);
    expect(document.querySelector('style')?.textContent).toContain('animation-play-state: paused');
    expect(pause).toHaveBeenCalledOnce();
    freezeTheme(false);
    expect(document.documentElement.classList.contains('forma-frozen')).toBe(false);
    expect(play).toHaveBeenCalledOnce();
    expect(offPlay).not.toHaveBeenCalled();
  });
});
