// Le dessin de la page Mode jeu, lu sans navigateur : les listes se font par
// clé stable, et l'en-tête attend quand « Ce n'est pas un jeu » est parti.
//
// Pourquoi les clés : lit-html réutilise les lignes d'une liste par position.
// Quand une app disparaît au-dessus de la case qui a le focus, la case garde
// son focus mais montre l'app suivante, et la touche Espace coche une autre app
// que celle que la personne avait choisie. Avec une clé par app, le nœud (donc
// le focus) suit son app. Le scénario complet est joué dans la vraie page par
// l'essai de la démo ; ici, on vérifie ce qui l'empêche : la clé.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { GameView, QuietApp } from '../src/backend/types.ts';
import { fr } from '../src/i18n/fr.ts';
import { initialState } from '../src/state.ts';

// lit-html a besoin d'un `document` à son import.
Object.assign(globalThis, { document: { createTreeWalker: () => ({}) } });

const { repeat } = await import('lit-html/directives/repeat.js');
const { gameView, pickerDialog } = await import('../src/views/game.ts');

const NO_ACTIONS = new Proxy({}, { get: () => () => {} });
const REPEAT = (repeat([], () => 0, () => 0) as unknown as { _$litDirective$: unknown })._$litDirective$;

interface Found {
  items: unknown[];
  keys: unknown[];
}

/** Toutes les listes à clé du dessin, avec les clés que chaque ligne reçoit. */
function keyed(value: unknown, found: Found[] = []): Found[] {
  if (Array.isArray(value)) {
    for (const item of value) {
      keyed(item, found);
    }
  } else if (typeof value === 'object' && value !== null) {
    const result = value as { _$litType$?: unknown; _$litDirective$?: unknown; values?: unknown[] };
    if (result._$litDirective$ === REPEAT && result.values !== undefined) {
      const [items, key] = result.values as [unknown[], (item: unknown, index: number) => unknown];
      found.push({ items, keys: items.map((item, index) => key(item, index)) });
    } else if (result.values !== undefined) {
      // Un gabarit, ou une autre directive (l'écran est enveloppé dans une directive) : on lit ce qu'ils contiennent.
      keyed(result.values, found);
    }
  }
  return found;
}

const app = (exe: string, parts: Partial<QuietApp> = {}): QuietApp => ({
  exe,
  label: exe,
  on: true,
  running: true,
  builtIn: true,
  ...parts,
});

function view(parts: Partial<GameView> = {}): GameView {
  return {
    auto: true,
    power: true,
    quietApps: true,
    agent: 'running',
    startupDisabled: false,
    session: null,
    last: null,
    powerSupport: 'available',
    onBattery: false,
    apps: [app('onedrive.exe'), app('chrome.exe'), app('qbittorrent.exe', { builtIn: false })],
    games: [
      { path: 'C:\\a\\a.exe', name: 'A', source: 'added', lastPlayed: null },
      { path: 'D:\\b\\b.exe', name: 'B', source: 'library', lastPlayed: null },
    ],
    notGames: [{ path: 'E:\\c\\c.exe', name: 'C' }],
    ...parts,
  };
}

const page = (game: GameView, gameLeaving: string | null = null) =>
  gameView({ ...initialState(0), game, gameLeaving, route: 'gameMode' }, NO_ACTIONS as never);

test('les listes de la page (apps, jeux, jeux écartés) se font par clé : le nom du fichier ou le chemin', () => {
  const lists = keyed(page(view()));
  assert.deepEqual(
    lists.map((list) => list.keys),
    [['onedrive.exe', 'chrome.exe', 'qbittorrent.exe'], ['C:\\a\\a.exe', 'D:\\b\\b.exe'], ['E:\\c\\c.exe']],
  );
});

test('une app qui disparaît au-dessus ne change pas la clé des suivantes : leur ligne, donc leur focus, les suit', () => {
  const before = keyed(page(view()))[0];
  const after = keyed(page(view({ apps: view().apps.slice(1) })))[0];
  assert.ok(before !== undefined && after !== undefined);
  assert.deepEqual(after.keys, ['chrome.exe', 'qbittorrent.exe']);
  for (const key of after.keys) {
    assert.ok(before.keys.includes(key));
  }
});

test('la liste du sélecteur se fait par numéro d’app', () => {
  const picker = pickerDialog(
    {
      ...initialState(0),
      picker: {
        kind: 'game',
        apps: [
          { id: 4, name: 'A', path: 'C:\\a.exe' },
          { id: 7, name: 'B', path: 'C:\\b.exe' },
        ],
      },
    },
    NO_ACTIONS as never,
  );
  assert.deepEqual(keyed(picker)[0]?.keys, [4, 7]);
});

/** Le texte de tout ce que le dessin contient, pour y chercher une phrase. */
function texts(value: unknown, found: string[] = []): string[] {
  if (typeof value === 'string') {
    found.push(value);
  } else if (Array.isArray(value)) {
    for (const item of value) {
      texts(item, found);
    }
  } else if (typeof value === 'object' && value !== null) {
    const result = value as { strings?: string[]; values?: unknown[] };
    texts(result.strings, found);
    texts(result.values, found);
  }
  return found;
}

test('quand « Ce n’est pas un jeu » est parti, l’en-tête dit qu’il remet tout en place, et ses boutons sont grisés', () => {
  const session = {
    name: 'Hollow Peaks',
    path: 'D:\\b\\b.exe',
    since: 0,
    source: 'library' as const,
    power: 'applied' as const,
    quietedApps: 1,
    released: false,
    restorePending: false,
  };
  const waiting = texts(page(view({ session }), session.path));
  assert.ok(waiting.includes(fr.game.textLeaving));
  assert.ok(!waiting.includes(fr.game.textPlaying('00:00')));
  const normal = texts(page(view({ session })));
  assert.ok(!normal.includes(fr.game.textLeaving));
  // Une autre partie que celle dont l'ordre est parti ne l'attend pas.
  assert.ok(!texts(page(view({ session }), 'C:\\autre.exe')).includes(fr.game.textLeaving));
});
