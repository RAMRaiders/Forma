import { describe, expect, it } from 'vitest';

import { offerNote } from './gamedata.js';

const offer = {
  id: 0,
  source: 'ScreenScraper',
  region: null,
  variant: null,
  format: null,
  size: null
};

describe('what tells a scraper offer from the others', () => {
  it('says its country, kind, format and size', () => {
    expect(
      offerNote({ ...offer, region: 'USA', variant: 'Small file', format: 'mp4', size: 12_400_000 })
    ).toBe('USA · Small file · MP4 · 12 MB');
  });

  it('keeps small sizes readable', () => {
    expect(offerNote({ ...offer, format: 'pdf', size: 2_500_000 })).toBe('PDF · 2.5 MB');
    expect(offerNote({ ...offer, size: 240_000 })).toBe('240 KB');
  });

  it('is empty when the source says nothing', () => {
    expect(offerNote(offer)).toBe('');
  });
});
