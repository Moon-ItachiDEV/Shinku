// La page Mode jeu côté application : sondage, gestes, sélecteur.
//
// L'application est testée sans navigateur : un faux cœur dont chaque réponse
// se résout à la main (on décide dans quel ordre les réponses arrivent), des
// minuteries simulées (`mock.timers`), et un dessin qui ne dessine rien.

import assert from 'node:assert/strict';
import { afterEach, beforeEach, mock, test } from 'node:test';

import type { Backend, GameView, OpenApp } from '../src/backend/types.ts';
import { fr } from '../src/i18n/fr.ts';
import { GAME_POLL_DELAY, GESTURE_TIMEOUT, gameWarning, LEAVE_GRACE, STOPPED_GRACE } from '../src/model/game.ts';

// ── Un faux environnement de page ──────────────────────────────────────────

/** Les minuteries d'intervalle en cours : « une seule minuterie » se compte ici. */
const intervals = new Set<unknown>();
const environment = { hidden: false };

Object.assign(globalThis, {
  window: {
    matchMedia: () => ({ matches: false, addEventListener: () => {} }),
    setInterval: (handler: () => void, delay: number) => {
      const id = globalThis.setInterval(handler, delay);
      intervals.add(id);
      return id;
    },
    clearInterval: (id: unknown) => {
      intervals.delete(id);
      globalThis.clearInterval(id as ReturnType<typeof setInterval>);
    },
    setTimeout: (handler: () => void, delay: number) => globalThis.setTimeout(handler, delay),
    clearTimeout: (id: unknown) => globalThis.clearTimeout(id as ReturnType<typeof setTimeout>),
    addEventListener: () => {},
  },
  document: {
    get hidden() {
      return environment.hidden;
    },
    activeElement: null,
    body: {},
    // lit-html en a besoin à son import ; le dessin, lui, est remplacé par un dessin vide.
    createTreeWalker: () => ({}),
    addEventListener: () => {},
  },
});

// Le module de l'application lit `window` en s'important : il vient après les faux.
const { App } = await import('../src/app.ts');

// ── Un faux cœur dont les réponses se résolvent à la main ──────────────────

interface Call {
  name: string;
  args: unknown[];
  resolve: (value: unknown) => void;
  reject: (error: unknown) => void;
}

function fakeBackend(): { backend: Backend; calls: Call[]; of: (name: string) => Call[] } {
  const calls: Call[] = [];
  const backend = new Proxy(
    {},
    {
      get:
        (_target, name: string) =>
        (...args: unknown[]) =>
          new Promise((resolve, reject) => {
            calls.push({ name, args, resolve, reject });
          }),
    },
  ) as Backend;
  return { backend, calls, of: (name) => calls.filter((call) => call.name === name) };
}

const NOW = 1_794_770_000_000;
const HOLLOW_PEAKS = 'D:\\SteamLibrary\\steamapps\\common\\Hollow Peaks\\hp.exe';
const MINECRAFT = 'C:\\jdk\\bin\\javaw.exe';

function gameView(parts: Partial<GameView> = {}): GameView {
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
    games: [
      { path: MINECRAFT, name: 'Minecraft', source: 'added', lastPlayed: null },
      { path: HOLLOW_PEAKS, name: 'Hollow Peaks', source: 'library', lastPlayed: NOW - 3_600_000 },
    ],
    notGames: [],
    ...parts,
  };
}

function playing(released = false): GameView {
  return gameView({
    session: {
      name: 'Hollow Peaks',
      path: HOLLOW_PEAKS,
      since: NOW - 38 * 60_000,
      source: 'library',
      power: 'applied',
      quietedApps: 1,
      released,
      restorePending: false,
    },
  });
}

/** Les clés de focus des lignes des vues d'essai. */
const KEYS = [`game:${MINECRAFT}`, `game:${HOLLOW_PEAKS}`, `never:${HOLLOW_PEAKS}`, 'quiet:qbittorrent.exe', 'quiet:onedrive.exe'];

/** Laisse passer tout ce qui est en attente de promesses (la page se redessine dans une micro-tâche). */
const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

function start() {
  const fake = fakeBackend();
  // Les boutons où le focus peut aller (`data-fk`) : les faux se contentent de noter qu'on les a focalisés.
  const focused: string[] = [];
  const root = {
    querySelectorAll: () =>
      ['title', 'add-game', 'add-app', 'header-notgame'].concat(KEYS).map((fk) => ({
        dataset: { fk },
        focus: () => focused.push(fk),
      })),
  } as unknown as HTMLElement;
  let drawn = 0;
  const app = new App(fake.backend, root, () => (drawn += 1));
  const actions = app['actions'];
  return {
    ...fake,
    focused,
    draws: () => drawn,
    app,
    actions,
    state: () => app['state'],
    /** Ouvre la page et lui donne sa première vue. */
    async open(view: GameView = gameView()): Promise<void> {
      actions.navigate('gameMode');
      fake.of('gameView')[0]?.resolve(view);
      await flush();
    },
    /** Laisse passer un sondage, et lui donne la vue. */
    async poll(view: GameView): Promise<void> {
      const before = fake.of('gameView').length;
      mock.timers.tick(GAME_POLL_DELAY);
      await flush();
      const call = fake.of('gameView')[before];
      assert.ok(call !== undefined, 'un sondage devait partir');
      call.resolve(view);
      await flush();
    },
  };
}

beforeEach(() => {
  environment.hidden = false;
  intervals.clear();
  mock.timers.enable({ apis: ['setInterval', 'setTimeout', 'Date'], now: NOW });
});

afterEach(() => {
  mock.timers.reset();
});

// ── Les tests ──────────────────────────────────────────────────────────────

test('un interrupteur refusé par le cœur retrouve son ancienne valeur, et la personne le sait', async () => {
  const page = start();
  await page.open(gameView({ power: true }));

  page.actions.setGameOption('power', false);
  // Le changement se voit tout de suite.
  assert.equal(page.state().game?.power, false);

  page.of('setGameOption')[0]?.reject({ kind: 'settingsNotSaved', reason: 'disque plein' });
  await flush();
  assert.equal(page.state().game?.power, true);
  assert.equal(page.state().toast, fr.toast.settingsNotSaved);
  assert.equal(page.state().toastShown, true);
  // La page relit alors la vue du cœur.
  assert.equal(page.of('gameView').length, 2);
});

test('une vue demandée avant un geste et arrivée après lui est ignorée', async () => {
  const page = start();
  await page.open(gameView({ auto: false }));

  // Un sondage part, puis la personne allume le mode.
  mock.timers.tick(GAME_POLL_DELAY);
  await flush();
  const oldPoll = page.of('gameView')[1];
  assert.ok(oldPoll !== undefined);
  page.actions.setGameOption('auto', true);
  assert.equal(page.state().game?.auto, true);

  // L'ancienne vue arrive : elle ne défait rien.
  oldPoll.resolve(gameView({ auto: false }));
  await flush();
  assert.equal(page.state().game?.auto, true);

  // La réponse du geste, elle, est adoptée.
  page.of('setGameOption')[0]?.resolve(gameView({ auto: true, agent: 'running' }));
  await flush();
  assert.equal(page.state().game?.auto, true);
  assert.equal(page.state().game?.agent, 'running');
});

test('aucun sondage ne part ni n’est adopté pendant un geste ; le suivant, après sa réponse, l’est', async () => {
  const page = start();
  await page.open(gameView({ auto: false }));

  page.actions.setGameOption('auto', true);
  const asked = page.of('gameView').length;
  mock.timers.tick(GAME_POLL_DELAY * 3);
  await flush();
  // Le cœur traite les gestes en parallèle des sondages : un sondage lu avant l'écriture dirait l'ancien réglage.
  assert.equal(page.of('gameView').length, asked, 'rien n’est demandé pendant le geste');

  page.of('setGameOption')[0]?.resolve(gameView({ auto: true }));
  await flush();
  await page.poll(gameView({ auto: true, agent: 'stopped' }));
  assert.equal(page.state().game?.agent, 'stopped', 'une fois le geste répondu, les sondages comptent de nouveau');
});

test('une vue de sondage partie avant la réponse d’un geste ne la défait pas', async () => {
  const page = start();
  await page.open(gameView({ auto: false }));

  // Le sondage part ; le geste part ; le geste répond ; puis le sondage arrive.
  mock.timers.tick(GAME_POLL_DELAY);
  await flush();
  const poll = page.of('gameView')[1];
  assert.ok(poll !== undefined);
  page.actions.setGameOption('auto', true);
  page.of('setGameOption')[0]?.resolve(gameView({ auto: true }));
  await flush();
  poll.resolve(gameView({ auto: false }));
  await flush();
  assert.equal(page.state().game?.auto, true);
});

test('trois allers-retours sur la page : une seule minuterie de deux secondes', async () => {
  const page = start();
  for (let visit = 0; visit < 3; visit += 1) {
    page.actions.navigate('gameMode');
    assert.equal(intervals.size, 1, `visite ${visit + 1}`);
    // Une deuxième demande de la même page (un clic sur son entrée) n'ajoute rien.
    page.actions.navigate('gameMode');
    assert.equal(intervals.size, 1);
    page.actions.navigate('home');
    assert.equal(intervals.size, 0);
  }
  page.actions.navigate('gameMode');
  await flush();
  const before = page.calls.filter((call) => call.name === 'gameView').length;
  mock.timers.tick(GAME_POLL_DELAY);
  await flush();
  // Une minuterie, donc au plus une demande par tour (les précédentes sont toujours en attente : aucune nouvelle).
  assert.ok(page.calls.filter((call) => call.name === 'gameView').length - before <= 1);
  assert.equal(intervals.size, 1);
  page.actions.navigate('home');
  assert.equal(intervals.size, 0);
});

test('une fenêtre cachée ne demande rien', async () => {
  const page = start();
  await page.open();
  environment.hidden = true;
  const asked = page.of('gameView').length;
  mock.timers.tick(GAME_POLL_DELAY * 4);
  await flush();
  assert.equal(page.of('gameView').length, asked);
  environment.hidden = false;
  mock.timers.tick(GAME_POLL_DELAY);
  await flush();
  assert.equal(page.of('gameView').length, asked + 1);
});

test('la liste du sélecteur arrivée après Échap est ignorée', async () => {
  const page = start();
  await page.open();
  const roblox: OpenApp = { id: 1, name: 'Roblox', path: 'C:\\roblox.exe' };
  const notepad: OpenApp = { id: 1, name: 'Notepad++', path: 'C:\\notepad++.exe' };

  page.actions.openPicker('game');
  assert.deepEqual(page.state().picker, { kind: 'game', apps: null });
  page.actions.closePicker();
  assert.equal(page.state().picker, null);
  page.of('openApps')[0]?.resolve([roblox]);
  await flush();
  assert.equal(page.state().picker, null, 'la liste arrivée trop tard ne rouvre rien');

  // Échap, puis réouverture tout de suite : seule la seconde liste compte, même arrivée la première.
  page.actions.openPicker('app');
  page.actions.closePicker();
  page.actions.openPicker('app');
  page.of('openApps')[2]?.resolve([notepad]);
  await flush();
  page.of('openApps')[1]?.resolve([roblox]);
  await flush();
  assert.deepEqual(page.state().picker, { kind: 'app', apps: [notepad] });
});

test('le choix du sélecteur part avec son numéro et son chemin, une seule fois, et la boîte se ferme à la réponse', async () => {
  const page = start();
  await page.open();
  const roblox: OpenApp = { id: 3, name: 'Roblox', path: 'C:\\roblox.exe' };
  page.actions.openPicker('game');
  page.of('openApps')[0]?.resolve([roblox]);
  await flush();

  page.actions.pickOpenApp(3);
  page.actions.pickOpenApp(3);
  assert.equal(page.of('addGame').length, 1);
  assert.deepEqual(page.of('addGame')[0]?.args, [3, 'C:\\roblox.exe']);
  // En vol, la boîte reste et ses choix sont grisés (la page lit gameBusy).
  assert.notEqual(page.state().picker, null);
  assert.ok(page.state().gameBusy.length > 0);

  page.of('addGame')[0]?.resolve(gameView());
  await flush();
  assert.equal(page.state().picker, null);
  assert.deepEqual(page.state().gameBusy, []);
});

test('un choix refusé ferme la boîte et le dit', async () => {
  const page = start();
  await page.open();
  page.actions.openPicker('app');
  page.of('openApps')[0]?.resolve([{ id: 1, name: 'Battle.net', path: 'C:\\bn.exe' }]);
  await flush();
  page.actions.pickOpenApp(1);
  assert.deepEqual(page.of('addQuietApp')[0]?.args, [1, 'C:\\bn.exe']);
  page.of('addQuietApp')[0]?.reject({ kind: 'gameRefused', reason: 'protectedApp' });
  await flush();
  assert.equal(page.state().picker, null);
  assert.equal(page.state().toast, fr.game.toast.protectedApp);
});

test('un double clic sur un geste n’envoie qu’un ordre, et le bouton se libère à la réponse', async () => {
  const page = start();
  await page.open(playing());

  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  assert.equal(page.of('setGameStatus').length, 1);
  assert.equal(page.state().gameBusy.length, 1);
  // Un autre jeu n'attend pas celui-là.
  page.actions.setGameStatus(MINECRAFT, 'forgotten');
  assert.equal(page.of('setGameStatus').length, 2);

  page.of('setGameStatus')[0]?.resolve(playing());
  page.of('setGameStatus')[1]?.resolve(playing());
  await flush();
  assert.deepEqual(page.state().gameBusy, []);
  // Libéré, le geste peut repartir.
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  assert.equal(page.of('setGameStatus').length, 3);
});

test('« Tout remettre maintenant » : la partie se montre remise en place tout de suite, un seul ordre part', async () => {
  const page = start();
  await page.open(playing());

  page.actions.releaseGame();
  page.actions.releaseGame();
  assert.equal(page.of('releaseGame').length, 1);
  assert.equal(page.state().game?.session?.released, true);

  // L'agent n'a pas encore écrit son journal : la réponse dit encore « pas remis en place ».
  page.of('releaseGame')[0]?.resolve(playing(false));
  await flush();
  assert.equal(page.state().game?.session?.released, true);
  assert.deepEqual(page.state().gameBusy, []);

  // Le sondage suivant le dément encore : la page tient bon pendant le délai de grâce.
  await page.poll(playing(false));
  assert.equal(page.state().game?.session?.released, true);
  // L'agent a rattrapé : la vue du cœur.
  await page.poll(playing(true));
  assert.equal(page.state().game?.session?.released, true);
  // Une fois rattrapé, une vue qui redit le contraire n'est plus contredite.
  await page.poll(playing(false));
  assert.equal(page.state().game?.session?.released, false);
});

test('« Tout remettre maintenant » que l’agent ne note jamais : la vue du cœur reprend la main après le délai', async () => {
  const page = start();
  await page.open(playing());
  page.actions.releaseGame();
  page.of('releaseGame')[0]?.resolve(playing(false));
  await flush();
  // Deux sondages (4 s) : dans le délai de grâce, la page tient encore la partie pour remise en place.
  await page.poll(playing(false));
  await page.poll(playing(false));
  assert.equal(page.state().game?.session?.released, true);
  // Deux de plus (8 s) : l'agent n'a rien noté, la vue du cœur fait foi.
  await page.poll(playing(false));
  await page.poll(playing(false));
  assert.equal(page.state().game?.session?.released, false);
});

test('« Tout remettre maintenant » refusé : la partie n’est plus dite remise en place', async () => {
  const page = start();
  await page.open(playing());
  page.actions.releaseGame();
  page.of('releaseGame')[0]?.reject({ kind: 'settingsNotSaved', reason: 'x' });
  await flush();
  assert.equal(page.state().game?.session?.released, false);
  assert.deepEqual(page.state().gameBusy, []);
  // Le sondage suivant n'est pas corrigé par un ordre oublié.
  page.of('gameView')[1]?.resolve(playing(false));
  await flush();
  assert.equal(page.state().game?.session?.released, false);
});

test('l’arrêt de l’agent noté pendant que le mode est éteint ne donne pas d’avertissement à l’allumage', async () => {
  const page = start();
  const off = gameView({ auto: false, agent: 'stopped' });
  await page.open(off);
  // Plus de 15 secondes d'agent arrêté, mode éteint : l'état normal.
  for (let turn = 0; turn < 9; turn += 1) {
    await page.poll(off);
  }
  assert.equal(page.state().gameStoppedSince, null);

  page.actions.setGameOption('auto', true);
  page.of('setGameOption')[0]?.resolve(gameView({ auto: true, agent: 'stopped' }));
  await flush();
  const view = page.state().game;
  assert.ok(view !== null);
  assert.equal(gameWarning(view, page.state().gameStoppedSince, page.state().now), null);

  // Le délai de grâce court à partir de là.
  for (let turn = 0; turn < 8; turn += 1) {
    await page.poll(gameView({ auto: true, agent: 'stopped' }));
  }
  const later = page.state();
  assert.ok(later.now - (later.gameStoppedSince ?? later.now) > STOPPED_GRACE);
  assert.equal(gameWarning(gameView({ auto: true, agent: 'stopped' }), later.gameStoppedSince, later.now), 'agentStopped');
});

test('quitter la page efface la vue et l’arrêt noté : la visite suivante repart de zéro', async () => {
  const page = start();
  const stopped = gameView({ auto: true, agent: 'stopped' });
  await page.open(stopped);
  await page.poll(stopped);
  assert.notEqual(page.state().gameStoppedSince, null);

  page.actions.navigate('home');
  assert.equal(page.state().game, null);
  assert.equal(page.state().gameStoppedSince, null);

  mock.timers.tick(16_000);
  page.actions.navigate('gameMode');
  assert.equal(page.state().game, null, 'rien de la visite précédente en attendant la première vue');
  page.of('gameView').at(-1)?.resolve(stopped);
  await flush();
  const view = page.state().game;
  assert.ok(view !== null);
  assert.equal(gameWarning(view, page.state().gameStoppedSince, page.state().now), null);
});

test('la réponse d’un geste arrivée après avoir quitté la page ne remplit pas la page suivante', async () => {
  const page = start();
  await page.open(gameView({ auto: false }));
  page.actions.setGameOption('auto', true);
  page.actions.navigate('home');
  page.of('setGameOption')[0]?.resolve(gameView({ auto: true }));
  await flush();
  assert.equal(page.state().game, null);
});

test('après trois échecs de suite de la vue, la page le dit ; elle continue de sonder', async () => {
  const page = start();
  page.actions.navigate('gameMode');
  page.of('gameView')[0]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().gameFailed, false);

  for (let turn = 1; turn <= 2; turn += 1) {
    mock.timers.tick(GAME_POLL_DELAY);
    await flush();
    page.of('gameView')[turn]?.reject(new Error('ipc'));
    await flush();
  }
  assert.equal(page.state().game, null);
  assert.equal(page.state().gameFailed, true);

  // Le cœur répond enfin : la note s'efface.
  await page.poll(gameView());
  assert.equal(page.state().gameFailed, false);
  assert.notEqual(page.state().game, null);
});

test('retirer une app ajoutée n’affiche rien avant la réponse, et son bouton attend', async () => {
  const page = start();
  const added = { exe: 'qbittorrent.exe', label: 'qBittorrent', on: true, running: true, builtIn: false };
  const view = gameView({ apps: [added] });
  await page.open(view);

  page.actions.setQuietApp('qbittorrent.exe', false);
  page.actions.setQuietApp('qbittorrent.exe', false);
  assert.equal(page.of('setQuietApp').length, 1);
  assert.equal(page.state().game?.apps.length, 1, 'la ligne est encore là, sans case décochée');
  assert.equal(page.state().game?.apps[0]?.on, true);
  page.of('setQuietApp')[0]?.resolve(gameView({ apps: [] }));
  await flush();
  assert.equal(page.state().game?.apps.length, 0);
});

// ── Deux gestes qui se croisent ────────────────────────────────────────────

test('deux gestes dont les réponses arrivent dans le désordre : la vue finale est celle du dernier geste parti', async () => {
  for (const order of [
    [1, 0],
    [0, 1],
  ] as const) {
    const page = start();
    await page.open(gameView({ power: true, quietApps: true }));
    page.actions.setGameOption('power', false);
    page.actions.setGameOption('quietApps', false);
    const first = gameView({ power: false, quietApps: true, agent: 'stopped' });
    const last = gameView({ power: false, quietApps: false, agent: 'running' });
    const answers = [first, last];
    for (const index of order) {
      page.of('setGameOption')[index]?.resolve(answers[index]);
      await flush();
    }
    // Le geste parti en dernier fait foi, quel que soit l'ordre des réponses.
    assert.equal(page.state().game?.agent, 'running', `ordre ${order.join('-')}`);
    assert.equal(page.state().game?.quietApps, false);
    assert.deepEqual(page.state().gameBusy, []);
    page.actions.navigate('home');
  }
});

// ── Un geste sans réponse ──────────────────────────────────────────────────

test('un geste resté sans réponse dix secondes est abandonné : bouton libéré, sondage repris, réponse tardive ignorée', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(MINECRAFT, 'forgotten');
  assert.deepEqual(page.state().gameBusy, [`status:${MINECRAFT}`]);
  const asked = page.of('gameView').length;
  mock.timers.tick(9_000);
  await flush();
  assert.equal(page.of('gameView').length, asked, 'avant le délai, le sondage attend toujours');
  assert.equal(page.state().gameBusy.length, 1);

  mock.timers.tick(1_000);
  await flush();
  // Le geste est abandonné : plus occupé, et la page relit tout de suite la vue.
  assert.deepEqual(page.state().gameBusy, []);
  assert.equal(page.of('gameView').length, asked + 1);
  page.of('gameView')[asked]?.resolve(playing());
  await flush();

  // Sa réponse, arrivée bien plus tard, ne change rien.
  page.of('setGameStatus')[0]?.resolve(gameView({ auto: false }));
  await flush();
  assert.equal(page.state().game?.auto, true);
  assert.deepEqual(page.state().gameBusy, []);
  // Le sondage tourne de nouveau, et le bouton peut repartir.
  await page.poll(playing());
  page.actions.setGameStatus(MINECRAFT, 'forgotten');
  assert.equal(page.of('setGameStatus').length, 2);
});

test('une réponse arrivée avant le délai annule l’abandon', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(MINECRAFT, 'forgotten');
  page.of('setGameStatus')[0]?.resolve(playing());
  await flush();
  const asked = page.of('gameView').length;
  mock.timers.tick(10_000);
  await flush();
  // Seuls les sondages ordinaires (toutes les 2 s) sont partis, pas un rattrapage de plus.
  assert.ok(page.of('gameView').length - asked <= 1);
  assert.deepEqual(page.state().gameBusy, []);
});

// ── « Ce n'est pas un jeu » sur la partie en cours ─────────────────────────

/** La partie en cours, une fois « Ce n'est pas un jeu » enregistré par le cœur (l'agent ne l'a pas encore terminée). */
const left = (parts: Partial<GameView> = {}): GameView => ({
  ...playing(),
  notGames: [{ path: HOLLOW_PEAKS, name: 'Hollow Peaks' }],
  ...parts,
});

test('« Ce n’est pas un jeu » : l’en-tête attend, puis le focus va au titre quand la partie a disparu', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  assert.equal(page.state().gameLeaving?.path, HOLLOW_PEAKS);
  page.of('setGameStatus')[0]?.resolve(left());
  await flush();
  // Le cœur a répondu mais l'agent n'a pas fini : l'en-tête attend toujours, rien ne bouge.
  assert.equal(page.state().gameLeaving?.path, HOLLOW_PEAKS);
  assert.deepEqual(page.focused, []);
  await page.poll(left());
  assert.equal(page.state().gameLeaving?.path, HOLLOW_PEAKS);

  // La partie n'est plus dans la vue : l'attente tombe, le focus va au titre.
  await page.poll(gameView({ notGames: left().notGames }));
  assert.equal(page.state().gameLeaving, null);
  assert.deepEqual(page.focused, ['title']);
});

test('« Ce n’est pas un jeu » refusé : l’en-tête reprend son état, le message s’affiche, le bouton retrouve le focus', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  assert.equal(page.state().gameLeaving?.path, HOLLOW_PEAKS);
  page.of('setGameStatus')[0]?.reject({ kind: 'gameRefused', reason: 'unknownGame' });
  await flush();
  assert.equal(page.state().gameLeaving, null);
  assert.equal(page.state().toast, fr.game.toast.unknownGame);
  assert.deepEqual(page.focused, ['header-notgame']);
});

test('l’attente de l’en-tête tombe quand la remise en place échoue : l’avertissement n’est pas caché', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  page.of('setGameStatus')[0]?.resolve(left());
  await flush();
  assert.equal(page.state().gameLeaving?.path, HOLLOW_PEAKS);

  // L'agent ferme la partie, mais la remise en place échoue : la session reste, en attente.
  const pending = left();
  await page.poll({ ...pending, session: { ...pending.session!, restorePending: true } });
  assert.equal(page.state().gameLeaving, null, 'plus d’attente : la page dit que la remise en place a échoué');
  assert.deepEqual(page.focused, ['header-notgame']);
});

test('l’attente de l’en-tête tombe si l’agent s’arrête', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  page.of('setGameStatus')[0]?.resolve(left());
  await flush();
  await page.poll(left({ agent: 'stopped' }));
  assert.equal(page.state().gameLeaving, null);
  assert.deepEqual(page.focused, ['header-notgame']);
});

test('l’attente de l’en-tête tombe quand le geste est abandonné faute de réponse', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  assert.equal(page.state().gameLeaving?.path, HOLLOW_PEAKS);
  mock.timers.tick(GESTURE_TIMEOUT);
  await flush();
  assert.equal(page.state().gameLeaving, null);
  assert.deepEqual(page.state().gameBusy, []);
  assert.deepEqual(page.focused, ['header-notgame']);
});

test('l’attente de l’en-tête tombe au bout du délai, même si le sondage est arrêté, et la page est redessinée', async () => {
  const page = start();
  await page.open(playing());
  page.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  page.of('setGameStatus')[0]?.resolve(left());
  await flush();
  // Le sondage ne répond plus (fenêtre cachée) : seul le délai peut finir l'attente.
  environment.hidden = true;
  const drawn = page.draws();
  mock.timers.tick(LEAVE_GRACE - 1_000);
  await flush();
  assert.equal(page.state().gameLeaving?.path, HOLLOW_PEAKS);
  mock.timers.tick(1_000);
  await flush();
  assert.equal(page.state().gameLeaving, null);
  assert.ok(page.draws() > drawn, 'la page est redessinée quand le délai est atteint');
  assert.deepEqual(page.focused, ['header-notgame']);
});

// ── Le focus quand une ligne disparaît ─────────────────────────────────────

test('« Retirer » un jeu : le focus va à la ligne qui suit, sinon à celle qui précède, sinon au bouton d’ajout', async () => {
  const page = start();
  await page.open(gameView());
  page.actions.setGameStatus(MINECRAFT, 'forgotten');
  page.of('setGameStatus')[0]?.resolve(gameView({ games: [gameView().games[1]!] }));
  await flush();
  assert.deepEqual(page.focused, [`game:${HOLLOW_PEAKS}`]);

  // Dernière ligne : celle d'avant. Seule ligne : le bouton d'ajout.
  const other = start();
  await other.open(gameView());
  other.actions.setGameStatus(HOLLOW_PEAKS, 'notGame');
  other.of('setGameStatus')[0]?.resolve(gameView());
  await flush();
  assert.deepEqual(other.focused, [`game:${MINECRAFT}`]);

  const alone = start();
  await alone.open(gameView({ games: [gameView().games[0]!] }));
  alone.actions.setGameStatus(MINECRAFT, 'forgotten');
  alone.of('setGameStatus')[0]?.resolve(gameView({ games: [] }));
  await flush();
  assert.deepEqual(alone.focused, ['add-game']);
});

test('« Rétablir » : le focus va à la ligne voisine des jeux écartés, sinon au bouton d’ajout', async () => {
  const page = start();
  await page.open(gameView({ notGames: [{ path: HOLLOW_PEAKS, name: 'Hollow Peaks' }] }));
  page.actions.setGameStatus(HOLLOW_PEAKS, 'forgotten');
  page.of('setGameStatus')[0]?.resolve(gameView());
  await flush();
  assert.deepEqual(page.focused, ['add-game']);
});

test('« Tout remettre maintenant » : le focus va au bouton « Ce n’est pas un jeu » de l’en-tête', async () => {
  const page = start();
  await page.open(playing());
  page.actions.releaseGame();
  await flush();
  assert.deepEqual(page.focused, ['header-notgame']);
});

test('retirer une app ajoutée : le focus va à l’app voisine, sinon au bouton d’ajout', async () => {
  const page = start();
  const added = { exe: 'qbittorrent.exe', label: 'qBittorrent', on: true, running: true, builtIn: false };
  const builtIn = { exe: 'onedrive.exe', label: 'OneDrive', on: true, running: true, builtIn: true };
  await page.open(gameView({ apps: [builtIn, added] }));
  page.actions.setQuietApp('qbittorrent.exe', false);
  page.of('setQuietApp')[0]?.resolve(gameView({ apps: [builtIn] }));
  await flush();
  assert.deepEqual(page.focused, ['quiet:onedrive.exe']);

  const alone = start();
  await alone.open(gameView({ apps: [added] }));
  alone.actions.setQuietApp('qbittorrent.exe', false);
  alone.of('setQuietApp')[0]?.resolve(gameView({ apps: [] }));
  await flush();
  assert.deepEqual(alone.focused, ['add-app']);
});

test('deux gestes croisés : la vue est relue dès que le dernier a répondu, sans attendre le sondage', async () => {
  const page = start();
  await page.open(gameView({ power: true, quietApps: true }));
  page.actions.setGameOption('power', false);
  page.actions.setGameOption('quietApps', false);
  const asked = page.of('gameView').length;
  page.of('setGameOption')[1]?.resolve(gameView({ power: true, quietApps: false }));
  await flush();
  assert.equal(page.of('gameView').length, asked, 'un geste est encore en vol : pas de relecture');
  page.of('setGameOption')[0]?.resolve(gameView({ power: false, quietApps: true }));
  await flush();
  // Le dernier a répondu : la vue est redemandée tout de suite (le cœur a pu les appliquer dans l'autre ordre).
  assert.equal(page.of('gameView').length, asked + 1);
  page.of('gameView')[asked]?.resolve(gameView({ power: false, quietApps: false }));
  await flush();
  assert.equal(page.state().game?.power, false);
  assert.equal(page.state().game?.quietApps, false);

  // Un geste seul n'ajoute pas de relecture.
  const before = page.of('gameView').length;
  page.actions.setGameOption('power', true);
  page.of('setGameOption')[2]?.resolve(gameView({ power: true, quietApps: false }));
  await flush();
  assert.equal(page.of('gameView').length, before);
});
