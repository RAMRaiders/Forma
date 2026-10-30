/**
 * @romdex/sdk — the public API for RomDex themes and custom interfaces.
 *
 * The default theme uses nothing but this package, which is what keeps it honest: anything a
 * bundled theme can do, a user's theme can do too. The full API (library, nav, launch, input,
 * events, settings, ra, netplay, system) lands in #28.
 */
export { SDK_VERSION, satisfies } from './version.js';
