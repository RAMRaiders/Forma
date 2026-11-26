import { FormaError, type Forma, type User } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

/**
 * Who's playing: every user; A enters as them, asking the PIN of one who has it. `entered`
 * follows, with who it was.
 */
export function whoScreen(
  forma: Forma,
  page: Page,
  entered: (user: User) => void
): () => Promise<Row[]> {
  return async () => {
    const { users, current } = await forma.users.list();
    return users.map((u, i) => ({
      heading: i === 0 ? "Who's playing?" : undefined,
      label: u.name,
      detail: [
        u.admin ? 'Admin' : null,
        u.pin ? 'PIN' : null,
        u.id === current.id ? 'In now' : null
      ]
        .filter(Boolean)
        .join(' · '),
      run: async () => {
        let pin: string | undefined;
        if (u.pin) {
          const typed = await page.ask({
            title: `${u.name}'s PIN`,
            value: '',
            secret: true,
            layout: 'address'
          });
          if (typed === null) return;
          pin = typed.trim();
        }
        let user: User | null = null;
        try {
          user = await forma.users.enter(u.id, pin);
        } catch (error) {
          page.notify(FormaError.from(error).message);
        }
        if (user) entered(user);
      }
    }));
  };
}

/** The admin's users: each one (Y removes, asking first), adding one, and the admin's PIN. */
export function usersScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const { users } = await forma.users.list();
    const admin = users.find((u) => u.admin);
    const others = users.filter((u) => !u.admin);
    const setPin = async () => {
      const typed = await page.ask({
        title: 'A PIN, 4 to 8 digits',
        value: '',
        secret: true,
        layout: 'address'
      });
      if (typed === null || !typed.trim()) return;
      await page.run('Saving…', async () => {
        await forma.users.setPin(typed.trim());
        return 'PIN set: it is asked when entering as the admin';
      });
    };
    const rename = (u: User) => async () => {
      const name = (await page.ask({ title: 'Their name', value: u.name }))?.trim();
      if (!name || name === u.name) return;
      await page.run('Renaming…', async () => {
        await forma.users.rename(u.id, name);
        return `Renamed ${name}`;
      });
    };
    return [
      {
        heading: 'Admin',
        label: admin?.name ?? 'Admin',
        detail: 'The library and Forma are theirs. A to rename',
        run: admin ? rename(admin) : undefined
      },
      {
        label: 'PIN',
        detail: admin?.pin
          ? 'PIN set: A to change it · Y to take it off'
          : 'No PIN: anyone can enter as the admin. A to set one',
        run: setPin,
        alt: admin?.pin
          ? {
              label: 'Take PIN off',
              run: () =>
                page.run('Saving…', async () => {
                  await forma.users.setPin(null);
                  return 'PIN taken off';
                })
            }
          : undefined
      },
      ...others.map((u, i): Row => ({
        heading: i === 0 ? 'Users' : undefined,
        label: u.name,
        detail: 'Their own list, favourites, saves and accounts · A to rename · Y to remove',
        run: rename(u),
        alt: { label: 'Remove', run: () => removeUser(forma, page, u) }
      })),
      {
        heading: others.length === 0 ? 'Users' : undefined,
        label: 'Add a user…',
        detail: 'Their list starts as every game; they take off what they do not want',
        run: async () => {
          const name = (await page.ask({ title: 'Their name', value: '' }))?.trim();
          if (!name) return;
          await page.run('Adding…', async () => {
            await forma.users.add(name);
            return `${name} added`;
          });
        }
      }
    ];
  };
}

/** Asks before removing a user, keeping them chosen: a stray A does no harm. */
function removeUser(forma: Forma, page: Page, user: User): Promise<void> {
  return page.push(`Remove ${user.name}?`, () => [
    { label: 'Keep them', detail: 'Nothing is removed', run: () => page.pop() },
    {
      danger: true,
      label: 'Remove',
      detail: 'Their list, favourites, history and accounts. Their saves stay on disk',
      run: () =>
        page.run('Removing…', async () => {
          await forma.users.remove(user.id);
          await page.pop();
          return `${user.name} removed`;
        })
    }
  ]);
}

/** My games: each system with games taken off the list; in one, A puts a game back. */
export function myGamesScreen(forma: Forma, page: Page): () => Promise<Row[]> {
  return async () => {
    const systems = await forma.library.systems();
    const off = await Promise.all(
      systems.map(async (s) => [s, await forma.library.unlisted(s.name).catch(() => [])] as const)
    );
    const rows: Row[] = off
      .filter(([, games]) => games.length > 0)
      .map(([s, games]) => ({
        label: s.fullname || s.name,
        detail: `${games.length} ${games.length === 1 ? 'game' : 'games'} off your list`,
        run: () =>
          page.push(`Add games: ${s.fullname || s.name}`, putBackScreen(forma, page, s.name))
      }));
    return rows.length > 0
      ? rows
      : [{ label: 'Every game is on your list', detail: 'Take one off with Select on it' }];
  };
}

/** A system's games off the user's list: A puts one back. */
function putBackScreen(forma: Forma, page: Page, system: string): () => Promise<Row[]> {
  return async () => {
    const games = await forma.library.unlisted(system);
    if (games.length === 0) return [{ label: 'Every game is back on your list' }];
    return games.map((g) => ({
      label: g.name,
      detail: 'A to put it back on your list',
      run: () =>
        page.run('Adding…', async () => {
          await forma.library.setListed(g.id, true);
          return `${g.name} is on your list`;
        })
    }));
  };
}

/** Select on a game: taking it off the user's list (Add games in Start puts it back). */
export function unlistRow(forma: Forma, page: Page, game: { id: number; name: string }): Row {
  return {
    icon: 'filter',
    label: 'Remove from my list',
    detail: 'It stays in the library: Start, Add games puts it back',
    run: () =>
      page.run('Removing…', async () => {
        await forma.library.setListed(game.id, false);
        page.close();
        return `${game.name} is off your list`;
      })
  };
}
