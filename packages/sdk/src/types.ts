/**
 * The shapes Forma sends and accepts. They mirror the Rust types' JSON exactly; the contract
 * test checks these field lists against the real engine's output.
 */

export interface SystemSummary {
  name: string;
  fullname: string;
  manufacturer: string | null;
  releaseYear: number | null;
  hardware: string | null;
  /** Visible games: present on disk and not hidden. */
  gameCount: number;
}

export type MediaKind = 'image' | 'thumbnail' | 'marquee' | 'video' | 'fanart' | 'manual';

export interface Game {
  id: number;
  system: string;
  /** Relative to the system's ROM folder. */
  path: string;
  name: string;
  description: string | null;
  developer: string | null;
  publisher: string | null;
  genre: string | null;
  players: string | null;
  /** `YYYY-MM-DD`. */
  releaseDate: string | null;
  /** 0.0–1.0. */
  rating: number | null;
  favorite: boolean;
  hidden: boolean;
  playCount: number;
  /** ISO 8601, UTC. */
  lastPlayed: string | null;
  /** Total seconds played. */
  gameTime: number;
  raGameId: number | null;
  /** Media kind → URL to load it from. */
  media: Partial<Record<MediaKind, string>>;
}

export interface ViewInfo {
  id: string;
  name: string;
  description: string | null;
  origin: 'bundled' | 'user';
}

export interface Page {
  offset: number;
  limit: number;
}

export interface Crumb {
  key: string;
  label: string;
}

export type GroupKey =
  | 'system'
  | 'manufacturer'
  | 'hardware'
  | 'genre'
  | 'developer'
  | 'publisher'
  | 'year'
  | 'decade'
  | 'players'
  | 'region';

export interface Group {
  /** Append to the path to open this group. Empty for "Unknown". */
  key: string;
  label: string;
  gameCount: number;
  /** The game whose image represents the group; see `library.mediaUrl`. */
  imageGame: number | null;
}

export interface BranchItem {
  id: string;
  name: string;
  gameCount: number;
}

export type Listing =
  | { kind: 'groups'; groupBy: GroupKey; items: Group[]; total: number }
  | { kind: 'branches'; items: BranchItem[] }
  | { kind: 'games'; items: Game[]; total: number };

export interface Browse {
  view: string;
  path: string[];
  /** Labels for a breadcrumb, one per path segment. */
  trail: Crumb[];
  listing: Listing;
}

export interface DefaultView {
  id: string | null;
  /** Why a preferred view was passed over, if one was. */
  notes: string[];
}

export type Netplay =
  { mode: 'host'; port?: number } | { mode: 'client'; host: string; port: number };

export interface LaunchRequest {
  /** Overrides Batocera's configured emulator/core for this launch only. */
  emulator?: string;
  core?: string;
  netplay?: Netplay;
}

export interface ThemeInfo {
  id: string;
  name: string | null;
  version: string | null;
  origin: 'bundled' | 'user';
  permissions: string[];
  current: boolean;
  /** Why the theme cannot load, if it cannot. */
  unavailable: string | null;
}

export interface Controller {
  index: number;
  /** SDL GUID. */
  guid: string;
  name: string;
}

export type Button =
  | 'dpadUp'
  | 'dpadDown'
  | 'dpadLeft'
  | 'dpadRight'
  | 'south'
  | 'east'
  | 'west'
  | 'north'
  | 'l1'
  | 'r1'
  | 'l2'
  | 'r2'
  | 'l3'
  | 'r3'
  | 'start'
  | 'select'
  | 'hotkey';

export type Action =
  | 'up'
  | 'down'
  | 'left'
  | 'right'
  | 'confirm'
  | 'back'
  | 'west'
  | 'north'
  | 'pageUp'
  | 'pageDown'
  | 'l2'
  | 'r2'
  | 'l3'
  | 'r3'
  | 'start'
  | 'select'
  | 'hotkey';

export interface InputEvent {
  action: Action;
  /** The physical button; `null` for an analog stick or the keyboard. */
  button: Button | null;
  /** Gamepad in connection order; -1 for the keyboard. */
  gamepad: number;
  pressed: boolean;
  /** Generated while a direction is held. */
  repeat: boolean;
}

export interface GameStart {
  type: 'game:start';
  gameId: number;
  system: string;
  name: string;
}

export interface GameEnd {
  type: 'game:end';
  gameId: number;
  seconds: number;
  exitCode: number | null;
  /** Why the launch failed, if it did. */
  error: string | null;
}

export interface LibraryChanged {
  /** Systems whose games changed: re-query anything showing them. */
  systems: string[];
}

export interface EventMap {
  /** The user picked a view in Forma's views list (Select): show it. */
  'nav:open': NavOpen;
  /** A setting changed in Forma's menu: one of the theme's own, or Forma's (`display.margins`). */
  'settings:changed': SettingsChanged;
  'music:changed': MusicStatus;
  /**
   * A new song started. Forma's shell shows it for `seconds`, unless the theme draws it itself
   * (`"draws": ["nowPlaying"]` in its theme.json), then the theme does, for as long.
   */
  'music:song': MusicSong;
  /** A controller came or went, or the connection changed (`forma.status.get()`). */
  'status:changed': StatusBar;
  'store:progress': StoreProgress;
  /** While `forma.bluetooth.pair()` looks for pads: what Batocera is doing. */
  'bluetooth:pairing': BluetoothPairing;
  /** While `forma.updates.install()` runs. */
  'updates:progress': UpdateProgress;
  'game:start': GameStart;
  'game:end': GameEnd;
  'library:changed': LibraryChanged;
  'views:changed': null;
  input: InputEvent;
}

export type Unsubscribe = () => void;

/**
 * What the theme shows now, for Forma's menu: its entries for the selected game or system
 * ("Super Mario World options…") and the views list's "Showing" mark follow it.
 */
export interface ShellContext {
  /** The view on screen. */
  view?: string | null;
  /** The selected system, or the system of the selected game. */
  system?: { name: string; fullname: string } | null;
  /** The selected game. */
  game?: { id: number; name: string } | null;
}

/**
 * What Forma shows in a corner of the screen: the controllers connected, by kind, and the
 * connection. Forma's shell draws it; a theme restyles it (`forma-shell::part(status)`) or
 * draws its own (`"draws": ["status"]` in its theme.json).
 */
export interface StatusBar {
  /** In player order. */
  controllers: ControllerStatus[];
  /** `null` where Forma cannot tell (not on a Batocera box). */
  network: NetworkLink | null;
}

export interface ControllerStatus {
  /** 1-based. */
  player: number;
  name: string;
  kind: ControllerKind;
}

/** By maker, else by name: for an icon. */
export type ControllerKind = 'xbox' | 'playstation' | 'nintendo' | 'generic';

export interface NetworkLink {
  /** The connection in use: a cable before Wi-Fi; `none` when not connected. */
  kind: 'ethernet' | 'wifi' | 'none';
  /** The internet answers over it. */
  internet: boolean;
}

/** A song that just started, as Forma announces it: the title without the file's extension. */
export interface MusicSong {
  title: string;
  artist: string | null;
  /** How long to show it: Forma's setting (Start → Music). Never 0: Forma says nothing then. */
  seconds: number;
}

export interface NavOpen {
  view: string;
}

export interface SettingsChanged {
  /** `theme` for the theme's own settings (declared in `theme.json`), `system` for Forma's. */
  scope: 'theme' | 'system';
  key: string;
  value: unknown;
}

/** An answer from an online service, and whether it is fresh. */
export interface Fetched<T> {
  data: T;
  /** Served from Forma's cache because the service could not be reached. */
  stale: boolean;
  /** When the data was fetched, ISO 8601 UTC. */
  fetchedAt: string;
}

export interface RaUser {
  username: string;
  points: number;
  softcorePoints: number;
  truePoints: number;
  rank: number | null;
  memberSince: string | null;
  richPresence: string | null;
  /** URL of the avatar. */
  avatar: string | null;
  recentlyPlayed: RaRecent[];
}

export interface RaRecent {
  raGameId: number;
  title: string;
  console: string;
  icon: string | null;
  lastPlayed: string | null;
}

export interface RaGame {
  raGameId: number;
  title: string;
  console: string;
  icon: string | null;
  achievementCount: number;
  earned: number;
  earnedHardcore: number;
  /** 0.0–1.0. */
  completion: number;
  achievements: RaAchievement[];
}

export interface RaAchievement {
  id: number;
  title: string;
  description: string;
  points: number;
  /** URL of the badge. */
  badge: string | null;
  earnedAt: string | null;
  earnedHardcoreAt: string | null;
}

export interface NetplayRoom {
  id: number;
  username: string;
  country: string | null;
  /** The ROM name the host is playing. */
  gameName: string;
  gameCrc: string | null;
  coreName: string;
  coreVersion: string | null;
  retroarchVersion: string | null;
  frontend: string | null;
  host: string;
  port: number | null;
  /** Relay server, for hosts that cannot be reached directly. */
  relayHost: string | null;
  relayPort: number | null;
  hasPassword: boolean;
  hasSpectatePassword: boolean;
  connectable: boolean;
  created: string | null;
  /** The local game with the same name, when there is one. */
  localGameId: number | null;
}

/** The box's own connections. Needs the `system.network` permission; Batocera only. */
export interface NetworkStatus {
  /** `null` when the box has no wired interface. */
  ethernet: EthernetStatus | null;
  /** `null` when the box has no Wi-Fi hardware. */
  wifi: WifiStatus | null;
}

export interface EthernetStatus {
  /** A cable is in and the box has an address on it. */
  connected: boolean;
  ipv4: Ipv4 | null;
  nameservers: string[];
  /** What is saved: DHCP unless a static address was set. */
  config: EthernetConfig;
}

export interface WifiStatus {
  /** Batocera's `wifi.enabled`. */
  enabled: boolean;
  /** The network in use, if any. */
  connected: string | null;
  ipv4: Ipv4 | null;
  /** The network Batocera joins whenever it can. */
  saved: string | null;
}

export interface Ipv4 {
  address: string;
  netmask: string;
  gateway: string | null;
}

export type EthernetConfig =
  | { method: 'dhcp' }
  | {
      method: 'manual';
      address: string;
      netmask: string;
      gateway: string | null;
      /** Up to three. */
      nameservers: string[];
    };

/** 802.1X networks (`enterprise`) cannot be joined from Batocera's Wi-Fi settings. */
export type WifiSecurity = 'open' | 'wep' | 'psk' | 'enterprise';

export interface WifiNetwork {
  ssid: string;
  /** 0–100. */
  strength: number | null;
  security: WifiSecurity;
  connected: boolean;
  saved: boolean;
}

/**
 * `saved`: a cable is plugged in, so Ethernet stays in use and the box joins the network once
 * it is unplugged.
 */
export type WifiJoined = 'connected' | 'saved';

/**
 * One of Batocera's RetroAchievements or netplay settings (`batocera.conf`). Needs the
 * `settings.batocera` permission; Batocera only.
 */
export interface BatoceraSetting {
  key: string;
  /** `null` for a secret, and for a key that is not set. */
  value: string | null;
  kind: 'bool' | 'text' | 'port' | 'secret' | 'choice';
  /** For a secret: whether one is saved. */
  set: boolean;
  /** The values a choice accepts. */
  choices: string[];
}

/**
 * Where an emulator option is set. A game's value overrides its system's, which overrides the
 * global one.
 */
export type OptionsScope =
  { level: 'global' } | { level: 'system'; system: string } | { level: 'game'; game: number };

/** One of Batocera's emulator, core or game options, as EmulationStation offers it. */
export interface OptionItem {
  /** `emulator`, `core`, or the option's `batocera.conf` key after the level prefix. */
  key: string;
  /** Batocera's label, upper-case as ES shows it. */
  label: string;
  /** A section of the page ES puts it in (`ADVANCED OPTIONS`)… */
  group: string | null;
  /** …or a page of its own (`DECORATIONS`). */
  submenu: string | null;
  description: string | null;
  kind: 'choice' | 'number' | 'text';
  choices: OptionChoice[];
  /** For a number. */
  min: number | null;
  max: number | null;
  step: number | null;
  /** What applies when no level sets it, when Batocera says. */
  default: string | null;
  /** Set at this level; `null` when it is not. */
  value: string | null;
  /** Set at a level this one inherits from, when this one does not. */
  inherited: { value: string; from: 'global' | 'system' } | null;
}

export interface OptionChoice {
  value: string;
  label: string;
}

/** The options of one level, ready to draw. Changing `emulator` or `core` changes the list. */
export interface OptionsPage {
  options: OptionItem[];
  /** What the chosen emulator supports: `netplay`, `cheevos`, `padtokeyboard`. */
  flags: string[];
}

/** One of Batocera's system settings, as EmulationStation's System settings menu offers it. */
export interface SystemSetting {
  /** The `batocera.conf` key (`storage`: Batocera's boot configuration). */
  key: string;
  label: string;
  /** `Display`, `Audio`, `Language & region`, `System` or `Storage`. */
  section: string;
  description: string;
  /** One of `choices`, a whole number from `min` to `max` by `step` (the volume), or text. */
  kind: 'choice' | 'number' | 'text';
  choices: OptionChoice[];
  /** For a `number`; `null` otherwise. */
  min: number | null;
  max: number | null;
  step: number | null;
  /** `null` when unset: Batocera's default applies. */
  value: string | null;
  /** When a change takes effect: at once, after Forma restarts, or after a reboot. */
  applies: 'now' | 'restart' | 'reboot';
}

/** A line of what the box is, e.g. `Model` and `Raspberry Pi 5 Model B`. */
export interface InfoItem {
  label: string;
  value: string;
}

/** A package from Batocera's content downloader: a free game, a port, decorations, music. */
export interface StorePackage {
  name: string;
  description: string;
  /** `sys-<system>` for a system's games, `bezel`, `music`, `ports`, `misc`. */
  group: string;
  /** The system a `sys-…` package is for, e.g. `nes`. */
  system: string | null;
  version: string;
  /** Where it comes from: a project or forum page. */
  url: string | null;
  packager: string | null;
  /** In bytes. */
  downloadSize: number;
  installedSize: number;
  /** A picture on store.batocera.org (loading it needs the `network` permission). */
  previewUrl: string | null;
  installed: boolean;
  /** Installed, and the store has a newer version. */
  update: boolean;
}

/** While a package installs. */
export interface StoreProgress {
  name: string;
  /** `download`, then `install`. */
  stage: 'download' | 'install';
  /** 0–100; while installing it only shows that something is happening. */
  percent: number;
}

/** How much of one system is matched to RetroAchievements. */
export interface RaMatch {
  system: string;
  /** Games on disk. */
  games: number;
  /** With a RetroAchievements hash: computed by Forma (rcheevos), or imported. */
  hashed: number;
  /** With a RetroAchievements game id, so `ra.game()` works for them. */
  matched: number;
}

export interface RaMatches {
  /** Forma is hashing or matching now: the numbers are still growing. */
  running: boolean;
  /** Each system RetroAchievements supports that has games. */
  systems: RaMatch[];
}

/** The box's Batocera and the newest on Batocera's update server for it. */
export interface UpdateCheck {
  /** The version the box runs, e.g. `43.1`. */
  current: string;
  latest: string;
  /** `stable`, `unstable` or `butterfly` (Batocera's `updates.type`). */
  channel: string;
  /** `latest` is not what the box runs. */
  available: boolean;
}

/** While an update installs: Batocera's word on it, and how far when it says. */
export interface UpdateProgress {
  message: string;
  percent: number | null;
}

/** A Bluetooth device Batocera has paired and trusts. */
export interface BluetoothDevice {
  /** `AA:BB:CC:DD:EE:FF`. */
  address: string;
  name: string;
  /** A gamepad or joystick, by its Bluetooth class; else something else (a headset…). */
  controller: boolean;
  connected: boolean;
}

export interface BluetoothStatus {
  /** Batocera's Bluetooth is on (`controllers.bluetooth.enabled`). Off, nothing is listed. */
  enabled: boolean;
  devices: BluetoothDevice[];
}

/** While pairing. */
export interface BluetoothPairing {
  /** Batocera's own words, e.g. `Discovering…` or `Pairing Wireless Controller`. */
  message: string;
}

/** Something the music player can play. Its `id` is stable: a theme can remember it. */
export interface MusicSource {
  id: string;
  /** A folder of the music folder (sub-folders included), a playlist file, or a web radio. */
  kind: 'folder' | 'playlist' | 'radio';
  name: string;
  /** 0 for a radio. */
  tracks: number;
}

export interface MusicStatus {
  state: 'stopped' | 'playing' | 'paused';
  source: MusicSource | null;
  /** The track, or for a radio the song it announces. */
  title: string | null;
  artist: string | null;
  album: string | null;
  /** Seconds; `null` for a radio's endless stream. */
  position: number | null;
  duration: number | null;
  /** 1-based, of `tracks`. */
  track: number | null;
  tracks: number | null;
  /** 0–100. */
  volume: number;
  shuffle: boolean;
}

export interface Radio {
  name: string;
  url: string;
}
