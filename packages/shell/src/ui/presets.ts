import type { Forma, Preset } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

/** Under every preset's page: how it sits with the user's own settings. */
const UNDER_YOURS =
  'Your own settings always win: a system’s or a game’s, and your global ones over the preset’s for every system';

/** A preset's page: on or off, then what it does and every option it sets. */
function presetScreen(forma: Forma, page: Page, id: string): () => Promise<Row[]> {
  return async () => {
    const p = (await forma.presets.list()).find((x) => x.id === id);
    if (!p) return [{ label: 'This preset is gone' }];
    const use = () =>
      page.run(p.active ? 'Stopping…' : 'Switching on…', async () => {
        await forma.presets.setActive(p.active ? null : p.id);
        return p.active
          ? `${p.name} off: your own settings alone, from the next launch`
          : `${p.name} on, from the next launch`;
      });
    return [
      {
        label: p.active ? `${p.name}: On` : `${p.name}: Off`,
        detail: p.active ? 'A to stop using it' : `${p.description} · A to use it`,
        badge: p.active ? { text: 'Active' } : undefined,
        run: use,
        adjust: use
      },
      ...p.notes.map((note, i) => ({ heading: i === 0 ? 'What it does' : undefined, label: note })),
      ...p.options.map((option, i) => ({
        heading: i === 0 ? 'Emulator options it sets' : undefined,
        label: option
      })),
      { heading: 'Your settings', label: UNDER_YOURS }
    ];
  };
}

function presetRow(forma: Forma, page: Page, p: Preset): Row {
  return {
    label: p.name,
    detail: p.description,
    badge: p.active ? { text: 'Active' } : undefined,
    run: () => page.push(p.name, presetScreen(forma, page, p.id))
  };
}

/**
 * Presets: ready-made setups for a kind of machine and screen (emulator options and the CRT
 * shader), one active or none, under the user's own settings.
 */
export function presetsScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const presets = await forma.presets.list();
    const active = presets.find((p) => p.active);
    const none = () =>
      page.run('Stopping…', async () => {
        await forma.presets.setActive(null);
        return 'No preset: your own settings alone, from the next launch';
      });
    return [
      ...presets.map((p) => presetRow(forma, page, p)),
      {
        heading: 'None',
        label: 'No preset',
        detail: active
          ? `Stop using ${active.name}: your own settings alone`
          : 'In use: your own settings alone',
        badge: active ? undefined : { text: 'Active' },
        run: active ? none : undefined
      }
    ];
  };
}
