import type { Forma, IdentifyStatus } from '@ramraiders/forma-sdk';
import { ProgressCard } from './progresscard.js';

/**
 * New or changed game files being read to identify them (copied in, say), a system at a time:
 * which system, how far, which file. Only when there are such files: what Forma already knows
 * is not read, and shows nothing.
 */
export class IdentifyProgress {
  readonly #card = new ProgressCard('identify-progress');
  readonly root = this.#card.root;
  /** Systems' full names, by folder name: asked once, at the first file. */
  #names: Promise<Map<string, string>> | null = null;
  /** The latest event: an earlier one still waiting for the names is then dropped. */
  #latest = 0;
  /** Where the time left is measured from: the system's first file seen, and when. */
  #since: { system: string; done: number; at: number } | null = null;

  constructor(private readonly forma: Forma) {
    forma.events.on('library:identifying', (status) => void this.show(status));
  }

  async show(status: IdentifyStatus): Promise<void> {
    const ticket = ++this.#latest;
    const files = (n: number) => `${n} ${n === 1 ? 'file' : 'files'}`;
    const left = this.#left(status);
    if (!status.running) {
      this.#card.show(
        'Games identified',
        files(status.total),
        'New and changed files read',
        1,
        true
      );
      return;
    }
    this.#names ??= this.forma.library
      .systems()
      .then((all) => new Map(all.map((s) => [s.name, s.fullname || s.name])))
      .catch(() => new Map());
    const name = (await this.#names).get(status.system) ?? status.system;
    if (ticket !== this.#latest) return;
    // Its file name alone: the folders above it say nothing new.
    const file = status.current?.split('/').pop() ?? '';
    this.#card.show(
      `Identifying ${name}`,
      `${status.done} / ${status.total}${left ? ` · ${left}` : ''}`,
      file,
      status.total ? status.done / status.total : 0
    );
  }

  /**
   * About how long the system has left, from how fast its files have gone so far: only once
   * there is enough to tell (a few files, half a minute), as a whole system of discs is hours.
   */
  #left(status: IdentifyStatus): string {
    const now = Date.now();
    const since = this.#since;
    if (!status.running) {
      this.#since = null;
      return '';
    }
    if (!since || since.system !== status.system || status.done < since.done) {
      this.#since = { system: status.system, done: status.done, at: now };
      return '';
    }
    const read = status.done - since.done;
    const spent = now - since.at;
    if (read < 3 || spent < 30_000) return '';
    const minutes = Math.max(1, Math.round(((status.total - status.done) * spent) / read / 60_000));
    if (minutes < 60) return `~${minutes} min left`;
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return rest ? `~${hours} h ${rest} min left` : `~${hours} h left`;
  }
}
