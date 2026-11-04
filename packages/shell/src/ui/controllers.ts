import type { BluetoothDevice, ControllerKind, Forma } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

const KIND: Record<ControllerKind, string> = {
  xbox: 'Xbox',
  playstation: 'PlayStation',
  nintendo: 'Nintendo',
  generic: 'Controller'
};

/**
 * Controllers, as ES's Controller settings have them: the pads in use now, by player; Bluetooth
 * on or off; pairing a new pad (it looks for a minute, saying what it finds); and the paired
 * ones, A to connect or disconnect, Y to forget.
 */
export function controllersScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  const pair = () =>
    page.run('Put your controller in pairing mode…', async () => {
      const stop = forma.events.on('bluetooth:pairing', (e) =>
        page.progress(`Put your controller in pairing mode… ${e.message}`)
      );
      try {
        const paired = await forma.bluetooth.pair();
        return paired.length
          ? `Paired ${paired.map((d) => d.name).join(', ')}`
          : 'No new controller found';
      } finally {
        stop();
      }
    });
  const device = (d: BluetoothDevice): Row => ({
    label: d.name,
    detail: [d.connected ? 'Connected' : 'Not connected', d.controller ? '' : 'Not a controller']
      .filter(Boolean)
      .join(' · '),
    run: () =>
      d.connected
        ? page.run(`Disconnecting ${d.name}…`, async () => {
            await forma.bluetooth.disconnect(d.address);
            return `${d.name} disconnected`;
          })
        : page.run(`Connecting ${d.name}… turn it on`, async () => {
            await forma.bluetooth.connect(d.address);
            return `${d.name} connected`;
          }),
    alt: {
      label: 'Forget',
      run: () =>
        page.run(`Forgetting ${d.name}…`, async () => {
          await forma.bluetooth.forget(d.address);
          return `${d.name} forgotten`;
        })
    }
  });

  return async () => {
    const [status, bluetooth] = await Promise.all([
      forma.status.get(),
      forma.bluetooth.status().catch(() => null)
    ]);
    const inUse: Row[] = status.controllers.length
      ? status.controllers.map((c, i) => ({
          heading: i === 0 ? 'In use' : undefined,
          label: `Player ${c.player}`,
          detail: `${c.name} · ${KIND[c.kind]}`
        }))
      : [{ heading: 'In use', label: 'No controller connected' }];
    if (!bluetooth) return inUse;
    const toggle = () =>
      page.run(bluetooth.enabled ? 'Turning Bluetooth off…' : 'Turning Bluetooth on…', () =>
        forma.bluetooth.setEnabled(!bluetooth.enabled)
      );
    const rows: Row[] = [
      ...inUse,
      {
        heading: 'Bluetooth',
        label: 'Bluetooth',
        detail: bluetooth.enabled ? 'On' : 'Off',
        run: toggle,
        adjust: toggle
      }
    ];
    if (!bluetooth.enabled) return rows;
    rows.push({
      label: 'Pair a controller…',
      detail: 'Looks for a minute for pads in pairing mode',
      run: pair
    });
    bluetooth.devices.forEach((d, i) =>
      rows.push({ ...device(d), heading: i === 0 ? 'Paired' : undefined })
    );
    return rows;
  };
}
