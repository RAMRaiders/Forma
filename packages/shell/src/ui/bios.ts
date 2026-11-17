import type { BiosFile, BiosMissing, Forma, SystemBios } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

const STATE: Record<BiosFile['state'], string> = {
  ok: 'There',
  missing: 'Missing',
  wrong: 'Wrong version'
};

/** A file as a row: where it goes, and how it is. */
const fileRow = (f: BiosFile, heading?: string): Row => ({
  heading,
  label: f.file,
  detail: f.md5 ? `${STATE[f.state]} · MD5 ${f.md5}` : STATE[f.state],
  badge: f.state === 'ok' ? { text: 'OK', kept: true } : { text: STATE[f.state] }
});

/** What a system's files come to, in a few words. */
function summary(s: SystemBios): string {
  const there = s.files.filter((f) => f.state === 'ok').length;
  const count = `${there} of ${s.files.length} there`;
  if (s.missing) return `Its games will not start · ${count}`;
  return s.needed ? `Ready · ${count}` : `Optional · ${count}`;
}

/**
 * Every system with games that lists BIOS files, those its games cannot start without and
 * lack first: each opens its files, where they go and the MD5 each should have.
 */
export function biosScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const systems = await forma.bios.status();
    if (systems.length === 0) {
      return [{ label: 'No system with games lists a BIOS', detail: 'Or this is not Batocera' }];
    }
    const sorted = [...systems].sort((a, b) => Number(b.missing) - Number(a.missing));
    return sorted.map((s, i): Row => ({
      heading:
        i === 0
          ? s.missing
            ? 'Missing'
            : 'Systems'
          : s.missing !== sorted[i - 1]!.missing
            ? 'Systems'
            : undefined,
      label: s.fullname,
      detail: summary(s),
      badge: s.missing ? { text: 'Missing' } : undefined,
      run: () =>
        page.push(
          s.fullname,
          () => s.files.map((f, j) => fileRow(f, j === 0 ? 'In /userdata' : undefined)),
          s.needed
            ? s.files.length > 1 && s.missing
              ? 'Its games need these BIOS files: one of them is enough, unless it says otherwise.'
              : 'Its games need these BIOS files.'
            : 'Its games start without these; some use them.'
        )
    }));
  };
}

/**
 * Before a game whose system lacks its BIOS: A starts it anyway, B (or Cancel) does not.
 * `start` makes the launch again with `ignoreBios`.
 */
export function biosAskScreen(
  missing: BiosMissing,
  start: () => void,
  cancel: () => void
): () => Row[] {
  return () => [
    { label: 'Start anyway', detail: `Play ${missing.name}`, run: start },
    { label: 'Cancel', detail: 'Back to the games', run: cancel },
    ...missing.files.map((f, i) => fileRow(f, i === 0 ? 'Not in /userdata' : undefined))
  ];
}
