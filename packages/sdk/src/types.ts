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
  'game:start': GameStart;
  'game:end': GameEnd;
  'library:changed': LibraryChanged;
  'views:changed': null;
  input: InputEvent;
}

export type Unsubscribe = () => void;

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
