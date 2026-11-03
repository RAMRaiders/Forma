import type { Forma, InputEvent, MusicSong, ShellContext, ViewInfo } from '@ramraiders/forma-sdk';
import { FormaError } from '@ramraiders/forma-sdk';

import { moveList } from './logic/nav.js';
import { el } from './ui/dom.js';
import { Menu, type MenuItem } from './ui/menu.js';
import { NetworkScreen } from './ui/network.js';
import { Page } from './ui/page.js';
import {
  type ThemeSettingValue,
  interfaceScreen,
  musicScreen,
  netplayScreen,
  retroAchievementsScreen
} from './ui/screens.js';
import { optionsScreen, systemSettingsScreen } from './ui/settings.js';
import { storeScreen } from './ui/store.js';
import { Switcher } from './ui/switcher.js';

/** What `shell_info` answers: the theme's context and its settings. */
export interface ShellInfo {
  context: ShellContext | null;
  theme: {
    id: string;
    name: string;
    version: string;
    settings: ThemeSettingValue[];
    /** The only views the theme shows, if it says; else every view. */
    views?: string[] | null;
    /** What the theme draws itself (`nowPlaying`): the shell leaves it to the theme. */
    draws?: string[];
  };
  formaVersion: string;
}

/** The calls only Forma's shell may make, next to the SDK's. */
export interface ShellCalls {
  info(): Promise<ShellInfo>;
  setOpen(open: boolean): Promise<void>;
  openView(view: string): Promise<void>;
  setThemeSetting(key: string, value: unknown): Promise<void>;
}

/** How long a power action waits for its second press. */
const CONFIRM_MS = 3000;

/**
 * Forma's menus over the theme: Start opens the menu, Select the views. Whatever the theme, the
 * user can always change theme, settings and power from here.
 */
export class Shell {
  readonly root = el('div', { class: 'shell' });
  readonly #menu = new Menu();
  readonly #switcher = new Switcher();
  readonly #page: Page;
  readonly #network: NetworkScreen;
  readonly #hints = el('footer', { class: 'hints', hidden: '' });
  readonly #toast = el('div', { class: 'toast', role: 'status', hidden: '' });
  #toastTimer: ReturnType<typeof setTimeout> | undefined;
  /**
   * The song that just started, for the time Forma's setting says. A theme restyles it from
   * outside with `forma-shell::part(now-playing)` (and `song-title`, `song-artist`), or draws its
   * own (`"draws": ["nowPlaying"]` in its theme.json).
   */
  readonly #song = el('div', { class: 'song', part: 'now-playing', role: 'status', hidden: '' });
  #songTimer: ReturnType<typeof setTimeout> | undefined;
  #info: ShellInfo | null = null;
  #views: ViewInfo[] = [];
  #home: string | null = null;
  #confirming: { id: string; until: number } | null = null;
  #wasOpen = false;
  /** Which settings page is open, for the events that redraw it. */
  #pageKind: string | null = null;
  /** A game is running: the shell stays shut. */
  playing = false;

  constructor(
    private readonly forma: Forma,
    private readonly calls: ShellCalls
  ) {
    const notify = (message: string) => this.notify(message);
    const changed = () => this.#changed();
    this.#page = new Page(notify, changed);
    this.#network = new NetworkScreen(forma, notify, changed);
    // What plays moves on by itself: the music page follows.
    forma.events.on('music:changed', () => {
      if (this.#page.open && this.#pageKind === 'music') void this.#page.refresh();
    });
    forma.events.on('music:song', (song) => void this.announce(song));
    this.root.append(
      this.#menu.root,
      this.#switcher.root,
      this.#network.root,
      this.#page.root,
      this.#hints,
      this.#song,
      this.#toast
    );
  }

  /** A new song: shown for its time, unless the theme draws it or a game is running. */
  async announce(song: MusicSong): Promise<void> {
    if (this.playing) return;
    const info = this.#info ?? (await this.#loadInfo());
    if (info?.theme.draws?.includes('nowPlaying')) return;
    this.#song.replaceChildren(
      el('span', { class: 'note', part: 'song-note', 'aria-hidden': 'true' }, '♪'),
      el(
        'span',
        { class: 'words' },
        el('span', { class: 'title', part: 'song-title' }, song.title),
        song.artist ? el('span', { class: 'artist', part: 'song-artist' }, song.artist) : ''
      )
    );
    this.#song.hidden = false;
    clearTimeout(this.#songTimer);
    this.#songTimer = setTimeout(() => (this.#song.hidden = true), song.seconds * 1000);
  }

  get open(): boolean {
    return this.#menu.open || this.#switcher.open || this.#network.open || this.#page.open;
  }

  input(e: InputEvent): void {
    if (this.playing) return;
    if (!this.open) {
      if (!e.pressed || e.repeat) return;
      if (e.action === 'start') void this.#openMenu();
      else if (e.action === 'select') void this.#openViews();
      return;
    }
    // Start shuts whatever is open, from anywhere, unless the keyboard is typing.
    if (e.pressed && !e.repeat && e.action === 'start' && !this.#typing()) {
      this.#closeAll();
      return;
    }
    if (this.#page.open) {
      this.#page.input(e);
      return this.#changed();
    }
    if (this.#network.open) {
      this.#network.input(e);
      return this.#changed();
    }
    if (!e.pressed) return;
    if (this.#menu.open) return this.#menuInput(e);
    if (this.#switcher.open) return this.#viewsInput(e);
  }

  notify(message: string): void {
    this.#toast.textContent = message;
    this.#toast.hidden = false;
    clearTimeout(this.#toastTimer);
    this.#toastTimer = setTimeout(() => (this.#toast.hidden = true), 4000);
  }

  #typing(): boolean {
    return this.#page.keyboard.open || this.#network.keyboard.open;
  }

  #closeAll(): void {
    this.#menu.close();
    this.#switcher.close();
    if (this.#network.open) this.#network.close();
    if (this.#page.open) this.#page.close();
    this.#changed();
  }

  async #loadInfo(): Promise<ShellInfo | null> {
    try {
      this.#info = await this.calls.info();
    } catch (error) {
      this.notify(FormaError.from(error).message);
    }
    return this.#info;
  }

  async #openMenu(): Promise<void> {
    this.#confirming = null;
    const info = await this.#loadInfo();
    const context = info?.context ?? {};
    const { game, system } = context;
    const items: MenuItem[] = [
      ...(game
        ? [
            {
              id: 'game-options',
              label: `${game.name} options…`,
              description: 'Emulator, core and options for this game only'
            }
          ]
        : []),
      ...(system
        ? [
            {
              id: 'system-options',
              label: `${system.fullname || system.name} options…`,
              description: 'Emulator, core and options for its games'
            }
          ]
        : []),
      ...(info?.theme.views && info.theme.views.length <= 1
        ? []
        : [{ id: 'views', label: 'Views…', description: 'Organise the library another way' }]),
      {
        id: 'interface',
        label: 'Interface…',
        description: info?.theme.settings.length
          ? `Theme, where Forma opens, screen margins, ${info.theme.name}’s own settings`
          : 'Theme, where Forma opens, screen margins'
      },
      { id: 'music', label: 'Music…', description: 'Folders, playlists and web radios' },
      {
        id: 'ra',
        label: 'RetroAchievements…',
        description: 'Your account and how achievements work'
      },
      { id: 'netplay', label: 'Netplay…', description: 'Playing online with others' },
      { id: 'network', label: 'Network…', description: 'Wi-Fi and Ethernet' },
      {
        id: 'store',
        label: 'Content downloader…',
        description: 'Free games, ports, decorations and music from Batocera'
      },
      {
        id: 'game-settings',
        label: 'Game settings…',
        description: 'Emulator options for every game: ratio, shaders, decorations…'
      },
      {
        id: 'system-settings',
        label: 'System settings…',
        description: 'Display, audio, language, region and storage'
      },
      {
        id: 'emulationstation',
        label: 'Switch to EmulationStation',
        description: 'For scraping and updates; Forma returns at the next boot'
      },
      { id: 'restart', label: 'Restart', description: 'Press A twice' },
      { id: 'shutdown', label: 'Shut down', description: 'Press A twice' }
    ];
    this.#menu.show(
      info ? `Forma ${info.formaVersion}`.trim() : 'Forma',
      items,
      info ? `${info.theme.name} ${info.theme.version}`.trim() : undefined
    );
    this.#changed();
  }

  async #openViews(): Promise<void> {
    try {
      const [views, home, info] = await Promise.all([
        this.forma.nav.views(),
        this.forma.nav.defaultView(),
        this.#loadInfo()
      ]);
      const offered = themeViews(views, info);
      if (offered.length <= 1) {
        // The theme shows the library one way only: nothing to pick.
        return this.notify(`${info?.theme.name ?? 'This theme'} shows the library one way`);
      }
      this.#views = offered;
      this.#home = home.id;
      this.#switcher.show(offered, info?.context?.view ?? null, home.id);
      this.#changed();
    } catch (error) {
      this.notify(FormaError.from(error).message);
    }
  }

  #viewsInput(e: InputEvent): void {
    const s = this.#switcher;
    if (e.action === 'up' || e.action === 'down') {
      return s.select(moveList(s.selected, s.count, e.action, 5, e.repeat));
    }
    if (e.repeat) return;
    const view = s.view;
    if (e.action === 'confirm' && view) {
      s.close();
      this.#changed();
      this.calls.openView(view.id).catch((error: unknown) => {
        this.notify(FormaError.from(error).message);
      });
    } else if (e.action === 'north' && view) {
      void this.forma.nav.setDefaultView(view.id).then(() => {
        this.#home = view.id;
        this.notify(`Forma will open on “${view.name}”`);
        s.show(this.#views, this.#info?.context?.view ?? null, this.#home);
        s.select(this.#views.indexOf(view));
      });
    } else if (e.action === 'back' || e.action === 'select') {
      s.close();
      this.#changed();
    }
  }

  #menuInput(e: InputEvent): void {
    const m = this.#menu;
    if (e.action === 'up' || e.action === 'down') {
      this.#confirming = null;
      return m.select(moveList(m.selected, m.count, e.action, 5, e.repeat));
    }
    if (e.repeat) return;
    if (e.action === 'back') {
      m.close();
      return this.#changed();
    }
    if (e.action !== 'confirm' || !m.item) return;
    const id = m.item.id;
    const context = this.#info?.context ?? {};
    const page = this.#page;
    const show = (title: string, build: Parameters<Page['show']>[1]) => {
      m.close();
      this.#pageKind = id;
      void page.show(title, build);
      this.#changed();
    };
    switch (id) {
      case 'views':
        m.close();
        return void this.#openViews();
      case 'network':
        m.close();
        this.#network.show();
        return this.#changed();
      case 'store':
        return show('Content downloader', storeScreen(this.forma, page));
      case 'system-settings':
        return show('System settings', systemSettingsScreen(this.forma, page));
      case 'game-settings':
        return show('Game settings', optionsScreen(this.forma, page, { level: 'global' }));
      case 'system-options':
        if (!context.system) return;
        return show(
          `${context.system.fullname || context.system.name} options`,
          optionsScreen(this.forma, page, { level: 'system', system: context.system.name })
        );
      case 'game-options':
        if (!context.game) return;
        return show(
          `${context.game.name} options`,
          optionsScreen(this.forma, page, { level: 'game', game: context.game.id })
        );
      case 'interface':
        return show(
          'Interface',
          interfaceScreen(this.forma, page, {
            theme: async () => {
              const info = await this.#loadInfo();
              return { settings: info?.theme.settings ?? [], views: info?.theme.views ?? null };
            },
            setThemeSetting: (key, value) => this.calls.setThemeSetting(key, value)
          })
        );
      case 'music':
        return show('Music', musicScreen(this.forma, page));
      case 'ra':
        return show('RetroAchievements', retroAchievementsScreen(this.forma, page));
      case 'netplay':
        return show('Netplay', netplayScreen(this.forma, page));
      default:
    }
    // Power actions need a second press soon after: a stray A must not shut down.
    const powerful = id === 'restart' || id === 'shutdown';
    const now = Date.now();
    if (powerful && (this.#confirming?.id !== id || now > this.#confirming.until)) {
      this.#confirming = { id, until: now + CONFIRM_MS };
      this.notify(`Press A again to ${id === 'restart' ? 'restart' : 'shut down'}`);
      return;
    }
    this.#confirming = null;
    const action =
      id === 'restart'
        ? this.forma.system.restart()
        : id === 'shutdown'
          ? this.forma.system.shutdown()
          : this.forma.system.quit();
    action.catch((error: unknown) => this.notify(FormaError.from(error).message));
  }

  /** After anything opens, closes or moves: the hints follow, and Forma learns if input is ours. */
  #changed(): void {
    const open = this.open;
    this.root.classList.toggle('open', open);
    if (open !== this.#wasOpen) {
      this.#wasOpen = open;
      this.calls.setOpen(open).catch(() => {});
    }
    const hints: [string, string][] = !open
      ? []
      : this.#page.open
        ? this.#page.hints()
        : this.#network.open
          ? this.#network.hints()
          : this.#switcher.open
            ? [
                ['A', 'Show'],
                ['Y', 'Make home view'],
                ['B', 'Close']
              ]
            : [
                ['A', 'Choose'],
                ['B', 'Close']
              ];
    this.#hints.hidden = hints.length === 0;
    this.#hints.replaceChildren(
      ...hints.map(([button, label]) => el('span', { class: 'hint' }, el('kbd', {}, button), label))
    );
  }
}

/** The views the theme shows: those it lists in its theme.json, else all of them. */
function themeViews<V extends { id: string }>(views: V[], info: ShellInfo | null): V[] {
  const only = info?.theme.views;
  return only ? views.filter((v) => only.includes(v.id)) : views;
}
