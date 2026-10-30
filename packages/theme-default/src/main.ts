import { connect } from '@romdex/sdk';

import { App } from './app.js';

const root = document.querySelector<HTMLElement>('#app')!;

try {
  const romdex = await connect();
  await new App(romdex, root).start();
} catch (error) {
  root.textContent = `Could not connect to RomDex: ${String(error)}`;
}
