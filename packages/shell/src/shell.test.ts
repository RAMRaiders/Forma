// @vitest-environment jsdom
/**
 * Forma's menus, driven like a gamepad would, against the SDK's mock. The mock grants the
 * permissions a test asks for; inside Forma the shell's token lets it past them whatever the
 * theme declares, which `ipc_tests.rs` checks on the Rust side.
 */
import { Forma, type InputEvent, type ShellContext } from '@ramraiders/forma-sdk';
import { mockBackend } from '@ramraiders/forma-sdk/mock';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { Shell, type ShellInfo } from './shell.js';

beforeEach(() => {
  // jsdom implements neither.
  Element.prototype.scrollIntoView = vi.fn();
  document.body.innerHTML = '';
});

function start(
  permissions: string[] = [],
  context: ShellContext = {},
  views: string[] | null = null,
  draws: string[] = []
) {
  const backend = mockBackend({ playMs: 50, permissions });
  const forma = new Forma(backend, '1.8.0', false);
  const open: boolean[] = [];
  const opened: string[] = [];
  const settings: [string, unknown][] = [];
  const theme: ShellInfo['theme'] = {
    id: 'neon',
    name: 'Neon',
    version: '2.0.0',
    settings: [
      {
        key: 'board',
        label: 'Board',
        group: 'Look',
        type: 'choice',
        choices: [
          { value: 'on', label: 'On' },
          { value: 'off', label: 'Off' }
        ],
        default: 'on',
        value: 'on'
      },
      { key: 'motion', label: 'Animations', type: 'toggle', default: true, value: true }
    ],
    views,
    draws
  };
  const shell = new Shell(forma, {
    info: () => Promise.resolve({ context, theme, formaVersion: '0.0.0' }),
    setOpen: (value) => {
      open.push(value);
      return Promise.resolve();
    },
    openView: (view) => {
      opened.push(view);
      return Promise.resolve();
    },
    setThemeSetting: (key, value) => {
      settings.push([key, value]);
      const setting = theme.settings.find((s) => s.key === key);
      if (setting) setting.value = value;
      return Promise.resolve();
    }
  });
  document.body.append(shell.root);
  const root = shell.root;

  const press = async (action: InputEvent['action'], repeat = false) => {
    shell.input({ action, button: null, gamepad: 0, pressed: true, repeat });
    await vi.waitFor(() => Promise.resolve());
    await new Promise((r) => setTimeout(r, 0));
  };
  const text = (selector: string) =>
    [...root.querySelectorAll(selector)].map((n) => n.textContent?.trim() ?? '');
  /** Start, down to the entry called `label`, A. */
  const openMenu = async (label: string) => {
    await press('start');
    await vi.waitFor(() => expect(text('.menu li .name').length).toBeGreaterThan(0));
    for (let i = 0; i < 20 && text('.menu li.selected .name')[0] !== label; i++) {
      await press('down');
    }
    expect(text('.menu li.selected .name')).toEqual([label]);
    await press('confirm');
  };
  return { shell, root, press, text, forma, open, opened, settings, openMenu, backend };
}

describe('Forma’s shell', () => {
  it('shows each controller by kind and player, and the connection, top right', async () => {
    const { shell, root } = start();
    await vi.waitFor(() => expect(root.querySelectorAll('.status .pad').length).toBe(1));
    expect(root.querySelector('.status .pad-xbox .player')?.textContent).toBe('1');
    expect(root.querySelector('.status .net-ethernet')?.classList.contains('offline')).toBe(false);
    await shell.showStatus({
      controllers: [
        { player: 1, name: 'Xbox 360 Controller', kind: 'xbox' },
        { player: 2, name: 'DualSense', kind: 'playstation' },
        { player: 3, name: 'Pro Controller', kind: 'nintendo' }
      ],
      network: { kind: 'wifi', internet: false }
    });
    const pads = [...root.querySelectorAll('.status .pad')].map((p) =>
      p.getAttribute('aria-label')
    );
    expect(pads).toEqual([
      'Player 1: Xbox 360 Controller',
      'Player 2: DualSense',
      'Player 3: Pro Controller'
    ]);
    const net = root.querySelector('.status .net')!;
    expect(net.getAttribute('aria-label')).toBe('Wi-Fi, no internet');
    expect(net.classList.contains('offline')).toBe(true);
    expect(root.querySelector('.status')?.getAttribute('part')).toBe('status');
    shell.playing = true;
    expect(root.querySelector<HTMLElement>('.status')!.hidden).toBe(true);
  });

  it('leaves the corner to a theme that draws its own status', async () => {
    const { shell, root } = start([], {}, null, ['status']);
    await shell.showStatus({ controllers: [], network: { kind: 'none', internet: false } });
    expect(root.querySelector<HTMLElement>('.status')!.hidden).toBe(true);
  });

  it('goes back to its menu on B from a page, on the entry it came from', async () => {
    const { press, text, shell, openMenu } = start(['settings.system']);
    await openMenu('Music…');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Music']));
    await press('back');
    await vi.waitFor(() => expect(text('.menu li.selected .name')).toEqual(['Music…']));
    expect(shell.open).toBe(true);
    // B in the menu itself closes it.
    await press('back');
    expect(shell.open).toBe(false);
  });

  it('goes back to its menu on B from the network screen and the views list', async () => {
    const { press, text, shell, openMenu } = start(['system.network']);
    await openMenu('Network…');
    await press('back');
    await vi.waitFor(() => expect(text('.menu li.selected .name')).toEqual(['Network…']));
    await press('back');
    await openMenu('Views…');
    await vi.waitFor(() => expect(shell.open).toBe(true));
    await press('back');
    await vi.waitFor(() => expect(text('.menu li.selected .name')).toEqual(['Views…']));
    // Start shuts everything, wherever it is.
    await press('start');
    expect(shell.open).toBe(false);
  });

  it('shows each new song for the time Forma’s setting says, then hides it', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    try {
      const { shell, root } = start();
      const song = root.querySelector<HTMLElement>('.song')!;
      expect(song.getAttribute('part')).toBe('now-playing');
      await shell.announce({ title: 'Angry Again', artist: 'Megadeth', seconds: 8 });
      expect(song.hidden).toBe(false);
      expect(song.querySelector('.title')?.textContent).toBe('Angry Again');
      expect(song.querySelector('.artist')?.textContent).toBe('Megadeth');
      vi.advanceTimersByTime(7900);
      expect(song.hidden).toBe(false);
      vi.advanceTimersByTime(200);
      expect(song.hidden).toBe(true);
    } finally {
      vi.useRealTimers();
    }
  });

  it('leaves the song to a theme that draws it, and shows none over a game', async () => {
    const drawn = start([], {}, null, ['nowPlaying']);
    await drawn.shell.announce({ title: 'Toxicity', artist: null, seconds: 8 });
    expect(drawn.root.querySelector<HTMLElement>('.song')!.hidden).toBe(true);

    const plain = start();
    plain.shell.playing = true;
    await plain.shell.announce({ title: 'Toxicity', artist: null, seconds: 8 });
    expect(plain.root.querySelector<HTMLElement>('.song')!.hidden).toBe(true);
  });

  it('sets how long a new song shows, from the music page', async () => {
    const { press, text, openMenu, forma } = start(['settings.system']);
    await openMenu('Music…');
    await vi.waitFor(() => expect(text('.page li .name')).toContain('Show each new song'));
    for (let i = 0; i < 30 && text('.page li.selected .name')[0] !== 'Show each new song'; i++) {
      await press('down');
    }
    expect(text('.page li.selected .description')).toEqual(['For 8 seconds']);
    await press('right');
    await vi.waitFor(() =>
      expect(text('.page li.selected .description')).toEqual(['For 10 seconds'])
    );
    expect(await forma.settings.getSystem('music.songSeconds')).toBe(10);
    for (let i = 0; i < 6; i++) await press('left');
    await vi.waitFor(() => expect(text('.page li.selected .description')).toEqual(['Off']));
  });

  it('keeps to the views a theme says it shows: one, and there is nothing to pick', async () => {
    const { press, text, shell, openMenu } = start([], {}, ['systems']);
    await press('start');
    expect(text('.switcher li .name')).not.toContain('Views…');
    await press('back');
    await press('select');
    expect(text('.toast')).toEqual(['Neon shows the library one way']);
    expect(shell.open).toBe(false);
    await openMenu('Interface…');
    await vi.waitFor(() => expect(text('.page li .name').length).toBeGreaterThan(0));
    expect(text('.page li .name').some((n) => n.startsWith('Start on'))).toBe(false);
  });

  it('opens its menu on Start, over any theme, and tells Forma input is its own', async () => {
    const { press, text, open, shell } = start();
    await press('confirm');
    expect(shell.open).toBe(false);
    await press('start');
    expect(text('.switcher h2')).toEqual(['Forma 0.0.0']);
    expect(text('.switcher .note')).toEqual(['Neon 2.0.0']);
    expect(text('.switcher li .name')).toEqual([
      'Views…',
      'Interface…',
      'Music…',
      'RetroAchievements…',
      'Netplay…',
      'Controllers…',
      'Network…',
      'Content downloader…',
      'Game settings…',
      'System settings…',
      'Switch to EmulationStation',
      'Restart',
      'Shut down'
    ]);
    expect(text('.hints .hint')).toEqual(['AChoose', 'BClose']);
    expect(open).toEqual([true]);
    await press('back');
    expect(shell.open).toBe(false);
    expect(open).toEqual([true, false]);
  });

  it('offers options for the game and system the theme says are selected', async () => {
    const { press, text } = start([], {
      view: 'systems',
      system: { name: 'snes', fullname: 'Super Nintendo' },
      game: { id: 3, name: 'Super Metroid' }
    });
    await press('start');
    expect(text('.switcher li .name').slice(0, 2)).toEqual([
      'Super Metroid options…',
      'Super Nintendo options…'
    ]);
  });

  it('power actions need a second press', async () => {
    const { press, text } = start();
    await press('start');
    for (let i = 0; i < 11; i++) await press('down');
    expect(text('.switcher li.selected .name')).toEqual(['Restart']);
    await press('confirm');
    expect(text('.toast')).toEqual(['Press A again to restart']);
    await press('confirm');
    await vi.waitFor(() => expect(text('.toast')[0]).toContain('system.power'));
  });

  it('lists views on Select, opens one in the theme, and sets the home view', async () => {
    const { press, text, forma, opened, shell } = start([], { view: 'systems' });
    await press('select');
    await vi.waitFor(() => expect(text('.switcher li.selected .name')).toEqual(['Systems']));
    expect(text('.switcher li.selected .badge')).toEqual(['Home', 'Showing']);
    await press('down');
    await press('north');
    await vi.waitFor(async () =>
      expect((await forma.nav.defaultView()).id).toBe('manufacturer-system')
    );
    await press('confirm');
    expect(opened).toEqual(['manufacturer-system']);
    expect(shell.open).toBe(false);
  });

  it('switches theme, and sets the theme’s own settings, from Interface', async () => {
    const { press, text, settings, forma } = start(['settings.system']);
    await press('start');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Interface']));
    expect(text('.page li.heading')).toEqual(['Theme', 'Screen', 'Look', 'This theme']);
    const names = text('.page li[role=option] .name');
    expect(names).toContain('Board: On');
    expect(names).toContain('Animations: On');

    // Down to the board, past the themes, Start on and the margins.
    const board = names.indexOf('Board: On');
    for (let i = 0; i < board; i++) await press('down');
    await press('right');
    await vi.waitFor(() => expect(settings).toEqual([['board', 'off']]));
    await vi.waitFor(() => expect(text('.page li.selected .name')).toEqual(['Board: Off']));

    // Margins are Forma's, shared by every theme.
    const margins = names.findIndex((n) => n.startsWith('Screen margins'));
    for (let i = board; i > margins; i--) await press('up');
    await press('right');
    await vi.waitFor(async () => expect(await forma.settings.getSystem('display.margins')).toBe(1));
  });

  it('Start closes whatever is open, except the keyboard, which it finishes', async () => {
    const { press, shell } = start(['system.network']);
    await press('start');
    await press('down');
    await press('confirm');
    expect(shell.open).toBe(true);
    await press('start');
    expect(shell.open).toBe(false);
  });

  it('stays shut while a game runs', async () => {
    const { press, shell } = start();
    shell.playing = true;
    await press('start');
    expect(shell.open).toBe(false);
  });

  it('joins a Wi-Fi network with a password typed on the on-screen keyboard', async () => {
    const { press, text } = start(['system.network']);
    await press('start');
    for (let i = 0; i < 6; i++) await press('down');
    expect(text('.switcher li.selected .name')).toEqual(['Network…']);
    await press('confirm');
    const rows = () => text('.network > .panel li .name');
    await vi.waitFor(() => expect(rows()).toContain('▮▮▯▯  Office 5G'));
    expect(rows()[0]).toBe('Ethernet');
    expect(rows()[1]).toBe('Wi-Fi: on');

    for (let i = 0; i < 4; i++) await press('down');
    await press('confirm');
    expect(text('.keyboard h2')).toEqual(['Password for Office 5G']);
    await press('confirm');
    for (const key of 'uiet-place') {
      window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    }
    await press('north');
    expect(text('.keyboard .field')).toEqual(['quiet-place']);
    await press('start');
    await vi.waitFor(() =>
      expect(text('.toast')[0]).toBe(
        'Office 5G is saved. The cable stays in use; unplug it to use Wi-Fi'
      )
    );
  });
});

describe('Forma’s settings pages', () => {
  it('music: picks a playlist, shows it playing, and adds a web radio', async () => {
    const { press, text, openMenu } = start(['music.manage']);
    await openMenu('Music…');
    const rows = () => text('.page > .panel li .name');
    await vi.waitFor(() => expect(rows()).toContain('Chiptune'));
    expect(rows()[0]).toBe('Nothing playing');
    expect(text('.page li.heading')).toEqual(['Folders and playlists', 'Web radios']);

    // Down to Chiptune: Nothing playing, Volume, Shuffle, Show each new song, Music, Chiptune.
    for (let i = 0; i < 5; i++) await press('down');
    expect(text('.page li.selected .name')).toEqual(['Chiptune']);
    await press('confirm');
    await vi.waitFor(() => expect(rows()[0]).toBe('Track 1'));
    // Rows were added above it (Pause, Next…): the selection stayed on Chiptune.
    expect(text('.page li.selected .name')).toEqual(['Chiptune']);
    expect(rows()).toContain('Pause');

    // Add a radio through the keyboard: name, then address.
    for (let i = 0; i < 20 && text('.page li.selected .name')[0] !== 'Add a web radio…'; i++) {
      await press('down');
    }
    expect(text('.page li.selected .name')).toEqual(['Add a web radio…']);
    await press('confirm');
    for (const key of 'Lofi') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    for (const key of 'lofi.example/live') {
      window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    }
    await press('start');
    await vi.waitFor(() => expect(text('.toast')[0]).toBe('Lofi added'));
    expect(rows()).toContain('Lofi');
  });

  it('RetroAchievements: signs in, and flips a switch', async () => {
    const { press, text, openMenu } = start(['settings.batocera', 'settings.system']);
    await openMenu('RetroAchievements…');
    const rows = () => text('.page > .panel li .name');
    await vi.waitFor(() => expect(rows()[0]).toBe('Sign in…'));
    await press('confirm');
    expect(text('.keyboard h2')).toEqual(['RetroAchievements username']);
    // Clear what is there (the fixture's user; B on an empty field would cancel), then type.
    const filled = text('.keyboard .field')[0]!.length;
    for (let i = 0; i < filled; i++) await press('back');
    for (const key of 'mike') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    expect(text('.keyboard h2')).toEqual(['Password for mike']);
    for (const key of 'secret!') window.dispatchEvent(new KeyboardEvent('keydown', { key }));
    await press('start');
    await vi.waitFor(() => expect(text('.toast')[0]).toBe('Signed in as mike'));
    expect(rows()[0]).toBe('Signed in as mike');

    await press('down');
    const before = text('.page li.selected .name')[0];
    await press('right');
    await vi.waitFor(() => expect(text('.page li.selected .name')[0]).not.toBe(before));
  });

  it('netplay: cycles the relay server and shows the custom one only when chosen', async () => {
    const { press, text, openMenu } = start(['settings.batocera']);
    await openMenu('Netplay…');
    const rows = () => text('.page > .panel li .name');
    await vi.waitFor(() => expect(rows()).toContain('Relay server'));
    expect(rows()).not.toContain('Custom server');
    while (text('.page li.selected .name')[0] !== 'Relay server') await press('down');
    // Back from the first choice wraps to the last: Custom server.
    await press('left');
    await vi.waitFor(() => expect(rows()).toContain('Custom server'));
    expect(text('.page li.selected .description')).toEqual(['Custom server']);
  });

  it('game settings: global options, a choice cycled, a page of its own and back', async () => {
    const { press, text, openMenu } = start(['settings.emulators']);
    await openMenu('Game settings…');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Game aspect ratio'));
    expect(text('.page h2')).toEqual(['Game settings']);
    expect(rows()).toContain('Game rendering & shaders…');
    while (selected() !== 'Game aspect ratio') await press('down');
    expect(text('.page li.selected .description')).toEqual(['Default']);
    await press('right');
    await vi.waitFor(() => expect(text('.page li.selected .description')).toEqual(['4/3']));
    // Its own page: B comes back to the row it was opened from.
    while (selected() !== 'Game rendering & shaders…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Game rendering & shaders']));
    expect(rows()).toEqual(['Shader set', 'Integer scaling (pixel perfect)']);
    await press('back');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Game settings']));
    expect(selected()).toBe('Game rendering & shaders…');
  });

  it("a system's options follow its core, and a game inherits them", async () => {
    const context: ShellContext = {
      view: 'systems',
      system: { name: 'nes', fullname: 'Nintendo Entertainment System' }
    };
    const { press, text, openMenu } = start(['settings.emulators'], context);
    const rows = () => text('.page > .panel li[role=option] .name');
    await openMenu('Nintendo Entertainment System options…');
    await vi.waitFor(() => expect(rows().slice(0, 2)).toEqual(['Emulator', 'Core']));
    expect(text('.page .heading')).toContain('Advanced options');
    expect(rows()).toContain('Palette');
    while (text('.page li.selected .name')[0] !== 'Core') await press('down');
    // Default (fceumm) → the first core in the list, then the next: nestopia, whose options
    // have no palette.
    await press('right');
    await press('right');
    await vi.waitFor(() => expect(text('.page li.selected .description')).toEqual(['nestopia']));
    expect(rows()).not.toContain('Palette');
    // B back to the menu, B again to close it.
    await press('back');
    await press('back');

    // A game of the NES selected in the theme: its core is the system's.
    context.game = { id: 2, name: 'Super Mario Bros.' };
    await openMenu('Super Mario Bros. options…');
    await vi.waitFor(() => expect(rows()).toContain('Core'));
    const core = rows().indexOf('Core');
    expect(text('.page > .panel li[role=option] .description')[core]).toBe(
      'Default · nestopia (system)'
    );
  });

  it('system settings: information, a long list picked from, and when it applies', async () => {
    const { press, text, openMenu } = start(['system.settings']);
    await openMenu('System settings…');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Time zone'));
    expect(text('.page .heading')).toEqual([
      'Display',
      'Audio',
      'Language & region',
      'System',
      'Storage'
    ]);
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['About this box']));
    expect(rows()[0]).toBe('Batocera');
    await press('back');
    // 36 languages: a list to pick from, starting on the one in use.
    while (selected() !== 'Language') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Language']));
    expect(selected()).toBe('✓ English (US)');
    await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['System settings']));
    await vi.waitFor(() =>
      expect(text('.page li.selected .description')).toEqual([
        'English (UK) · after Forma restarts'
      ])
    );
  });

  it('turns Batocera labels into sentence case, keeping acronyms', async () => {
    const { sentence } = await import('./ui/settings.js');
    expect(sentence('GAME ASPECT RATIO')).toBe('Game aspect ratio');
    expect(sentence('ENABLE AI TRANSLATION SERVICE')).toBe('Enable AI translation service');
    expect(sentence('STRETCH BEZELS (4K & ULTRAWIDE)')).toBe('Stretch bezels (4K & ultrawide)');
    expect(sentence('Already fine')).toBe('Already fine');
  });

  it('content downloader: a category, an install with its progress, and removing it', async () => {
    const { press, text, openMenu } = start(['store.manage']);
    await openMenu('Content downloader…');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Refresh the list'));
    // One package has a newer version; systems by their names in the library.
    expect(rows()[0]).toBe('Update all (1)');
    expect(rows()).toContain('Nintendo Entertainment System');
    expect(rows().indexOf('Decorations')).toBeLessThan(rows().indexOf('Other'));
    while (selected() !== 'Nintendo Entertainment System') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page h2')).toEqual(['Nintendo Entertainment System']));
    const before = text('.page li.selected .description')[0]!;
    expect(before).not.toContain('Installed');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page .note')[0]).toMatch(/^Downloading .*%$/));
    await vi.waitFor(() =>
      expect(text('.page li.selected .description')[0]).toContain('Installed')
    );
    expect(text('.toast')[0]).toMatch(/ installed$/);
    await press('north');
    await vi.waitFor(() => expect(text('.toast')[0]).toMatch(/ removed$/));
    expect(text('.page li.selected .description')[0]).toBe(before);
  });

  it('controllers: the pads in use, pairing a new one, connecting and forgetting', async () => {
    const { press, text, openMenu } = start(['system.controllers']);
    await openMenu('Controllers…');
    const rows = () => text('.page > .panel li[role=option] .name');
    const selected = () => text('.page li.selected .name')[0];
    await vi.waitFor(() => expect(rows()).toContain('Pair a controller…'));
    expect(rows()[0]).toBe('Player 1');
    expect(rows()).toContain('8BitDo Pro 2');
    while (selected() !== 'Pair a controller…') await press('down');
    await press('confirm');
    await vi.waitFor(() => expect(text('.toast')[0]).toBe('Paired Wireless Controller'));
    await vi.waitFor(() => expect(rows()).toContain('Wireless Controller'));
    while (selected() !== 'PLAYSTATION(R)3 Controller') await press('down');
    expect(text('.page li.selected .description')[0]).toBe('Not connected');
    await press('confirm');
    await vi.waitFor(() => expect(text('.page li.selected .description')[0]).toBe('Connected'));
    await press('north');
    await vi.waitFor(() => expect(rows()).not.toContain('PLAYSTATION(R)3 Controller'));
  });
});
