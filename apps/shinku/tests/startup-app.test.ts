// La page Démarrage côté application : chargement à l'arrivée et au retour du
// focus, gestes optimistes, refus, réponses en retard, zone de retour, focus.
//
// Comme pour le mode jeu, l'application est testée sans navigateur : un faux
// cœur dont chaque réponse se résout à la main, des minuteries simulées, et un
// dessin qui ne dessine rien.

import assert from 'node:assert/strict';
import { afterEach, beforeEach, mock, test } from 'node:test';

import type { Backend, StartupEntry, StartupView } from '../src/backend/types.ts';
import { fr } from '../src/i18n/fr.ts';
import { SAY_DELAY, STARTUP_RELOAD_GAP } from '../src/model/startup.ts';
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

function entry(id: string, parts: Partial<StartupEntry> = {}): StartupEntry {
  return {
    id,
    name: id,
    alias: null,
    publisher: null,
    scope: 'user',
    origin: 'registry',
    state: 'enabled',
    disabledAt: null,
    mark: null,
    target: 'found',
    host: null,
    hint: null,
    own: false,
    adjustable: true,
    ...parts,
  };
}

const DISCORD = entry('ur-discord', { name: 'Discord' });
const STEAM = entry('ur-steam', { name: 'Steam' });
const ONEDRIVE = entry('ur-onedrive', { name: 'OneDrive', hint: 'sync' });
const SHINKU = entry('ur-shinku', { name: 'Shinku', own: true, adjustable: false });
const OLD = entry('ur-old', { name: 'Ancien outil', state: 'unknown', adjustable: false });
const SECURITY = entry('mr-security', { name: 'Sécurité Windows', scope: 'machine', adjustable: false });

function view(entries: StartupEntry[] = [DISCORD, STEAM, ONEDRIVE, SHINKU, OLD, SECURITY], unreadable: StartupView['unreadable'] = []): StartupView {
  return { entries, unreadable };
}

/** La vue telle que le cœur la rend après avoir coupé `id`. */
function cut(source: StartupView, id: string): StartupView {
  return {
    ...source,
    entries: source.entries.map((found) =>
      found.id === id
        ? { ...found, state: 'disabled', disabledAt: NOW, mark: { kind: 'cutByShinku', at: NOW } }
        : found,
    ),
  };
}

/**
 * Un faux bouton portant une clé de focus : la liste des fois où le focus lui a été rendu, et celle
 * des fois où sa ligne a été amenée à l'écran.
 */
function fakeButton(key: string) {
  const reveal = mock.fn();
  return {
    dataset: { fk: key },
    focus: mock.fn(),
    reveal,
    closest: () => ({ scrollIntoView: reveal }),
  };
}

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

function start(buttons: Array<ReturnType<typeof fakeButton>> = []) {
  const fake = fakeBackend();
  const root = { querySelectorAll: () => buttons } as unknown as HTMLElement;
  const app = new App(fake.backend, root, () => {});
  const actions = app['actions'];
  return {
    ...fake,
    app,
    actions,
    state: () => app['state'],
    /** Le démarrage de l'application : les écouteurs de la fenêtre sont posés. */
    async boot(): Promise<void> {
      const started = app.start();
      fake.of('overview')[0]?.resolve({ settings: { theme: 'system' } });
      fake.of('sessionView')[0]?.resolve(IDLE_SESSION);
      await started;
      await flush();
    },
    /** Ouvre la page et lui donne sa première vue. */
    async open(first: StartupView = view()): Promise<void> {
      actions.navigate('startup');
      fake.of('startupView').at(-1)?.resolve(first);
      await flush();
    },
    focusWindow: (): void => listeners.get('window:focus')?.(),
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

// ── Chargement ─────────────────────────────────────────────────────────────

test('la vue se charge une fois à l’arrivée sur la page, sans aucun sondage', async () => {
  const page = start();
  page.actions.navigate('startup');
  assert.equal(page.of('startupView').length, 1);
  assert.equal(page.state().startup, null, 'rien en attendant la réponse');
  page.of('startupView')[0]?.resolve(view());
  await flush();
  assert.equal(page.state().startup?.entries.length, 6);

  // Pas de minuterie propre à la page, et le temps qui passe ne redemande rien.
  assert.equal(intervals.size, 0);
  mock.timers.tick(10 * 60_000);
  await flush();
  assert.equal(page.of('startupView').length, 1);
});

test('au retour du focus de la fenêtre, la vue est relue ; hors de la page ou fenêtre cachée, non', async () => {
  const page = start();
  await page.boot();
  // Hors de la page : le focus ne demande rien.
  page.focusWindow();
  assert.equal(page.of('startupView').length, 0);

  await page.open(view([DISCORD]));
  mock.timers.tick(STARTUP_RELOAD_GAP);
  page.focusWindow();
  assert.equal(page.of('startupView').length, 2);
  // L'état que la personne a réglé dans Windows pendant ce temps se voit.
  page.of('startupView')[1]?.resolve(view([cut(view([DISCORD]), 'ur-discord').entries[0]!]));
  await flush();
  assert.equal(page.state().startup?.entries[0]?.state, 'disabled');

  mock.timers.tick(STARTUP_RELOAD_GAP);
  environment.hidden = true;
  page.focusWindow();
  assert.equal(page.of('startupView').length, 2, 'une fenêtre cachée ne demande rien');
  environment.hidden = false;
  listeners.get('document:visibilitychange')?.();
  assert.equal(page.of('startupView').length, 3);

  page.actions.navigate('home');
  page.focusWindow();
  assert.equal(page.of('startupView').length, 3, 'la page quittée ne demande plus rien');
});

test('la vue d’une visite ne sert pas à la suivante', async () => {
  const page = start();
  await page.open();
  page.actions.navigate('home');
  assert.equal(page.state().startup, null);
  page.actions.navigate('startup');
  assert.equal(page.state().startup, null, 'rien de la visite précédente en attendant la première vue');
  assert.equal(page.of('startupView').length, 2);
  // Un clic sur l'entrée du menu quand on y est déjà relit la vue, sans l'effacer.
  page.of('startupView')[1]?.resolve(view([DISCORD]));
  await flush();
  page.actions.navigate('startup');
  assert.equal(page.of('startupView').length, 3);
  assert.equal(page.state().startup?.entries.length, 1);
});

test('une vue qui n’arrive pas : la page le dit, puis le retour du focus réessaie', async () => {
  const page = start();
  await page.boot();
  page.actions.navigate('startup');
  page.of('startupView')[0]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().startupFailed, true);
  assert.equal(page.state().startup, null);
  mock.timers.tick(STARTUP_RELOAD_GAP);
  page.focusWindow();
  page.of('startupView')[1]?.resolve(view());
  await flush();
  assert.equal(page.state().startupFailed, false);
  assert.equal(page.state().startup?.entries.length, 6);
  // Une panne plus tard garde la dernière vue au lieu de la remplacer par une erreur.
  mock.timers.tick(STARTUP_RELOAD_GAP);
  page.focusWindow();
  page.of('startupView')[2]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().startupFailed, false);
  assert.equal(page.state().startup?.entries.length, 6);
});

test('une vue arrivée après avoir quitté la page n’est pas gardée', async () => {
  const page = start();
  page.actions.navigate('startup');
  page.actions.navigate('home');
  page.of('startupView')[0]?.resolve(view());
  await flush();
  assert.equal(page.state().startup, null);
});

// ── Gestes ─────────────────────────────────────────────────────────────────

test('couper : l’interrupteur bouge tout de suite, puis la vue du cœur et la phrase de retour', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);

  // Tout de suite : coupée par Shinku, tous les interrupteurs attendent.
  const optimistic = page.state().startup?.entries.find((found) => found.id === 'ur-discord');
  assert.equal(optimistic?.state, 'disabled');
  assert.deepEqual(optimistic?.mark, { kind: 'cutByShinku', at: NOW });
  assert.equal(page.state().startupBusy, 'ur-discord');
  assert.deepEqual(page.of('setStartupEntry')[0]?.args, ['ur-discord', false]);

  const answer = cut(view(), 'ur-discord');
  page.of('setStartupEntry')[0]?.resolve(answer);
  await flush();
  assert.deepEqual(page.state().startup, answer);
  assert.equal(page.state().startupBusy, null);
  assert.equal(page.state().startupSay?.text, fr.startup.say.cut('Discord', false));
  assert.deepEqual(page.state().startupSay?.undo, { id: 'ur-discord', enabled: true });
});

test('le cœur refuse : l’interrupteur revient, et la phrase dit que rien n’a changé', async () => {
  const page = start();
  const first = view();
  await page.open(first);
  page.actions.setStartupEntry('ur-steam', false);
  assert.equal(page.state().startup?.entries.find((found) => found.id === 'ur-steam')?.state, 'disabled');

  page.of('setStartupEntry')[0]?.reject({ kind: 'startupRefused', reason: 'failed' });
  await flush();
  assert.deepEqual(page.state().startup, first, 'la ligne est revenue comme elle était, marque comprise');
  assert.equal(page.state().startupBusy, null);
  assert.equal(page.state().startupSay?.text, fr.startup.say.failed);
  assert.equal(page.state().startupSay?.undo, null, 'rien à annuler : rien n’a changé');
  // La page relit alors la vue du cœur.
  assert.equal(page.of('startupView').length, 2);
});

test('un refus d’une entrée oubliée dit qu’elle n’existe plus et recharge la liste', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.reject({ kind: 'startupRefused', reason: 'unknownEntry' });
  await flush();
  assert.equal(page.state().startupSay?.text, 'Cette entrée n’existe plus.');
  assert.equal(page.of('startupView').length, 2);
  const reloaded = view([STEAM]);
  page.of('startupView')[1]?.resolve(reloaded);
  await flush();
  assert.deepEqual(page.state().startup, reloaded);
});

test('« notAdjustable » le dit ; « busy » ne dit rien mais remet la ligne', async () => {
  const page = start();
  const first = view();
  await page.open(first);
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.reject({ kind: 'startupRefused', reason: 'notAdjustable' });
  await flush();
  assert.equal(page.state().startupSay?.text, 'Cette entrée ne se règle pas ici.');

  page.actions.setStartupEntry('ur-steam', false);
  // Le geste suivant efface la phrase du précédent tout de suite.
  assert.equal(page.state().startupSay, null);
  page.of('setStartupEntry')[1]?.reject({ kind: 'startupRefused', reason: 'busy' });
  await flush();
  assert.equal(page.state().startupSay, null, 'le geste est ignoré : rien à dire');
  assert.deepEqual(page.state().startup, first);
  assert.equal(page.state().startupBusy, null);
});

test('une panne inattendue est un changement qui n’a pas pu être fait', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().startupSay?.text, fr.startup.say.failed);
  assert.equal(page.state().startup?.entries[0]?.state, 'enabled');
});

test('un seul changement à la fois : le deuxième geste, même sur une autre ligne, est ignoré', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.actions.setStartupEntry('ur-discord', false);
  page.actions.setStartupEntry('ur-steam', false);
  assert.equal(page.of('setStartupEntry').length, 1);
  assert.equal(page.state().startup?.entries.find((found) => found.id === 'ur-steam')?.state, 'enabled');
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  // Libéré, le geste suivant part.
  page.actions.setStartupEntry('ur-steam', false);
  assert.equal(page.of('setStartupEntry').length, 2);
});

test('une entrée que la page ne sait pas régler n’envoie rien au cœur', async () => {
  const page = start();
  await page.open();
  for (const id of ['ur-shinku', 'ur-old', 'mr-security', 'ur-inconnu']) {
    page.actions.setStartupEntry(id, false);
  }
  // Déjà dans l'état demandé : rien à faire non plus.
  page.actions.setStartupEntry('ur-discord', true);
  assert.equal(page.of('setStartupEntry').length, 0);
  assert.equal(page.state().startupBusy, null);
});

test('aucune vue demandée pendant un geste n’est adoptée, ni envoyée', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  const asked = page.of('startupView').length;
  page.focusWindow();
  assert.equal(page.of('startupView').length, asked, 'pas de demande pendant le geste');
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  page.focusWindow();
  assert.equal(page.of('startupView').length, asked + 1, 'après la réponse, les demandes repartent');
});

test('une vue demandée avant le geste et arrivée après est ignorée, qu’il soit en vol ou terminé', async () => {
  const page = start();
  await page.boot();
  await page.open(view());

  // Une vue part (retour du focus), puis la personne coupe Discord.
  mock.timers.tick(STARTUP_RELOAD_GAP);
  page.focusWindow();
  const old = page.of('startupView')[1];
  assert.ok(old !== undefined);
  page.actions.setStartupEntry('ur-discord', false);
  // Elle arrive pendant le geste : elle ne défait pas l'interrupteur.
  old.resolve(view());
  await flush();
  assert.equal(page.state().startup?.entries[0]?.state, 'disabled');

  // Le geste répond ; une autre vue, partie avant lui, arrive encore après.
  const answer = cut(view(), 'ur-discord');
  page.of('setStartupEntry')[0]?.resolve(answer);
  await flush();
  assert.deepEqual(page.state().startup, answer);

  page.focusWindow();
  const second = page.of('startupView')[2];
  assert.ok(second !== undefined);
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[1]?.resolve(cut(answer, 'ur-steam'));
  await flush();
  second.resolve(answer);
  await flush();
  assert.equal(page.state().startup?.entries.find((found) => found.id === 'ur-steam')?.state, 'disabled', 'la vue d’avant le geste n’a rien défait');
});

test('de deux vues demandées de suite, seule la dernière compte', async () => {
  const page = start();
  await page.boot();
  await page.open(view([DISCORD]));
  // Un clic sur l'entrée du menu relit la vue sans attendre : deux de suite, la dernière gagne.
  page.actions.navigate('startup');
  page.actions.navigate('startup');
  const [, older, newer] = page.of('startupView');
  assert.ok(older !== undefined && newer !== undefined);
  newer.resolve(view([STEAM]));
  await flush();
  older.resolve(view([DISCORD]));
  await flush();
  assert.deepEqual(page.state().startup?.entries.map((found) => found.id), ['ur-steam']);
});

test('la réponse d’un geste arrivée après avoir quitté la page ne remplit pas la page suivante', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.actions.navigate('home');
  assert.equal(page.state().startup, null);
  assert.equal(page.state().startupBusy, null);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  assert.equal(page.state().startup, null);
  assert.equal(page.state().startupSay, null);

  // Revenue sur la page avant la réponse : la page repart de zéro et relit la vue quand le geste a répondu.
  const again = start();
  await again.open();
  again.actions.setStartupEntry('ur-discord', false);
  again.actions.navigate('home');
  again.actions.navigate('startup');
  assert.equal(again.of('startupView').length, 1, 'pas de demande tant qu’un geste est en vol');
  again.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  assert.equal(again.of('startupView').length, 2);
  assert.equal(again.state().startupSay, null);
});

// ── Zone de retour ─────────────────────────────────────────────────────────

test('la zone de retour s’efface seule après douze secondes, et au geste suivant', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  assert.notEqual(page.state().startupSay, null);
  assert.equal(SAY_DELAY, 12_000);
  mock.timers.tick(SAY_DELAY - 1);
  assert.notEqual(page.state().startupSay, null);
  mock.timers.tick(1);
  assert.equal(page.state().startupSay, null);

  // Une phrase remplacée ne laisse pas sa minuterie effacer la suivante.
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[1]?.resolve(cut(view(), 'ur-steam'));
  await flush();
  mock.timers.tick(8000);
  page.actions.setStartupEntry('ur-onedrive', false);
  assert.equal(page.state().startupSay, null, 'effacée au geste');
  page.of('setStartupEntry')[2]?.resolve(cut(view(), 'ur-onedrive'));
  await flush();
  mock.timers.tick(8000);
  assert.notEqual(page.state().startupSay, null, 'la minuterie de la phrase d’avant est annulée');
  mock.timers.tick(4000);
  assert.equal(page.state().startupSay, null);
});

test('la phrase d’une app de synchronisation coupée prévient que les fichiers ne se synchronisent plus', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-onedrive', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-onedrive'));
  await flush();
  assert.equal(page.state().startupSay?.text, fr.startup.say.cut('OneDrive', true));
  assert.match(page.state().startupSay?.text ?? '', /ne se synchroniseront plus/);
});

test('« Annuler » fait le geste inverse, puis la phrase de la remise', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();

  page.actions.undoStartup();
  assert.deepEqual(page.of('setStartupEntry')[1]?.args, ['ur-discord', true]);
  assert.equal(page.state().startup?.entries[0]?.state, 'enabled', 'l’interrupteur revient tout de suite');
  assert.equal(page.state().startupSay, null);
  page.of('setStartupEntry')[1]?.resolve(view());
  await flush();
  assert.equal(page.state().startupSay?.text, fr.startup.say.restored('Discord'));
  assert.deepEqual(page.state().startupSay?.undo, { id: 'ur-discord', enabled: false });

  // Sans phrase, ou sans bouton, « Annuler » n'envoie rien.
  page.actions.undoStartup();
  page.of('setStartupEntry')[2]?.reject({ kind: 'startupRefused', reason: 'failed' });
  await flush();
  assert.equal(page.state().startupSay?.undo, null);
  page.actions.undoStartup();
  assert.equal(page.of('setStartupEntry').length, 3);
});

// ── Focus ──────────────────────────────────────────────────────────────────

test('après un geste, le focus reste sur l’interrupteur que la personne utilisait', async () => {
  const discord = fakeButton('startup:ur-discord');
  const steam = fakeButton('startup:ur-steam');
  const page = start([discord, steam]);
  await page.open();

  // Le bouton désactivé a perdu le focus : la page le lui rend, à la réponse.
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  assert.equal(discord.focus.mock.callCount(), 1);
  assert.equal(steam.focus.mock.callCount(), 0);

  // Un refus de même.
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[1]?.reject({ kind: 'startupRefused', reason: 'failed' });
  await flush();
  assert.equal(steam.focus.mock.callCount(), 1);

  // « Annuler » disparaît avec sa phrase : le focus va à la ligne du geste défait.
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[2]?.resolve(cut(view(), 'ur-steam'));
  await flush();
  page.actions.undoStartup();
  page.of('setStartupEntry')[3]?.resolve(view());
  await flush();
  assert.equal(steam.focus.mock.callCount(), 3);
  assert.equal(discord.focus.mock.callCount(), 1);
});

test('le focus n’est pas pris à quelqu’un qui l’a déjà', async () => {
  const discord = fakeButton('startup:ur-discord');
  const page = start([discord]);
  await page.open();
  environment.active = { tagName: 'BUTTON' };
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  assert.equal(discord.focus.mock.callCount(), 0);
});

// ── Un seul chargement au retour de la fenêtre ─────────────────────────────

test('le focus et la visibilité arrivent ensemble : un seul chargement, jamais deux en vol', async () => {
  const page = start();
  await page.boot();
  await page.open(view([DISCORD]));
  mock.timers.tick(STARTUP_RELOAD_GAP);
  page.focusWindow();
  listeners.get('document:visibilitychange')?.();
  page.focusWindow();
  assert.equal(page.of('startupView').length, 2, 'une demande en vol n’est pas relancée');
  page.of('startupView')[1]?.resolve(view([DISCORD]));
  await flush();

  // Finie depuis moins d'une demi-seconde : pas relancée non plus.
  mock.timers.tick(STARTUP_RELOAD_GAP - 1);
  page.focusWindow();
  assert.equal(page.of('startupView').length, 2);
  mock.timers.tick(1);
  page.focusWindow();
  assert.equal(page.of('startupView').length, 3, 'au bout d’une demi-seconde, on relit');
});

test('une demande d’arrivée, de « Réessayer » ou après un refus n’attend pas la demi-seconde', async () => {
  const page = start();
  await page.boot();
  page.actions.navigate('startup');
  page.of('startupView')[0]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().startupFailed, true);
  // « Réessayer » relance tout de suite, et la page repasse à « Chargement… ».
  page.actions.retryStartup();
  assert.equal(page.of('startupView').length, 2);
  assert.equal(page.state().startupFailed, false);
  page.of('startupView')[1]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().startupFailed, true, 'toujours en panne : le bouton revient');
  page.actions.retryStartup();
  page.of('startupView')[2]?.resolve(view([DISCORD]));
  await flush();
  assert.equal(page.state().startupFailed, false);
  assert.equal(page.state().startup?.entries.length, 1);

  // Un refus recharge la liste tout de suite.
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.reject({ kind: 'startupRefused', reason: 'failed' });
  await flush();
  assert.equal(page.of('startupView').length, 4);
});

// ── Une vue rechargée change la liste sous le focus ────────────────────────

/** Les boutons de la page : les interrupteurs de la liste et le titre. */
function listButtons() {
  return {
    discord: fakeButton('startup:ur-discord'),
    steam: fakeButton('startup:ur-steam'),
    onedrive: fakeButton('startup:ur-onedrive'),
    title: fakeButton('startup-title'),
  };
}

const THREE = [DISCORD, STEAM, ONEDRIVE];

/** La personne a le focus sur `button` ; la vue suivante arrive et sa ligne a disparu : le bouton n'existe plus, le focus est au corps. */
async function reloadUnder(
  page: ReturnType<typeof start>,
  button: ReturnType<typeof fakeButton>,
  next: StartupView,
): Promise<void> {
  mock.timers.tick(STARTUP_RELOAD_GAP);
  page.focusWindow();
  environment.sequence = [button, null];
  page.of('startupView').at(-1)?.resolve(next);
  await flush();
}

test('la ligne qui a le focus disparaît : le focus passe à la suivante, sinon à la précédente, sinon au titre', async () => {
  // Une ligne au milieu : la suivante.
  let buttons = listButtons();
  let page = start(Object.values(buttons));
  await page.boot();
  await page.open(view(THREE));
  await reloadUnder(page, buttons.steam, view([DISCORD, ONEDRIVE]));
  assert.equal(buttons.onedrive.focus.mock.callCount(), 1);
  assert.equal(buttons.discord.focus.mock.callCount(), 0);

  // La dernière ligne : la précédente.
  buttons = listButtons();
  page = start(Object.values(buttons));
  await page.boot();
  await page.open(view(THREE));
  await reloadUnder(page, buttons.onedrive, view([DISCORD, STEAM]));
  assert.equal(buttons.steam.focus.mock.callCount(), 1);

  // La seule ligne : le titre.
  buttons = listButtons();
  page = start(Object.values(buttons));
  await page.boot();
  await page.open(view([DISCORD]));
  await reloadUnder(page, buttons.discord, view([]));
  assert.equal(buttons.title.focus.mock.callCount(), 1);

  // La ligne est encore là : le focus lui reste, rien n'est déplacé.
  buttons = listButtons();
  page = start(Object.values(buttons));
  await page.boot();
  await page.open(view(THREE));
  await reloadUnder(page, buttons.steam, view(THREE));
  assert.equal(buttons.steam.focus.mock.callCount(), 1, 'elle le garde (rendu seulement si le corps l’a repris)');
  assert.equal(buttons.onedrive.focus.mock.callCount(), 0);
});

test('une entrée oubliée par le cœur : la ligne disparaît, le focus passe à sa voisine, la phrase dit pourquoi', async () => {
  const buttons = listButtons();
  const page = start(Object.values(buttons));
  await page.open(view(THREE));
  // La personne coupe Steam ; le cœur dit qu'il ne la connaît plus.
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[0]?.reject({ kind: 'startupRefused', reason: 'unknownEntry' });
  await flush();
  assert.equal(page.state().startupSay?.text, fr.startup.say.unknownEntry);
  // La vue rechargée n'a plus Steam, et c'est le corps qui a le focus quand elle arrive.
  environment.sequence = [buttons.steam, null];
  page.of('startupView')[1]?.resolve(view([DISCORD, ONEDRIVE]));
  await flush();
  assert.deepEqual(page.state().startup?.entries.map((found) => found.id), ['ur-discord', 'ur-onedrive']);
  assert.equal(buttons.onedrive.focus.mock.callCount(), 1);
  assert.notEqual(page.state().startupSay, null, 'la phrase du refus n’annonce aucun état : elle reste');
});

// ── La phrase de retour et la vue rechargée ────────────────────────────────

test('une phrase que la vue rechargée démentirait s’efface ; une phrase encore vraie reste', async () => {
  const page = start();
  await page.open(view(THREE));
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(THREE), 'ur-steam'));
  await flush();
  assert.notEqual(page.state().startupSay, null);

  // Rechargée : Steam est toujours coupée, la phrase reste.
  page.actions.navigate('startup');
  page.of('startupView').at(-1)?.resolve(cut(view(THREE), 'ur-steam'));
  await flush();
  assert.notEqual(page.state().startupSay, null);

  // Remise dans Windows entre-temps : « Steam est coupée » n'est plus vrai.
  page.actions.navigate('startup');
  page.of('startupView').at(-1)?.resolve(view(THREE));
  await flush();
  assert.equal(page.state().startupSay, null);

  // Elle a disparu de la liste : de même.
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[1]?.resolve(cut(view(THREE), 'ur-steam'));
  await flush();
  assert.notEqual(page.state().startupSay, null);
  page.actions.navigate('startup');
  page.of('startupView').at(-1)?.resolve(view([DISCORD, ONEDRIVE]));
  await flush();
  assert.equal(page.state().startupSay, null);
});

test('le délai de douze secondes attend tant que la zone a le focus ou le pointeur, puis repart', async () => {
  const page = start();
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();

  mock.timers.tick(SAY_DELAY - 1000);
  page.actions.holdStartupSay('focus', true);
  mock.timers.tick(10 * SAY_DELAY);
  assert.notEqual(page.state().startupSay, null, 'sous le focus, elle ne s’efface pas');
  page.actions.holdStartupSay('hover', true);
  page.actions.holdStartupSay('focus', false);
  mock.timers.tick(10 * SAY_DELAY);
  assert.notEqual(page.state().startupSay, null, 'le pointeur la retient encore');
  page.actions.holdStartupSay('hover', false);
  mock.timers.tick(SAY_DELAY - 1);
  assert.notEqual(page.state().startupSay, null, 'un délai neuf de douze secondes repart');
  mock.timers.tick(1);
  assert.equal(page.state().startupSay, null);

  // Une zone qui n'est plus là n'a rien à retenir ni à relancer.
  page.actions.holdStartupSay('focus', false);
  page.actions.holdStartupSay('hover', false);
  mock.timers.tick(10 * SAY_DELAY);
  assert.equal(page.state().startupSay, null);
});

test('la zone avait le focus quand elle s’efface : il retourne à l’interrupteur de la ligne', async () => {
  const buttons = listButtons();
  const page = start(Object.values(buttons));
  await page.open(view(THREE));
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(THREE), 'ur-steam'));
  await flush();
  const before = buttons.steam.focus.mock.callCount();

  // « Annuler » a le focus : le geste efface la phrase, et le focus va à la ligne.
  page.actions.holdStartupSay('focus', true);
  page.actions.undoStartup();
  await flush();
  assert.equal(buttons.steam.focus.mock.callCount(), before + 1);
  page.of('setStartupEntry')[1]?.resolve(view(THREE));
  await flush();

  // Sous le pointeur seulement, rien n'est déplacé.
  const count = buttons.discord.focus.mock.callCount();
  page.actions.setStartupEntry('ur-discord', false);
  page.actions.holdStartupSay('hover', true);
  page.of('setStartupEntry')[2]?.resolve(cut(view(THREE), 'ur-discord'));
  await flush();
  page.actions.holdStartupSay('hover', false);
  mock.timers.tick(SAY_DELAY);
  assert.equal(page.state().startupSay, null);
  assert.equal(buttons.discord.focus.mock.callCount(), count + 1, 'le seul rendu du geste, pas un de plus');

  // Une phrase périmée par la vue qui l'avait sous le focus : la ligne a disparu, sa voisine prend le focus.
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[3]?.resolve(cut(view(THREE), 'ur-steam'));
  await flush();
  page.actions.holdStartupSay('focus', true);
  page.actions.navigate('startup');
  page.of('startupView').at(-1)?.resolve(view([DISCORD, ONEDRIVE]));
  await flush();
  assert.equal(page.state().startupSay, null);
  assert.equal(buttons.onedrive.focus.mock.callCount(), 1);
  assert.equal(buttons.title.focus.mock.callCount(), 0);
});

test('après un geste, la ligne est amenée à l’écran sans que la barre flottante la cache', async () => {
  const discord = fakeButton('startup:ur-discord');
  const page = start([discord]);
  await page.open();
  page.actions.setStartupEntry('ur-discord', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(), 'ur-discord'));
  await flush();
  assert.equal(discord.reveal.mock.callCount(), 1);
  assert.deepEqual(discord.reveal.mock.calls[0]?.arguments, [{ block: 'nearest' }]);
});

// ── Deuxième relecture ─────────────────────────────────────────────────────

test('la fenêtre qui perd le focus système ne relance pas le délai de la barre qui avait le focus', async () => {
  const buttons = listButtons();
  const page = start(Object.values(buttons));
  await page.open(view(THREE));
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[0]?.resolve(cut(view(THREE), 'ur-steam'));
  await flush();
  const before = buttons.steam.focus.mock.callCount();

  page.actions.holdStartupSay('focus', true);
  // Alt+Tab : « focusout » arrive, mais le document n'a plus le focus et l'élément actif n'a pas changé.
  environment.windowFocused = false;
  page.actions.holdStartupSay('focus', false);
  mock.timers.tick(10 * SAY_DELAY);
  assert.notEqual(page.state().startupSay, null, 'la barre reste tant que le focus logique y est');

  // La personne revient (le focus rentre dans la barre), puis en sort vraiment : le délai repart.
  environment.windowFocused = true;
  page.actions.holdStartupSay('focus', true);
  page.actions.holdStartupSay('focus', false);
  mock.timers.tick(SAY_DELAY - 1);
  assert.notEqual(page.state().startupSay, null);
  mock.timers.tick(1);
  assert.equal(page.state().startupSay, null);

  // Et si la barre part pendant que la fenêtre est absente puis que le focus y était : il retourne à la ligne.
  page.actions.setStartupEntry('ur-steam', true);
  page.of('setStartupEntry')[1]?.resolve(view(THREE));
  await flush();
  page.actions.holdStartupSay('focus', true);
  environment.windowFocused = false;
  page.actions.holdStartupSay('focus', false);
  page.actions.undoStartup();
  await flush();
  assert.ok(buttons.steam.focus.mock.callCount() > before, 'le focus revient à l’interrupteur de la ligne');
});

/** Un bouton de fin de ligne, tel que la page le pose. */
const endButton = (id: string) => fakeButton(`startup-end:${id}`);

test('le bouton de fin de ligne qui a le focus : même ligne si elle existe, sinon voisine, sinon titre', async () => {
  const run = async (buttons: ReturnType<typeof listButtons> & { ownEnd: ReturnType<typeof endButton> }, first: StartupView, next: StartupView) => {
    const page = start(Object.values(buttons));
    await page.boot();
    await page.open(first);
    await reloadUnder(page, buttons.ownEnd, next);
  };
  const unreadable = entry('ur-old', { name: 'Ancien outil', state: 'unknown', adjustable: false });
  const readable = entry('ur-old', { name: 'Ancien outil' });

  // La ligne a maintenant un interrupteur : le focus va à cet interrupteur.
  let buttons = { ...listButtons(), ownEnd: endButton('ur-old') };
  buttons.discord = fakeButton('startup:ur-old');
  await run(buttons, view([unreadable, STEAM]), view([readable, STEAM]));
  assert.equal(buttons.discord.focus.mock.callCount(), 1);
  assert.equal(buttons.steam.focus.mock.callCount(), 0);

  // La ligne a disparu : la voisine qui a un contrôle.
  buttons = { ...listButtons(), ownEnd: endButton('ur-old') };
  await run(buttons, view([unreadable, STEAM]), view([STEAM]));
  assert.equal(buttons.steam.focus.mock.callCount(), 1);

  // Plus aucune ligne : le titre.
  buttons = { ...listButtons(), ownEnd: endButton('ur-old') };
  await run(buttons, view([unreadable]), view([]));
  assert.equal(buttons.title.focus.mock.callCount(), 1);

  // Elle devient une ligne du PC (aucun contrôle) : la voisine.
  buttons = { ...listButtons(), ownEnd: endButton('ur-old') };
  await run(buttons, view([unreadable, STEAM]), view([entry('ur-old', { scope: 'machine', adjustable: false }), STEAM]));
  assert.equal(buttons.steam.focus.mock.callCount(), 1);
});

test('le cœur accepte mais la vue montre l’entrée dans l’autre état : la barre dit l’échec, sans « Annuler »', async () => {
  const page = start();
  await page.open(view(THREE));
  page.actions.setStartupEntry('ur-steam', false);
  // La réponse montre Steam toujours active.
  page.of('setStartupEntry')[0]?.resolve(view(THREE));
  await flush();
  assert.deepEqual(page.state().startupSay, { text: fr.startup.say.failed, undo: null });
  assert.equal(page.state().startup?.entries.find((found) => found.id === 'ur-steam')?.state, 'enabled');

  // Une réponse juste dit bien « est coupée », avec « Annuler ».
  page.actions.setStartupEntry('ur-steam', false);
  page.of('setStartupEntry')[1]?.resolve(cut(view(THREE), 'ur-steam'));
  await flush();
  assert.equal(page.state().startupSay?.text, fr.startup.say.cut('Steam', false));
  assert.notEqual(page.state().startupSay?.undo, null);
});

test('« Réessayer » : après le rechargement le focus va au titre ; si la panne dure, il revient au bouton', async () => {
  const buttons = { ...listButtons(), retry: fakeButton('startup-retry') };
  const page = start(Object.values(buttons));
  await page.boot();
  page.actions.navigate('startup');
  page.of('startupView')[0]?.reject(new Error('ipc'));
  await flush();

  page.actions.retryStartup();
  page.of('startupView')[1]?.resolve(view(THREE));
  await flush();
  assert.equal(buttons.title.focus.mock.callCount(), 1);
  assert.equal(buttons.retry.focus.mock.callCount(), 0);

  // Une panne qui dure : le bouton revient et reprend le focus.
  page.actions.navigate('home');
  page.actions.navigate('startup');
  page.of('startupView').at(-1)?.reject(new Error('ipc'));
  await flush();
  page.actions.retryStartup();
  page.of('startupView').at(-1)?.reject(new Error('ipc'));
  await flush();
  assert.equal(buttons.retry.focus.mock.callCount(), 1);
  assert.equal(buttons.title.focus.mock.callCount(), 1, 'pas de focus au titre sans vue');
});
