import { readFileSync } from 'node:fs';

import { defineConfig } from 'vite';

/**
 * La politique de sécurité de contenu de la fenêtre, lue dans la configuration
 * de Tauri : il n'en existe qu'une seule écriture.
 */
function contentSecurityPolicy(): string {
  const text = readFileSync(new URL('./src-tauri/tauri.conf.json', import.meta.url), 'utf8');
  const config = JSON.parse(text) as { app: { security: { csp: string } } };
  return config.app.security.csp;
}

// L'interface est servie par Tauri depuis des fichiers locaux, sous une
// politique de sécurité de contenu stricte : rien n'est mis en ligne dans la
// page (ni script, ni style, ni police en « data: »).
export default defineConfig({
  clearScreen: false,
  server: {
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
  },
  preview: {
    host: '127.0.0.1',
    port: 4173,
    strictPort: true,
    // `npm run preview` sert l'interface compilée sous la même politique que
    // la fenêtre : un style ou un script refusé là-bas l'est aussi ici.
    headers: { 'Content-Security-Policy': contentSecurityPolicy() },
  },
  build: {
    // WebView2 suit Edge : pas besoin de viser d'anciens navigateurs.
    target: 'esnext',
    outDir: 'dist',
    emptyOutDir: true,
    assetsInlineLimit: 0,
    cssCodeSplit: false,
    modulePreload: { polyfill: false },
  },
});
