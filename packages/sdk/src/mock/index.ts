/**
 * `@ramraiders/forma-sdk/mock`: Forma without Forma, for building and testing themes in a plain
 * browser or under a test runner. `connect()` already uses it outside the app; import it to drive
 * a theme yourself, e.g. `connect({ backend: mockBackend({ permissions: ['system.network'] }) })`.
 */
export { mockBackend, type MockOptions } from './backend.js';
