// La page Confidentialité côté application : chargement à l'arrivée et au retour du focus, cases vides
// à chaque visite, confirmation (Annuler n'appelle pas le cœur), seuls des identifiants du cœur qui partent,
// issues, refus, page inerte pendant l'effacement, réponses en retard.
//
// Comme pour le Démarrage, l'application est testée sans navigateur : un faux cœur dont chaque réponse se
// résout à la main, des minuteries simulées, et un dessin qui ne dessine rien.

import assert from 'node:assert/strict';
import { afterEach, beforeEach, mock, test } from 'node:test';

import type { Backend, PrivacyGroup, PrivacyView, TraceItem } from '../src/backend/types.ts';
import { fr } from '../src/i18n/fr.ts';
import { checkableIds, PRIVACY_RELOAD_GAP, privacyFocus } from '../src/model/privacy.ts';
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

function item(id: string, parts: Partial<TraceItem> = {}): TraceItem {
  return { id, kind: 'history', state: 'present', bytes: 1024, count: null, ...parts };
}

function group(key: string, app: PrivacyGroup['app'], items: TraceItem[], parts: Partial<PrivacyGroup> = {}): PrivacyGroup {
  return { key, kind: app === null ? 'windows' : 'browser', app, profile: null, slot: app === null ? null : 0, running: false, items, ...parts };
}

const CHROME = group('chrome:0', 'chrome', [item('t-chrome')]);
const EDGE = group('edge:0', 'edge', [item('t-edge')]);
const WINDOWS = group('windows', null, [
  item('t-recent', { kind: 'recent', bytes: null, count: 4 }),
  item('t-typed', { kind: 'typed', bytes: null, count: 3 }),
  item('t-clip', { kind: 'clipboard', bytes: null }),
]);

function view(groups: PrivacyGroup[] = [CHROME, EDGE, WINDOWS]): PrivacyView {
  return { groups, firefox: false, edgeImportsChrome: true };
}

/** La vue telle que le cœur la rend quand `ids` sont effacées : ces traces sont vides. */
function erased(source: PrivacyView, ids: string[]): PrivacyView {
  return {
    ...source,
    groups: source.groups.map((one) => ({
      ...one,
      items: one.items.map((found) => (ids.includes(found.id) ? { ...found, state: 'empty' as const, bytes: null, count: null } : found)),
    })),
  };
}

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));

function start() {
  const fake = fakeBackend();
  const root = { querySelectorAll: () => [] } as unknown as HTMLElement;
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
    /** Ouvre la page et lui donne sa première vue. */
    async open(first: PrivacyView = view()): Promise<void> {
      actions.navigate('privacy');
      fake.of('privacyView').at(-1)?.resolve(first);
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

test('la vue se charge une fois à l’arrivée, sans sondage, et rien n’est coché', async () => {
  const page = start();
  page.actions.navigate('privacy');
  assert.equal(page.of('privacyView').length, 1);
  assert.equal(page.state().privacy, null);
  page.of('privacyView')[0]?.resolve(view());
  await flush();
  assert.equal(page.state().privacy?.groups.length, 3);
  assert.deepEqual(page.state().privacyChecked, [], 'rien n’est jamais coché d’avance');
  assert.equal(page.state().privacyAsk, null);
  assert.equal(intervals.size, 0);
  mock.timers.tick(10 * 60_000);
  await flush();
  assert.equal(page.of('privacyView').length, 1, 'le temps qui passe ne redemande rien');
});

test('au retour du focus, la vue est relue ; hors de la page ou fenêtre cachée, non', async () => {
  const page = start();
  await page.boot();
  page.focusWindow();
  assert.equal(page.of('privacyView').length, 0);
  await page.open();
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  page.focusWindow();
  assert.equal(page.of('privacyView').length, 2);
  page.of('privacyView')[1]?.resolve(view([CHROME, WINDOWS]));
  await flush();
  assert.equal(page.state().privacy?.groups.length, 2, 'un navigateur disparu ne reste pas');
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  environment.hidden = true;
  page.focusWindow();
  assert.equal(page.of('privacyView').length, 2, 'une fenêtre cachée ne demande rien');
  environment.hidden = false;
  page.actions.navigate('home');
  page.focusWindow();
  assert.equal(page.of('privacyView').length, 2, 'la page quittée ne demande plus rien');
});

test('un navigateur ouvert depuis : sa case est décochée à la relecture', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.toggleTrace('t-edge');
  assert.deepEqual(page.state().privacyChecked, ['t-chrome', 't-edge']);
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  page.focusWindow();
  page.of('privacyView')[1]?.resolve(view([{ ...CHROME, running: true }, EDGE, WINDOWS]));
  await flush();
  assert.deepEqual(page.state().privacyChecked, ['t-edge']);
});

test('les cases repartent vides à chaque retour sur la page, avec la confirmation et les issues', async () => {
  const page = start();
  await page.open();
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  assert.notEqual(page.state().privacyAsk, null);
  page.actions.navigate('home');
  assert.deepEqual(page.state().privacyChecked, []);
  assert.equal(page.state().privacyAsk, null);
  assert.equal(page.state().privacy, null);
  page.actions.navigate('privacy');
  assert.deepEqual(page.state().privacyChecked, []);
  page.of('privacyView')[1]?.resolve(view());
  await flush();
  assert.deepEqual(page.state().privacyChecked, []);
  // Un clic sur l'entrée du menu quand on y est déjà relit la vue, sans effacer les cases.
  page.actions.toggleTrace('t-edge');
  page.actions.navigate('privacy');
  assert.deepEqual(page.state().privacyChecked, ['t-edge']);
});

test('une vue qui n’arrive pas : la page le dit, « Réessayer » la redemande', async () => {
  const page = start();
  await page.boot();
  page.actions.navigate('privacy');
  page.of('privacyView')[0]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().privacyFailed, true);
  page.actions.retryPrivacy();
  assert.equal(page.state().privacyFailed, false);
  page.of('privacyView')[1]?.resolve(view());
  await flush();
  assert.equal(page.state().privacy?.groups.length, 3);
  // Une panne plus tard garde la dernière vue.
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  page.focusWindow();
  page.of('privacyView')[2]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().privacyFailed, false);
  assert.equal(page.state().privacy?.groups.length, 3);
});

test('une vue arrivée après avoir quitté la page n’est pas gardée', async () => {
  const page = start();
  page.actions.navigate('privacy');
  page.actions.navigate('home');
  page.of('privacyView')[0]?.resolve(view());
  await flush();
  assert.equal(page.state().privacy, null);
});

// ── Cocher ─────────────────────────────────────────────────────────────────

test('cocher et décocher ; une trace inconnue, vide, illisible ou d’un navigateur ouvert ne se coche pas', async () => {
  const page = start();
  const mixed = view([
    CHROME,
    group('brave:0', 'brave', [item('t-empty', { state: 'empty', bytes: null })]),
    group('vivaldi:0', 'vivaldi', [item('t-open')], { running: true }),
    group('opera:0', 'edge', [item('t-bad', { state: 'unreadable', bytes: null })]),
    WINDOWS,
  ]);
  await page.open(mixed);
  for (const id of ['t-empty', 't-open', 't-bad', 'inconnu']) {
    page.actions.toggleTrace(id);
  }
  assert.deepEqual(page.state().privacyChecked, []);
  page.actions.toggleTrace('t-clip');
  page.actions.toggleTrace('t-chrome');
  assert.deepEqual(page.state().privacyChecked, ['t-chrome', 't-clip'], 'dans l’ordre de la page');
  page.actions.toggleTrace('t-clip');
  assert.deepEqual(page.state().privacyChecked, ['t-chrome']);
});

// ── La confirmation ────────────────────────────────────────────────────────

test('« Effacer ce qui est coché » sans rien de coché n’ouvre rien', async () => {
  const page = start();
  await page.open();
  page.actions.askErase();
  assert.equal(page.state().privacyAsk, null);
});

test('Annuler referme la confirmation sans rien envoyer au cœur, et les cases restent', async () => {
  const page = start();
  await page.open();
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  assert.deepEqual(page.state().privacyAsk?.ids, ['t-edge']);
  assert.equal(page.of('eraseTraces').length, 0, 'ouvrir la confirmation n’envoie rien');
  page.actions.dismissErase();
  assert.equal(page.state().privacyAsk, null);
  assert.equal(page.of('eraseTraces').length, 0, 'Annuler n’appelle pas le cœur');
  assert.deepEqual(page.state().privacyChecked, ['t-edge']);
  // Une confirmation qui n'existe plus ne part pas non plus.
  page.actions.confirmErase();
  assert.equal(page.of('eraseTraces').length, 0);
});

// ── Effacer ────────────────────────────────────────────────────────────────

test('le parcours complet : cocher, confirmer, la page est inerte, puis une issue par ligne et des cases vides', async () => {
  const page = start();
  const first = view();
  await page.boot();
  await page.open(first);
  page.actions.toggleTrace('t-clip');
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.confirmErase();

  // Seuls les identifiants que le cœur a donnés partent, dans l'ordre de la page.
  assert.equal(page.of('eraseTraces').length, 1);
  assert.deepEqual(page.of('eraseTraces')[0]?.args, [['t-chrome', 't-clip']]);
  assert.equal(page.state().privacyAsk, null, 'la confirmation est fermée');
  assert.equal(page.state().privacyBusy, true);

  // Pendant l'attente, la page est inerte : on ne coche rien, on ne rouvre rien, la fenêtre ne relit rien.
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  page.actions.confirmErase();
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  page.focusWindow();
  assert.deepEqual(page.state().privacyChecked, ['t-chrome', 't-clip']);
  assert.equal(page.state().privacyAsk, null);
  assert.equal(page.of('eraseTraces').length, 1, 'un seul effacement à la fois');
  assert.equal(page.of('privacyView').length, 1, 'aucune vue demandée pendant l’effacement');

  page.of('eraseTraces')[0]?.resolve({
    view: erased(first, ['t-chrome']),
    results: [
      { id: 't-chrome', outcome: 'erased' },
      { id: 't-clip', outcome: 'failed' },
    ],
  });
  await flush();
  assert.equal(page.state().privacyBusy, false);
  assert.deepEqual(page.state().privacyChecked, [], 'les cases repartent vides, même celle qui a échoué');
  assert.deepEqual(
    page.state().privacyLast.map((line) => [line.trace, line.outcome]),
    [
      ['Historique de navigation', 'erased'],
      ['Presse-papiers', 'failed'],
    ],
  );
  assert.equal(page.state().privacy?.groups[0]?.items[0]?.state, 'empty', 'la vue est celle que le cœur a relue');
  assert.ok(page.state().privacyAnnounce.includes(fr.privacy.outcomeShort.erased));
  assert.ok(page.state().privacyAnnounce.includes(fr.privacy.outcomeShort.failed));
  assert.equal(page.state().privacyRefused, null);
});

test('seuls des identifiants que le cœur a fabriqués partent : jamais un nom, un chemin, ni un identifiant que la vue n’a pas', async () => {
  const page = start();
  const shown = view();
  await page.open(shown);
  for (const id of ['inconnu', '../../Login Data', 'C:\\Users\\Léa', ...checkableIds(shown)]) {
    page.actions.toggleTrace(id);
  }
  page.actions.askErase();
  page.actions.confirmErase();
  const sent = page.of('eraseTraces')[0]?.args[0] as string[];
  assert.deepEqual(sent, checkableIds(shown));
  const known = new Set(shown.groups.flatMap((one) => one.items.map((found) => found.id)));
  assert.ok(sent.every((id) => known.has(id)));
  assert.equal(page.of('eraseTraces')[0]?.args.length, 1, 'rien d’autre que la liste des identifiants');
});

test('le cœur refuse parce qu’un grand vide est en cours : la page le dit, rien n’est effacé, les cases restent', async () => {
  const page = start();
  await page.open();
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[0]?.reject({ kind: 'privacyRefused', reason: 'busy' });
  await flush();
  assert.equal(page.state().privacyBusy, false);
  assert.equal(page.state().privacyRefused, 'busy');
  assert.deepEqual(page.state().privacyLast, [], 'aucune issue : rien n’est parti');
  assert.deepEqual(page.state().privacyChecked, ['t-edge']);
  assert.equal(page.state().privacyAnnounce, fr.privacy.refused.busy);
  // La vue est relue.
  assert.equal(page.of('privacyView').length, 2);
  // Changer une case efface le refus.
  page.of('privacyView')[1]?.resolve(view());
  await flush();
  page.actions.toggleTrace('t-edge');
  assert.equal(page.state().privacyRefused, null);
});

test('trop de traces d’un coup : le refus est dit comme tel', async () => {
  const page = start();
  await page.open();
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[0]?.reject({ kind: 'privacyRefused', reason: 'tooMany' });
  await flush();
  assert.equal(page.state().privacyRefused, 'tooMany');
  assert.equal(page.state().privacyAnnounce, fr.privacy.refused.tooMany);
});

test('une autre panne : un message bref, la page redevient active et relit la vue', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[0]?.reject(new Error('ipc'));
  await flush();
  assert.equal(page.state().privacyBusy, false);
  assert.equal(page.state().toast, fr.toast.unexpected);
  assert.equal(page.state().privacyRefused, null);
  assert.equal(page.of('privacyView').length, 2);
  assert.deepEqual(page.state().privacyChecked, [], 'après une panne, les cases repartent vides');
  assert.equal(page.state().privacyAnnounce, fr.toast.unexpected);
});

test('une vue demandée avant l’effacement et arrivée après lui est ignorée', async () => {
  const page = start();
  await page.boot();
  await page.open();
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  page.focusWindow();
  assert.equal(page.of('privacyView').length, 2);
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.confirmErase();
  const after = erased(view(), ['t-chrome']);
  page.of('eraseTraces')[0]?.resolve({ view: after, results: [{ id: 't-chrome', outcome: 'erased' }] });
  await flush();
  // L'ancienne vue arrive en retard : elle montrerait encore l'historique.
  page.of('privacyView')[1]?.resolve(view());
  await flush();
  assert.equal(page.state().privacy?.groups[0]?.items[0]?.state, 'empty');
});

test('la personne est partie pendant l’effacement : les cases repartent vides, mais le dernier effacement reste', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.confirmErase();
  page.actions.navigate('home');
  page.actions.navigate('privacy');
  page.of('eraseTraces')[0]?.resolve({ view: erased(view(), ['t-chrome']), results: [{ id: 't-chrome', outcome: 'erased' }] });
  await flush();
  assert.equal(page.state().privacyBusy, false);
  // « Dernier effacement » est gardé jusqu'au prochain effacement : la personne retrouve ce qui s'est passé.
  assert.deepEqual(page.state().privacyLast.map((line) => [line.trace, line.outcome]), [['Historique de navigation', 'erased']]);
  assert.deepEqual(page.state().privacyChecked, []);
  // Elle relit la vue au lieu de croire la réponse (pendant l'effacement, la page revenue n'en demandait aucune).
  assert.equal(page.of('privacyView').length, 2);
  page.of('privacyView')[1]?.resolve(erased(view(), ['t-chrome']));
  await flush();
  assert.equal(page.state().privacy?.groups[0]?.items[0]?.state, 'empty');
});

test('un nouvel effacement efface les issues du précédent', async () => {
  const page = start();
  await page.open();
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[0]?.resolve({ view: view(), results: [{ id: 't-edge', outcome: 'inUse' }] });
  await flush();
  assert.equal(page.state().privacyLast.length, 1);
  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  page.actions.confirmErase();
  assert.deepEqual(page.state().privacyLast, []);
  assert.equal(page.state().privacyAnnounce, '');
});

// ── Liste figée, issues inconnues, retour sur la page ─────────────────────

const RUNNING_CHROME = group('chrome:0', 'chrome', [item('t-chrome')], { running: true });

test('la liste change pendant la confirmation (Chrome s’ouvre) : la confirmation se ferme, rien ne part, la page le dit', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.toggleTrace('t-clip');
  page.actions.askErase();
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  page.focusWindow();
  page.of('privacyView')[1]?.resolve(view([RUNNING_CHROME, EDGE, WINDOWS]));
  await flush();
  assert.equal(page.state().privacyAsk, null, 'la confirmation est fermée');
  assert.equal(page.state().privacyRefused, 'listChanged');
  assert.equal(page.state().privacyAnnounce, fr.privacy.refused.listChanged);
  page.actions.confirmErase();
  assert.equal(page.of('eraseTraces').length, 0, 'rien n’est envoyé, pas même la trace de Windows');
});

test('garde juste avant l’envoi : une trace de la liste figée n’est plus cochable, rien ne part, jamais d’envoi partiel', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.toggleTrace('t-clip');
  page.actions.askErase();
  // La vue change sans passer par la relecture : seule la garde de l'envoi peut le voir.
  page.app['set']({ privacy: view([RUNNING_CHROME, EDGE, WINDOWS]) });
  page.actions.confirmErase();
  assert.equal(page.of('eraseTraces').length, 0);
  assert.equal(page.state().privacyAsk, null);
  assert.equal(page.state().privacyBusy, false);
  assert.equal(page.state().privacyRefused, 'listChanged');
  assert.equal(page.state().privacyAnnounce, fr.privacy.refused.listChanged);
});

test('la liste est figée à l’ouverture : cocher autre chose pendant la confirmation n’ajoute rien à l’envoi', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.toggleTrace('t-edge');
  page.actions.confirmErase();
  assert.deepEqual(page.of('eraseTraces')[0]?.args, [['t-chrome']]);
});

test('une issue absente, inconnue ou une réponse vide : chaque trace demandée garde sa ligne « Résultat inconnu »', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.toggleTrace('t-edge');
  page.actions.toggleTrace('t-clip');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[0]?.resolve({
    view: view(),
    results: [
      { id: 't-chrome', outcome: 'erased' },
      { id: 't-edge', outcome: 'surprise' },
    ],
  });
  await flush();
  assert.deepEqual(
    page.state().privacyLast.map((line) => [line.trace, line.outcome]),
    [
      ['Historique de navigation', 'erased'],
      ['Historique de navigation', 'unknown'],
      ['Presse-papiers', 'unknown'],
    ],
  );
  assert.ok(!page.state().privacyAnnounce.includes('undefined'));
  assert.ok(page.state().privacyAnnounce.includes('Résultat inconnu'));

  page.actions.toggleTrace('t-edge');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[1]?.resolve({ view: view(), results: [] });
  await flush();
  assert.deepEqual(page.state().privacyLast.map((line) => line.outcome), ['unknown']);
});

test('quitter puis revenir sur la page garde « Dernier effacement » ; un nouvel effacement le remplace', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[0]?.resolve({ view: erased(view(), ['t-chrome']), results: [{ id: 't-chrome', outcome: 'erased' }] });
  await flush();
  page.actions.navigate('home');
  page.actions.navigate('privacy');
  assert.equal(page.state().privacyLast.length, 1);
  assert.deepEqual(page.state().privacyChecked, []);
});

test('la confirmation se referme parce que la liste a changé : le focus va sur « Effacer ce qui est coché » de l’en-tête, jamais sur le corps', async () => {
  const fake = fakeBackend();
  const focused: string[] = [];
  const buttons = [{ dataset: { fk: 'autre' }, focus: () => focused.push('autre') }, { dataset: { fk: privacyFocus.erase }, focus: () => focused.push('erase') }];
  const root = { querySelectorAll: () => buttons } as unknown as HTMLElement;
  const app = new App(fake.backend, root, () => {});
  const actions = app['actions'];
  actions.navigate('privacy');
  fake.of('privacyView')[0]?.resolve(view());
  await flush();
  actions.toggleTrace('t-chrome');
  actions.askErase();
  app['set']({ privacy: view([RUNNING_CHROME, EDGE, WINDOWS]) });
  environment.active = null;
  actions.confirmErase();
  await flush();
  assert.deepEqual(focused, ['erase']);
});

test('seul un poids change dans la vue relue : la confirmation reste ouverte, sa liste est la même, aucun refus', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.toggleTrace('t-recent');
  page.actions.askErase();
  const before = page.state().privacyAsk;
  mock.timers.tick(PRIVACY_RELOAD_GAP);
  page.focusWindow();
  page.of('privacyView')[1]?.resolve(
    view([group('chrome:0', 'chrome', [item('t-chrome', { bytes: 4096 })]), EDGE, group('windows', null, [item('t-recent', { kind: 'recent', bytes: null, count: 9 }), item('t-typed', { kind: 'typed', bytes: null, count: 3 }), item('t-clip', { kind: 'clipboard', bytes: null })])]),
  );
  await flush();
  assert.equal(page.state().privacyAsk, before, 'la confirmation est celle d’avant');
  assert.equal(page.state().privacyRefused, null);
  page.actions.confirmErase();
  assert.deepEqual(page.of('eraseTraces')[0]?.args, [['t-chrome', 't-recent']]);
});

test('au retour sur la page : la carte reste avec son heure, aucune ligne ne porte l’issue ; recocher la retire aussi', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[0]?.resolve({ view: view(), results: [{ id: 't-chrome', outcome: 'erased' }] });
  await flush();
  assert.equal(page.state().privacyLineOutcomes, true);
  assert.equal(page.state().privacyLastAt, NOW);
  page.actions.toggleTrace('t-edge');
  assert.equal(page.state().privacyLineOutcomes, false, 'cocher de nouveau retire l’issue des lignes');
  assert.equal(page.state().privacyLast.length, 1, 'la carte reste');
  // Retour sur la page : même chose.
  page.actions.askErase();
  page.actions.confirmErase();
  page.of('eraseTraces')[1]?.resolve({ view: view(), results: [{ id: 't-edge', outcome: 'erased' }] });
  await flush();
  assert.equal(page.state().privacyLineOutcomes, true);
  page.actions.navigate('home');
  page.actions.navigate('privacy');
  assert.equal(page.state().privacyLineOutcomes, false);
  assert.equal(page.state().privacyLast.length, 1);
  assert.equal(page.state().privacyLastAt, NOW);
});

test('une issue arrivée pendant que la personne était ailleurs est annoncée une fois au retour, puis plus', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.confirmErase();
  page.actions.navigate('home');
  page.of('eraseTraces')[0]?.resolve({ view: erased(view(), ['t-chrome']), results: [{ id: 't-chrome', outcome: 'erased' }] });
  await flush();
  assert.equal(page.state().privacyAnnounce, '', 'pas d’annonce ailleurs');
  page.actions.navigate('privacy');
  assert.ok(page.state().privacyAnnounce.includes('Effacement terminé'));
  page.actions.navigate('home');
  page.actions.navigate('privacy');
  assert.equal(page.state().privacyAnnounce, '', 'puis plus');
});

test('revenue sur la page avant la réponse : l’issue est annoncée tout de suite, et pas une seconde fois à un autre retour', async () => {
  const page = start();
  await page.boot();
  await page.open();
  page.actions.toggleTrace('t-chrome');
  page.actions.askErase();
  page.actions.confirmErase();
  page.actions.navigate('home');
  page.actions.navigate('privacy');
  page.of('eraseTraces')[0]?.resolve({ view: erased(view(), ['t-chrome']), results: [{ id: 't-chrome', outcome: 'erased' }] });
  await flush();
  assert.ok(page.state().privacyAnnounce.includes('Effacement terminé'));
  page.actions.navigate('home');
  page.actions.navigate('privacy');
  assert.equal(page.state().privacyAnnounce, '');
});

// ── Dans Windows ───────────────────────────────────────────────────────────

test('les trois pages de Windows s’ouvrent par la commande qui ouvre une page de Windows', async () => {
  const page = start();
  for (const name of ['activityHistory', 'searchPermissions', 'diagnostics'] as const) {
    page.actions.openWindowsSettings(name);
  }
  assert.deepEqual(
    page.of('openWindowsSettings').map((call) => call.args),
    [['activityHistory'], ['searchPermissions'], ['diagnostics']],
  );
});
