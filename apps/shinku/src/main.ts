// Le point d'entrée de la page : il choisit à qui l'interface va parler, puis
// lance l'application.

import { App, renderOutside } from './app.ts';
import { insideTauri, tauriBackend } from './backend/tauri.ts';
import type { Backend } from './backend/types.ts';
import { silenceBrowser } from './shortcuts.ts';
import { applyRememberedTheme } from './theme.ts';

/**
 * Dans la fenêtre de Shinku, l'interface parle au cœur. En mode démo
 * (`npm run demo`), elle parle à un faux cœur qui ne lit rien sur le disque ;
 * ce faux cœur n'est jamais embarqué dans l'application. Partout ailleurs,
 * elle n'a personne à qui parler.
 */
async function chooseBackend(): Promise<Backend | null> {
  if (insideTauri()) {
    return tauriBackend();
  }
  if (import.meta.env.MODE === 'demo') {
    const { demoOptions, mockBackend } = await import('./backend/mock.ts');
    return mockBackend(demoOptions(window.location.search));
  }
  return null;
}

async function main(): Promise<void> {
  // Le bon mode d'affichage tout de suite, sans attendre le cœur.
  applyRememberedTheme();

  const root = document.getElementById('app');
  if (root === null) {
    throw new Error('La page ne contient pas l’élément #app.');
  }

  if (import.meta.env.PROD && import.meta.env.MODE !== 'demo') {
    // Dans l'application livrée, la page n'est plus un site : ni F5, ni
    // Ctrl+P, ni menu du navigateur. En développement et dans la démo, tout
    // reste disponible.
    silenceBrowser(window);
  }

  const backend = await chooseBackend();
  if (backend === null) {
    renderOutside(root);
    return;
  }
  await new App(backend, root).start();
}

void main();
