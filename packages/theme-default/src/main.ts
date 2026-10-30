import { connect } from '@romdex/sdk';

import { App } from './app.js';
import { startPerfOverlay } from './ui/perf.js';

const root = document.querySelector<HTMLElement>('#app')!;
if (location.hash === '#perf') startPerfOverlay();

try {
  const romdex = await connect();
  await new App(romdex, root).start();
} catch (error) {
  root.textContent = `Could not connect to RomDex: ${String(error)}`;
}
