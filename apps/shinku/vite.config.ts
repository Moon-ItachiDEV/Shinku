import { defineConfig } from 'vite';

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
