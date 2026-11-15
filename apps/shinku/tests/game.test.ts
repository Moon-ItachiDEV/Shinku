// Ce que la page Mode jeu tire d'une vue du cœur : l'état de l'en-tête,
// l'avertissement unique, les pastilles, les listes d'apps, les gestes
// optimistes.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { GameSession, GameView, KnownGame, PowerOutcome, QuietApp } from '../src/backend/types.ts';
import { fr } from '../src/i18n/fr.ts';
import {
  canRelease,
  gameWarning,
  busyKey,
  focusKey,
  headerState,
  inGame,
  isRemovable,
  isRemovableApp,
  neighbour,
  noteStop,
  optionOf,
  RELEASE_GRACE,
  releaseExpected,
  type Releasing,
  powerChip,
  powerRow,
  shownApps,
  showsButtons,
  showsChips,
  STOPPED_GRACE,
  stoppedSince,
  waitingApps,
  withExpectedRelease,
  withOption,
  withQuietApp,
  withReleased,
} from '../src/model/game.ts';

const NOW = 1_794_770_000_000;

function app(exe: string, label: string, parts: Partial<QuietApp> = {}): QuietApp {
  return { exe, label, on: false, running: false, builtIn: true, ...parts };
}

function session(parts: Partial<GameSession> = {}): GameSession {
  return {
    name: 'Hollow Peaks',
    path: 'D:\\SteamLibrary\\steamapps\\common\\Hollow Peaks\\hp.exe',
    since: NOW - 38 * 60_000,
    source: 'library',
    power: 'applied',
    quietedApps: 1,
    released: false,
    restorePending: false,
    ...parts,
  };
}

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
    apps: [],
    games: [],
    notGames: [],
    ...parts,
  };
}

// ── L'en-tête ──────────────────────────────────────────────────────────────

test('l’en-tête est éteint tant que le mode automatique l’est, sans partie', () => {
  assert.equal(headerState(view({ auto: false })), 'off');
});

test('l’en-tête veille quand le mode automatique est allumé et qu’aucune partie n’est notée', () => {
  assert.equal(headerState(view()), 'idle');
});

test('une partie en cours donne l’état « en jeu », et son nom au titre', () => {
  const playing = view({ session: session() });
  assert.equal(headerState(playing), 'playing');
  assert.ok(inGame(headerState(playing)));
  assert.equal(fr.game.titlePlaying('Hollow Peaks'), 'En jeu\u00a0: Hollow Peaks');
});

test('une partie notée au journal passe devant le réglage, même éteint', () => {
  // L'agent finira la partie au pas suivant : jusque-là, la page dit la vérité.
  assert.equal(headerState(view({ auto: false, session: session() })), 'playing');
});

test('une partie remise en place est « released »', () => {
  assert.equal(headerState(view({ session: session({ released: true }) })), 'released');
});

test('une remise en place en attente n’est pas une partie en cours : titre neutre, ni pastilles ni boutons', () => {
  const pending = view({ session: session({ restorePending: true }) });
  const state = headerState(pending);
  assert.equal(state, 'pending');
  assert.ok(!inGame(state));
  assert.ok(!showsChips(state));
  assert.ok(!showsButtons(state));
  assert.equal(fr.game.titlePending, 'Remise en place en cours');
  // Pendant la partie, les boutons sont là.
  assert.ok(showsButtons('playing'));
  assert.ok(showsButtons('released'));
  assert.ok(!showsButtons('idle'));
  assert.ok(!showsButtons('off'));
});

test('une remise en place en attente passe devant « remis en place »', () => {
  const both = view({ session: session({ released: true, restorePending: true }) });
  assert.equal(headerState(both), 'pending');
  assert.equal(headerState(view({ session: session({ restorePending: true }) })), 'pending');
});

test('seules les parties dont les réglages sont appliqués montrent des pastilles', () => {
  assert.ok(showsChips('playing'));
  assert.ok(!showsChips('released'));
  assert.ok(!showsChips('pending'));
  assert.ok(!showsChips('idle'));
  assert.ok(!showsChips('off'));
});

test('« Tout remettre maintenant » disparaît une fois la partie remise en place', () => {
  assert.ok(canRelease(session()));
  assert.ok(canRelease(session({ restorePending: true })));
  assert.ok(!canRelease(session({ released: true })));
});

test('la pastille de l’alimentation ne dit rien de ce que la page explique ailleurs', () => {
  const outcomes: Record<PowerOutcome, string | null> = {
    applied: 'applied',
    alreadyFast: 'alreadyFast',
    onBattery: 'onBattery',
    custom: 'custom',
    refused: 'refused',
    off: null,
    unavailable: null,
    unsuited: null,
  };
  for (const [outcome, chip] of Object.entries(outcomes)) {
    assert.equal(powerChip(outcome as PowerOutcome), chip, outcome);
  }
});

// ── L'avertissement unique ─────────────────────────────────────────────────

test('sans souci, aucun avertissement', () => {
  assert.equal(gameWarning(view(), null, NOW), null);
});

test('l’agent absent de l’installation passe devant tout, même si le mode est éteint', () => {
  const all = view({ auto: true, agent: 'missing', startupDisabled: true });
  assert.equal(gameWarning(all, NOW - 60_000, NOW), 'agentMissing');
  assert.equal(gameWarning(view({ auto: false, agent: 'missing' }), null, NOW), 'agentMissing');
});

test('l’agent arrêté n’est signalé qu’au bout de 15 secondes', () => {
  const stopped = view({ agent: 'stopped', startupDisabled: true });
  // Le démarrage désactivé attend son tour : l'agent a peut-être juste besoin de temps.
  assert.equal(gameWarning(stopped, NOW - 5_000, NOW), 'startupDisabled');
  assert.equal(gameWarning(stopped, NOW - STOPPED_GRACE, NOW), 'startupDisabled');
  assert.equal(gameWarning(stopped, NOW - STOPPED_GRACE - 1, NOW), 'agentStopped');
  assert.equal(gameWarning(view({ agent: 'stopped' }), NOW - 5_000, NOW), null);
});

test('un agent arrêté n’inquiète pas quand le mode automatique est éteint', () => {
  assert.equal(gameWarning(view({ auto: false, agent: 'stopped' }), NOW - 60_000, NOW), null);
});

test('le démarrage désactivé dans Windows n’est signalé que si le mode automatique est allumé', () => {
  assert.equal(gameWarning(view({ startupDisabled: true }), null, NOW), 'startupDisabled');
  assert.equal(gameWarning(view({ auto: false, startupDisabled: true }), null, NOW), null);
});

test('la date d’arrêt de l’agent se note une fois, et s’efface dès qu’il repart', () => {
  assert.equal(stoppedSince(null, 'stopped', NOW), NOW);
  assert.equal(stoppedSince(NOW - 9_000, 'stopped', NOW), NOW - 9_000);
  assert.equal(stoppedSince(NOW - 9_000, 'running', NOW), null);
  assert.equal(stoppedSince(NOW - 9_000, 'missing', NOW), null);
  assert.equal(stoppedSince(null, 'running', NOW), null);
});

test('un arrêt de l’agent n’est noté que si le mode automatique est allumé', () => {
  const stopped = view({ auto: false, agent: 'stopped' });
  // Éteint : l'agent arrêté est son état normal, aucun délai de grâce ne court.
  assert.equal(noteStop(null, stopped, NOW), null);
  assert.equal(noteStop(NOW - 20_000, stopped, NOW), null);
  // Allumé : la date est notée, et garde sa valeur d'une vue à l'autre.
  assert.equal(noteStop(null, { ...stopped, auto: true }, NOW), NOW);
  assert.equal(noteStop(NOW - 9_000, { ...stopped, auto: true }, NOW), NOW - 9_000);
  assert.equal(noteStop(NOW - 9_000, view({ agent: 'running' }), NOW), null);
});

// ── Alimentation ───────────────────────────────────────────────────────────

test('la ligne d’alimentation porte un interrupteur seulement si Windows s’y prête', () => {
  assert.equal(powerRow(view()), 'switch');
  assert.equal(powerRow(view({ powerSupport: 'unavailable' })), 'unavailable');
  assert.equal(powerRow(view({ powerSupport: 'unsuited' })), 'unsuited');
});

// ── Apps en veilleuse ──────────────────────────────────────────────────────

const APPS: QuietApp[] = [
  app('onedrive.exe', 'OneDrive', { on: true, running: true }),
  app('dropbox.exe', 'Dropbox', { on: true }),
  app('googledrivefs.exe', 'Google Drive', { on: true }),
  app('chrome.exe', 'Google Chrome', { running: true }),
  app('firefox.exe', 'Firefox'),
  app('qbittorrent.exe', 'qBittorrent', { on: true, builtIn: false }),
];

test('les cases montrent les apps qui tournent et celles que la personne a ajoutées', () => {
  assert.deepEqual(
    shownApps(APPS).map((entry) => entry.exe),
    ['onedrive.exe', 'chrome.exe', 'qbittorrent.exe'],
  );
});

test('la phrase d’attente nomme les apps cochées d’office qui ne tournent pas', () => {
  assert.deepEqual(waitingApps(APPS), ['Dropbox', 'Google Drive']);
  assert.deepEqual(waitingApps(APPS.filter((entry) => entry.running)), []);
});

test('une app décochée d’office ne figure ni dans les cases ni dans la phrase d’attente', () => {
  const off = APPS.map((entry) => (entry.exe === 'dropbox.exe' ? { ...entry, on: false } : entry));
  assert.ok(!waitingApps(off).includes('Dropbox'));
  assert.ok(!shownApps(off).some((entry) => entry.exe === 'dropbox.exe'));
});

// ── Gestes optimistes ──────────────────────────────────────────────────────

test('un interrupteur change seulement son champ', () => {
  const base = view({ auto: false, power: true, quietApps: true });
  assert.deepEqual(withOption(base, 'auto', true), { ...base, auto: true });
  assert.deepEqual(withOption(base, 'power', false), { ...base, power: false });
  assert.deepEqual(withOption(base, 'quietApps', false), { ...base, quietApps: false });
  assert.equal(optionOf(base, 'auto'), false);
  assert.equal(optionOf(base, 'power'), true);
  assert.equal(optionOf(base, 'quietApps'), true);
  // La vue de départ n'est pas modifiée : de quoi défaire le geste.
  assert.equal(base.auto, false);
});

test('la case d’une app change seulement cette app', () => {
  const base = view({ apps: APPS });
  const next = withQuietApp(base, 'chrome.exe', true);
  assert.equal(next.apps.find((entry) => entry.exe === 'chrome.exe')?.on, true);
  assert.equal(next.apps.find((entry) => entry.exe === 'firefox.exe')?.on, false);
  assert.equal(base.apps.find((entry) => entry.exe === 'chrome.exe')?.on, false);
});

// ── Les jeux ───────────────────────────────────────────────────────────────

test('un jeu ajouté se retire, un jeu reconnu seul se refuse', () => {
  const added: KnownGame = { path: 'C:\\x\\javaw.exe', name: 'Minecraft', source: 'added', lastPlayed: null };
  assert.ok(isRemovable(added));
  assert.ok(!isRemovable({ ...added, source: 'library' }));
});

test('une app ajoutée par la personne se retire ; une app intégrée se coche', () => {
  assert.ok(isRemovableApp(app('qbittorrent.exe', 'qBittorrent', { builtIn: false })));
  assert.ok(!isRemovableApp(app('onedrive.exe', 'OneDrive')));
});

// ── « Tout remettre maintenant » ───────────────────────────────────────────

test('la partie remise en place se montre tout de suite, et se défait si le cœur refuse', () => {
  const playing = view({ session: session() });
  assert.equal(withReleased(playing, true).session?.released, true);
  assert.equal(withReleased(withReleased(playing, true), false).session?.released, false);
  // Sans partie, rien à changer.
  const idle = view();
  assert.equal(withReleased(idle, true), idle);
});

test('la page tient la partie pour remise en place le temps que l’agent le note, pas plus', () => {
  const playing = view({ session: session() });
  const expected: Releasing = { path: session().path, until: NOW + RELEASE_GRACE };
  // L'agent n'a pas encore réécrit son journal : la vue dément, la page ne revient pas en arrière.
  assert.ok(releaseExpected(playing, expected, NOW + 1_000));
  assert.equal(withExpectedRelease(playing, expected, NOW + 1_000).session?.released, true);
  // Le délai passé, la vue du cœur fait foi.
  assert.ok(!releaseExpected(playing, expected, NOW + RELEASE_GRACE));
  assert.equal(withExpectedRelease(playing, expected, NOW + RELEASE_GRACE), playing);
  // L'agent a rattrapé la demande, ou la partie est finie, ou c'en est une autre : rien à tenir.
  const done = view({ session: session({ released: true }) });
  assert.ok(!releaseExpected(done, expected, NOW + 1_000));
  assert.ok(!releaseExpected(view(), expected, NOW + 1_000));
  assert.ok(!releaseExpected(view({ session: session({ path: 'D:\\autre.exe' }) }), expected, NOW + 1_000));
  assert.ok(!releaseExpected(playing, null, NOW));
});

// ── Gestes en vol et focus ─────────────────────────────────────────────────

test('chaque geste a sa clé, et deux gestes sur deux jeux ne se gênent pas', () => {
  assert.notEqual(busyKey.status('C:\\a.exe'), busyKey.status('C:\\b.exe'));
  assert.notEqual(busyKey.status('C:\\a.exe'), busyKey.quiet('a.exe'));
  assert.notEqual(focusKey.game('C:\\a.exe'), focusKey.never('C:\\a.exe'));
});

test('quand une ligne disparaît, le focus va à la suivante, sinon à la précédente, sinon nulle part', () => {
  const rows = ['a', 'b', 'c'];
  assert.equal(neighbour(rows, 'a'), 'b');
  assert.equal(neighbour(rows, 'b'), 'c');
  assert.equal(neighbour(rows, 'c'), 'b');
  assert.equal(neighbour(['seule'], 'seule'), null);
  assert.equal(neighbour(rows, 'absente'), null);
  assert.equal(neighbour([], 'a'), null);
});
