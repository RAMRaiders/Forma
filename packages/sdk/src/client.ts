import { type Backend, inTauri, tauriBackend } from './backend.js';
import { FormaError } from './errors.js';
import { keyToInput } from './keyboard.js';
import type { MockOptions } from './mock/backend.js';
import type {
  Action,
  Browse,
  Controller,
  DefaultView,
  EthernetConfig,
  EventMap,
  Fetched,
  Game,
  InputEvent,
  LaunchRequest,
  MediaKind,
  MusicSource,
  MusicStatus,
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
  ThemeInfo,
  Unsubscribe,
  ViewInfo,
  BatoceraSetting,
  OptionsPage,
  OptionsScope,
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
  readonly network;
  readonly bluetooth;
  readonly updates;
  readonly batocera;
  readonly options;
  readonly store;
  readonly music;
  readonly shell;
  readonly status;

  readonly #backend: Backend;
  readonly #inputHandlers = new Set<(e: InputEvent) => void>();
  #detachInput: Unsubscribe | null = null;

  constructor(backend: Backend, hostVersion: string, keyboard: boolean) {
    this.#backend = backend;
    this.hostVersion = hostVersion;
    this.backendKind = backend.kind;
    const invoke = <T>(command: string, args?: Record<string, unknown>) =>
      call<T>(backend, command, args);

    this.library = {
      systems: () => invoke<SystemSummary[]>('library_systems'),
      game: (id: number) => invoke<Game | null>('library_game', { id }),
      search: (query: string, limit?: number) => invoke<Game[]>('library_search', { query, limit }),
      setFavorite: (id: number, favorite: boolean) =>
        invoke<void>('library_set_favorite', { id, favorite }),
      /**
       * A URL for one kind of a game's media, e.g. a group's `imageGame`. With `width`, images
       * come resized to about that many pixels: ask for what the layout shows, which is much
       * lighter than full-size art on a Raspberry Pi.
       */
      mediaUrl: (gameId: number, kind: MediaKind = 'image', width?: number) =>
        backend.mediaUrl(gameId, kind, width)
    };

    this.nav = {
      views: () => invoke<ViewInfo[]>('nav_views'),
      /** One page of a view at `path` (group keys or branch ids, from the root down). */
      browse: (view: string, path: string[] = [], page?: Page) =>
        invoke<Browse>('nav_browse', { view, path, page }),
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
      list: () => invoke<ThemeInfo[]>('themes_list'),
      select: (id: string) => invoke<void>('themes_select', { id })
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
