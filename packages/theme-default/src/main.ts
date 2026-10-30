import { SDK_VERSION } from '@romdex/sdk';

// Placeholder until the SDK API (#28) and the theme views (#32–#35) land.
const status = document.querySelector<HTMLParagraphElement>('#status');
if (status) status.textContent = `SDK ${SDK_VERSION}`;
