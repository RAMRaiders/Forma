import type { Forma, Game, InputEvent, SystemSummary } from '@ramraiders/forma-sdk';

/**
 * One screen: the systems, then a system's games. Up and down move, A opens or plays, B goes
 * back. Start and Select belong to Forma: its menus open over the theme on their own.
 */
export class App {
  #systems: SystemSummary[] = [];
  #games: Game[] | null = null;
  #system: SystemSummary | null = null;
  #selected = 0;
  #playing = false;

  constructor(
    readonly forma: Forma,
    readonly root: HTMLElement
  ) {}

  async start(): Promise<void> {
    this.#systems = (await this.forma.library.systems()).filter((s) => s.gameCount > 0);
    this.forma.input.on((e) => void this.#input(e));
    this.forma.events.on('game:start', () => (this.#playing = true));
    this.forma.events.on('game:end', () => (this.#playing = false));
    this.forma.events.on('library:changed', () => void this.#reload());
    this.#render();
  }

  async #input(e: InputEvent): Promise<void> {
    if (!e.pressed || this.#playing) return;
    const count = this.#games?.length ?? this.#systems.length;
    if (e.action === 'down' && count > 0) this.#selected = (this.#selected + 1) % count;
    else if (e.action === 'up' && count > 0) this.#selected = (this.#selected - 1 + count) % count;
    else if (e.action === 'confirm') await this.#confirm();
    else if (e.action === 'back' && this.#games) {
      this.#selected = Math.max(0, this.#systems.indexOf(this.#system!));
      this.#games = null;
      this.#system = null;
    } else return;
    this.#render();
  }

  async #confirm(): Promise<void> {
    if (this.#games) {
      const game = this.#games[this.#selected];
      if (game) await this.forma.launch.game(game.id);
      return;
    }
    const system = this.#systems[this.#selected];
    if (!system) return;
    // The `systems` view: a system's key opens its games, by name.
    const page = await this.forma.nav.browse('systems', [system.name]);
    this.#games = page.listing.kind === 'games' ? page.listing.items : [];
    this.#system = system;
    this.#selected = 0;
  }

  async #reload(): Promise<void> {
    this.#systems = (await this.forma.library.systems()).filter((s) => s.gameCount > 0);
    if (this.#system) {
      const page = await this.forma.nav.browse('systems', [this.#system.name]);
      this.#games = page.listing.kind === 'games' ? page.listing.items : [];
    }
    this.#selected = Math.min(
      this.#selected,
      Math.max(0, (this.#games ?? this.#systems).length - 1)
    );
    this.#render();
  }

  #render(): void {
    const title = document.createElement('h1');
    title.textContent = this.#system ? this.#system.fullname : 'Systems';
    const list = document.createElement('ul');
    const rows = this.#games
      ? this.#games.map((g) => g.name)
      : this.#systems.map((s) => `${s.fullname} · ${s.gameCount}`);
    rows.forEach((text, i) => {
      const li = document.createElement('li');
      li.textContent = text;
      if (i === this.#selected) li.className = 'selected';
      list.append(li);
    });
    if (rows.length === 0)
      list.append(Object.assign(document.createElement('li'), { textContent: 'Nothing here yet' }));
    this.root.replaceChildren(title, list);
    list.querySelector('.selected')?.scrollIntoView?.({ block: 'nearest' });
    // Forma's menu offers options for what is selected: tell it.
    const game = this.#games?.[this.#selected];
    void this.forma.shell
      .setContext({
        view: 'systems',
        game: game ? { id: game.id, name: game.name } : null,
        system: this.#system ? { name: this.#system.name, fullname: this.#system.fullname } : null,
        list: this.#games ? 'games' : 'systems'
      })
      .catch(() => {});
  }
}
