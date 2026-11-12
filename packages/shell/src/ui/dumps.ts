import type { DumpReport, Forma, GameDump } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

/** Forma's setting: check games against the No-Intro and Redump DATs. */
const DUMPS_SETTING = 'library.verifyDumps';

function count(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

/** One system's line: how many verified, and what else. */
function dumpSummary(r: DumpReport): string {
  const parts = [`${r.verified.toLocaleString()} of ${count(r.games, 'game')} verified`];
  if (r.notCatalogued > 0) parts.push(`${r.notCatalogued.toLocaleString()} not catalogued`);
  if (r.doubtful > 0) parts.push(`${r.doubtful.toLocaleString()} doubtful`);
  const waiting = r.games - r.checked;
  if (waiting > 0) parts.push(`${waiting.toLocaleString()} not checked yet`);
  return parts.join(' · ');
}

/**
 * Start → Games → ROM check: whether games' files are the dumps No-Intro and Redump list, per
 * system, and the switch for it. Forma does the work in the background.
 */
export function dumpsScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const [checks, systems] = await Promise.all([
      forma.library.dumpChecks(),
      forma.library.systems().catch(() => [])
    ]);
    const names = new Map(systems.map((s) => [s.name, s.fullname || s.name]));
    const flip = () => page.run('…', () => forma.settings.setSystem(DUMPS_SETTING, !checks.on));
    const rows: Row[] = [
      {
        label: `Check ROMs: ${checks.on ? 'On' : 'Off'}`,
        detail: checks.on
          ? 'Each file’s checksums against No-Intro and Redump, fetched from libretro-database'
          : 'Off: nothing is fetched, and games show no dump status',
        adjust: flip,
        run: flip
      }
    ];
    if (checks.running)
      rows.push({ label: 'Checking…', detail: 'In the background, never during a game' });
    if (checks.on && checks.systems.length === 0) {
      rows.push({
        label: 'Nothing to check',
        detail: 'No system here has a No-Intro or Redump catalogue'
      });
    }
    if (checks.on) {
      checks.systems.forEach((r, i) =>
        rows.push({
          heading: i === 0 ? 'By system' : undefined,
          label: names.get(r.system) ?? r.system,
          detail: dumpSummary(r)
        })
      );
      rows.push({
        heading: 'What it means',
        label: 'Verified',
        detail: 'The file is exactly the dump the catalogue lists'
      });
      rows.push({
        label: 'Not catalogued',
        detail: 'A hack, translation, homebrew, or a changed or damaged file'
      });
      rows.push({
        label: 'Doubtful',
        detail: 'A dump the catalogue marks bad, or matched without a SHA1 to confirm it'
      });
    }
    return rows;
  };
}

/** A game's dump status in words, for its menu: `Verified · No-Intro`. */
export function dumpLabel(dump: GameDump): string {
  const catalogue =
    dump.catalogue === 'redump' ? 'Redump' : dump.catalogue === 'no-intro' ? 'No-Intro' : null;
  switch (dump.status) {
    case 'verified':
      return `Verified · ${catalogue}`;
    case 'crcOnly':
      return `Matched by CRC32 · ${catalogue}`;
    case 'badDump':
      return `Bad dump · ${catalogue}`;
    default:
      return 'Not catalogued';
  }
}
