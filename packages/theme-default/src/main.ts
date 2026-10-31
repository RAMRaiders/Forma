import { connect } from '@romdex/sdk';

import { App } from './app.js';
import { startPerfOverlay } from './ui/perf.js';

const root = document.querySelector<HTMLElement>('#app')!;

try {
  const romdex = await connect();
  if (location.hash === '#perf') {
    // Saved as a theme setting too, for reading off a device remotely.
    startPerfOverlay((stats) => void romdex.settings.set('perf', stats));
  }
  await new App(romdex, root).start();
} catch (error) {
  root.textContent = `Could not connect to RomDex: ${String(error)}`;
}
