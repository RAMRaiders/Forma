// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';

import { Cover, coverInfo } from './cover.js';

afterEach(() => vi.useRealTimers());

const info = {
  batocera: '43.1 2026/05/29',
  forma: '0.1.0',
  theme: 'Cyber 0.2.0',
  margins: 2
};

describe('Forma’s cover', () => {
  it('reads what to show from the shell’s script tag', () => {
    const script = document.createElement('script');
    Object.assign(script.dataset, {
      batocera: '43.1',
      theme: 'Cyber 0.2.0',
      margins: '40'
    });
    expect(coverInfo(script)).toEqual({
      batocera: '43.1',
      forma: '',
      theme: 'Cyber 0.2.0',
      margins: 10
    });
    expect(coverInfo(null).margins).toBe(0);
  });

  it('shows Forma’s logo, a loader and the three versions, and fades when the theme is ready', () => {
    vi.useFakeTimers();
    const cover = new Cover(info);
    document.body.append(cover.root);
    const text = [...cover.root.querySelectorAll('.versions li')].map((li) => li.textContent);
    expect(text).toEqual(['Batocera43.1 2026/05/29', 'Forma0.1.0', 'ThemeCyber 0.2.0']);
    expect(cover.root.querySelector('.boot')).toBeNull();
    expect(cover.root.querySelector('[role=progressbar]')).not.toBeNull();
    const started = vi.fn();
    cover.onStarted = started;
    cover.ready();
    expect(started).toHaveBeenCalledOnce();
    expect(cover.root.classList.contains('leaving')).toBe(true);
    vi.advanceTimersByTime(300);
    expect(cover.root.isConnected).toBe(false);
  });

  it('does not report the UI started when it goes over a theme that never started', () => {
    const cover = new Cover(info);
    const started = vi.fn();
    cover.onStarted = started;
    cover.hide();
    cover.ready();
    expect(started).not.toHaveBeenCalled();
  });
});
