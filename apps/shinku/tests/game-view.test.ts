// Le dessin de la page Mode jeu, lu sans navigateur : les listes se font par
// clé stable, et l'en-tête attend quand « Ce n'est pas un jeu » est parti.
//
// Pourquoi les clés : lit-html réutilise les lignes d'une liste par position.
// Quand une app disparaît au-dessus de la case qui a le focus, la case garde
// son focus mais montre l'app suivante, et la touche Espace coche une autre app
// que celle que la personne avait choisie. Avec une clé par app, le nœud (donc
// le focus) suit son app. Sans navigateur, on ne peut pas jouer le scénario ;
// on vérifie ce qui l'empêche : la clé.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { GameView, QuietApp } from '../src/backend/types.ts';
import { formatSince } from '../src/format.ts';
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

const page = (game: GameView, gameLeaving: { path: string; until: number } | null = null) =>
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

/** Le gabarit qui porte ce texte de gabarit, quelque part dans le dessin. */
function templateWith(value: unknown, marker: string): { strings: string[]; values: unknown[] } | null {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = templateWith(item, marker);
      if (found !== null) {
        return found;
      }
    }
  } else if (typeof value === 'object' && value !== null) {
    const result = value as { strings?: string[]; values?: unknown[] };
    if (result.strings?.join('').includes(marker) === true) {
      return { strings: result.strings, values: result.values ?? [] };
    }
    return templateWith(result.values, marker);
  }
  return null;
}

/** Les valeurs de tous les attributs `?disabled` d'un gabarit et de ses gabarits imbriqués. */
function disabled(value: unknown, found: unknown[] = []): unknown[] {
  if (Array.isArray(value)) {
    for (const item of value) {
      disabled(item, found);
    }
  } else if (typeof value === 'object' && value !== null) {
    const result = value as { strings?: string[]; values?: unknown[] };
    result.strings?.forEach((text, index) => {
      if (/\?disabled=$/.test(text.trimEnd())) {
        found.push(result.values?.[index]);
      }
    });
    disabled(result.values, found);
  }
  return found;
}

const SESSION = {
  name: 'Hollow Peaks',
  path: 'D:\\b\\b.exe',
  since: 0,
  source: 'library' as const,
  power: 'applied' as const,
  quietedApps: 1,
  released: false,
  restorePending: false,
};
const LEAVING = { path: SESSION.path, until: 15_000 };
const NOTED = [{ path: SESSION.path, name: 'Hollow Peaks' }];

test('quand « Ce n’est pas un jeu » est parti, l’en-tête dit qu’il remet tout en place, et ses deux boutons sont grisés', () => {
  const playing = view({ session: SESSION, notGames: NOTED });
  const waiting = page(playing, LEAVING);
  const texts_ = texts(waiting);
  assert.ok(texts_.includes(fr.game.textLeaving));
  assert.ok(!texts_.includes(fr.game.textPlaying(formatSince(0, 0))));
  const buttons = templateWith(waiting, 'class="pbtns"');
  assert.ok(buttons !== null);
  assert.deepEqual(disabled(buttons), [true, true]);

  // Sans attente, rien n'est grisé et la phrase est celle de la partie.
  const normal = page(playing);
  assert.ok(!texts(normal).includes(fr.game.textLeaving));
  assert.deepEqual(disabled(templateWith(normal, 'class="pbtns"')), [false, false]);
  // Une autre partie que celle dont l'ordre est parti ne l'attend pas.
  assert.ok(!texts(page(playing, { path: 'C:\\autre.exe', until: 15_000 })).includes(fr.game.textLeaving));
});

test('l’attente ne cache jamais « n’a pas encore pu tout remettre en place »', () => {
  const pending = view({ session: { ...SESSION, restorePending: true }, notGames: NOTED });
  const shown = texts(page(pending, LEAVING));
  assert.ok(shown.includes(fr.game.textPending));
  assert.ok(!shown.includes(fr.game.textLeaving));
  assert.ok(shown.includes(fr.game.titlePending));
});

test('une attente dépassée, un agent arrêté ou un ordre non noté ne grisent plus l’en-tête', () => {
  const noted = view({ session: SESSION, notGames: NOTED });
  const expired = { ...initialState(20_000), game: noted, gameLeaving: LEAVING, route: 'gameMode' as const };
  assert.ok(!texts(gameView(expired, NO_ACTIONS as never)).includes(fr.game.textLeaving));
  assert.ok(!texts(page(view({ session: SESSION, notGames: NOTED, agent: 'stopped' }), LEAVING)).includes(fr.game.textLeaving));
  assert.ok(!texts(page(view({ session: SESSION }), LEAVING)).includes(fr.game.textLeaving));
});
