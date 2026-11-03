import type { Forma, UpdateCheck } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

/**
 * Batocera's updates, as ES's Updates menu has them: the version the box runs, the newest for
 * its channel, and, only after a second page asks, the update itself, its progress shown while
 * it downloads and unpacks. Restarting is left to the user, with a row for it.
 */
export function updatesScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  let installed: string | null = null;

  const install = (check: UpdateCheck) =>
    page.run(`Updating to Batocera ${check.latest}…`, async () => {
      const stop = forma.events.on('updates:progress', (p) =>
        page.progress(
          `Updating to Batocera ${check.latest}… ${p.message}${p.percent === null ? '' : ` ${p.percent}%`}`
        )
      );
      try {
        await forma.updates.install();
      } finally {
        stop();
      }
      installed = check.latest;
      await page.pop();
      return `Batocera ${check.latest} installed: restart to use it`;
    });

  const confirm = (check: UpdateCheck) =>
    page.push(
      `Update to Batocera ${check.latest}?`,
      () => [
        {
          label: 'Update now',
          detail: 'Downloads a few gigabytes; Forma keeps working until you restart',
          run: () => install(check)
        },
        { label: 'Not now', run: () => page.pop() }
      ],
      'Keep the box plugged in while it updates.'
    );

  return async () => {
    if (installed) {
      return [
        { label: 'Installed', detail: `Batocera ${installed}, from the next start` },
        {
          label: 'Restart now',
          run: () => page.run('Restarting…', () => forma.system.restart())
        }
      ];
    }
    const check = await forma.updates.check();
    const rows: Row[] = [
      { label: 'This box', detail: `Batocera ${check.current}` },
      {
        label: `Newest (${check.channel})`,
        detail: check.available ? `Batocera ${check.latest}` : 'Up to date'
      }
    ];
    if (check.available) {
      rows.push({ label: `Update to Batocera ${check.latest}…`, run: () => confirm(check) });
    }
    rows.push({ label: 'Check again', run: () => page.run('Checking…', async () => {}) });
    return rows;
  };
}
