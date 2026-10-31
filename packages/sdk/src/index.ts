/**
 * @ramraiders/forma-sdk — the public API for Forma themes and custom interfaces.
 *
 * ```ts
 * import { connect } from '@ramraiders/forma-sdk';
 *
 * const forma = await connect();
 * const home = await forma.nav.defaultView();
 * const page = await forma.nav.browse(home.id ?? 'systems');
 * forma.input.onPress('confirm', () => …);
 * ```
 *
 * The bundled default theme uses nothing but this package: anything it does, a user's theme
 * can do too. Outside Forma, `connect()` falls back to a mock library so themes can be built
 * in a normal browser.
 */
export { connect, Forma, type ConnectOptions } from './client.js';
export { FormaError, type ErrorCode } from './errors.js';
export type { Backend } from './backend.js';
export type { MockOptions } from './mock/backend.js';
export { keyToInput } from './keyboard.js';
export { SDK_VERSION, satisfies } from './version.js';
export type * from './types.js';
