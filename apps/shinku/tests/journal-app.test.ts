// Le Journal et « Voir leurs licences » côté application : la page se charge à l'arrivée et pas autrement
// (ni sondage, ni retour du focus), une réponse en retard est jetée, « Réessayer » redemande, l'oubli des
// désinstallations passe par une confirmation, un grand vide qui se termine pendant que la page est ouverte
// la recharge une fois.
//
// Comme pour la Confidentialité : un faux cœur dont chaque réponse se résout à la main, des minuteries
// simulées, et un dessin qui ne dessine rien.

import assert from 'node:assert/strict';
import { afterEach, beforeEach, mock, test } from 'node:test';

import type { Backend, HistoryView } from '../src/backend/types.ts';
import { journalFocus } from '../src/model/journal.ts';
import { IDLE_SESSION, initialState } from '../src/state.ts';

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
const { pageKey } = await import('../src/views/shell.ts');

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

const UNINSTALL = { kind: 'uninstall', atMs: NOW - 1000, name: 'Essai', leftovers: 1, leftoverBytes: 10 } as const;
const CLEAN = {
  kind: 'clean',
  startedMs: NOW - 5000,
  finishedMs: null,
  state: 'running',
  freedBytes: 0,
  recoverableBytes: 0,
  skippedFiles: 0,
} as const;

function view(...lines: HistoryView['lines']): HistoryView {
  return { lines, more: false };
}

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

function start(focusable: unknown[] = []) {
  const fake = fakeBackend();
  const root = { querySelectorAll: () => focusable } as unknown as HTMLElement;
  const app = new App(fake.backend, root, () => {});
  const actions = app['actions'];
  return {
    ...fake,
    app,
    actions,
    state: () => app['state'],
    focusWindow: (): void => listeners.get('window:focus')?.(),
    /** Ouvre la page et lui donne sa première vue. */
    async open(first: HistoryView = view(UNINSTALL)): Promise<void> {
      actions.navigate('journal');
      fake.of('historyView').at(-1)?.resolve(first);
      await flush();
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

test('le Journal se demande une fois à l’arrivée, sans sondage ni relecture au retour du focus', async () => {
  const page = start();
  page.actions.navigate('journal');
  assert.equal(page.of('historyView').length, 1);
  assert.equal(page.state().history, null);
  page.of('historyView')[0]?.resolve(view(UNINSTALL));
  await flush();
  assert.equal(page.state().history?.lines.length, 1);
  mock.timers.tick(10 * 60_000);
  page.focusWindow();
  await flush();
  assert.equal(page.of('historyView').length, 1, 'ni le temps ni le focus ne redemandent');
});

test('la page quittée oublie la vue, et une autre arrivée la redemande', async () => {
  const page = start();
  await page.open();
  page.actions.navigate('home');
  assert.equal(page.state().history, null);
  page.actions.navigate('journal');
  assert.equal(page.of('historyView').length, 2);
});

test('une réponse d’une visite dépassée est jetée', async () => {
  const page = start();
  page.actions.navigate('journal');
  page.actions.navigate('home');
  page.actions.navigate('journal');
  page.of('historyView')[0]?.resolve(view(UNINSTALL));
  await flush();
  assert.equal(page.state().history, null, 'la première visite ne remplit pas la seconde');
  page.of('historyView')[1]?.resolve(view());
  await flush();
  assert.equal(page.state().history?.lines.length, 0);
});

test('une vue qui n’arrive pas : la page le dit, « Réessayer » la redemande', async () => {
  const page = start();
  page.actions.navigate('journal');
  page.of('historyView')[0]?.reject(new Error('non'));
  await flush();
  assert.equal(page.state().historyFailed, true);
  page.actions.retryJournal();
  assert.equal(page.state().historyFailed, false);
  assert.equal(page.of('historyView').length, 2);
  page.of('historyView')[1]?.resolve(view(UNINSTALL));
  await flush();
  assert.equal(page.state().history?.lines.length, 1);
  assert.equal(page.state().historyFailed, false);
});

test('« Oublier » ouvre une confirmation ; Annuler n’appelle pas le cœur', async () => {
  const page = start();
  await page.open();
  page.actions.askForgetUninstalls();
  assert.equal(page.state().historyAsk, true);
  assert.equal(page.of('forgetUninstalls').length, 0);
  page.actions.dismissForgetUninstalls();
  assert.equal(page.state().historyAsk, false);
  assert.equal(page.of('forgetUninstalls').length, 0);
});

test('confirmer l’oubli adopte la vue rendue par le cœur, une seule fois', async () => {
  const page = start();
  await page.open(view(UNINSTALL, CLEAN));
  page.actions.askForgetUninstalls();
  page.actions.confirmForgetUninstalls();
  page.actions.confirmForgetUninstalls();
  assert.equal(page.of('forgetUninstalls').length, 1, 'un second geste pendant l’appel ne part pas');
  assert.equal(page.state().historyAsk, false);
  assert.equal(page.state().historyBusy, true);
  page.of('forgetUninstalls')[0]?.resolve(view(CLEAN));
  await flush();
  assert.deepEqual(page.state().history?.lines, [CLEAN]);
  assert.equal(page.state().historyBusy, false);
});

test('un grand vide qui se termine pendant que le Journal est ouvert le recharge', async () => {
  const page = start();
  await page.open(view(CLEAN));
  const cleaning = { ...IDLE_SESSION, state: 'cleaning' as const };
  page.app['adopt'](cleaning);
  assert.equal(page.of('historyView').length, 1, 'tant qu’il tourne, rien n’est redemandé');
  page.app['adopt']({ ...IDLE_SESSION, state: 'cleaned' as const });
  assert.equal(page.of('historyView').length, 2);
  page.of('historyView')[1]?.resolve(view({ ...CLEAN, state: 'completed', finishedMs: NOW }));
  await flush();
  assert.equal(page.state().history?.lines[0]?.kind, 'clean');
});

test('un grand vide qui se termine ailleurs que sur le Journal ne le charge pas', async () => {
  const page = start();
  page.actions.navigate('home');
  page.app['adopt']({ ...IDLE_SESSION, state: 'cleaning' as const });
  page.app['adopt']({ ...IDLE_SESSION, state: 'cleaned' as const });
  assert.equal(page.of('historyView').length, 0);
});

test('« Voir leurs licences » : rien n’est dit si le fichier s’ouvre, une phrase sinon', async () => {
  const page = start();
  page.actions.openLicences();
  assert.equal(page.of('openLicences').length, 1);
  page.of('openLicences')[0]?.resolve('opened');
  await flush();
  assert.equal(page.state().licencesAnswer, null);
  page.actions.openLicences();
  page.of('openLicences')[1]?.resolve('missing');
  await flush();
  assert.equal(page.state().licencesAnswer, 'missing');
  page.actions.openLicences();
  page.of('openLicences')[2]?.reject(new Error('non'));
  await flush();
  assert.equal(page.state().licencesAnswer, 'failed');
  page.actions.navigate('home');
  assert.equal(page.state().licencesAnswer, null, 'la réponse ne suit pas hors des Paramètres');
});

test('le repère du titre existe pour rendre le focus après l’oubli', () => {
  assert.equal(journalFocus.title, 'journal-title');
});

test('un double clic sur « Voir leurs licences » n’appelle le cœur qu’une fois', async () => {
  const page = start();
  page.actions.openLicences();
  page.actions.openLicences();
  assert.equal(page.of('openLicences').length, 1, 'le second geste, pendant l’appel, ne part pas');
  page.of('openLicences')[0]?.resolve('opened');
  await flush();
  page.actions.openLicences();
  assert.equal(page.of('openLicences').length, 2, 'une fois la réponse venue, on peut recommencer');
});

test('un grand vide qui se termine sur le Journal ne redessine pas toute la page', () => {
  // `keyed` redessine tout ce qui est sous une clé qui change, et le bouton qui avait le focus le perd.
  const state = (pane: 'home' | 'scan', route: 'journal' | 'privacy') => ({ ...initialState(NOW), route, pane });
  assert.equal(pageKey(state('home', 'journal')), pageKey(state('scan', 'journal')));
  // Les autres pages gardent leur clé par volet.
  assert.notEqual(pageKey(state('home', 'privacy')), pageKey(state('scan', 'privacy')));
  const cleaning = { ...state('home', 'journal'), session: { ...IDLE_SESSION, state: 'cleaning' as const, clean: {} as never } };
  assert.equal(pageKey(cleaning), pageKey(state('home', 'journal')));
});

test('après « Oublier » confirmé, le focus est rendu au titre de la page', async () => {
  const focused: string[] = [];
  const element = (key: string) => ({ dataset: { fk: key }, focus: () => focused.push(key) });
  const page = start([element(journalFocus.forget), element(journalFocus.title)]);
  await page.open(view(UNINSTALL));
  page.actions.askForgetUninstalls();
  page.actions.confirmForgetUninstalls();
  page.of('forgetUninstalls')[0]?.resolve(view());
  await flush();
  await flush();
  assert.deepEqual(focused, [journalFocus.title]);
});
