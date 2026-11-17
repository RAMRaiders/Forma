import type { Forma, NetplayRoom, RoomFilter } from '@ramraiders/forma-sdk';
import { FormaError } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

/** Rooms answering slower than this are left out of "your games": too far to play. */
const MAX_PING_MS = 1000;

/**
 * Playing: join a room, online or on the local network. Joining starts the local copy of the
 * room's game with the room's core, as RetroArch needs. Hosting is a game's: Select → Start game.
 */
export function playRows(forma: Forma, page: Page): Row[] {
  const rows: Row[] = [
    {
      label: 'Rooms online…',
      detail: 'Games others are hosting in the libretro lobby',
      run: () => page.push('Rooms online', lobbyPage(forma, page))
    },
    {
      label: 'On this network…',
      detail: 'Boxes and PCs hosting here, without the internet',
      run: () => page.push('On this network', lanPage(forma, page))
    }
  ];
  rows[0] = { ...rows[0]!, heading: 'Play' };
  return rows;
}

/** Starts `game` hosting a netplay room, as Start → Netplay sets it up. */
export function hostRow(forma: Forma, page: Page, game: { id: number; name: string }): Row {
  return {
    label: 'Start hosting netplay',
    detail:
      'Others join from the lobby or this network, as Start → Netplay sets it. Y: with a password',
    run: () =>
      page.run('Starting…', async () => {
        await forma.netplay.host(game.id);
      }),
    alt: {
      label: 'Password',
      run: async () => {
        const password = await page.ask({ title: 'Password for players', value: '' });
        if (!password) return;
        await page.run('Starting…', async () => {
          await forma.netplay.host(game.id, { password });
        });
      }
    }
  };
}

function lobbyPage(forma: Forma, page: Page): () => Promise<Row[]> {
  let everything = false;
  return async () => {
    const filter: RoomFilter = everything ? {} : { owned: true, maxPingMs: MAX_PING_MS };
    const toggle: Row = {
      label: everything ? 'Showing: every room' : 'Showing: rooms for your games',
      detail: everything
        ? 'A: only games on this box'
        : 'Games on this box, answering within a second. A: every room',
      run: () => {
        everything = !everything;
        return page.refresh();
      }
    };
    let fetched;
    try {
      fetched = await forma.netplay.lobby(filter);
    } catch (error) {
      // The lobby is down or the box offline, with nothing saved: say so, and try again on A.
      return [
        toggle,
        {
          label: 'The lobby does not answer',
          detail: `${FormaError.from(error).message}. A: try again`,
          run: () => page.run('Asking…', async () => {})
        }
      ];
    }
    const { data, stale } = fetched;
    return [
      toggle,
      ...(stale ? [{ label: 'Offline: rooms from a moment ago' }] : []),
      ...(data.length ? data.map((r) => roomRow(forma, page, r)) : [{ label: 'No rooms' }])
    ];
  };
}

function lanPage(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const rooms = await forma.netplay.lan();
    return [
      {
        label: 'Look again',
        detail: 'RetroArch hosts answer on this network; not those behind a relay',
        run: () => page.run('Looking…', async () => {})
      },
      ...(rooms.length
        ? rooms.map((r) => roomRow(forma, page, r))
        : [{ label: 'Nobody is hosting here' }])
    ];
  };
}

/** What stops a room being joined from here, if anything. */
function blocker(room: NetplayRoom): string | null {
  if (room.localGameId === null) return 'Not on this box';
  if (room.core === null) return `${room.coreName} is not installed`;
  if (room.join === null) return 'No way in';
  return null;
}

function roomRow(forma: Forma, page: Page, room: NetplayRoom): Row {
  const why = blocker(room);
  const facts = [
    room.username || 'Someone',
    room.coreName,
    ...(room.pingMs !== null ? [`${room.pingMs} ms`] : []),
    ...(room.hasPassword ? ['Password'] : []),
    ...(why ? [why] : [])
  ];
  const join = (spectate: boolean) => async () => {
    let password: string | undefined;
    if (room.hasPassword || (spectate && room.hasSpectatePassword)) {
      const typed = await page.ask({ title: `Password for ${room.username}’s room`, value: '' });
      if (typed === null) return;
      password = typed;
    }
    await page.run('Joining…', async () => {
      await forma.netplay.join(room, { password, spectate });
    });
  };
  return {
    label: room.gameName,
    detail: facts.join(' · '),
    ...(why ? {} : { run: join(false), alt: { label: 'Watch', run: join(true) } })
  };
}
