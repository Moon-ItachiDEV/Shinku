// La page Désinstallation côté application : chargement, sondage de 2 s (jamais
// empilé, arrêté hors attente), gestes optimistes, refus, réponses en retard, focus.
//
// Comme pour le mode jeu, l'application est testée sans navigateur : un faux
// cœur dont chaque réponse se résout à la main, des minuteries simulées, et un
// dessin qui ne dessine rien.

import assert from 'node:assert/strict';
import { afterEach, beforeEach, mock, test } from 'node:test';

import type { AppRow, AppsView, Backend, FollowView, LeftoverRow, RemoveLeftovers } from '../src/backend/types.ts';
import { APPS_POLL_DELAY, APPS_RELOAD_GAP, uninstallFocus } from '../src/model/uninstall.ts';
import { fr } from '../src/i18n/fr.ts';
import { IDLE_SESSION } from '../src/state.ts';

// ── Un faux environnement de page ──────────────────────────────────────────

const intervals = new Set<unknown>();
const listeners = new Map<string, () => void>();
const environment = {
  hidden: false,
  active: null as unknown,
  /** Les prochaines lectures de `document.activeElement`, une par une, avant de revenir à `active`. */
  sequence: [] as unknown[],
  /** Le document a le focus système ; faux quand la fenêtre est passée derrière une autre. */
  windowFocused: true,
};

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
    addEventListener: (name: string, handler: () => void) => listeners.set(`window:${name}`, handler),
  },
  document: {
    get hidden() {
      return environment.hidden;
    },
    get activeElement() {
      return environment.sequence.length > 0 ? environment.sequence.shift() : environment.active;
    },
    hasFocus: () => environment.windowFocused,
    documentElement: { dataset: {} },
    body: {},
    createTreeWalker: () => ({}),
    addEventListener: (name: string, handler: () => void) => listeners.set(`document:${name}`, handler),
  },
});

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

function app(id: string, parts: Partial<AppRow> = {}): AppRow {
  return { id, name: id, scope: 'user', kind: 'app', removal: 'offered', ...parts } as AppRow;
}

const DISCORD = app('a-discord', { name: 'Discord' });
const STEAM = app('a-steam', { name: 'Steam' });

function view(follow: FollowView = { phase: 'idle' }, apps: AppRow[] = [DISCORD, STEAM]): AppsView {
  return { apps, complete: true, follow };
}

const waiting = (seconds = 0): FollowView => ({ phase: 'waiting', id: 'a-discord', name: 'Discord', seconds });
const settling: FollowView = { phase: 'settling', id: 'a-discord', name: 'Discord' };
const left = (key: number, path: string): LeftoverRow => ({ key, kind: 'folder', path, bytes: 1000 });
const removed = (leftovers: LeftoverRow[]): FollowView => ({
  phase: 'removed',
  id: 'a-discord',
  name: 'Discord',
  leftovers,
  family: false,
  complete: true,
});

function fakeButton(key: string) {
  return { dataset: { fk: key }, focus: mock.fn(), closest: () => ({ scrollIntoView: () => {} }) };
}

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

function start(buttons: Array<ReturnType<typeof fakeButton>> = []) {
  const fake = fakeBackend();
  const root = { querySelectorAll: () => buttons, querySelector: () => null } as unknown as HTMLElement;
  const app = new App(fake.backend, root, () => {});
  const actions = app['actions'];
  return {
    ...fake,
    app,
    actions,
    state: () => app['state'],
    async boot(): Promise<void> {
      const started = app.start();
      fake.of('overview')[0]?.resolve({ settings: { theme: 'system' } });
      fake.of('sessionView')[0]?.resolve(IDLE_SESSION);
      await started;
      await flush();
    },
    async open(first: AppsView = view()): Promise<void> {
      actions.navigate('uninstall');
      fake.of('appsView').at(-1)?.resolve(first);
      await flush();
    },
    focusWindow: (): void => listeners.get('window:focus')?.(),
    /** Laisse passer un tour du sondage et rend la demande qu'il a faite, ou `undefined`. */
    async tick(): Promise<Call | undefined> {
      const before = fake.of('appsView').length;
      mock.timers.tick(APPS_POLL_DELAY);
      await flush();
      return fake.of('appsView').length > before ? fake.of('appsView').at(-1) : undefined;
    },
  };
}

beforeEach(() => {
  environment.hidden = false;
  environment.active = null;
  environment.sequence = [];
  environment.windowFocused = true;
  intervals.clear();
  listeners.clear();
  mock.timers.enable({ apis: ['setInterval', 'setTimeout', 'Date'], now: NOW });
});

afterEach(() => {
  mock.timers.reset();
});

test('la vue se charge une fois à l’arrivée, sans sondage tant que rien ne se désinstalle', async () => {
  const page = start();
  page.actions.navigate('uninstall');
  assert.equal(page.of('appsView').length, 1);
  assert.equal(page.state().apps, null);
  page.of('appsView')[0]?.resolve(view());
  await flush();
  assert.equal(page.state().apps?.apps.length, 2);
  mock.timers.tick(10 * 60_000);
  await flush();
  assert.equal(page.of('appsView').length, 1);
});

test('une vue qui n’arrive pas : la page le dit, « Réessayer » relit', async () => {
  const page = start();
  await page.boot();
  page.actions.navigate('uninstall');
  page.of('appsView')[0]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().appsFailed, true);
  page.actions.retryApps();
  assert.equal(page.state().appsFailed, false);
  page.of('appsView')[1]?.resolve(view());
  await flush();
  assert.equal(page.state().apps?.apps.length, 2);
});

test('au retour du focus la vue est relue, sauf hors page, fenêtre cachée ou demande trop proche', async () => {
  const page = start();
  await page.boot();
  page.focusWindow();
  assert.equal(page.of('appsView').length, 0);
  await page.open();
  page.focusWindow();
  assert.equal(page.of('appsView').length, 1, 'trop proche de la dernière lecture');
  mock.timers.tick(APPS_RELOAD_GAP);
  environment.hidden = true;
  page.focusWindow();
  assert.equal(page.of('appsView').length, 1);
  environment.hidden = false;
  page.focusWindow();
  assert.equal(page.of('appsView').length, 2);
  page.actions.navigate('home');
  mock.timers.tick(APPS_RELOAD_GAP);
  page.focusWindow();
  assert.equal(page.of('appsView').length, 2);
});

test('« Désinstaller » demande confirmation, puis montre launching tout de suite, et ne part qu’une fois', async () => {
  const page = start();
  await page.open();
  page.actions.askUninstall('a-discord');
  assert.equal(page.state().appsConfirm?.id, 'a-discord');
  void page.actions.confirmUninstall();
  void page.actions.confirmUninstall();
  assert.equal(page.of('startUninstall').length, 1, 'un geste en vol n’est pas renvoyé');
  assert.deepEqual(page.of('startUninstall')[0]?.args, ['a-discord'], 'seul l’id part');
  assert.equal(page.state().apps?.follow.phase, 'launching');
  assert.equal(page.state().appsBusy, 'start');
  assert.match(page.state().appsAnnounce, /Discord/);
  // Une demande « rien en cours » arrivée pendant le lancement est ignorée.
  const poll = await page.tick();
  poll?.resolve(view());
  await flush();
  assert.equal(page.state().apps?.follow.phase, 'launching');
  page.of('startUninstall')[0]?.resolve(view(waiting()));
  await flush();
  assert.equal(page.state().appsBusy, null);
  assert.equal(page.state().apps?.follow.phase, 'waiting');
});

test('le sondage ne s’empile jamais : une demande à la fois, la suivante deux secondes après la réponse', async () => {
  const page = start();
  await page.open(view(waiting()));
  const first = await page.tick();
  assert.ok(first);
  mock.timers.tick(10 * APPS_POLL_DELAY);
  await flush();
  assert.equal(page.of('appsView').length, 2, 'rien de plus tant que la réponse n’est pas là');
  first.resolve(view(waiting(2)));
  await flush();
  assert.equal(page.of('appsView').length, 2);
  const second = await page.tick();
  assert.ok(second);
  second.resolve(view(settling));
  await flush();
  assert.ok(await page.tick(), 'settling sonde encore');
});

test('le sondage s’arrête dès que la phase n’est plus launching, waiting ou settling', async () => {
  const page = start();
  await page.open(view(waiting()));
  (await page.tick())?.resolve(view(removed([left(0, 'C:\\Users\\x\\Discord')])));
  await flush();
  assert.equal(await page.tick(), undefined);
  assert.equal(await page.tick(), undefined);
  // Et l'idle l'arrête aussi.
  const other = start();
  await other.open(view(waiting()));
  (await other.tick())?.resolve(view());
  await flush();
  assert.equal(await other.tick(), undefined);
});

test('quitter la page pendant l’attente garde le sondage ; la retrouver rend la scène', async () => {
  const page = start();
  await page.open(view(waiting()));
  page.actions.navigate('home');
  assert.ok(await page.tick(), 'l’attente continue ailleurs');
  page.actions.navigate('uninstall');
  assert.equal(page.state().apps?.follow.phase, 'waiting', 'la scène revient sans liste vide entre-temps');
});

test('quitter la page hors attente oublie tout, requête et tri compris', async () => {
  const page = start();
  await page.open();
  page.actions.setAppsQuery('dis');
  page.actions.setAppsSort('size');
  page.actions.navigate('home');
  assert.equal(page.state().apps, null);
  assert.equal(page.state().appsQuery, '');
  assert.equal(page.state().appsSort, 'name');
});

test('un refus du cœur : le toast, la vue relue, le bouton de la ligne retrouve le focus', async () => {
  const button = fakeButton(uninstallFocus.row('a-discord'));
  const page = start([button]);
  await page.open();
  page.actions.askUninstall('a-discord');
  void page.actions.confirmUninstall();
  page.of('startUninstall')[0]?.reject({ kind: 'uninstallRefused', reason: 'broken' });
  await flush();
  assert.equal(page.state().apps?.follow.phase, 'idle');
  assert.equal(page.state().appsBusy, null);
  assert.ok(page.state().toast, 'un toast dit pourquoi');
  page.of('appsView').at(-1)?.resolve(view());
  await flush();
  assert.equal(button.focus.mock.callCount() >= 1, true);
});

test('une annulation dans la fenêtre de Windows ne fait aucun bruit', async () => {
  const page = start();
  await page.open();
  page.actions.askUninstall('a-discord');
  void page.actions.confirmUninstall();
  page.of('startUninstall')[0]?.reject({ kind: 'uninstallRefused', reason: 'cancelled' });
  await flush();
  assert.ok(!page.state().toast);
});

test('une vue en retard ne remplace pas celle d’un geste plus récent', async () => {
  const page = start();
  await page.open(view(waiting()));
  const late = await page.tick();
  void page.actions.stopUninstall();
  assert.equal(page.of('stopUninstall').length, 1);
  page.of('stopUninstall')[0]?.resolve(view());
  await flush();
  late?.resolve(view(waiting(9)));
  await flush();
  assert.equal(page.state().apps?.follow.phase, 'idle');
});

test('la corbeille : seules les clés cochées partent, un second geste en vol est ignoré', async () => {
  const page = start();
  await page.open(view(removed([left(0, 'C:\\a'), left(1, 'C:\\b')])));
  page.actions.removeLeftovers();
  assert.equal(page.of('removeLeftovers').length, 0, 'rien de coché, rien ne part');
  page.actions.toggleLeftover('C:\\b');
  void page.actions.removeLeftovers();
  void page.actions.removeLeftovers();
  assert.equal(page.of('removeLeftovers').length, 1);
  assert.deepEqual(page.of('removeLeftovers')[0]?.args, [[1]]);
  assert.equal(page.state().appsBusy, 'remove');
  const answer: RemoveLeftovers = {
    view: view(removed([left(0, 'C:\\a')])),
    results: [{ key: 1, outcome: 'removed' }],
    bytes: 1000,
  };
  page.of('removeLeftovers')[0]?.resolve(answer);
  await flush();
  assert.equal(page.state().appsBusy, null);
  assert.deepEqual(page.state().appsChecked, []);
});

test('« Terminé » : le cœur oublie la désinstallation, la liste revient', async () => {
  const page = start();
  await page.open(view(removed([])));
  void page.actions.stopUninstall();
  void page.actions.stopUninstall();
  assert.equal(page.of('stopUninstall').length, 1);
  page.of('stopUninstall')[0]?.resolve(view());
  await flush();
  assert.equal(page.state().apps?.follow.phase, 'idle');
  assert.equal(page.state().appsBusy, null);
});

test('« outcome: kept » est dit une fois par un toast', async () => {
  const page = start();
  await page.open(view(waiting()));
  (await page.tick())?.resolve({ ...view(), outcome: 'kept' });
  await flush();
  assert.ok(page.state().toast);
});

test('un « kept » dans une réponse jetée est dit quand même : le cœur ne le dit qu’une fois', async () => {
  const page = start();
  await page.open(view(waiting()));
  const late = await page.tick();
  assert.ok(late);
  // La personne revient sur la page : une nouvelle demande part, et la réponse en vol devient périmée.
  page.actions.navigate('uninstall');
  assert.equal(page.state().toast ? 1 : 0, 0);
  late.resolve({ ...view(), outcome: 'kept' });
  await flush();
  assert.ok(page.state().toast, 'le toast est dit');
  page.of('appsView').at(-1)?.resolve(view());
  await flush();
  assert.equal(page.state().apps?.follow.phase, 'idle');
});

test('après un refus, la région vivante revient à la liste : une seconde tentative s’annonce de nouveau', async () => {
  const page = start();
  await page.open();
  page.actions.askUninstall('a-discord');
  void page.actions.confirmUninstall();
  const launching = page.state().appsAnnounce;
  assert.match(launching, /Discord/);
  page.of('startUninstall')[0]?.reject({ kind: 'uninstallRefused', reason: 'cancelled' });
  await flush();
  assert.equal(page.state().appsAnnounce, fr.uninstall.announce.list);
  page.of('appsView').at(-1)?.resolve(view());
  await flush();
  page.actions.askUninstall('a-discord');
  void page.actions.confirmUninstall();
  assert.equal(page.state().appsAnnounce, launching, 'la même phrase revient, après une autre');
});

test('les cases repartent vides : à l’entrée dans le bilan, et après « Chercher à nouveau »', async () => {
  const A = left(0, 'C:\\a');
  const B = left(1, 'C:\\b');
  // Après « Chercher à nouveau ».
  const rescanning = start();
  await rescanning.open(view(removed([A, B])));
  rescanning.actions.toggleLeftover(A.path);
  assert.deepEqual(rescanning.state().appsChecked, [A.path]);
  void rescanning.actions.rescanLeftovers();
  rescanning.of('rescanLeftovers')[0]?.resolve(view(removed([A, B])));
  await flush();
  assert.deepEqual(rescanning.state().appsChecked, [], 'les cases repartent vides');

  // À l'entrée dans un nouveau bilan qui retrouve le même chemin.
  const again = start();
  await again.open(view(removed([A])));
  again.actions.toggleLeftover(A.path);
  void again.actions.stopUninstall();
  again.of('stopUninstall')[0]?.resolve(view());
  await flush();
  again.actions.askUninstall('a-discord');
  void again.actions.confirmUninstall();
  again.of('startUninstall')[0]?.resolve(view(waiting()));
  await flush();
  (await again.tick())?.resolve(view(removed([A])));
  await flush();
  assert.equal(again.state().apps?.follow.phase, 'removed');
  assert.deepEqual(again.state().appsChecked, [], 'jamais cochés d’avance');
});

test('une case ne bouge pas pendant un geste, et un reste qui n’est pas proposé ne se coche pas', async () => {
  const A = left(0, 'C:\\a');
  const page = start();
  await page.open(view(removed([A])));
  page.actions.toggleLeftover('C:\\absent');
  assert.deepEqual(page.state().appsChecked, [], 'un chemin qui n’est pas proposé est refusé');
  void page.actions.stopUninstall();
  assert.equal(page.state().appsBusy, 'stop');
  page.actions.toggleLeftover(A.path);
  assert.deepEqual(page.state().appsChecked, [], 'pendant un geste, la case ne bouge pas');
});

test('« Désinstaller » n’est accepté que sur la liste au repos, et pas pendant un geste', async () => {
  const waitingPage = start();
  await waitingPage.open(view(waiting()));
  waitingPage.actions.askUninstall('a-steam');
  assert.equal(waitingPage.state().appsConfirm, null, 'refusé hors idle');

  const removedPage = start();
  await removedPage.open(view(removed([])));
  removedPage.actions.askUninstall('a-steam');
  assert.equal(removedPage.state().appsConfirm, null);

  const busy = start();
  await busy.open();
  busy.actions.askUninstall('a-discord');
  void busy.actions.confirmUninstall();
  busy.actions.askUninstall('a-steam');
  assert.equal(busy.state().appsConfirm, null, 'refusé pendant le lancement');
  assert.equal(busy.of('startUninstall').length, 1);
});

test('plusieurs rafraîchissements pendant l’attente n’arment qu’une minuterie : une seule demande par délai', async () => {
  const page = start();
  await page.open(view(waiting()));
  const armed = page.app['appsTimer'];
  assert.notEqual(armed, undefined, 'la minuterie est armée pendant l’attente');
  for (let index = 0; index < 4; index += 1) {
    page.app['syncAppsPoll']();
  }
  assert.equal(page.app['appsTimer'], armed, 'la minuterie armée n’est pas remplacée : jamais deux');
  const before = page.of('appsView').length;
  mock.timers.tick(APPS_POLL_DELAY);
  await flush();
  assert.equal(page.of('appsView').length, before + 1, 'une seule demande part');
});

test('« destroyed » : l’élément n’est plus là, la page le garde sans case, ne le renvoie pas, et le focus va à ce qui reste', async () => {
  const A = left(0, 'C:\\a');
  const B = left(1, 'C:\\b');
  const buttons = [fakeButton(uninstallFocus.done), fakeButton(uninstallFocus.leftover(0))];
  const page = start(buttons);
  await page.open(view(removed([A, B])));
  page.actions.toggleLeftover(A.path);
  page.actions.toggleLeftover(B.path);
  void page.actions.removeLeftovers();
  assert.deepEqual(page.of('removeLeftovers')[0]?.args, [[0, 1]]);
  page.of('removeLeftovers')[0]?.resolve({
    view: view(removed([B])),
    results: [
      { key: 0, outcome: 'failed', reason: 'destroyed' },
      { key: 1, outcome: 'failed', reason: 'inUse' },
    ],
    bytes: 0,
  } satisfies RemoveLeftovers);
  await flush();
  assert.deepEqual(page.state().appsDestroyed.map((row) => row.path), [A.path]);
  assert.equal(page.state().appsFailures[A.path], 'destroyed');
  assert.equal(page.state().appsFailures[B.path], 'inUse');
  // Une ligne détruite ne se coche plus, et ne repart pas.
  page.actions.toggleLeftover(A.path);
  assert.deepEqual(page.state().appsChecked, []);
  page.actions.toggleLeftover(B.path);
  void page.actions.removeLeftovers();
  assert.deepEqual(page.of('removeLeftovers')[1]?.args, [[1]], 'seul ce qui reste part');
  page.of('removeLeftovers')[1]?.resolve({ view: view(removed([])), results: [{ key: 1, outcome: 'removed' }], bytes: 10 } satisfies RemoveLeftovers);
  await flush();
  assert.deepEqual(page.state().appsDestroyed.map((row) => row.path), [A.path], 'elle reste à l’écran');
  assert.ok((buttons[0]?.focus.mock.callCount() ?? 0) >= 1, 'le focus va à « Terminé »');
});
