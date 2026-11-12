import type {
  Forma,
  OptionChoice,
  OptionsScope,
  ShaderInherited,
  ShaderPage,
  ShaderParam
} from '@ramraiders/forma-sdk';
import { FormaError } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';

/** Where an inherited value comes from, said briefly. */
const FROM: Record<ShaderInherited<unknown>['from'], string> = {
  global: 'global',
  system: 'system',
  preset: 'preset',
  shader: 'koko-aio'
};

/** A setting that is only on or off. */
function isSwitch(p: ShaderParam): boolean {
  return p.min === 0 && p.max === 1 && p.step === 1;
}

/** `0.450000` as `0.45`, to as many decimals as its step has. */
export function shown(value: number, step: number): string {
  const decimals = Math.min(4, (String(step).split('.')[1] ?? '').length);
  return String(Number(value.toFixed(decimals)));
}

function presetLabel(choices: OptionChoice[], value: string): string {
  return choices.find((c) => c.value === value)?.label ?? value.split('/').pop()!;
}

function valueText(p: ShaderParam, value: number): string {
  return isSwitch(p) ? (value >= 0.5 ? 'On' : 'Off') : shown(value, p.step);
}

/** What a setting is now: set here, or what it inherits, and from where. */
function paramDetail(p: ShaderParam): string {
  if (p.value !== null) return valueText(p, p.value);
  return `Default · ${valueText(p, p.inherited.value)} (${FROM[p.inherited.from]})`;
}

function paramRow(forma: Forma, page: Page, scope: OptionsScope, p: ShaderParam): Row {
  const set = (value: number | null) =>
    forma.shaders
      .set(scope, p.key, value)
      .catch((error: unknown) => page.notify(FormaError.from(error).message))
      .then(() => page.refresh());
  const now = p.value ?? p.inherited.value;
  const row: Row = {
    label: p.label,
    detail: paramDetail(p),
    alt: p.value !== null ? { label: 'Default', run: () => set(null) } : undefined
  };
  if (isSwitch(p)) {
    const flip = () => set(now >= 0.5 ? 0 : 1);
    row.adjust = flip;
    row.run = flip;
  } else {
    // Left and right step it and stop at the ends; A to type a value.
    row.adjust = (dir) => {
      const next = Math.min(p.max, Math.max(p.min, Number((now + dir * p.step).toFixed(6))));
      return next === now ? undefined : set(next);
    };
    row.run = async () => {
      const typed = await page.ask({
        title: `${p.label} (${shown(p.min, p.step)} to ${shown(p.max, p.step)})`,
        value: shown(now, p.step)
      });
      if (typed === null) return;
      if (typed.trim() === '') return set(null);
      const n = Number(typed.replace(',', '.'));
      if (!Number.isFinite(n)) return page.notify(`${typed} is not a number`);
      return set(Math.min(p.max, Math.max(p.min, n)));
    };
  }
  return row;
}

/** The preset a level starts from: its own, or the level above's (Default). */
function presetRow(forma: Forma, page: Page, scope: OptionsScope, s: ShaderPage): Row {
  const { value, inherited, choices } = s.preset;
  const set = (preset: string | null) =>
    page.run('Saving…', () => forma.shaders.setPreset(scope, preset));
  return {
    label: 'Starts from',
    detail:
      value !== null
        ? presetLabel(choices, value)
        : `Default · ${presetLabel(choices, inherited.value)} (${FROM[inherited.from]})`,
    alt: value !== null ? { label: 'Default', run: () => set(null) } : undefined,
    run: () => {
      const at = value === null ? 0 : choices.findIndex((c) => c.value === value) + 1;
      const choose = (preset: string | null) => async () => {
        try {
          await forma.shaders.setPreset(scope, preset);
        } catch (error) {
          page.notify(FormaError.from(error).message);
        }
        await page.pop();
      };
      return page.push(
        'Starts from',
        () => [
          {
            label: `${value === null ? '✓ ' : ''}Default · ${presetLabel(choices, inherited.value)}`,
            detail: 'What the level above uses',
            run: choose(null)
          },
          ...choices.map((c) => ({
            label: c.value === value ? `✓ ${c.label}` : c.label,
            run: choose(c.value)
          }))
        ],
        undefined,
        Math.max(0, at)
      );
    }
  };
}

/** Every setting of one of koko-aio's sections. */
function sectionScreen(
  forma: Forma,
  page: Page,
  scope: OptionsScope,
  title: string
): () => Promise<Row[]> {
  return async () => {
    const s = await forma.shaders.get(scope);
    const section = s.sections.find((x) => x.title === title);
    return (section?.params ?? []).map((p) => paramRow(forma, page, scope, p));
  };
}

/** koko-aio's sections, each a page of its settings. */
function allScreen(forma: Forma, page: Page, scope: OptionsScope): () => Promise<Row[]> {
  return async () => {
    const s = await forma.shaders.get(scope);
    return s.sections.map((section) => {
      const lead = section.switch ? section.params.find((p) => p.key === section.switch) : null;
      const set = section.params.filter((p) => p.value !== null).length;
      const state = lead ? valueText(lead, lead.value ?? lead.inherited.value) : null;
      return {
        label: `${section.title}…`,
        detail: [state, set > 0 ? `${set} set here` : `${section.params.length} settings`]
          .filter(Boolean)
          .join(' · '),
        run: () => page.push(section.title, sectionScreen(forma, page, scope, section.title))
      };
    });
  };
}

/**
 * Forma's CRT shader at one level: on or off (for every game), the preset it starts from, the
 * settings worth having at hand, and every setting by koko-aio's sections. Used by the next
 * launch; a level that sets nothing inherits the one above.
 */
export function shaderScreen(forma: Forma, page: Page, scope: OptionsScope): () => Promise<Row[]> {
  return async () => {
    const s = await forma.shaders.get(scope);
    if (!s.available) {
      return [{ label: 'koko-aio is not installed', detail: 'Batocera 41 and later ship it' }];
    }
    const toggle = () =>
      page.run(s.enabled ? 'Switching off…' : 'Switching on…', async () => {
        await forma.shaders.setEnabled(!s.enabled);
        return s.enabled
          ? 'CRT shader off: Batocera’s own shaders again'
          : 'CRT shader on for every game, from the next launch';
      });
    const rows: Row[] = [
      {
        label: `CRT shader: ${s.enabled ? 'On' : 'Off'}`,
        detail: s.enabled
          ? 'koko-aio on every RetroArch game (RetroArch on Vulkan)'
          : 'Batocera’s own shaders · A to switch koko-aio on for every game',
        adjust: toggle,
        run: toggle
      }
    ];
    if (!s.enabled && scope.level !== 'global') return rows;
    rows.push(presetRow(forma, page, scope, s));
    const params = new Map(s.sections.flatMap((x) => x.params).map((p) => [p.key, p]));
    s.essentials.forEach((key, i) => {
      const p = params.get(key);
      if (p)
        rows.push({
          ...paramRow(forma, page, scope, p),
          heading: i === 0 ? 'Essentials' : undefined
        });
    });
    const set = [...params.values()].filter((p) => p.value !== null).length;
    rows.push({
      heading: 'More',
      label: 'All settings…',
      detail: `${params.size} settings in ${s.sections.length} sections${set > 0 ? ` · ${set} set here` : ''}`,
      run: () => page.push('All settings', allScreen(forma, page, scope))
    });
    return rows;
  };
}
