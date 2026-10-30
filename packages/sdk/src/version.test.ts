import { describe, expect, it } from 'vitest';
import { SDK_VERSION, satisfies } from './version.js';

describe('satisfies', () => {
  it('accepts the same major with any later minor or patch', () => {
    expect(satisfies('^1', '1.0.0')).toBe(true);
    expect(satisfies('^1', '1.9.3')).toBe(true);
    expect(satisfies('^1.2', '1.2.0')).toBe(true);
    expect(satisfies('^1.2', '1.3.0')).toBe(true);
    expect(satisfies('^1.2.3', '1.2.4')).toBe(true);
  });

  it('rejects a different major or an older minor/patch', () => {
    expect(satisfies('^1', '2.0.0')).toBe(false);
    expect(satisfies('^2', '1.9.9')).toBe(false);
    expect(satisfies('^1.2', '1.1.9')).toBe(false);
    expect(satisfies('^1.2.3', '1.2.2')).toBe(false);
  });

  it('rejects ranges and versions it cannot read rather than guessing', () => {
    expect(satisfies('1', '1.0.0')).toBe(false);
    expect(satisfies('>=1', '1.0.0')).toBe(false);
    expect(satisfies('^1', '1.0')).toBe(false);
    expect(satisfies('^1', 'next')).toBe(false);
  });

  it('is satisfied by its own version', () => {
    expect(satisfies('^1', SDK_VERSION)).toBe(true);
  });
});
