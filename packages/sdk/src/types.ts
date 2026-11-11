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

/** A Forma add-on installed on this machine (see `docs/addons.md`). */
export interface AddonInfo {
  id: string;
  name: string;
  version: string | null;
  author: string | null;
  description: string | null;
  /**
   * What it adds to Forma: `scraper` (a source beside the built-in ones), `music` (web radios
   * in Music), `menu` (pages in Forma's menu, see `menu`).
   */
  provides: string[];
  permissions: string[];
  /** What each of `permissions` lets it do, in words. */
  permissionDescriptions: string[];
  /** The only sites it may reach. */
  hosts: string[];
  enabled: boolean;
  /** Why it cannot run, if it cannot. */
  problem: string | null;
  /** Its id among the scraper's sources, for a scraper add-on. */
  scraperSource: string | null;
  /** What the user enters for it (`addons.setSetting`); secret values are never sent back. */
  settings: ScraperCredential[];
  /** Its tile in Forma's menu, for a `menu` add-on: its pages come from `addons.page`. */
  menu: { label: string; description: string | null } | null;
  /** It has a system part: scripts run with full control of the machine. */
  system: boolean;
  /** Installed from Forma's own catalogue, so a system part may run. */
  verified: boolean;
  /** A newer version in the catalogue this Forma can run. */
  update: string | null;
}

/**
 * A row of an add-on's page, drawn by Forma in its own menu. A (or Enter) does what the row
 * says: `open` a sub-page, or `action` (after asking for text, with `ask`); left and right send
 * `adjust` with `-1` or `1`; Y sends `alt.action`.
 */
export interface AddonRow {
  label: string;
  detail: string | null;
  heading: string | null;
  action: string | null;
  adjust: string | null;
  open: string[] | null;
  ask: { title: string; secret: boolean; value: string | null } | null;
  alt: { label: string; action: string } | null;
}

/** One of an add-on's pages: `path` is `[]` for its first, then what its rows `open`. */
export interface AddonPage {
  title: string | null;
  rows: AddonRow[];
}

/** What an add-on's action asked Forma to do next. */
export interface AddonActionResult {
  /** A message for the user. */
  notify: string | null;
  /** A page to open. */
  open: string[] | null;
  /** Go back a page. */
  back: boolean;
}

/** An add-on in the catalogue. */
export interface CatalogueAddon {
  id: string;
  name: string;
  version: string;
  author: string | null;
  description: string | null;
  permissions: string[];
  permissionDescriptions: string[];
  /** The version installed, if any. */
  installed: string | null;
  update: boolean;
  runsHere: boolean;
}

/** A kind of media, as EmulationStation names it (and its `gamelist.xml` tag). */
export type MediaKind =
  | 'thumbnail'
  | 'image'
  | 'titleshot'
  | 'marquee'
  | 'wheel'
  | 'boxback'
  /** The box's spine, upright: Forma's own `<boxside>` tag. */
  | 'boxside'
  /** The whole cover laid flat (back, spine, front): Forma's own `<boxfull>`. */
  | 'boxfull'
  /** A wide banner: Forma's own `<banner>`. */
  | 'banner'
  /** An arcade game's flyer: Forma's own `<flyer>`. */
  | 'flyer'
  /** An arcade cabinet: Forma's own `<cabinet>`. */
  | 'cabinet'
  | 'boxart'
  | 'fanart'
  | 'cartridge'
  | 'map'
  | 'bezel'
  | 'mix'
  | 'video'
  | 'manual';

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
  /** As the gamelist gives it, e.g. `us`, `eu`, `jp`, `wor`. Since 1.22.0. */
  region: string | null;
  /** Languages, as the gamelist gives them, e.g. `en,fr,de`. Since 1.22.0. */
  lang: string | null;
  /**
   * Age ratings, `BOARD:RATING` comma-separated, e.g. `ESRB:M,PEGI:18`. Boards: ESRB, PEGI, CERO,
   * USK, ACB, GRAC, CLASSIND, BBFC. Since 1.24.0.
   */
  ageRating: string | null;
  /** One of the games its system is remembered for, on the lists Forma ships. Since 1.25.0. */
  iconic: boolean;
  /** Media kind → URL to load it from. */
  media: Partial<Record<MediaKind, string>>;
}

/** A title with more than one version among a system's games. Since 1.26.0. */
export interface VersionGroup {
  title: string;
  games: {
    id: number;
    name: string;
    path: string;
    /** What its name has in brackets: `Japan`, `Rev 1`, `T-En by Someone v1.0`. */
    tags: string[];
    /** The first version in the group with the same game data, when that is known. */
    identicalTo: number | null;
  }[];
}

/** Games LaunchBox says are one game under different names, by its name for it. Since 1.29.0. */
export interface DuplicateGroup {
  title: string;
  games: { id: number; name: string; path: string }[];
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

/** A filter on the game list on screen: everything set must hold. Since 1.27.0. */
export interface ListFilter {
  /** Words the name contains. */
  name?: string | null;
  /** `A` to `Z`, or `#` for a name starting with anything else. */
  letter?: string | null;
  genre?: string | null;
  developer?: string | null;
  publisher?: string | null;
  region?: string | null;
  year?: number | null;
  /** At least this many players at once. */
  players?: number | null;
  favorites?: boolean;
}

export interface FilterChoice {
  value: string;
  /** Games it would leave. */
  count: number;
}

/** The filter on the list on screen, and the choices to refine it. Since 1.27.0. */
export interface FilterOptions {
  filter: ListFilter;
  total: number;
  /** Games in the list without the filter. */
  all: number;
  letters: FilterChoice[];
  genres: FilterChoice[];
  developers: FilterChoice[];
  publishers: FilterChoice[];
  regions: FilterChoice[];
  years: FilterChoice[];
  players: FilterChoice[];
}

export interface Browse {
  view: string;
  path: string[];
  /** Labels for a breadcrumb, one per path segment. */
  trail: Crumb[];
  listing: Listing;
}

/**
 * One of the user's collections: EmulationStation's custom collections, so they carry over.
 * Browse them in the `collections` view: a branch per collection, its games by name.
 */
export interface Collection {
  name: string;
  /** Its games on the box now. */
  games: number;
}

export interface DefaultView {
  id: string | null;
  /** Why a preferred view was passed over, if one was. */
  notes: string[];
}

/**
 * Netplay, through Batocera's launcher. Nickname, relay and the host's own port come from
 * Batocera's netplay settings. Usually built by `forma.netplay.join()` and `host()`.
 */
export type Netplay =
  | {
      mode: 'host';
      port?: number;
      /** Players must give it to join. One word, no quotes. */
      password?: string;
    }
  | {
      mode: 'client';
      host: string;
      port: number;
      password?: string;
      /** A relay's session for the room, when joining through a relay. */
      session?: string;
      /** Watch instead of play. */
      spectate?: boolean;
    };

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
  /** Who made it (`author` in its theme.json). */
  author: string | null;
  /** What the theme is like (`description` in its theme.json). */
  description: string | null;
  permissions: string[];
  /** What each of `permissions` lets the theme do, in words, in the same order. */
  permissionDescriptions: string[];
  /** Installed by the user and not in use: `themes.remove` can take it away. */
  removable: boolean;
  /** A newer version in the theme catalogue (as fetched last); `themes.install` gets it. */
  update: string | null;
  current: boolean;
  /** Why the theme cannot load, if it cannot. */
  unavailable: string | null;
  /** Where to load the theme's logo from (`logo` in its theme.json), if it has one. */
  logo: string | null;
}

/** Something the user enters for a scraper source. Secret values are never sent back. */
export interface ScraperCredential {
  key: string;
  label: string;
  secret: boolean;
  /** The source is not asked without it. */
  required: boolean;
  set: boolean;
  /** What was entered, for one that is not secret. */
  value: string | null;
}

/** A place the scraper asks, in the user's order. */
export interface ScraperSource {
  id: string;
  name: string;
  /** What it gives and what it needs, in a line. */
  about: string;
  enabled: boolean;
  /** Every required credential is entered. */
  ready: boolean;
  needs: ScraperCredential[];
  gives: MediaKind[];
  /** It has text (descriptions, dates…), not only media. */
  metadata: boolean;
}

export interface ScraperSettings {
  /** In the order they are asked: the first with an answer wins, per field and per kind. */
  sources: ScraperSource[];
  /** The kinds to fetch. */
  media: MediaKind[];
  /** Every kind there is, with its name. */
  mediaKinds: { kind: MediaKind; label: string }[];
  /** Fetch text: name, description, developer, publisher, genre, players, date, rating. */
  metadata: boolean;
  /** Replace what games have; otherwise only fill what is missing. */
  replace: boolean;
  /** Fetch what the theme in use asks for too, on top of `media`. Since 1.24.0. */
  themeNeeds: boolean;
  /**
   * What the theme in use asks for in its `theme.json` (`scraper`), or null: media kinds, and
   * details beyond the usual ones (`ageRating`). Since 1.24.0. `shows` is every kind it draws
   * (empty when it does not say) and `backups` the kinds it draws only when another is missing,
   * with the one each replaces. Since 1.26.0.
   */
  theme: {
    name: string;
    view: string | null;
    media: MediaKind[];
    fields: string[];
    shows: MediaKind[];
    backups: Partial<Record<MediaKind, MediaKind>>;
  } | null;
}

/** What the sources have for a game, section by section, to pick from. Since 1.25.0. */
export interface ScraperOffers {
  text: {
    /** `name`, `description`, `developer`, `publisher`, `genre`, `players`, `releaseDate`, `rating`, `ageRating`. */
    field: string;
    label: string;
    /** What the game has now. */
    current: string | null;
    /** `note`: the country or language it is in, when the source says. */
    offers: { source: string; value: string; note: string | null }[];
  }[];
  media: {
    kind: MediaKind;
    label: string;
    /** Whether the game has one now. */
    current: boolean;
    /** `id` names it to `preview` and `apply`. */
    offers: {
      id: number;
      source: string;
      /** The country or region it is for, in words. */
      region: string | null;
      /** What sets it apart from others of its kind: `Small file`. */
      variant: string | null;
      format: string | null;
      /** In bytes. */
      size: number | null;
    }[];
  }[];
  /** Sources that could not answer, and why. */
  problems: string[];
}

/** What to scrape. */
export type ScrapeScope =
  | { kind: 'game'; id: number }
  | { kind: 'system'; name: string }
  | { kind: 'all' }
  /** The logos of the publishers and developers that have none yet. Since 1.28.0. */
  | { kind: 'companies' };

/** A scraper run, going on or the last one. */
export interface ScraperStatus {
  running: boolean;
  /** A game's name, a system's, or `Everything`. */
  scope: string | null;
  total: number;
  done: number;
  found: number;
  notFound: number;
  /** Games that already had everything asked for. */
  skipped: number;
  current: string | null;
  /** The latest problems, e.g. a source refusing its credentials. */
  problems: string[];
  stopped: boolean;
}

/**
 * A magazine in `/userdata/magazines`: a CBZ/ZIP, CBR/RAR, CB7/7Z, CBT/TAR or PDF file, or a
 * folder of page images. Folders below group them.
 */
export interface Magazine {
  /** Its path below the magazines folder, `/`-separated. */
  id: string;
  title: string;
  /** The folders it is in, `/`-separated; `''` at the top. */
  folder: string;
  format: 'zip' | 'rar' | 'sevenzip' | 'tar' | 'pdf' | 'folder';
  /** Where the user left it (from 0), if they opened it before. */
  page: number | null;
  /** Its pages' URL with `{page}` in it: use `magazines.pageUrl`. */
  pageUrl: string;
}

/** A theme in the catalogue (`themes.catalogue()`), to install or update from Forma. */
export interface CatalogueTheme {
  id: string;
  name: string;
  version: string;
  author: string | null;
  description: string | null;
  /** What it would ask for, as in its theme.json, and the same in words. */
  permissions: string[];
  permissionDescriptions: string[];
  /** `https://` pictures of it. */
  previews: string[];
  /** The version installed now, if any. */
  installed: string | null;
  /** Installed, and this is a newer version this Forma can run. */
  update: boolean;
  /** This Forma can run it (its SDK range allows this Forma's). */
  runsHere: boolean;
}

export interface ThemeCatalogue {
  themes: CatalogueTheme[];
  /** Entries left out, and why. */
  skipped: string[];
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

/** A game's media was replaced; since 1.27.0. */
export interface MediaChanged {
  game: number;
}

export interface EventMap {
  /** The user picked a view in Forma's views list (Select): show it. */
  'nav:open': NavOpen;
  /**
   * The user chose a game in Forma's menu (Game versions, Possible duplicates): show it, in its
   * system, as if browsed to. Since 1.29.0.
   */
  'nav:game': NavGame;
  /** Forma's shell is asked to open its magazines (`id`: one of them; `null`: the list). */
  'magazines:open': { id: string | null };
  /** After each game of a scraper run, and when it ends. */
  'scraper:progress': ScraperStatus;
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
  /**
   * Shortly after `game:end`: the RetroAchievements the session earned, when it earned any.
   * Forma's shell shows them, unless the theme draws them (`"draws": ["achievements"]`).
   */
  'ra:unlocked': RaSession;
  'library:changed': LibraryChanged;
  /**
   * A game's pictures were replaced, often under the same file names: `library.mediaUrl` has
   * new URLs for it from now on, so draw again whatever shows it. Since 1.27.0.
   */
  'media:changed': MediaChanged;
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
  /**
   * The kind of list the selection moves in: Select opens Forma's options for it (for
   * `systems`, the order they are listed in).
   */
  list?: 'categories' | 'systems' | 'games' | null;
}

/**
 * How `library.systems()` lists systems, Forma's setting `library.systemsOrder` (scope
 * `system`): `name` (A to Z, the default), `maker` (by manufacturer, then oldest first) or
 * `year` (oldest first). The user sets it in Forma's Select menu; when it changes, Forma sends
 * `settings:changed` and a theme reads `library.systems()` again.
 */
export type SystemsOrder = 'name' | 'maker' | 'year';

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

export interface NavGame {
  game: number;
  system: string;
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

/** What a play session earned on RetroAchievements. */
export interface RaSession {
  /** Forma's game id. */
  gameId: number;
  /** The game's progress now, after the session. */
  game: RaGame;
  /** Earned during the session, in the order they were earned. */
  unlocked: RaAchievement[];
  /** Their points. */
  points: number;
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
  /** Found on the local network (`forma.netplay.lan()`), not in the lobby. */
  lan: boolean;
  /** Where to connect: the host, or its relay with the room's session. `null`: no way in. */
  join: { host: string; port: number; session: string | null } | null;
  /** The local game with the same name or CRC, when there is one. */
  localGameId: number | null;
  /** The installed core that can join it (e.g. `fceumm`), when there is one. */
  core: string | null;
  /** How long connecting took, when measured (`maxPingMs`). */
  pingMs: number | null;
}

/** Which rooms `forma.netplay.lobby()` and `lan()` keep. */
export interface RoomFilter {
  /** Only rooms playing a game on this box. */
  owned?: boolean;
  /** Only rooms an installed core can join. */
  joinable?: boolean;
  /** Only rooms without a password. */
  open?: boolean;
  /** Only rooms with this core (the room's name for it, e.g. `FCEUmm`), any case. */
  coreName?: string;
  /** Only rooms answering within this many milliseconds; measures each room's ping. */
  maxPingMs?: number;
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
  /**
   * A folder of the music folder (sub-folders included), a playlist file, or a web radio (the
   * user's, `radio:<url>`, or a `music` add-on's, `addon:<id>:<url>`).
   */
  kind: 'folder' | 'playlist' | 'radio';
  name: string;
  /** 0 for a radio. */
  tracks: number;
}

/**
 * Where on the screen a game's video shows (`forma.video.play(id, { area })`): fractions of the
 * screen, 0–1, from its top left. Leave the page see-through there.
 */
export interface VideoArea {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * A game's video, played by Forma (`forma.video.play`): full screen or in an area, under the
 * theme's page.
 * The user turns its sound on or off in Start → Audio → Video sound (Forma's setting
 * `video.sound`; a change comes as `settings:changed`).
 */
export interface VideoPlay {
  /**
   * It shows: leave the page see-through (transparent backgrounds) where it should be seen.
   * `false`: the game has no video, or it could not play; show something else.
   */
  playing: boolean;
  /** Heard, the music lowered meanwhile. */
  sound: boolean;
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
