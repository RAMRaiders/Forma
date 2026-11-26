import { type Backend, inTauri, tauriBackend } from './backend.js';
import { FormaError } from './errors.js';
import { keyToInput } from './keyboard.js';
import type { MockOptions } from './mock/backend.js';
import type {
  Action,
  Browse,
  Controller,
  DefaultView,
  DumpChecks,
  GameManual,
  FilterOptions,
  ListFilter,
  EthernetConfig,
  EventMap,
  Fetched,
  Game,
  GameSaves,
  InputEvent,
  LaunchRequest,
  MediaChanged,
  MediaKind,
  MusicSource,
  MusicStatus,
  VideoArea,
  VideoPlay,
  NetplayRoom,
  RoomFilter,
  NetworkStatus,
  Page,
  Radio,
  RaGame,
  RaUser,
  ShellContext,
  StatusBar,
  SystemSummary,
  Magazine,
  SystemBios,
  AddonActionResult,
  AddonInfo,
  AddonPage,
  CatalogueAddon,
  ScrapeScope,
  ScraperOffers,
  NameStyle,
  ScraperSettings,
  ScraperStatus,
  ThemeCatalogue,
  ThemeInfo,
  Unsubscribe,
  VersionGroup,
  DuplicateGroup,
  ViewInfo,
  BatoceraSetting,
  OptionsPage,
  OptionsScope,
  ShaderPage,
  InfoItem,
  SystemSetting,
  StorePackage,
  WifiJoined,
  WifiNetwork,
  BluetoothDevice,
  Collection,
  BluetoothStatus,
  UpdateCheck,
  RaMatches
} from './types.js';
import { SDK_VERSION, satisfies } from './version.js';

export interface ConnectOptions {
  /** Defaults to Tauri inside Forma and the mock in a plain browser. */
  backend?: Backend;
  /** Options for the mock, when it is used. */
  mock?: MockOptions;
  /** Deliver arrow keys, Enter, Escape… as input events too. Default `true`. */
  keyboard?: boolean;
  /**
   * Hold Forma's cover (its logo, over the theme while it loads) until the theme calls
   * `forma.ready()`, e.g. once its first screen is drawn, rather than lifting it as soon as
   * `connect()` resolves. Default `false`.
   */
  deferReady?: boolean;
}

/**
 * Connects to Forma: checks the host speaks a compatible SDK version, then tells it the theme
 * has started (a theme that never does is replaced by the default one), unless `deferReady`
 * leaves that to `forma.ready()`.
 */
export async function connect(options: ConnectOptions = {}): Promise<Forma> {
  let backend = options.backend;
  if (!backend) {
    if (inTauri()) {
      backend = tauriBackend();
    } else {
      // Loaded only outside Forma, so the mock's data never ships inside the app's bundle path.
      const { mockBackend } = await import('./mock/backend.js');
      console.info('[forma] not running inside Forma: using the mock library');
      backend = mockBackend(options.mock);
    }
  }
  const host = await call<string>(backend, 'sdk_api_version');
  if (!satisfies(`^${SDK_VERSION}`, host)) {
    throw new FormaError(
      'incompatible',
      `this theme needs Forma SDK ^${SDK_VERSION}, but Forma provides ${host}`
    );
  }
  const sdk = new Forma(backend, host, options.keyboard ?? true);
  if (!options.deferReady) await sdk.ready();
  return sdk;
}

async function call<T>(backend: Backend, command: string, args?: Record<string, unknown>) {
  try {
    return await backend.invoke<T>(command, args);
  } catch (cause) {
    throw FormaError.from(cause);
  }
}

export class Forma {
  /** The SDK API version Forma provides. */
  readonly hostVersion: string;
  /** `tauri` inside Forma, `mock` in a browser. */
  readonly backendKind: Backend['kind'];

  readonly library;
  readonly nav;
  readonly launch;
  readonly settings;
  readonly themes;
  readonly input;
  readonly events;
  readonly system;
  readonly ra;
  readonly netplay;
  readonly collections;
  readonly magazines;
  readonly bios;
  readonly scraper;
  readonly addons;
  readonly network;
  readonly bluetooth;
  readonly updates;
  readonly batocera;
  readonly options;
  readonly shaders;
  readonly store;
  readonly music;
  readonly video;
  readonly shell;
  readonly status;

  readonly #backend: Backend;
  readonly #inputHandlers = new Set<(e: InputEvent) => void>();
  #detachInput: Unsubscribe | null = null;
  /** How often each game's media was replaced, so its URLs change and the webview reads it again. */
  readonly #mediaVersions = new Map<number, number>();

  constructor(backend: Backend, hostVersion: string, keyboard: boolean) {
    this.#backend = backend;
    backend.listen<MediaChanged>('media:changed', ({ game }) => {
      this.#mediaVersions.set(game, (this.#mediaVersions.get(game) ?? 0) + 1);
    });
    this.hostVersion = hostVersion;
    this.backendKind = backend.kind;
    const invoke = <T>(command: string, args?: Record<string, unknown>) =>
      call<T>(backend, command, args);

    this.library = {
      /** In the order the user chose (`SystemsOrder`): keep it, rather than sorting them. */
      systems: () => invoke<SystemSummary[]>('library_systems'),
      game: (id: number) => invoke<Game | null>('library_game', { id }),
      /**
       * What a game has saved: RetroArch's battery save and save states (a standalone emulator's
       * own are not counted). Since 1.35.0.
       */
      saves: (id: number) => invoke<GameSaves>('library_saves', { id }),
      /** A system's titles with more than one version. Since 1.26.0. */
      versions: (system: string) => invoke<VersionGroup[]>('library_versions', { system }),
      /**
       * A system's games that are one game under different names (`Rockman`, `Mega Man`), by
       * LaunchBox's data on this machine; `null` until it has some. Since 1.31.0.
       */
      duplicates: (system: string) =>
        invoke<DuplicateGroup[] | null>('library_duplicates', { system }),
      /** Deletes a game and its files for good. Only Forma's own menu can. Since 1.25.0. */
      deleteGame: (id: number) => invoke<void>('library_delete_game', { id }),
      search: (query: string, limit?: number) => invoke<Game[]>('library_search', { query, limit }),
      /**
       * Games in a random order, for a screensaver or a "surprise me": those with any of the
       * `media` kinds (any game without), only favourites, or one system's. At most 200.
       * Since 1.31.0.
       */
      random: (options?: {
        media?: MediaKind[];
        limit?: number;
        favorites?: boolean;
        system?: string | null;
      }) =>
        invoke<Game[]>('library_random', {
          media: options?.media ?? [],
          limit: options?.limit,
          favorites: options?.favorites ?? false,
          system: options?.system ?? null
        }),
      /**
       * How much of the library is checked against the No-Intro and Redump DATs, per system.
       * Forma hashes games in the background (never while one runs) and fetches the DATs of
       * the systems present from libretro-database. Since 1.32.0.
       */
      dumpChecks: () => invoke<DumpChecks>('library_dump_checks'),
      /**
       * A game's manual (its `manual` media: a PDF, an archive of pages or one picture) as
       * pages made for the screen; `null` without one. Since 1.33.0.
       */
      manual: (id: number) => invoke<GameManual | null>('library_manual', { gameId: id }),
      /** The URL of page `page` (from 0) of a manual, about `width` pixels wide. Since 1.33.0. */
      manualPageUrl: (manual: Pick<GameManual, 'pageUrl'>, page: number, width = 1280) =>
        `${manual.pageUrl.replace('{page}', String(page))}${
          manual.pageUrl.includes('?') ? '&' : '?'
        }w=${Math.round(width)}`,
      /**
       * Opens a game's manual in Forma's reader, over the theme: pages, zoom and closing are
       * Forma's. Since 1.33.0.
       */
      openManual: (id: number) => invoke<void>('library_open_manual', { gameId: id }),
      setFavorite: (id: number, favorite: boolean) =>
        invoke<void>('library_set_favorite', { id, favorite }),
      /**
       * A URL for one kind of a game's media, e.g. a group's `imageGame`. With `width`, images
       * come resized to about that many pixels: ask for what the layout shows, which is much
       * lighter than full-size art on a Raspberry Pi.
       */
      mediaUrl: (gameId: number, kind: MediaKind = 'image', width?: number) =>
        backend.mediaUrl(gameId, kind, width, this.#mediaVersions.get(gameId)),
      /**
       * Has Forma make resized pictures ahead of time, the nearest (first) first, while
       * nothing on screen waits: the games next to the one shown, at the sizes `mediaUrl` will
       * ask, so they show at once when reached. Nothing is downloaded; videos are ignored.
       */
      prepareMedia: (items: { gameId: number; kind: MediaKind; width: number }[]) =>
        invoke<void>('library_prepare_media', {
          items: items.map((i) => ({ ...i, width: Math.round(i.width) }))
        }),
      /**
       * A game list just opened (`view` at `path`, the selection at `around`): Forma makes the
       * `pictures` of up to 300 of its games ahead, nearest the selection first, when nothing
       * else waits and not while a game runs. The next call replaces this one.
       */
      prepareList: (
        view: string,
        path: string[],
        around: number,
        pictures: { kind: MediaKind; width: number }[]
      ) =>
        invoke<void>('library_prepare_list', {
          view,
          path,
          around: Math.max(0, Math.round(around)),
          pictures: pictures.map((p) => ({ ...p, width: Math.round(p.width) }))
        })
    };

    this.nav = {
      views: () => invoke<ViewInfo[]>('nav_views'),
      /** One page of a view at `path` (group keys or branch ids, from the root down). */
      browse: (view: string, path: string[] = [], page?: Page) =>
        invoke<Browse>('nav_browse', { view, path, page }),
      /** The filter on the game list on screen and its choices, or null. Since 1.27.0. */
      filterOptions: () => invoke<FilterOptions | null>('nav_filter_options'),
      /**
       * Filters the game list on screen, `null` clearing it. Returns the games left: at `0`
       * nothing changes. Themes need no code: `library:changed` follows and they browse again.
       * Since 1.27.0.
       */
      setFilter: (filter: ListFilter | null) => invoke<number>('nav_set_filter', { filter }),
      /** The view Forma opens with: the user's choice, else the theme's, else `systems`. */
      defaultView: () => invoke<DefaultView>('nav_default_view'),
      /** Makes `id` the boot view; `null` clears the choice. */
      setDefaultView: (id: string | null) => invoke<void>('nav_set_default_view', { id })
    };

    this.launch = {
      /** Starts a game and resolves at once; `game:start` and `game:end` follow. */
      game: (id: number, request?: LaunchRequest) => invoke<void>('launch_game', { id, request })
    };

    this.settings = {
      /** The theme's own settings, private to it. */
      get: <T = unknown>(key: string) => invoke<T | null>('settings_get', { key }),
      set: (key: string, value: unknown) => invoke<void>('settings_set', { key, value }),
      /** Forma's settings. Needs the `settings.system` permission. */
      getSystem: <T = unknown>(key: string) =>
        invoke<T | null>('settings_get', { key, scope: 'system' }),
      setSystem: (key: string, value: unknown) =>
        invoke<void>('settings_set', { key, value, scope: 'system' })
    };

    this.themes = {
      /** The themes the user can pick: not Forma's plain fallback, nor a copy another stands in for. */
      list: () => invoke<ThemeInfo[]>('themes_list'),
      select: (id: string) => invoke<void>('themes_select', { id }),
      /**
       * Deletes a theme the user installed (see `ThemeInfo.removable`): not one that comes with
       * Forma, nor the one in use. Needs the `themes.manage` permission.
       */
      remove: (id: string) => invoke<void>('themes_remove', { id }),
      /**
       * The theme catalogue: themes to install, and updates to installed ones. Fetched at most
       * every few hours; offline, the last one, `stale`.
       */
      catalogue: () => invoke<Fetched<ThemeCatalogue>>('themes_catalogue'),
      /**
       * Installs a theme from the catalogue, or updates it, once its download matches the
       * catalogue's SHA-256 and it can load. The theme in use reloads into its new version.
       * Needs the `themes.manage` permission.
       */
      install: (id: string) => invoke<void>('themes_install', { id })
    };

    this.input = {
      /** Every input event: gamepads and, unless disabled, the keyboard. */
      on: (handler: (e: InputEvent) => void): Unsubscribe => {
        this.#inputHandlers.add(handler);
        this.#attachInput(keyboard);
        return () => {
          this.#inputHandlers.delete(handler);
          if (this.#inputHandlers.size === 0) this.#detach();
        };
      },
      /** Presses (and repeats) of one action. */
      onPress: (action: Action, handler: (e: InputEvent) => void): Unsubscribe =>
        this.input.on((e) => {
          if (e.pressed && e.action === action) handler(e);
        }),
      controllers: () => invoke<Controller[]>('input_controllers')
    };

    this.events = {
      on: <K extends Exclude<keyof EventMap, 'input'>>(
        event: K,
        handler: (payload: EventMap[K]) => void
      ): Unsubscribe => backend.listen<EventMap[K]>(event, handler)
    };

    this.ra = {
      /** The user's RetroAchievements profile. Read-only: unlocking happens in RetroArch. */
      user: () => invoke<Fetched<RaUser>>('ra_user'),
      /** A local game's achievements and the user's progress in it. */
      game: (id: number) => invoke<Fetched<RaGame>>('ra_game', { id }),
      /**
       * How much of the library is matched to RetroAchievements, per system. Forma hashes games
       * in the background as rcheevos does (never while a game runs) and matches them once a
       * web API key is set; until then, `matched` counts only imported ids.
       */
      matches: () => invoke<RaMatches>('ra_matches'),
      /**
       * Logs in to RetroAchievements and saves the account (and the token the emulators use) in
       * Batocera's settings, as EmulationStation does. Resolves with the user name; rejects with
       * `invalid` when RetroAchievements refuses. Needs `settings.batocera`.
       */
      login: (username: string, password: string) =>
        invoke<string>('ra_login', { username, password }),
      /** Forgets the account and turns achievements off. Needs `settings.batocera`. */
      logout: () => invoke<void>('ra_logout')
    };

    /**
     * Scanned magazines, read in Forma's own reader: a theme lists them (with covers from
     * `pageUrl(m, 0, …)`) and calls `open`; the reader, its controls and where the user left
     * off are Forma's. No permission needed.
     */
    /** BIOS files, as Batocera lists them, checked by Forma. Since 1.39.0. */
    this.bios = {
      /** Every system with games that lists BIOS files, and how its files are. */
      status: () => invoke<SystemBios[]>('bios_status')
    };

    this.magazines = {
      list: () => invoke<Magazine[]>('magazines_list'),
      /** How many pages it has; the first time, a big archive takes a moment. */
      pages: (id: string) => invoke<number>('magazines_pages', { id }),
      /** Remembers the page the user is on (from 0). */
      setPage: (id: string, page: number) => invoke<void>('magazines_set_page', { id, page }),
      /** Opens Forma's reader on a magazine, or Forma's magazines list (no `id`). */
      open: (id?: string) => invoke<void>('magazines_open', { id: id ?? null }),
      /**
       * A page (from 0) as an image about `width` pixels wide: ask for what is drawn, which on
       * a Raspberry Pi is much lighter than the full scan.
       */
      pageUrl: (magazine: Pick<Magazine, 'pageUrl'>, page: number, width = 1280) =>
        `${magazine.pageUrl.replace('{page}', String(page))}${
          magazine.pageUrl.includes('?') ? '&' : '?'
        }w=${Math.round(width)}`
    };

    /**
     * Forma add-ons: what is installed, turning them on and off, removing them, and the
     * catalogue's. Forma's menu manages them; a theme needs `addons.manage`.
     */
    this.addons = {
      list: () => invoke<AddonInfo[]>('addons_list'),
      setEnabled: (id: string, enabled: boolean) =>
        invoke<AddonInfo[]>('addons_set_enabled', { id, enabled }),
      remove: (id: string) => invoke<void>('addons_remove', { id }),
      /** Fetched at most every few hours; offline, the last one, `stale`. */
      catalogue: () => invoke<Fetched<{ addons: CatalogueAddon[] }>>('addons_catalogue'),
      /**
       * Installs or updates one from the catalogue. An add-on with a system part installs only
       * from Forma's own catalogue, and its install script runs then.
       */
      install: (id: string) => invoke<AddonInfo>('addons_install', { id }),
      /** Sets one of its settings; `null` clears it. */
      setSetting: (id: string, key: string, value: string | null) =>
        invoke<AddonInfo[]>('addons_set_setting', { id, key, value }),
      /** One of a `menu` add-on's pages; `[]` is its first. */
      page: (id: string, path: string[] = []) => invoke<AddonPage>('addons_page', { id, path }),
      /** Does what a row of its page asks: `input` is the text typed, or `-1`/`1` to adjust. */
      action: (id: string, action: string, input?: string | number) =>
        invoke<AddonActionResult>('addons_action', { id, action, input: input ?? null })
    };

    /**
     * The scraper: details and media for games, from every source the user turns on, written
     * where EmulationStation writes them (`images/`, `videos/`, `manuals/`, `gamelist.xml`).
     * Forma's menu runs it; a theme with `scraper.manage` can too.
     */
    this.scraper = {
      settings: () => invoke<ScraperSettings>('scraper_settings'),
      /**
       * Changes what is given and keeps the rest. `sources` is the whole list, in order. What
       * to fetch is the theme's, not set here (since 1.36.0).
       */
      configure: (changes: {
        sources?: { id: string; enabled: boolean }[];
        replace?: boolean;
        skipScraped?: boolean;
        ownRegion?: boolean;
        regions?: string[];
        /** Since 1.37.0. */
        manuals?: boolean;
        /** Since 1.44.0. Every game's name is shown and sorted anew; `library:changed` follows. */
        nameStyle?: NameStyle;
        /** Since 1.44.0. */
        namePrefixes?: string[];
      }) => invoke<ScraperSettings>('scraper_configure', changes),
      /** Sets one of a source's credentials; `null` clears it. */
      setCredential: (source: string, key: string, value: string | null) =>
        invoke<ScraperSettings>('scraper_set_credential', { source, key, value }),
      /** Starts a run in the background; `scraper:progress` follows it. */
      start: (scope: ScrapeScope) => invoke<ScraperStatus>('scraper_start', { scope }),
      status: () => invoke<ScraperStatus>('scraper_status'),
      /** Stops after the game it is on. */
      stop: () => invoke<void>('scraper_stop'),
      /** Asks every source that is on about a game, for the user to pick from. Since 1.25.0. */
      offers: (id: number) => invoke<ScraperOffers>('scraper_offers', { id }),
      /** A small picture of one of the last offers, or null. Since 1.25.0. */
      preview: (offer: number) => invoke<string | null>('scraper_preview', { offer }),
      /**
       * Saves what was picked from the last offers: text by field, media by offer. Returns what
       * could not be saved. Since 1.25.0.
       */
      apply: (id: number, text: Record<string, string>, media: number[]) =>
        invoke<string[]>('scraper_apply', { id, text, media }),
      /** Changes a game's details by hand; `''` clears one. Since 1.25.0. */
      edit: (id: number, fields: Record<string, string>) =>
        invoke<void>('scraper_edit', { id, fields }),
      /**
       * Takes these kinds of media off a game and deletes their files (not one another game
       * shows too). Returns what could not be deleted. Since 1.30.0.
       */
      removeMedia: (id: number, kinds: MediaKind[]) =>
        invoke<string[]>('scraper_remove_media', { id, kinds })
    };

    /**
     * The user's own lists of games, as EmulationStation keeps them; browse them in the
     * `collections` view. Like favourites, they need no permission. Names are any case.
     */
    this.collections = {
      list: () => invoke<Collection[]>('collections_list'),
      /** The collections a game is in. */
      of: (gameId: number) => invoke<string[]>('collections_of', { gameId }),
      /** Resolves with the name as kept (trimmed). */
      create: (name: string) => invoke<string>('collections_create', { name }),
      /** The collection goes; its games stay in the library. */
      delete: (name: string) => invoke<void>('collections_delete', { name }),
      add: (name: string, gameId: number) =>
        invoke<void>('collections_set', { name, gameId, member: true }),
      remove: (name: string, gameId: number) =>
        invoke<void>('collections_set', { name, gameId, member: false })
    };

    this.netplay = {
      /** Open rooms, joinable first, each matched to a local game and core where possible. */
      lobby: (filter?: RoomFilter) => invoke<Fetched<NetplayRoom[]>>('netplay_lobby', { filter }),
      /** RetroArch hosts on the local network; takes a second or two to listen. */
      lan: (filter?: RoomFilter) => invoke<NetplayRoom[]>('netplay_lan', { filter }),
      /**
       * Joins a room: starts the local game with the room's core, connected to the host or its
       * relay. Fails without a local game, an installed core, or a way in.
       */
      join: (room: NetplayRoom, options: { password?: string; spectate?: boolean } = {}) => {
        if (room.localGameId === null) {
          return Promise.reject(new FormaError('notFound', `${room.gameName} is not on this box`));
        }
        if (room.core === null) {
          return Promise.reject(
            new FormaError('unsupported', `the ${room.coreName} core is not installed`)
          );
        }
        if (room.join === null) {
          return Promise.reject(new FormaError('unsupported', 'this room gives no way in'));
        }
        return invoke<void>('launch_game', {
          id: room.localGameId,
          request: {
            emulator: 'libretro',
            core: room.core,
            netplay: {
              mode: 'client',
              host: room.join.host,
              port: room.join.port,
              ...(room.join.session ? { session: room.join.session } : {}),
              ...(options.password ? { password: options.password } : {}),
              ...(options.spectate ? { spectate: true } : {})
            }
          } satisfies LaunchRequest
        });
      },
      /** Hosts a game: others join it from the lobby or the local network. */
      host: (id: number, options: { password?: string } = {}) =>
        invoke<void>('launch_game', {
          id,
          request: {
            netplay: {
              mode: 'host',
              ...(options.password ? { password: options.password } : {})
            }
          } satisfies LaunchRequest
        })
    };

    /** The box's Wi-Fi and Ethernet. Needs the `system.network` permission; Batocera only. */
    this.network = {
      status: () => invoke<NetworkStatus>('network_status'),
      /** Networks in range, the one in use and the saved one first. Takes a few seconds. */
      wifiScan: () => invoke<WifiNetwork[]>('network_wifi_scan'),
      /**
       * Saves `ssid` as the Wi-Fi network and waits, up to 30 seconds, for it to connect. Fails
       * with `invalid` for a bad password format and `network` when the network refuses it.
       */
      joinWifi: (ssid: string, passphrase?: string) =>
        invoke<WifiJoined>('network_wifi_join', { ssid, passphrase }),
      setWifiEnabled: (enabled: boolean) => invoke<void>('network_wifi_set_enabled', { enabled }),
      /** DHCP or a static address, kept across reboots. The connection drops for a moment. */
      configureEthernet: (config: EthernetConfig) =>
        invoke<void>('network_ethernet_configure', { config })
    };

    /**
     * Bluetooth controllers, through Batocera's own tools, as EmulationStation's Controllers
     * menu offers them. Needs the `system.controllers` permission; Batocera only.
     */
    this.bluetooth = {
      /** Whether Bluetooth is on, and the devices paired with this box. */
      status: () => invoke<BluetoothStatus>('bluetooth_status'),
      /** Turns Bluetooth on or off; Batocera restarts its Bluetooth services. */
      setEnabled: (enabled: boolean) => invoke<void>('bluetooth_set_enabled', { enabled }),
      /**
       * Looks for pads in pairing mode for up to a minute, pairing each it finds; resolves with
       * those it paired. `bluetooth:pairing` reports on the way.
       */
      pair: () => invoke<BluetoothDevice[]>('bluetooth_pair'),
      connect: (address: string) => invoke<void>('bluetooth_connect', { address }),
      disconnect: (address: string) => invoke<void>('bluetooth_disconnect', { address }),
      /** Unpairs a device; it has to be paired again to be used. */
      forget: (address: string) => invoke<void>('bluetooth_forget', { address })
    };

    /**
     * Batocera's own updates, as EmulationStation's Updates menu has them. Needs the
     * `system.update` permission; Batocera only. Nothing updates unless `install` is called.
     */
    this.updates = {
      /** The box's version and the newest for its board and channel. */
      check: () => invoke<UpdateCheck>('updates_check'),
      /**
       * Downloads (gigabytes) and unpacks the update; it takes effect after a reboot.
       * `updates:progress` reports on the way.
       */
      install: () => invoke<void>('updates_install')
    };

    /**
     * Batocera's RetroAchievements and netplay settings, as EmulationStation's menus set them.
     * Needs the `settings.batocera` permission; Batocera only.
     */
    this.batocera = {
      /** Settings whose key starts with `prefix`, e.g. `global.netplay`; `''` for all. */
      settings: (prefix = '') => invoke<BatoceraSetting[]>('batocera_settings', { prefix }),
      /** Sets one; an empty string clears it. Secrets are set by `ra.login`, not here. */
      set: (key: string, value: string) => invoke<void>('batocera_set', { key, value })
    };

    /**
     * Batocera's content downloader: free games, ports, decorations and music, as
     * EmulationStation offers them. Needs the `store.manage` permission; Batocera only. One
     * operation at a time: another is refused as `unsupported` while one runs.
     */
    this.store = {
      list: () => invoke<StorePackage[]>('store_list'),
      /** Reads the catalogue again from Batocera's server. */
      refresh: () => invoke<void>('store_refresh'),
      /** Resolves once installed; `store:progress` reports on the way. */
      install: (name: string) => invoke<void>('store_install', { name }),
      remove: (name: string) => invoke<void>('store_remove', { name }),
      /** Updates every installed package that has a newer version. */
      update: () => invoke<void>('store_update')
    };

    /**
     * Batocera's emulator, core and game options, as EmulationStation's game settings menus
     * offer them: global, per system, per game. Used by the next launch. Needs the
     * `settings.emulators` permission; Batocera only.
     */
    this.options = {
      /** The options at a level, with what is set there and what it inherits. */
      get: (scope: OptionsScope) => invoke<OptionsPage>('options_get', { scope }),
      /** Sets one, checked against its choices or range; `null` unsets it (it inherits again). */
      set: (scope: OptionsScope, key: string, value: string | null) =>
        invoke<void>('options_set', { scope, key, value })
    };

    /**
     * Forma's CRT shader for games: koko-aio (Batocera ships it), on for every game or off, and
     * tuned per level as the emulator options are (global, system, game). Each level starts
     * from a preset (the level above's unless it picks its own) and changes some of its
     * settings. Used by the next launch. Needs the `settings.emulators` permission; Batocera
     * only. Since 1.34.0.
     */
    this.shaders = {
      /** A level's shader and its settings, with what it sets and what it inherits. */
      get: (scope: OptionsScope) => invoke<ShaderPage>('shaders_get', { scope }),
      /** Sets a setting, checked against its range; `null` unsets it (it inherits again). */
      set: (scope: OptionsScope, key: string, value: number | null) =>
        invoke<void>('shaders_set', { scope, key, value }),
      /**
       * The shader at a level, as one choice from `ShaderPage.shader.choices`: `none`,
       * `crt:<preset>` (Forma's koko-aio from that preset, tuned by its settings) or
       * `set:<name>` (one of Batocera's shader sets); `null` for the level above's (not
       * globally). Forma owns Batocera's `shaderset` keys. Since 1.35.0.
       */
      choose: (scope: OptionsScope, choice: string | null) =>
        invoke<void>('shaders_choose', { scope, choice })
    };

    /**
     * Forma's music player: the music folder's folders and playlists, and web radios. Forma plays
     * it (pausing for games); a theme shows it and sends commands. `music:changed` follows every
     * change, the player moving on to the next track included.
     */
    this.music = {
      sources: () => invoke<MusicSource[]>('music_sources'),
      status: () => invoke<MusicStatus>('music_status'),
      /** Plays a source, remembered for the next boot. */
      play: (source: string, options?: { shuffle?: boolean }) =>
        invoke<MusicStatus>('music_play', { source, options }),
      /** Pauses, or resumes. */
      toggle: () => invoke<MusicStatus>('music_toggle'),
      next: () => invoke<MusicStatus>('music_next'),
      previous: () => invoke<MusicStatus>('music_previous'),
      stop: () => invoke<MusicStatus>('music_stop'),
      /** 0–100. */
      setVolume: (volume: number) => invoke<MusicStatus>('music_set_volume', { volume }),
      radios: () => invoke<Radio[]>('music_radios'),
      /** Adds a web radio to `radios.m3u`. Needs `music.manage`. */
      addRadio: (name: string, url: string) => invoke<void>('music_add_radio', { name, url }),
      /** Needs `music.manage`. */
      removeRadio: (url: string) => invoke<void>('music_remove_radio', { url })
    };

    /**
     * Games' videos. Forma plays them itself, full screen under the theme's page (a page's own
     * `<video>` breaks up on a Raspberry Pi), heard or not as the user chose, the music lowered
     * meanwhile.
     */
    this.video = {
      /**
       * Plays a game's video on a loop, replacing any other, and resolves once it shows. Then
       * leave the page see-through where it should be seen. It stops by itself when a game
       * starts or the page reloads. Full screen, or in `area` of the screen (fractions of it):
       * the picture fills the area, cut to its shape. `muted` (since 1.31.0): silent this time,
       * whatever the user's video sound setting says.
       */
      play: (gameId: number, options?: { area?: VideoArea; muted?: boolean }) =>
        invoke<VideoPlay>('video_play', {
          gameId,
          area: options?.area ?? null,
          muted: options?.muted ?? false
        }),
      /** Stops it, as soon as it should no longer be seen or heard. */
      stop: () => invoke<void>('video_stop')
    };

    /**
     * Forma's own menus, drawn over every theme: Start (the menu) and Select (views). Those two
     * buttons are Forma's and never reach the theme. A theme tells the menu what is on screen,
     * and follows `nav:open` and `settings:changed`.
     */
    this.shell = {
      /** What the theme shows now. Call it as the selection moves; it costs one small call. */
      setContext: (context: ShellContext) => invoke<void>('shell_set_context', { context })
    };

    /** Controllers and the connection, as Forma shows them in a corner; follows `status:changed`. */
    this.status = {
      get: () => invoke<StatusBar>('status_get')
    };

    this.system = {
      /** Needs the `system.power` permission. */
      quit: () => invoke<void>('system_quit'),
      restart: () => invoke<void>('system_power', { action: 'restart' }),
      shutdown: () => invoke<void>('system_power', { action: 'shutdown' }),
      /** What the box is: Batocera's version, model, CPU, memory, storage, address. */
      info: () => invoke<InfoItem[]>('system_info'),
      /**
       * Batocera's system settings, as EmulationStation's System settings menu offers them.
       * These and `info` need the `system.settings` permission; Batocera only.
       */
      settings: () => invoke<SystemSetting[]>('system_settings'),
      /** Sets one, checked against its choices; `null` unsets it. Some wait for a reboot. */
      set: (key: string, value: string | null) =>
        invoke<void>('system_settings_set', { key, value })
    };
  }

  /**
   * Tells Forma the theme has started: its cover lifts, and the theme is not replaced. `connect()`
   * does it, unless asked to wait (`deferReady`). Calling it again does no harm.
   */
  ready(): Promise<void> {
    return call<void>(this.#backend, 'theme_ready');
  }

  #attachInput(keyboard: boolean): void {
    if (this.#detachInput) return;
    const dispatch = (e: InputEvent) => {
      for (const handler of this.#inputHandlers) handler(e);
    };
    const stopGamepad = this.#backend.listen<InputEvent>('input', dispatch);
    let stopKeyboard: Unsubscribe = () => {};
    if (keyboard && typeof window !== 'undefined') {
      const onKey = (pressed: boolean) => (e: KeyboardEvent) => {
        const input = keyToInput(e.key, pressed, e.repeat);
        if (!input) return;
        e.preventDefault();
        dispatch(input);
      };
      const down = onKey(true);
      const up = onKey(false);
      window.addEventListener('keydown', down);
      window.addEventListener('keyup', up);
      stopKeyboard = () => {
        window.removeEventListener('keydown', down);
        window.removeEventListener('keyup', up);
      };
    }
    this.#detachInput = () => {
      stopGamepad();
      stopKeyboard();
    };
  }

  #detach(): void {
    this.#detachInput?.();
    this.#detachInput = null;
  }
}
