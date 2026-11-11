import type { Forma, Game, InputEvent, MediaKind } from '@ramraiders/forma-sdk';
import { FormaError } from '@ramraiders/forma-sdk';

import { el } from './dom.js';

import type { Page, Row } from './page.js';

/** Forma's setting holding the screensaver's choices, as one object. */
const SCREENSAVER_SETTING = 'screensaver';

export type ScreensaverType = 'dim' | 'black' | 'slideshow' | 'video';
export type GameInfo = 'off' | 'start' | 'always';
export type GamesFrom = 'all' | 'favorites' | 'system';

/** What EmulationStation's screensaver settings offer, the same defaults where it has them. */
export interface ScreensaverSettings {
  /** Minutes without a press before it starts; 0: never. */
  after: number;
  type: ScreensaverType;
  /** Seconds before the next picture or video. */
  swap: number;
  /** The game's name and system over its picture or video. */
  info: GameInfo;
  /** Pictures fill the screen, cut to its shape, rather than fit inside it. */
  stretch: boolean;
  /** The slideshow's picture of each game. */
  picture: MediaKind;
  /** Videos are heard (and the music lowered), as Start → Audio → Video sound says. */
  sound: boolean;
  /** Whose pictures and videos. */
  games: GamesFrom;
  /** Left/right go to the next or last game, A plays the one shown; else any button wakes. */
  controls: boolean;
  /** The time in a corner. */
  clock: boolean;
}

const SCREENSAVER_DEFAULTS: ScreensaverSettings = {
  after: 5,
  type: 'dim',
  swap: 10,
  info: 'always',
  stretch: false,
  picture: 'image',
  sound: false,
  games: 'all',
  controls: true,
  clock: false
};

const AFTER_MINUTES = [0, 1, 2, 3, 5, 10, 15, 20, 30, 45, 60] as const;
const SWAP_SECONDS = [5, 8, 10, 15, 20, 30, 45, 60, 90, 120] as const;

const TYPES: { value: ScreensaverType; label: string; about: string }[] = [
  { value: 'dim', label: 'Dim', about: 'The screen darkens; the theme stays under it' },
  { value: 'black', label: 'Black', about: 'A black screen' },
  { value: 'slideshow', label: 'Slideshow', about: 'Pictures of your games, one after another' },
  { value: 'video', label: 'Videos', about: 'Your games’ videos, one after another' }
];

const PICTURES: { value: MediaKind; label: string }[] = [
  { value: 'image', label: 'Screenshot' },
  { value: 'titleshot', label: 'Title screen' },
  { value: 'fanart', label: 'Fan art' },
  { value: 'boxart', label: 'Box' },
  { value: 'mix', label: 'Mix' }
];

const INFO: { value: GameInfo; label: string }[] = [
  { value: 'always', label: 'Always' },
  { value: 'start', label: 'At the start' },
  { value: 'off', label: 'Never' }
];

const GAMES: { value: GamesFrom; label: string; about: string }[] = [
  { value: 'all', label: 'All games', about: 'From the whole library' },
  { value: 'favorites', label: 'Favourites', about: 'Only your favourites' },
  {
    value: 'system',
    label: 'Selected system',
    about: 'The system the theme shows, else the whole library'
  }
];

/** Stored settings over the defaults; anything unknown or out of range gives the default. */
function screensaverSettings(stored: unknown): ScreensaverSettings {
  const s = (stored && typeof stored === 'object' ? stored : {}) as Partial<
    Record<keyof ScreensaverSettings, unknown>
  >;
  const d = SCREENSAVER_DEFAULTS;
  const oneOf = <T>(values: readonly T[], v: unknown, fallback: T): T =>
    values.includes(v as T) ? (v as T) : fallback;
  const bool = (v: unknown, fallback: boolean) => (typeof v === 'boolean' ? v : fallback);
  const num = (v: unknown, min: number, max: number, fallback: number) =>
    typeof v === 'number' && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;
  return {
    after: num(s.after, 0, 600, d.after),
    type: oneOf(
      TYPES.map((t) => t.value),
      s.type,
      d.type
    ),
    swap: num(s.swap, 3, 600, d.swap),
    info: oneOf(
      INFO.map((i) => i.value),
      s.info,
      d.info
    ),
    stretch: bool(s.stretch, d.stretch),
    picture: oneOf(
      PICTURES.map((p) => p.value),
      s.picture,
      d.picture
    ),
    sound: bool(s.sound, d.sound),
    games: oneOf(
      GAMES.map((g) => g.value),
      s.games,
      d.games
    ),
    controls: bool(s.controls, d.controls),
    clock: bool(s.clock, d.clock)
  };
}

function step<T>(values: readonly T[], now: T, dir: -1 | 1, wrap = true): T {
  const at = values.indexOf(now);
  if (at === -1) return values[0]!;
  const next = at + dir;
  if (wrap) return values[(next + values.length) % values.length]!;
  return values[Math.max(0, Math.min(values.length - 1, next))]!;
}

/** The nearest of `values` to `now`, for a value set outside the menu. */
function nearest(values: readonly number[], now: number): number {
  return values.reduce((a, b) => (Math.abs(b - now) < Math.abs(a - now) ? b : a));
}

function minutes(n: number): string {
  return n === 0 ? 'Never' : n === 1 ? '1 minute' : `${n} minutes`;
}

function seconds(n: number): string {
  return n >= 60 && n % 60 === 0 ? `${n / 60} min` : `${n} s`;
}

/**
 * Start → Screensaver: EmulationStation's screensaver settings, in Forma's words. Every row
 * switches on the spot (A or left/right); the last one shows it now.
 */
export function screensaverScreen(
  forma: Forma,
  page: Page,
  preview: () => void
): () => Promise<Row[]> {
  return async () => {
    const s = screensaverSettings(
      await forma.settings.getSystem(SCREENSAVER_SETTING).catch(() => null)
    );
    const save = (change: Partial<ScreensaverSettings>) =>
      page.run('…', () => forma.settings.setSystem(SCREENSAVER_SETTING, { ...s, ...change }));
    const toggle = (key: 'stretch' | 'sound' | 'controls' | 'clock', label: string) => {
      const flip = () => save({ [key]: !s[key] });
      return { label: `${label}: ${s[key] ? 'On' : 'Off'}`, adjust: flip, run: flip };
    };
    const type = TYPES.find((t) => t.value === s.type)!;
    const types = TYPES.map((t) => t.value);
    const after = nearest(AFTER_MINUTES, s.after);
    const swap = nearest(SWAP_SECONDS, s.swap);
    const moving = s.type === 'slideshow' || s.type === 'video';
    const rows: Row[] = [
      {
        label: `Start after: ${minutes(s.after)}`,
        detail:
          s.after === 0
            ? 'The screensaver is off'
            : 'With no button pressed for that long, and no game running',
        adjust: (dir) => save({ after: step(AFTER_MINUTES, after, dir, false) }),
        run: () => save({ after: step(AFTER_MINUTES, after, 1) })
      },
      {
        label: `Type: ${type.label}`,
        detail: type.about,
        adjust: (dir) => save({ type: step(types, s.type, dir) }),
        run: () => save({ type: step(types, s.type, 1) })
      }
    ];
    if (moving) {
      const games = GAMES.find((g) => g.value === s.games)!;
      const info = INFO.find((i) => i.value === s.info)!;
      rows.push(
        {
          heading: s.type === 'video' ? 'Videos' : 'Slideshow',
          label: `Next game after: ${seconds(s.swap)}`,
          detail:
            s.type === 'video' ? 'A short video loops until then' : 'How long each picture shows',
          adjust: (dir) => save({ swap: step(SWAP_SECONDS, swap, dir, false) }),
          run: () => save({ swap: step(SWAP_SECONDS, swap, 1) })
        },
        {
          label: `Games: ${games.label}`,
          detail: games.about,
          adjust: (dir) =>
            save({
              games: step(
                GAMES.map((g) => g.value),
                s.games,
                dir
              )
            }),
          run: () =>
            save({
              games: step(
                GAMES.map((g) => g.value),
                s.games,
                1
              )
            })
        },
        {
          label: `Game info: ${info.label}`,
          detail: 'The game’s name and system over its picture',
          adjust: (dir) =>
            save({
              info: step(
                INFO.map((i) => i.value),
                s.info,
                dir
              )
            }),
          run: () =>
            save({
              info: step(
                INFO.map((i) => i.value),
                s.info,
                1
              )
            })
        }
      );
      if (s.type === 'slideshow') {
        const picture = PICTURES.find((p) => p.value === s.picture) ?? PICTURES[0]!;
        const kinds = PICTURES.map((p) => p.value);
        rows.push(
          {
            label: `Picture: ${picture.label}`,
            detail: 'Which of each game’s pictures; games without one are left out',
            adjust: (dir) => save({ picture: step(kinds, s.picture, dir) }),
            run: () => save({ picture: step(kinds, s.picture, 1) })
          },
          {
            ...toggle('stretch', 'Fill the screen'),
            detail: s.stretch
              ? 'Pictures fill the screen, their edges cut off'
              : 'Pictures fit inside the screen, whole'
          }
        );
      } else {
        rows.push({
          ...toggle('sound', 'Sound'),
          detail: s.sound
            ? 'Heard, the music lowered, as Audio → Video sound allows'
            : 'Videos play muted; the music goes on'
        });
      }
      rows.push({
        ...toggle('controls', 'Controls'),
        detail: s.controls
          ? 'Left and right change game, A plays it; other buttons wake'
          : 'Any button wakes'
      });
    }
    rows.push(
      { ...toggle('clock', 'Clock'), detail: 'The time in a corner while it shows' },
      {
        heading: 'Try it',
        label: 'Start the screensaver now',
        detail: 'Any button stops it',
        run: preview
      }
    );
    return rows;
  };
}

/** How the screensaver reaches Forma's core, through the shell's own calls. */
export interface ScreensaverHooks {
  /** It came on or went: input is the shell's meanwhile, and the theme hears of it. */
  setActive(active: boolean): Promise<void>;
  /** Not now: a game runs, a magazine is open, the keyboard types. */
  blocked(): boolean;
  /** The system the theme shows, for "Selected system". */
  system(): Promise<string | null>;
}

/** Pictures and videos that will not load before the next one is tried. */
const MAX_MISSES = 8;
/** How long "At the start" shows the game's name. */
const INFO_MS = 5000;
/** The opacity fade between pictures, as in style.css. */
const FADE_MS = 900;

/**
 * Forma's screensaver, over any theme: after the minutes the user chose with no button
 * pressed, it dims or blacks the screen, or shows the library's pictures or videos. The
 * first press only wakes it; with controls on, left/right change game and A plays it.
 */
export class Screensaver {
  readonly root = el('div', { class: 'screensaver', hidden: '' });
  #settings: ScreensaverSettings = SCREENSAVER_DEFAULTS;
  #idle: ReturnType<typeof setTimeout> | undefined;
  #swap: ReturnType<typeof setTimeout> | undefined;
  #clockTimer: ReturnType<typeof setInterval> | undefined;
  #infoTimer: ReturnType<typeof setTimeout> | undefined;
  #games: Game[] = [];
  #at = -1;
  #misses = 0;
  /** Counts each start and stop, so a late answer from Forma is dropped. */
  #run = 0;
  /** Hides the theme's page while a video shows under the window. */
  #seeThrough: HTMLStyleElement | null = null;
  readonly #stage = el('div', { class: 'stage' });
  readonly #info = el('div', { class: 'info', part: 'screensaver-info' });
  readonly #clock = el('div', { class: 'clock', part: 'screensaver-clock' });

  constructor(
    private readonly forma: Forma,
    private readonly hooks: ScreensaverHooks
  ) {
    this.root.append(this.#stage, this.#info, this.#clock);
    // A change that comes first outranks the first read, which may predate it.
    let changed = false;
    forma.settings
      .getSystem(SCREENSAVER_SETTING)
      .then((v) => changed || this.#apply(v))
      .catch(() => changed || this.#apply(null));
    forma.events.on('settings:changed', ({ scope, key, value }) => {
      if (scope !== 'system' || key !== SCREENSAVER_SETTING) return;
      changed = true;
      this.#apply(value);
    });
  }

  get active(): boolean {
    return !this.root.hidden;
  }

  #apply(stored: unknown): void {
    this.#settings = screensaverSettings(stored);
    if (!this.active) this.poke();
  }

  /**
   * Someone pressed something: the wait starts again. Answers whether the press only woke
   * the screensaver, so it goes no further.
   */
  poke(): boolean {
    clearTimeout(this.#idle);
    const minutes = this.#settings.after;
    if (minutes > 0) this.#idle = setTimeout(() => this.#due(), minutes * 60_000);
    if (!this.active) return false;
    void this.stop();
    return true;
  }

  /** While it shows: what a press does. Answers whether it was used (else it wakes). */
  input(e: InputEvent): boolean {
    if (!this.active) return false;
    const moving = this.#settings.type === 'slideshow' || this.#settings.type === 'video';
    if (this.#settings.controls && moving && e.pressed) {
      if (e.action === 'left' || e.action === 'right') {
        if (!e.repeat) void this.#next(e.action === 'right' ? 1 : -1);
        return true;
      }
      const game = this.#games[this.#at];
      if (e.action === 'confirm' && game && !e.repeat) {
        void this.stop().then(() =>
          this.forma.launch
            .game(game.id)
            .catch((error: unknown) => console.error(FormaError.from(error).message))
        );
        return true;
      }
    }
    // A release after a press that woke it, or a held button: nothing to wake.
    if (!e.pressed || e.repeat) return true;
    this.poke();
    return true;
  }

  #due(): void {
    if (this.active) return;
    if (this.hooks.blocked()) return void this.poke();
    void this.start();
  }

  /** Shows it now, whatever the wait (the settings page's "Start now"). */
  async start(): Promise<void> {
    if (this.active) return;
    clearTimeout(this.#idle);
    const run = ++this.#run;
    // Read again: the page that started it may have just changed them.
    this.#settings = screensaverSettings(
      await this.forma.settings.getSystem(SCREENSAVER_SETTING).catch(() => this.#settings)
    );
    if (run !== this.#run || this.active) return;
    const s = this.#settings;
    this.root.className = `screensaver type-${s.type}${s.stretch ? ' stretch' : ''}`;
    this.#stage.replaceChildren();
    this.#info.hidden = true;
    this.root.hidden = false;
    this.#tick();
    this.#clock.hidden = !s.clock;
    if (s.clock) this.#clockTimer = setInterval(() => this.#tick(), 10_000);
    await this.hooks.setActive(true).catch(() => {});
    // The theme's video goes too, sound and all; the theme plays it again on `screensaver`.
    if (s.type !== 'video') await this.forma.video.stop().catch(() => {});
    if (s.type !== 'slideshow' && s.type !== 'video') return;
    const system = s.games === 'system' ? await this.hooks.system().catch(() => null) : null;
    try {
      this.#games = await this.forma.library.random({
        media: [s.type === 'video' ? 'video' : s.picture],
        limit: 200,
        favorites: s.games === 'favorites',
        system
      });
    } catch {
      this.#games = [];
    }
    if (run !== this.#run) return;
    if (this.#games.length === 0) {
      // Nothing to show: dimmed, saying why.
      this.root.classList.replace(`type-${s.type}`, 'type-dim');
      this.#info.replaceChildren(
        el(
          'span',
          { class: 'name' },
          s.type === 'video' ? 'No game has a video yet' : 'No game has that picture yet'
        ),
        el('span', { class: 'system' }, 'Scrape your games from Start → Games → Scrapers')
      );
      this.#info.hidden = false;
      return;
    }
    if (s.type === 'video') {
      this.#seeThrough = document.createElement('style');
      this.#seeThrough.textContent =
        'html, body { visibility: hidden !important; background: transparent !important; }';
      document.documentElement.append(this.#seeThrough);
    }
    this.#at = -1;
    this.#misses = 0;
    await this.#next(1);
  }

  async stop(): Promise<void> {
    if (!this.active) return;
    const wasVideo = this.#seeThrough !== null || this.#settings.type === 'video';
    this.#run++;
    clearTimeout(this.#swap);
    clearTimeout(this.#infoTimer);
    clearInterval(this.#clockTimer);
    this.root.hidden = true;
    this.#stage.replaceChildren();
    this.#seeThrough?.remove();
    this.#seeThrough = null;
    this.#games = [];
    if (wasVideo) await this.forma.video.stop().catch(() => {});
    await this.hooks.setActive(false).catch(() => {});
    this.poke();
  }

  #tick(): void {
    this.#clock.textContent = new Date().toLocaleTimeString([], {
      hour: '2-digit',
      minute: '2-digit'
    });
  }

  async #next(dir: -1 | 1): Promise<void> {
    const run = this.#run;
    clearTimeout(this.#swap);
    const count = this.#games.length;
    if (count === 0) return;
    this.#at = (this.#at + dir + count) % count;
    const game = this.#games[this.#at]!;
    const s = this.#settings;
    const shown =
      s.type === 'video'
        ? await this.forma.video
            .play(game.id, { muted: !s.sound })
            .then((v) => v.playing)
            .catch(() => false)
        : await this.#picture(game);
    if (run !== this.#run) return;
    if (!shown) {
      // A file gone or unreadable: on to the next, but not for ever.
      if (++this.#misses >= Math.min(MAX_MISSES, count)) return void this.stop();
      return this.#next(dir);
    }
    this.#misses = 0;
    this.#showInfo(game);
    this.#swap = setTimeout(() => void this.#next(1), s.swap * 1000);
  }

  /** The game's picture over the last one, faded in once it has loaded. */
  #picture(game: Game): Promise<boolean> {
    const width = Math.round(window.innerWidth * (window.devicePixelRatio || 1));
    const img = el('img', {
      alt: '',
      src: this.forma.library.mediaUrl(game.id, this.#settings.picture, width)
    });
    // In at once, unseen until it loads; then it fades in over the last, which goes after.
    this.#stage.append(img);
    return new Promise((resolve) => {
      img.addEventListener('load', () => {
        requestAnimationFrame(() => img.classList.add('in'));
        const old = [...this.#stage.children].filter((c) => c !== img);
        setTimeout(() => old.forEach((o) => o.remove()), FADE_MS);
        resolve(true);
      });
      img.addEventListener('error', () => {
        img.remove();
        resolve(false);
      });
    });
  }

  #showInfo(game: Game): void {
    clearTimeout(this.#infoTimer);
    const info = this.#settings.info;
    if (info === 'off') return void (this.#info.hidden = true);
    const year = game.releaseDate?.slice(0, 4);
    const system = this.#systemName(game.system);
    this.#info.replaceChildren(
      el('span', { class: 'name' }, game.name),
      el('span', { class: 'system' }, [system, year].filter(Boolean).join(' · '))
    );
    this.#info.hidden = false;
    if (info === 'start') this.#infoTimer = setTimeout(() => (this.#info.hidden = true), INFO_MS);
  }

  #systems: Map<string, string> | null = null;

  #systemName(name: string): string {
    if (!this.#systems) {
      this.#systems = new Map();
      this.forma.library
        .systems()
        .then((all) => all.forEach((s) => this.#systems!.set(s.name, s.fullname || s.name)))
        .catch(() => {});
    }
    return this.#systems.get(name) ?? name;
  }
}
