import { connect } from '@ramraiders/forma-sdk';

import { App } from './app.js';

const root = document.querySelector<HTMLElement>('#app')!;

try {
  // Outside Forma (`npm run dev` in a browser) this connects to the SDK's mock library.
  // deferReady keeps Forma's cover up until the first screen is drawn.
  const forma = await connect({ deferReady: true });
  await new App(forma, root).start();
  await forma.ready();
} catch (error) {
  root.textContent = `Could not connect to Forma: ${String(error)}`;
}
