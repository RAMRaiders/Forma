/**
 * The SDK API version this package speaks. Kept in lockstep with `SDK_API_VERSION` in
 * `forma-core`: the host reports its version and the SDK refuses a mismatch up front.
 */
export const SDK_VERSION = '1.11.0';

function parse(version: string): [number, number, number] | null {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version.trim());
  if (!match) return null;
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/**
 * Whether `version` satisfies a caret `range` as written in `theme.json` (`^1`, `^1.2`,
 * `^1.2.3`). Only caret ranges are accepted: a theme pins a major version and works with any
 * later minor, which is the whole compatibility promise the SDK makes.
 */
export function satisfies(range: string, version: string): boolean {
  const target = parse(version);
  const req = /^\^(\d+)(?:\.(\d+))?(?:\.(\d+))?$/.exec(range.trim());
  if (!target || !req) return false;

  const [major, minor, patch] = target;
  const wantMajor = Number(req[1]);
  const wantMinor = Number(req[2] ?? 0);
  const wantPatch = Number(req[3] ?? 0);

  if (major !== wantMajor) return false;
  if (minor !== wantMinor) return minor > wantMinor;
  return patch >= wantPatch;
}
