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
  NetplayRoom,
  NetworkStatus,
  Page,
  RaGame,
  RaUser,
  SystemSummary,
  ThemeInfo,
  Unsubscribe,
  ViewInfo,
  WifiJoined,
  WifiNetwork
} from './types.js';
import { SDK_VERSION, satisfies } from './version.js';

export interface ConnectOptions {
  /** Defaults to Tauri inside Forma and the mock in a plain browser. */
  backend?: Backend;
  /** Options for the mock, when it is used. */
  mock?: MockOptions;
  /** Deliver arrow keys, Enter, Escape… as input events too. Default `true`. */
  keyboard?: boolean;
}

/**
 * Connects to Forma: checks the host speaks a compatible SDK version, then tells it the theme
 * has started (a theme that never does is replaced by the default one).
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
  await call(backend, 'theme_ready');
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
  readonly network;

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
      game: (id: number) => invoke<Fetched<RaGame>>('ra_game', { id })
    };

    this.netplay = {
      /** Open rooms, joinable first, each matched to a local game where possible. */
      lobby: () => invoke<Fetched<NetplayRoom[]>>('netplay_lobby')
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

    this.system = {
      /** Needs the `system.power` permission. */
      quit: () => invoke<void>('system_quit'),
      restart: () => invoke<void>('system_power', { action: 'restart' }),
      shutdown: () => invoke<void>('system_power', { action: 'shutdown' })
    };
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
