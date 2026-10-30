/**
 * @romdex/sdk — the public API for RomDex themes and custom interfaces.
 *
 * ```ts
 * import { connect } from '@romdex/sdk';
 *
 * const romdex = await connect();
 * const home = await romdex.nav.defaultView();
 * const page = await romdex.nav.browse(home.id ?? 'systems');
 * romdex.input.onPress('confirm', () => …);
 * ```
 *
 * The bundled default theme uses nothing but this package: anything it does, a user's theme
 * can do too. Outside RomDex, `connect()` falls back to a mock library so themes can be built
 * in a normal browser.
 */
export { connect, RomDex, type ConnectOptions } from './client.js';
export { RomDexError, type ErrorCode } from './errors.js';
export type { Backend } from './backend.js';
export type { MockOptions } from './mock/backend.js';
export { keyToInput } from './keyboard.js';
export { SDK_VERSION, satisfies } from './version.js';
export type * from './types.js';
