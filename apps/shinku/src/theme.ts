// Le mode d'affichage : Clair (« Brume »), Sombre (« Crépuscule »), ou celui
// de Windows.
//
// Le réglage vit dans settings.json, côté cœur. Pour que la fenêtre s'ouvre
// tout de suite dans le bon mode, le dernier mode appliqué est aussi gardé
// dans le stockage de la page : c'est un simple rappel, jamais la référence.

import type { Theme } from './backend/types.ts';

const STORAGE_KEY = 'shinku.theme';
const dark = window.matchMedia('(prefers-color-scheme: dark)');

function resolve(theme: Theme): 'light' | 'dark' {
  if (theme === 'system') {
    return dark.matches ? 'dark' : 'light';
  }
  return theme;
}

function isTheme(value: unknown): value is Theme {
  return value === 'system' || value === 'light' || value === 'dark';
}

let current: Theme = 'system';

/** Applique un mode d'affichage à toute l'interface. */
export function applyTheme(theme: Theme): void {
  current = theme;
  document.documentElement.dataset['theme'] = resolve(theme);
  try {
    window.localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Stockage indisponible : le mode sera appliqué un instant plus tard, c'est tout.
  }
}

/** Applique le mode gardé en rappel, avant que le cœur n'ait répondu. */
export function applyRememberedTheme(): void {
  let remembered: unknown = null;
  try {
    remembered = window.localStorage.getItem(STORAGE_KEY);
  } catch {
    // Voir plus haut.
  }
  applyTheme(isTheme(remembered) ? remembered : 'system');
}

// En mode « Système », l'interface suit Windows quand il change de mode.
dark.addEventListener('change', () => {
  if (current === 'system') {
    applyTheme('system');
  }
});
