import { connect } from '@romdex/sdk';

// Placeholder until the theme views (#32–#35) land: proves the SDK round trip end to end.
const status = document.querySelector<HTMLParagraphElement>('#status');

try {
  const romdex = await connect();
  const systems = await romdex.library.systems();
  const games = systems.reduce((n, s) => n + s.gameCount, 0);
  if (status) {
    status.textContent = `${systems.length} systems, ${games} games · SDK ${romdex.hostVersion} (${romdex.backendKind})`;
  }
} catch (error) {
  if (status) status.textContent = `Could not connect to RomDex: ${String(error)}`;
}
