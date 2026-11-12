import type {
  Forma,
  InputEvent,
  MusicSong,
  RaSession,
  ShellContext,
  StatusBar,
  ViewInfo
} from '@ramraiders/forma-sdk';
import { FormaError } from '@ramraiders/forma-sdk';

import { moveList } from './logic/nav.js';
import { el } from './ui/dom.js';
import { Menu, type MenuItem } from './ui/menu.js';
import { NetworkScreen } from './ui/network.js';
import { Page } from './ui/page.js';
import {
  type ThemeOnSelect,
  type ThemeSettingValue,
  gamesScreen,
  interfaceScreen,
  musicScreen,
  netplayScreen,
  onSelect,
  retroAchievementsScreen,
  systemsScreen
} from './ui/screens.js';
import { presetsScreen } from './ui/presets.js';
import { magazinesScreen } from './ui/magazines.js';
import { duplicatesScreen, versionsScreen } from './ui/gamedata.js';
import { dumpsScreen } from './ui/dumps.js';
import { addonsScreen } from './ui/addons.js';
import { scraperScreen } from './ui/scraper.js';
import { Reader } from './ui/reader.js';
import { optionsScreen, systemSettingsScreen } from './ui/settings.js';
import { screenScreen } from './ui/shader.js';
import { StatusView } from './ui/status.js';
import { storeScreen } from './ui/store.js';
import { UnlocksView } from './ui/unlocks.js';
import { collectionsScreen } from './ui/collections.js';
import { controllersScreen } from './ui/controllers.js';
import { Switcher } from './ui/switcher.js';
import { Screensaver, screensaverScreen } from './ui/screensaver.js';
import { freezeTheme } from './ui/freeze.js';

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
    /** Where to load the theme's logo from, shown in place of its name. */
    logo?: string | null;
  };
  formaVersion: string;
}

/** The calls only Forma's shell may make, next to the SDK's. */
export interface ShellCalls {
  info(): Promise<ShellInfo>;
  setOpen(open: boolean): Promise<void>;
  openView(view: string): Promise<void>;
  /** Shows a game in the theme, as if browsed to. */
  openGame(id: number): Promise<void>;
  setThemeSetting(key: string, value: unknown): Promise<void>;
  /** The screensaver came on or went: input is the shell's meanwhile. */
  screensaver(active: boolean): Promise<void>;
}

/** How long a power action waits for its second press. */
const CONFIRM_MS = 3000;
/** Select held this long, on a game list, is "favourites only" on or off; shorter, its menu. */
export const HOLD_MS = 600;

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
  /** Controllers and the connection, in the top-right corner. */
  readonly #status = new StatusView();
  /** After a game: what it earned on RetroAchievements. */
  readonly #unlocks = new UnlocksView();
  /** Forma's magazine reader, over everything else. */
  readonly #reader: Reader;
  /** After the minutes the user chose with no press: over everything. */
  readonly #saver: Screensaver;
  #playing = false;
  /** The menu entry the open page, network screen or views list came from: B goes back to it. */
  #returnTo: number | null = null;
  #info: ShellInfo | null = null;
  #views: ViewInfo[] = [];
  #home: string | null = null;
  #confirming: { id: string; until: number } | null = null;
  #wasOpen = false;
  /** Which settings page is open, for the events that redraw it. */
  #pageKind: string | null = null;
  /** A game is running: the shell stays shut. */
  /** A game runs: nothing of Forma's shows over it. */
  get playing(): boolean {
    return this.#playing;
  }

  set playing(playing: boolean) {
    this.#playing = playing;
    this.#status.root.hidden = playing;
    // Back from a game: the wait starts afresh.
    if (!playing) this.#saver.poke();
  }

  /** The screensaver shows: every key and button is the shell's. */
  get saving(): boolean {
    return this.#saver.active;
  }

  /** A press the theme had, or a key: no screensaver yet. */
  poke(): void {
    if (!this.playing) this.#saver.poke();
  }

  constructor(
    private readonly forma: Forma,
    private readonly calls: ShellCalls
  ) {
    const notify = (message: string) => this.notify(message);
    const changed = () => this.#changed();
    this.#page = new Page(notify, changed);
    this.#network = new NetworkScreen(forma, notify, changed);
    this.#reader = new Reader(forma, notify);
    this.#saver = new Screensaver(forma, {
      setActive: async (active) => {
        // The corner's pads and connection would show through its videos.
        this.root.classList.toggle('saving', active);
        // Under it, the theme's page holds still (Dim shows it through).
        freezeTheme(active);
        await calls.screensaver(active);
        // Gone: input goes back to wherever it went before (a menu left open, or the theme).
        if (!active) await calls.setOpen(this.open);
      },
      blocked: () => this.playing || this.#reader.open || this.#typing(),
      system: async () => (await calls.info()).context?.system?.name ?? null
    });
    // A theme's shortcut asks for Forma's magazines, or one of them.
    forma.events.on('magazines:open', ({ id }) => void this.openMagazines(id));
    forma.events.on('manual:open', ({ gameId }) => void this.openManual(gameId));
    // What plays moves on by itself: the music page follows.
    forma.events.on('music:changed', () => {
      if (this.#page.open && this.#pageKind === 'music') void this.#page.refresh();
    });
    forma.events.on('music:song', (song) => void this.announce(song));
    forma.events.on('ra:unlocked', (session) => void this.showUnlocks(session));
    forma.events.on('game:start', () => {
      this.#unlocks.hide();
      // A game started from a page (netplay): the page goes with Forma's window.
      if (this.#page.open) {
        this.#page.close();
        this.#changed();
      }
    });
    forma.events.on('status:changed', (status) => void this.showStatus(status));
    forma.status
      .get()
      .then((status) => this.showStatus(status))
      .catch(() => {});
    // Clear of the edges a TV cuts off, as themes are, with the same setting.
    const margins = (value: unknown) =>
      this.root.style.setProperty(
        '--margin',
        `${typeof value === 'number' ? Math.min(10, Math.max(0, value)) : 0}vh`
      );
    forma.settings
      .getSystem<number>('display.margins')
      .then(margins)
      .catch(() => {});
    forma.events.on('settings:changed', ({ scope, key, value }) => {
      if (scope === 'system' && key === 'display.margins') margins(value);
    });
    this.root.append(
      this.#menu.root,
      this.#switcher.root,
      this.#network.root,
      this.#page.root,
      this.#hints,
      this.#status.root,
      this.#song,
      this.#unlocks.root,
      this.#reader.root,
      this.#saver.root,
      this.#toast
    );
  }

  /** The corner's controllers and connection, unless the theme draws them. */
  async showStatus(status: StatusBar): Promise<void> {
    const info = this.#info ?? (await this.#loadInfo());
    if (info?.theme.draws?.includes('status')) {
      this.#status.root.hidden = true;
      return;
    }
    this.#status.show(status);
  }

  /** A session's achievements, unless the theme draws them or another game already runs. */
  async showUnlocks(session: RaSession): Promise<void> {
    if (this.playing) return;
    const info = this.#info ?? (await this.#loadInfo());
    if (info?.theme.draws?.includes('achievements')) return;
    this.#unlocks.show(session);
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
    return (
      this.#menu.open ||
      this.#switcher.open ||
      this.#network.open ||
      this.#page.open ||
      this.#reader.open
    );
  }

  /** Forma's magazines page; with `id`, straight into that magazine (B then shows the list). */
  async openMagazines(id: string | null = null): Promise<void> {
    if (this.playing) return;
    this.#closeAll();
    this.#pageKind = 'magazines';
    await this.#page.show(
      'Magazines',
      magazinesScreen(this.forma, (m) => this.#read(m))
    );
    this.#changed();
    if (id) {
      const magazine = (await this.forma.magazines.list()).find((m) => m.id === id);
      if (magazine) await this.#read(magazine);
      else this.notify('That magazine is no longer there');
    }
  }

  /** The reader over Select's page, which is there again when it closes. */
  async #look(open: () => void | Promise<void>): Promise<void> {
    await open();
    this.#changed();
  }

  /** A theme asked for a game's manual (`library.openManual`): the reader, on its own. */
  async openManual(gameId: number): Promise<void> {
    if (this.playing) return;
    const game = await this.forma.library.game(gameId).catch(() => null);
    if (!game?.media.manual) return this.notify('This game has no manual');
    this.#closeAll();
    await this.#reader.showManual(game);
    this.#changed();
  }

  async #read(magazine: Parameters<Reader['show']>[0]): Promise<void> {
    await this.#reader.show(magazine);
    this.#changed();
  }

  input(e: InputEvent): void {
    if (this.playing) return;
    // Showing, it takes every press; otherwise any press puts it off.
    if (this.#saver.active) return void this.#saver.input(e);
    this.#saver.poke();
    if (!this.open) {
      if (!e.pressed && e.action === 'select' && this.#selectHold !== undefined) {
        clearTimeout(this.#selectHold);
        this.#selectHold = undefined;
        void this.#openContext();
        return;
      }
      if (!e.pressed || e.repeat) return;
      // Opened afresh: nothing to go back to.
      this.#returnTo = null;
      if (e.action === 'start') void this.#openMenu();
      else if (e.action === 'select') {
        // Its menu on release; held, the favourites filter instead.
        clearTimeout(this.#selectHold);
        this.#selectHold = setTimeout(() => {
          this.#selectHold = undefined;
          void this.#toggleFavourites();
        }, HOLD_MS);
      }
      return;
    }
    // Select shuts the page it opened, as it shuts the views list.
    if (
      e.pressed &&
      !e.repeat &&
      e.action === 'select' &&
      this.#page.open &&
      this.#pageKind === 'context'
    ) {
      this.#closeAll();
      return;
    }
    // Start shuts whatever is open, from anywhere, unless the keyboard is typing.
    if (e.pressed && !e.repeat && e.action === 'start' && !this.#typing()) {
      this.#returnTo = null;
      this.#closeAll();
      return;
    }
    if (this.#reader.open) {
      this.#reader.input(e);
      // Back on the list, which shows where the magazine was left.
      if (!this.#reader.open) void this.#page.refresh();
      return this.#changed();
    }
    if (this.#page.open) {
      this.#page.input(e);
      if (!this.#page.open) return this.#backToMenu(e);
      return this.#changed();
    }
    if (this.#network.open) {
      this.#network.input(e);
      if (!this.#network.open) return this.#backToMenu(e);
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
    if (this.#reader.open) this.#reader.close();
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

  /**
   * A page opened from the menu just closed: B (back) brings the menu back, on the entry it was
   * opened from, rather than shutting everything.
   */
  #backToMenu(e: InputEvent): void {
    const at = this.#returnTo;
    this.#returnTo = null;
    if (e.action === 'back' && at !== null) return void this.#openMenu(at);
    this.#changed();
  }

  async #openMenu(at = 0): Promise<void> {
    this.#confirming = null;
    const [info, active] = await Promise.all([
      this.#loadInfo(),
      this.forma.presets
        .list()
        .then((list) => list.find((p) => p.active) ?? null)
        .catch(() => null)
    ]);
    const context = info?.context ?? {};
    const { game, system } = context;
    const views = !(info?.theme.views && info.theme.views.length <= 1);
    const items: MenuItem[] = [
      ...(game
        ? [
            {
              id: 'game-options',
              label: 'Game options',
              description: `Emulator, core and options for ${game.name} only`,
              section: 'Selected'
            }
          ]
        : []),
      ...(system
        ? [
            {
              id: 'system-options',
              label: 'System options',
              description: `Emulator, core and options for every ${system.fullname || system.name} game`,
              section: 'Selected'
            }
          ]
        : []),
      {
        id: 'collections',
        label: 'Collections',
        description: game ? `Put ${game.name} in your own lists` : 'Your own lists of games',
        section: 'Library'
      },
      ...(views
        ? [
            {
              id: 'views',
              label: 'Views',
              description: 'Organise the library another way',
              section: 'Library'
            }
          ]
        : []),
      {
        id: 'store',
        label: 'Downloads',
        description: 'Free games, ports, decorations and music from Batocera',
        section: 'Library'
      },
      {
        id: 'magazines',
        label: 'Magazines',
        description: 'Read scanned magazines from /userdata/magazines',
        section: 'Library'
      },
      {
        id: 'games',
        label: 'Games',
        description: 'Scrapers and emulator options',
        section: 'Library'
      },
      {
        id: 'ra',
        label: 'Achievements',
        description: 'Your RetroAchievements account and how achievements work',
        section: 'Play'
      },
      {
        id: 'netplay',
        label: 'Netplay',
        description: game ? `Host ${game.name}, or join a room` : 'Play online with others',
        section: 'Play'
      },
      {
        id: 'presets',
        label: 'Presets',
        description: active
          ? `${active.name} in use · ready-made setups for your machine and screen`
          : 'Ready-made setups for your machine and screen: emulator options and CRT shader',
        section: 'Setup'
      },
      {
        id: 'interface',
        label: 'Interface',
        description: info?.theme.settings.length
          ? `Theme, where Forma opens, screen margins, ${info.theme.name}’s own settings`
          : 'Theme, where Forma opens, screen margins',
        section: 'Settings'
      },
      {
        id: 'screensaver',
        label: 'Screensaver',
        description: 'When it starts, and what it shows: dim, black, pictures or videos',
        section: 'Settings'
      },
      {
        id: 'music',
        label: 'Audio',
        description: 'Video sound, music folders, playlists and web radios',
        section: 'Settings'
      },
      {
        id: 'system-settings',
        label: 'System',
        description: 'Display, audio, language, region, storage and updates',
        section: 'Settings'
      },
      {
        id: 'addons',
        label: 'Add-ons',
        description: 'Extend Forma: more scraper sources and more, from the catalogue',
        section: 'Settings'
      },
      {
        id: 'controllers',
        label: 'Controllers',
        description: 'Pads in use, Bluetooth pairing',
        section: 'Devices'
      },
      {
        id: 'network',
        label: 'Network',
        description: 'Wi-Fi and Ethernet',
        section: 'Devices'
      },
      {
        id: 'emulationstation',
        label: 'Switch to ES',
        description: 'EmulationStation; Forma returns at the next boot',
        section: 'Power'
      },
      {
        id: 'restart',
        label: 'Restart',
        description: 'Restart the console. Press A twice',
        section: 'Power'
      },
      {
        id: 'shutdown',
        label: 'Shut down',
        description: 'Turn the console off. Press A twice',
        section: 'Power'
      }
    ];
    this.#menu.show(
      info ? `Forma ${info.formaVersion}`.trim() : 'Forma',
      items,
      info ? `${info.theme.name} ${info.theme.version}`.trim() : undefined,
      info?.theme.logo ? { src: info.theme.logo, name: info.theme.name } : undefined
    );
    this.#menu.select(Math.min(at, items.length - 1));
    this.#changed();
  }

  /**
   * Select: Forma's options for what the theme shows (its `list` in `setContext`). Showing
   * systems, their order; showing games, the theme's own settings for a game list, under its
   * logo (those it marks `"select": ["games"]`); elsewhere, the views list.
   */
  #selectHold: ReturnType<typeof setTimeout> | undefined;

  /**
   * Select held on a game list: shows only its favourites, or, already so, takes that away
   * again; the rest of the filter stays. Elsewhere, Select's menu as a press opens it.
   */
  async #toggleFavourites(): Promise<void> {
    const info = await this.#loadInfo();
    if (info?.context?.list !== 'games') return this.#openContext();
    const options = await this.forma.nav.filterOptions().catch(() => null);
    if (!options) return;
    const { filter } = options;
    const on = !filter.favorites;
    const left = await this.forma.nav.setFilter({ ...filter, favorites: on }).catch(() => null);
    if (left === null) return;
    if (on && left === 0) {
      await this.forma.nav.setFilter(filter).catch(() => {});
      return this.notify('No favourites in this list');
    }
    this.notify(on ? 'Favourites only' : 'Favourites or not');
  }

  async #openContext(): Promise<void> {
    const info = await this.#loadInfo();
    const list = info?.context?.list;
    const offers = onSelect(info?.theme.settings ?? [], list ?? '').length > 0;
    const game = info?.context?.game ?? null;
    // Categories hold systems: their order is what Select offers there too.
    const systems = list === 'systems' || list === 'categories';
    if (!systems && !(list === 'games' && (offers || game))) return this.#openViews();
    const views = themeViews(await this.forma.nav.views().catch(() => []), info);
    const openViews =
      views.length > 1
        ? () => {
            this.#page.close();
            void this.#openViews();
          }
        : undefined;
    const theme: ThemeOnSelect = {
      theme: async () => {
        const now = await this.#loadInfo();
        return now ? { name: now.theme.name, settings: now.theme.settings } : null;
      },
      setThemeSetting: (key, value) => this.calls.setThemeSetting(key, value)
    };
    // The game's file, as it is on disk, small under its name.
    const file =
      list === 'games' && game
        ? await this.forma.library
            .game(game.id)
            .then((g) => g?.path.split('/').pop() ?? undefined)
            .catch(() => undefined)
        : undefined;
    this.#pageKind = 'context';
    await (systems
      ? this.#page.show(
          info?.context?.system?.fullname || info?.context?.system?.name || 'Systems',
          systemsScreen(this.forma, this.#page, openViews, theme, info?.context?.system ?? null)
        )
      : this.#page.show(
          game?.name ?? 'Games',
          gamesScreen(this.forma, this.#page, theme, game, openViews, () => this.#closeAll(), {
            manual: (g) => void this.#look(() => this.#reader.showManual(g)),
            media: (g) =>
              void this.#look(() =>
                // Its video played over the theme's: the theme shows the game afresh after.
                this.#reader.showMedia(g, () => void this.calls.openGame(g.id).catch(() => {}))
              )
          }),
          undefined,
          file
        ));
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
      // Opened from the menu: B goes back to it; Select, like Start, just shuts.
      const at = this.#returnTo;
      this.#returnTo = null;
      if (e.action === 'back' && at !== null) return void this.#openMenu(at);
      this.#changed();
    }
  }

  #menuInput(e: InputEvent): void {
    const m = this.#menu;
    if (e.action === 'up' || e.action === 'down' || e.action === 'left' || e.action === 'right') {
      this.#confirming = null;
      return m.move(e.action, e.repeat);
    }
    if (e.repeat) return;
    if (e.action === 'back') {
      m.close();
      return this.#changed();
    }
    if (e.action !== 'confirm' || !m.item) return;
    const id = m.item.id;
    this.#returnTo = m.selected;
    const context = this.#info?.context ?? {};
    const page = this.#page;
    // A game chosen in a list of games: the menu shuts and the theme shows it.
    const openGame = (id: number) => {
      this.#closeAll();
      this.calls.openGame(id).catch((error: unknown) => {
        this.notify(FormaError.from(error).message);
      });
    };
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
      case 'controllers':
        return show('Controllers', controllersScreen(this.forma, page));
      case 'store':
        return show('Content downloader', storeScreen(this.forma, page));
      case 'system-settings':
        return show('System settings', systemSettingsScreen(this.forma, page));
      case 'presets':
        return show('Presets', presetsScreen(this.forma, page));
      case 'games':
        return show('Games', () => [
          {
            label: 'Scrapers',
            detail: 'Choose scrapers, add accounts, and scrape boxes, videos and details',
            run: () => page.push('Scrapers', scraperScreen(this.forma, page, context))
          },
          {
            label: 'Emulator options',
            detail: 'For every game: ratio, rewind, saves, controls…',
            run: () =>
              page.push('Emulator options', optionsScreen(this.forma, page, { level: 'global' }))
          },
          {
            label: 'ROM check',
            detail: 'Which files are the dumps No-Intro and Redump list',
            run: () => page.push('ROM check', dumpsScreen(this.forma, page))
          },
          {
            label: 'Game versions',
            detail: 'Which games have several versions, per system',
            run: () => page.push('Game versions', versionsScreen(this.forma, page, openGame))
          },
          {
            label: 'Sibling games',
            detail: 'One game under different names, by LaunchBox',
            run: () => page.push('Sibling games', duplicatesScreen(this.forma, page, openGame))
          },
          {
            label: 'Screen',
            detail: 'For every game: CRT shader, picture and decorations',
            run: () => page.push('Screen', screenScreen(this.forma, page, { level: 'global' }))
          }
        ]);
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
            setThemeSetting: (key, value) => this.calls.setThemeSetting(key, value),
            notify: (message) => this.notify(message)
          })
        );
      case 'music':
        return show('Audio', musicScreen(this.forma, page));
      case 'screensaver':
        return show(
          'Screensaver',
          screensaverScreen(this.forma, page, () => {
            this.#returnTo = null;
            this.#closeAll();
            void this.#saver.start();
          })
        );
      case 'ra':
        return show('RetroAchievements', retroAchievementsScreen(this.forma, page));
      case 'netplay':
        return show('Netplay', netplayScreen(this.forma, page, context.game ?? null));
      case 'magazines':
        m.close();
        return void this.openMagazines();
      case 'addons':
        return show(
          'Add-ons',
          addonsScreen(this.forma, page, (m) => this.notify(m))
        );
      case 'collections':
        return show(
          'Collections',
          collectionsScreen(this.forma, page, (m) => this.notify(m), context.game ?? null)
        );
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
