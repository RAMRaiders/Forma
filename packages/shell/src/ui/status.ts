import type { ControllerKind, NetworkLink, StatusBar } from '@ramraiders/forma-sdk';

import { el } from './dom.js';

/**
 * Forma's status, in the top-right corner over every theme: one icon per controller connected,
 * drawn for its kind and numbered by player, then the connection. A theme restyles it from
 * outside (`forma-shell::part(status)`, `status-controller`, `status-network`), or draws its own
 * (`"draws": ["status"]` in its theme.json).
 */
export class StatusView {
  readonly root = el('div', { class: 'status', part: 'status', 'aria-label': 'Status' });

  show(status: StatusBar): void {
    this.root.replaceChildren(
      ...status.controllers.map((c) => {
        const item = el(
          'span',
          {
            class: `pad pad-${c.kind}`,
            part: `status-controller status-${c.kind}`,
            title: `Player ${c.player}: ${c.name}`,
            'aria-label': `Player ${c.player}: ${c.name}`
          },
          el('span', { class: 'player' }, String(c.player))
        );
        item.insertAdjacentHTML('afterbegin', PADS[c.kind]);
        return item;
      }),
      ...(status.network ? [network(status.network)] : [])
    );
  }
}

function network(link: NetworkLink): HTMLElement {
  const label =
    link.kind === 'none'
      ? 'Not connected'
      : `${link.kind === 'wifi' ? 'Wi-Fi' : 'Cable'}${link.internet ? ', online' : ', no internet'}`;
  const item = el('span', {
    class: `net net-${link.kind}${link.kind !== 'none' && !link.internet ? ' offline' : ''}`,
    part: 'status-network',
    title: label,
    'aria-label': label
  });
  // The signal waves for any connection, cable included: the icon everyone reads as "online".
  item.innerHTML = link.kind === 'none' ? NETWORK.none : NETWORK.wifi;
  return item;
}

const svg = (body: string) =>
  `<svg viewBox="0 0 32 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

/** A pad's body: two grips and a bridge, the outline every kind shares but Nintendo's. */
const BODY =
  '<path d="M9 5h14c3.6 0 5.5 2.6 6.4 6.6l1.1 5.2c.6 2.9-1.9 4.9-4.2 3.2L22.6 17H9.4l-3.7 3c-2.3 1.7-4.8-.3-4.2-3.2l1.1-5.2C3.5 7.6 5.4 5 9 5z"/>';

const PADS: Record<ControllerKind, string> = {
  // The guide button in the middle, an X across it.
  xbox: svg(
    `${BODY}<circle cx="16" cy="10" r="3.2" fill="currentColor" stroke="none"/><path d="M14.2 8.2l3.6 3.6M17.8 8.2l-3.6 3.6" stroke="var(--bg, #04060c)" stroke-width="1.3"/>`
  ),
  // The four symbols: triangle, circle, cross, square.
  playstation: svg(
    `${BODY}<path d="M23 7.4l1.3 2.2h-2.6z" stroke-width="1"/><circle cx="26" cy="11.2" r="1.1" stroke-width="1"/><path d="M22.2 12.2l1.6 1.6M23.8 12.2l-1.6 1.6" stroke-width="1"/><rect x="19.1" y="10.1" width="2" height="2" stroke-width="1"/>`
  ),
  // Two Joy-Con, side by side.
  nintendo: svg(
    '<path d="M15 4H11a5 5 0 0 0-5 5v6a5 5 0 0 0 5 5h4z"/><path d="M17 4h4a5 5 0 0 1 5 5v6a5 5 0 0 1-5 5h-4z"/><circle cx="11" cy="9" r="1.6" fill="currentColor" stroke="none"/><circle cx="21" cy="14" r="1.6" fill="currentColor" stroke="none"/>'
  ),
  // A D-pad and two buttons.
  generic: svg(
    `${BODY}<path d="M9 9v4M7 11h4" stroke-width="1.5"/><circle cx="22" cy="10" r="1" fill="currentColor" stroke="none"/><circle cx="24.5" cy="12.2" r="1" fill="currentColor" stroke="none"/>`
  )
};

const NETWORK = {
  wifi: svg(
    '<path d="M4 9.5a17 17 0 0 1 24 0M8 13.5a11 11 0 0 1 16 0M12 17.4a5 5 0 0 1 8 0"/><circle cx="16" cy="20.5" r="1.4" fill="currentColor" stroke="none"/>'
  ),
  // Wi-Fi, struck through.
  none: svg(
    '<path d="M4 9.5a17 17 0 0 1 24 0M8 13.5a11 11 0 0 1 16 0M12 17.4a5 5 0 0 1 8 0" opacity=".45"/><circle cx="16" cy="20.5" r="1.4" fill="currentColor" stroke="none" opacity=".45"/><path d="M6 3l20 19"/>'
  )
};
