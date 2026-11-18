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

  constructor(private readonly forma: Forma) {
    forma.events.on('library:identifying', (status) => void this.show(status));
  }

  async show(status: IdentifyStatus): Promise<void> {
    const ticket = ++this.#latest;
    const files = (n: number) => `${n} ${n === 1 ? 'file' : 'files'}`;
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
      `${status.done} / ${status.total}`,
      file,
      status.total ? status.done / status.total : 0
    );
  }
}
