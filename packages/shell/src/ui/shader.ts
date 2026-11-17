import type {
  Forma,
  OptionItem,
  OptionsScope,
  ShaderInherited,
  ShaderPage,
  ShaderParam
} from '@ramraiders/forma-sdk';
import { FormaError } from '@ramraiders/forma-sdk';

import type { Page, Row } from './page.js';
import { SCREEN_MENUS, optionRow, optionRows, pick, sentence } from './settings.js';

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

/** A shader choice's label (`crt:<preset>` too, when its preset is not among them, and
 * Batocera's default below global, where it is not a choice but is inherited). */
function choiceLabel(s: ShaderPage, value: string): string {
  if (value === 'default' && !s.shader.choices.some((c) => c.value === value)) {
    return 'Batocera default';
  }
  return (
    s.shader.choices.find((c) => c.value === value)?.label ??
    `CRT · ${value.split('/').pop()!.replace(/[-_]/g, ' ')}`
  );
}

/**
 * The shader at one level, as one choice: none, Forma's koko-aio from one of its presets, or
 * one of Batocera's sets. With koko-aio's, its essential settings and every other below it.
 */
function shaderRows(forma: Forma, page: Page, scope: OptionsScope, s: ShaderPage): Row[] {
  if (!s.available) {
    return [
      {
        heading: 'Shader',
        label: 'Shader',
        detail: 'koko-aio is not installed (Batocera 41 and later ship it)'
      }
    ];
  }
  const global = scope.level === 'global';
  const { value, inherited, choices } = s.shader;
  const choose = (choice: string | null) =>
    forma.shaders.choose(scope, choice).then(() => undefined);
  const rows: Row[] = [
    {
      heading: 'Shader',
      label: 'Shader',
      detail:
        value !== null
          ? choiceLabel(s, value)
          : `Default · ${choiceLabel(s, inherited.value)} (${FROM[inherited.from]})`,
      alt:
        !global && value !== null
          ? { label: 'Default', run: () => page.run('Saving…', () => choose(null)) }
          : undefined,
      run: () =>
        pick(
          page,
          'Shader',
          choices,
          value,
          choose,
          global ? null : `Default · ${choiceLabel(s, inherited.value)}`
        )
    }
  ];
  if (!(value ?? inherited.value).startsWith('crt:')) return rows;
  const params = new Map(s.sections.flatMap((x) => x.params).map((p) => [p.key, p]));
  for (const key of s.essentials) {
    const p = params.get(key);
    if (p) rows.push(paramRow(forma, page, scope, p));
  }
  const set = [...params.values()].filter((p) => p.value !== null).length;
  rows.push({
    label: 'All settings…',
    detail: `${params.size} settings in ${s.sections.length} sections${set > 0 ? ` · ${set} set here` : ''}`,
    run: () => page.push('All settings', allScreen(forma, page, scope))
  });
  return rows;
}

/**
 * Emulator options at one level, with how games look there among them: the shader (one choice:
 * none, Forma's koko-aio from a preset with its settings, or one of Batocera's sets), then
 * Batocera's picture and decoration options, after the core. Its decoration gives way to
 * koko-aio's bezel where that is on: two frames round a game are one too many. Global sets them
 * for every game; a system or a game says otherwise for itself. Used by the next launch.
 */
export function emulatorScreen(
  forma: Forma,
  page: Page,
  scope: OptionsScope
): () => Promise<Row[]> {
  return async () => {
    const [optionsPage, s] = await Promise.all([
      forma.options.get(scope),
      forma.shaders.get(scope).catch(() => null)
    ]);
    const { options } = optionsPage;
    const rows = optionRows(forma, page, scope, optionsPage);
    // After the emulator and core (and anything else ungrouped), before the option groups.
    const at = rows.findIndex((r) => r.heading);
    const screen = s ? screenRows(forma, page, scope, s, options) : [];
    rows.splice(at < 0 ? rows.length : at, 0, ...screen);
    return rows;
  };
}

/** How games look at one level: the shader, then Batocera's picture and decorations (its
 * shader set not among them: the shader row is the one place to choose it). */
function screenRows(
  forma: Forma,
  page: Page,
  scope: OptionsScope,
  s: ShaderPage,
  options: OptionItem[]
): Row[] {
  const rows = shaderRows(forma, page, scope, s);
  SCREEN_MENUS.forEach((menu) => {
    options
      .filter((o) => o.submenu === menu && o.key !== 'shaderset')
      .forEach((o, i) => {
        const heading = i === 0 ? (menu === 'DECORATIONS' ? 'Decorations' : 'Picture') : undefined;
        if (o.key === 'bezel' && s.drawsBezel) {
          rows.push({
            heading,
            label: sentence(o.label),
            detail: 'Off · koko-aio draws the bezel'
          });
        } else {
          rows.push({ ...optionRow(forma, page, scope, o), heading });
        }
      });
  });
  return rows;
}
