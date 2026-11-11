/**
 * Forma's shell, first thing in every theme page (see `crates/forma-app/src/shell.rs`).
 *
 * In this order, before any theme script runs: take Tauri's invoke, claim this page's token,
 * put up the entry cover. Then, once the page has a body, Forma's menus wait for Start and
 * Select. Everything lives in a closed shadow root, so the theme's CSS cannot restyle the shell
 * and the shell's cannot leak into the theme.
 */
import { Forma, FormaError, keyToInput } from '@ramraiders/forma-sdk';
import { listen } from '@tauri-apps/api/event';

import { shellBackend, tauriInvoke } from './backend.js';
import { Cover, coverInfo } from './cover.js';
import { type ShellInfo, Shell } from './shell.js';
import css from './style.css?inline';

/** A theme that never reports ready is replaced by Forma; the cover goes anyway by then. */
const COVER_MAX_MS = 16_000;

function start(): void {
  const invoke = tauriInvoke();
  if (!invoke) return;
  // Read while this script runs: it is the current one only now.
  const info = coverInfo(document.currentScript);
  const claim = invoke('shell_claim') as Promise<string>;

  const host = document.createElement('forma-shell');
  // Above anything a theme can stack, and out of its layout.
  host.setAttribute(
    'style',
    'all: initial; position: fixed; inset: 0; z-index: 2147483647; pointer-events: none;'
  );
  const shadow = host.attachShadow({ mode: 'closed' });
  const style = document.createElement('style');
  style.textContent = css;
  const cover = new Cover(info);
  shadow.append(style, cover.root);
  if (info.seeThrough) {
    // The boot picture shows through the cover: the theme page, loading under it, stays out of
    // sight (backgrounds too) until the cover goes. The shell's host is `all: initial`, visible.
    const hide = document.createElement('style');
    hide.textContent =
      'html, body { visibility: hidden !important; background: transparent !important; }';
    document.documentElement.append(hide);
    cover.onReveal = () => hide.remove();
  }
  // `<head>` is still being parsed: the document element is all there is.
  document.documentElement.append(host);
  // The theme may replace `<body>`'s content but not move us: keep the host last at the top.
  const keepLast = () => {
    if (host.parentNode !== document.documentElement || host.nextSibling) {
      document.documentElement.append(host);
    }
  };
  document.addEventListener('DOMContentLoaded', keepLast, { once: true });

  void listen('shell:theme-ready', () => cover.ready());
  setTimeout(() => cover.hide(), COVER_MAX_MS);

  claim
    .then(async (token) => {
      const backend = shellBackend(invoke, token);
      // Forma holds the background music until its UI is up.
      cover.onStarted = () => void backend.invoke('shell_started').catch(() => {});
      const forma = new Forma(backend, await backend.invoke<string>('sdk_api_version'), false);
      const call = <T>(command: string, args?: Record<string, unknown>) =>
        backend.invoke<T>(command, args).catch((error: unknown) => {
          throw FormaError.from(error);
        });
      const shell = new Shell(forma, {
        info: () => call<ShellInfo>('shell_info'),
        setOpen: (open) => call('shell_set_open', { open }),
        openView: (view) => call('shell_open_view', { view }),
        openGame: (id) => call('shell_open_game', { id }),
        setThemeSetting: (key, value) => call('shell_set_theme_setting', { key, value })
      });
      shadow.append(shell.root);

      forma.input.on((e) => shell.input(e));
      forma.events.on('game:start', () => {
        shell.playing = true;
      });
      forma.events.on('game:end', () => {
        shell.playing = false;
      });
      listenToKeyboard(shell);
    })
    .catch((error: unknown) => {
      // Someone claimed the page before us: Forma's menus stay away rather than run unproven.
      console.error('[forma] the shell could not start:', FormaError.from(error).message);
      cover.hide();
    });
}

/**
 * The keyboard, for development and a keyboard on the box: F1 (Start) and Tab (Select) are
 * Forma's, as the gamepad buttons are, and while the shell is open every key is. Listening in the
 * capture phase on the window, registered before any theme script, the shell sees keys first and
 * stops them reaching the theme.
 */
function listenToKeyboard(shell: Shell): void {
  const onKey = (pressed: boolean) => (e: KeyboardEvent) => {
    const input = keyToInput(e.key, pressed, e.repeat);
    if (!input) return;
    const ours = shell.open || input.action === 'start' || input.action === 'select';
    if (!ours) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    shell.input(input);
  };
  window.addEventListener('keydown', onKey(true), { capture: true });
  window.addEventListener('keyup', onKey(false), { capture: true });
}

start();
