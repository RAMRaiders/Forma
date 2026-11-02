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

function start(permissions: string[] = [], context: ShellContext = {}) {
  const backend = mockBackend({ playMs: 50, permissions });
  const forma = new Forma(backend, '1.4.0', false);
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
    ]
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
  return { shell, root, press, text, forma, open, opened, settings };
}

describe('Forma’s shell', () => {
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
    for (let i = 0; i < 10; i++) await press('down');
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
    for (let i = 0; i < 5; i++) await press('down');
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
