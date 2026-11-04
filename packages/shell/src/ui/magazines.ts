import type { Forma, Magazine } from '@ramraiders/forma-sdk';

import type { Row } from './page.js';

const FORMATS: Record<Magazine['format'], string> = {
  zip: 'CBZ',
  rar: 'CBR',
  sevenzip: 'CB7',
  tar: 'CBT',
  pdf: 'PDF',
  folder: 'Pictures'
};

/** The magazines in `/userdata/magazines`, by folder; A opens one in the reader. */
export function magazinesScreen(
  forma: Forma,
  read: (magazine: Magazine) => unknown
): () => Promise<Row[]> {
  return async () => {
    const all = await forma.magazines.list();
    if (all.length === 0) {
      return [
        {
          label: 'No magazines yet',
          detail: 'Put CBZ, CBR, CB7, PDF files or folders of scans in /userdata/magazines'
        }
      ];
    }
    let folder: string | null = null;
    return all.map((m) => {
      const row: Row = {
        heading: m.folder !== folder ? m.folder || 'Magazines' : undefined,
        label: m.title,
        detail: `${FORMATS[m.format]}${m.page !== null ? ` · read to page ${m.page + 1}` : ''}`,
        run: () => read(m)
      };
      folder = m.folder;
      return row;
    });
  };
}
