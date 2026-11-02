// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';

import { Cover, coverInfo } from './cover.js';

afterEach(() => vi.useRealTimers());

const info = {
  boot: false,
  batocera: '43.1 2026/05/29',
  forma: '0.1.0',
  theme: 'Cyber 0.2.0',
  margins: 2
};

describe('Forma’s cover', () => {
  it('reads what to show from the shell’s script tag', () => {
    const script = document.createElement('script');
    Object.assign(script.dataset, {
      boot: '1',
      batocera: '43.1',
      theme: 'Cyber 0.2.0',
      margins: '40'
    });
    expect(coverInfo(script)).toEqual({
      boot: true,
      batocera: '43.1',
      forma: '',
      theme: 'Cyber 0.2.0',
      margins: 10
    });
    expect(coverInfo(null).boot).toBe(false);
  });

  it('shows Forma’s logo, a loader and the three versions, and goes when the theme is ready', () => {
    const cover = new Cover(info);
    document.body.append(cover.root);
    const text = [...cover.root.querySelectorAll('.versions li')].map((li) => li.textContent);
    expect(text).toEqual(['Batocera43.1 2026/05/29', 'Forma0.1.0', 'ThemeCyber 0.2.0']);
    expect(cover.root.querySelector('.boot')).toBeNull();
    expect(cover.root.querySelector('[role=progressbar]')).not.toBeNull();
    cover.ready();
    expect(cover.root.isConnected).toBe(false);
  });

  it('shows Batocera’s boot image first, and stays until it has had its time', () => {
    vi.useFakeTimers();
    const cover = new Cover({ ...info, boot: true });
    document.body.append(cover.root);
    expect(cover.root.querySelector<HTMLImageElement>('.boot')?.src).toContain('boot-logo');
    cover.ready();
    expect(cover.root.isConnected).toBe(true);
    vi.advanceTimersByTime(2000);
    expect(cover.root.isConnected).toBe(false);
  });
});
