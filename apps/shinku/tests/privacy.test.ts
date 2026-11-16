// Ce que la page Confidentialité tire d'une vue du cœur (profils, poids, ce qui se coche, la
// confirmation, les issues), et ce que ses gabarits écrivent : chaque état de ligne, les phrases
// « Ce qui reste », rien de coché d'avance, aucun des mots qu'on ne doit jamais écrire.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { html } from 'lit-html';
import { unsafeHTML } from 'lit-html/directives/unsafe-html.js';
import { test } from 'node:test';

import type { PrivacyGroup, PrivacyView, TraceItem, TraceKind, TraceOutcome } from '../src/backend/types.ts';
import { formatBytes, NBSP } from '../src/format.ts';
import { fr } from '../src/i18n/fr.ts';
import {
  announceLines,
  askFor,
  browserName,
  checkableIds,
  checkLabel,
  groupTitle,
  hasBrowser,
  isCheckable,
  lastLines,
  askHolds,
  nothingToErase,
  outcomeHelp,
  outcomeLine,
  outcomeShort,
  profileLabel,
  remainsOfBrowsers,
  remainsOfWindows,
  browserSections,
  rowKind,
  stillChecked,
  toggled,
  traceSize,
  WINDOWS_PAGES,
} from '../src/model/privacy.ts';
import { type Node, parse, query, queryAll, textOfNode } from './html-tree.ts';
import { clicksOf, fakeDocument, htmlOf, markupOf, textOf } from './template.ts';

fakeDocument();
const { privacyDialog, privacyView } = await import('../src/views/privacy.ts');
const { initialState } = await import('../src/state.ts');

const NOW = new Date(2026, 10, 16, 12).getTime();

// ── Une machine d'exemple ──────────────────────────────────────────────────

function trace(id: string, kind: TraceKind, parts: Partial<TraceItem> = {}): TraceItem {
  return { id, kind, state: 'present', bytes: null, count: null, ...parts };
}

function browser(
  app: NonNullable<PrivacyGroup['app']>,
  slot: number,
  profile: string | null,
  history: Partial<TraceItem>,
  running = false,
): PrivacyGroup {
  const key = `${app}:${slot}`;
  return { key, kind: 'browser', app, profile, slot, running, items: [trace(`${key}-h`, 'history', history)] };
}

const CHROME_MAIN = browser('chrome', 0, 'Léa', { bytes: 48.3 * 1024 ** 2 });
const CHROME_TWO = browser('chrome', 2, null, { state: 'empty' });
const CHROME_THREE = browser('chrome', 3, null, { bytes: 9 * 1024 ** 2 });
const EDGE = browser('edge', 0, null, { bytes: 12 * 1024 ** 2 });
const BRAVE = browser('brave', 0, null, { state: 'unreadable' });
const VIVALDI_OPEN = browser('vivaldi', 0, null, { bytes: 3 * 1024 ** 2 }, true);

const WINDOWS: PrivacyGroup = {
  key: 'windows',
  kind: 'windows',
  app: null,
  profile: null,
  slot: null,
  running: false,
  items: [
    trace('w-recent', 'recent', { count: 23 }),
    trace('w-typed', 'typed', { count: 1 }),
    trace('w-clip', 'clipboard'),
  ],
};

function view(groups: PrivacyGroup[] = [CHROME_MAIN, CHROME_TWO, EDGE, WINDOWS], parts: Partial<PrivacyView> = {}): PrivacyView {
  return { groups, firefox: false, edgeImportsChrome: groups.some((g) => g.app === 'chrome') && groups.some((g) => g.app === 'edge'), ...parts };
}

const FULL = view([CHROME_MAIN, CHROME_TWO, CHROME_THREE, EDGE, BRAVE, VIVALDI_OPEN, WINDOWS], { firefox: true, edgeImportsChrome: true });

// ── Le modèle ──────────────────────────────────────────────────────────────

test('le libellé d’un profil : le nom du navigateur tel quel, sinon « Profil principal » ou « Profil N »', () => {
  assert.equal(profileLabel(CHROME_MAIN), 'Léa');
  assert.equal(profileLabel(CHROME_TWO), 'Profil 2');
  assert.equal(profileLabel(browser('edge', 0, null, {})), 'Profil principal');
  assert.equal(profileLabel(browser('edge', 12, null, {})), 'Profil 12');
  // Un nom que le navigateur a choisi n'est jamais retouché par la typographie : ni espace insécable, ni espace fine.
  const odd = browser('chrome', 1, 'Alerte ! Boulot : compta ?', {});
  assert.equal(profileLabel(odd), 'Alerte ! Boulot : compta ?');
  assert.equal(groupTitle(odd), 'Chrome · Alerte ! Boulot : compta ?');
  assert.ok(!/[  ]/.test(groupTitle(odd)));
  assert.equal(checkLabel(odd, odd.items[0] as TraceItem), 'Chrome, Alerte ! Boulot : compta ?, historique de navigation');
  assert.equal(checkLabel(WINDOWS, WINDOWS.items[2] as TraceItem), 'Windows, Presse-papiers');
  assert.equal(groupTitle(WINDOWS), 'Windows');
  assert.equal(groupTitle(CHROME_MAIN), 'Chrome · Léa');
  assert.equal(browserName(EDGE), 'Edge');
});

test('le poids d’un fichier, le nombre d’éléments, ou rien pour une trace vide ou illisible', () => {
  assert.equal(traceSize(CHROME_MAIN.items[0] as TraceItem), formatBytes(48.3 * 1024 ** 2));
  assert.equal(traceSize(WINDOWS.items[0] as TraceItem), `23${NBSP}raccourcis`);
  assert.equal(traceSize(WINDOWS.items[1] as TraceItem), `1${NBSP}élément`);
  // Le presse-papiers n'a aucune mesure (ni nombre, ni « Du contenu ») : le cœur ne sait pas ce que l'historique contient. Il reste cochable.
  assert.equal(traceSize(WINDOWS.items[2] as TraceItem), null);
  assert.equal(traceSize({ ...(WINDOWS.items[2] as TraceItem), count: 3 }), null);
  assert.equal(isCheckable(WINDOWS, WINDOWS.items[2] as TraceItem), true);
  assert.ok(!('someContent' in fr.privacy));
  assert.equal(traceSize(CHROME_TWO.items[0] as TraceItem), null);
  assert.equal(traceSize(BRAVE.items[0] as TraceItem), null);
});

test('seule une trace présente d’un navigateur fermé se coche', () => {
  const states = FULL.groups.flatMap((group) => group.items.map((item) => `${group.key}:${rowKind(group, item)}`));
  assert.deepEqual(states, [
    'chrome:0:check',
    'chrome:2:empty',
    'chrome:3:check',
    'edge:0:check',
    'brave:0:unreadable',
    'vivaldi:0:running',
    'windows:check',
    'windows:check',
    'windows:check',
  ]);
  assert.equal(isCheckable(VIVALDI_OPEN, VIVALDI_OPEN.items[0] as TraceItem), false);
  assert.deepEqual(checkableIds(FULL), ['chrome:0-h', 'chrome:3-h', 'edge:0-h', 'w-recent', 'w-typed', 'w-clip']);
  assert.equal(nothingToErase(FULL), false);
  assert.equal(nothingToErase(view([CHROME_TWO, BRAVE])), true);
  // Des traces présentes, mais aucune ne se coche (navigateur ouvert) : rien à effacer pour l'instant.
  assert.equal(nothingToErase(view([VIVALDI_OPEN, { ...WINDOWS, items: WINDOWS.items.map((item) => ({ ...item, state: 'empty' as const })) }])), true);
});

test('cocher : un identifiant inconnu ou inerte ne change rien, et l’ordre est celui de la page', () => {
  assert.deepEqual(toggled(FULL, [], 'edge:0-h'), ['edge:0-h']);
  assert.deepEqual(toggled(FULL, ['edge:0-h'], 'chrome:0-h'), ['chrome:0-h', 'edge:0-h']);
  assert.deepEqual(toggled(FULL, ['edge:0-h', 'chrome:0-h'], 'edge:0-h'), ['chrome:0-h']);
  assert.deepEqual(toggled(FULL, [], 'inconnu'), []);
  assert.deepEqual(toggled(FULL, [], 'vivaldi:0-h'), [], 'un navigateur ouvert ne se coche pas');
  assert.deepEqual(toggled(FULL, [], 'chrome:2-h'), [], 'une trace vide ne se coche pas');
  assert.deepEqual(toggled(FULL, [], 'brave:0-h'), [], 'une trace illisible ne se coche pas');
  // Une vue relue : ce qui ne se coche plus est décoché.
  assert.deepEqual(stillChecked(view([CHROME_MAIN, { ...EDGE, running: true }]), ['chrome:0-h', 'edge:0-h', 'parti']), ['chrome:0-h']);
});

test('la confirmation liste ce qui est coché, par bloc, et rien d’autre', () => {
  assert.equal(askFor(FULL, []), null);
  assert.equal(askFor(FULL, ['vivaldi:0-h']), null, 'rien de cochable : pas de confirmation');
  const ask = askFor(FULL, ['w-clip', 'edge:0-h', 'chrome:0-h', 'vivaldi:0-h', 'inconnu']);
  assert.deepEqual(ask?.ids, ['chrome:0-h', 'edge:0-h', 'w-clip'], 'les identifiants du cœur, dans l’ordre de la page, sans le navigateur ouvert ni l’inconnu');
  assert.deepEqual(
    ask?.blocks.map((block) => [block.title, block.traces.map((one) => one.name)]),
    [
      ['Chrome', [`Léa${NBSP}: historique de navigation`]],
      ['Edge', [`Profil principal${NBSP}: historique de navigation`]],
      ['Windows', ['Presse-papiers']],
    ],
  );
  // Un navigateur : un seul titre, une puce par profil coché.
  const two = askFor(FULL, ['chrome:3-h', 'chrome:0-h']);
  assert.deepEqual(two?.blocks.map((block) => [block.title, block.traces.length]), [['Chrome', 2]]);
});

test('l’issue de chaque trace demandée, dans l’ordre ; une issue qu’on n’a pas demandée est ignorée', () => {
  const ask = askFor(FULL, ['chrome:0-h', 'w-recent']);
  assert.ok(ask !== null);
  const lines = lastLines(ask, [
    { id: 'w-recent', outcome: 'partial' },
    { id: 'chrome:0-h', outcome: 'erased' },
    { id: 'edge:0-h', outcome: 'erased' },
  ]);
  assert.deepEqual(
    lines.map((line) => [line.block, line.trace, line.browser, line.profile, line.outcome]),
    [
      ['Chrome · Léa', 'Historique de navigation', 'Chrome', 'Léa', 'erased'],
      ['Windows', 'Fichiers et dossiers récents', null, null, 'partial'],
    ],
  );
  assert.equal(
    announceLines(lines),
    `Effacement terminé. Chrome · Léa, Historique de navigation${NBSP}: Effacé. Windows, Fichiers et dossiers récents${NBSP}: Effacé en partie. Il en reste${NBSP}: regarde de nouveau.`,
  );
});

test('une ligne pour chaque trace demandée : absente des résultats ou issue inconnue, c’est « Résultat inconnu », jamais « Effacé »', () => {
  const ask = askFor(FULL, ['chrome:0-h', 'edge:0-h', 'w-recent']);
  assert.ok(ask !== null);
  // Le cœur ne dit rien.
  const none = lastLines(ask, []);
  assert.deepEqual(none.map((line) => line.outcome), ['unknown', 'unknown', 'unknown']);
  // Une issue qu'aucun des six ne nomme, ou qui n'est pas du texte.
  const odd = lastLines(ask, [
    { id: 'chrome:0-h', outcome: 'deleted' as never },
    { id: 'edge:0-h', outcome: undefined as never },
    { id: 'w-recent', outcome: 'erased' },
  ]);
  assert.deepEqual(odd.map((line) => line.outcome), ['unknown', 'unknown', 'erased']);
  for (const line of none) {
    assert.equal(outcomeShort(line.outcome), 'Résultat inconnu');
    assert.equal(outcomeHelp(line), 'Regarde de nouveau.');
    assert.equal(outcomeLine(line).includes('Effacé'), false);
  }
  const spoken = announceLines(none);
  assert.ok(!/undefined|Effacé/.test(spoken), spoken);
  assert.equal((spoken.match(/Résultat inconnu/g) ?? []).length, 3);
  // Une issue pour un identifiant qu'on n'a pas demandé n'ajoute pas de ligne.
  assert.equal(lastLines(ask, [{ id: 'w-clip', outcome: 'erased' }]).length, 3);
});

test('les issues courtes : six, plus « inconnue » ; « changed » d’une trace de Windows ne parle pas de profil', () => {
  const expected: Record<TraceOutcome, string> = {
    erased: 'Effacé',
    nothing: 'Rien à effacer',
    inUse: 'En cours d’utilisation',
    changed: 'Profil changé',
    partial: 'Effacé en partie',
    failed: 'Pas pu être effacé',
  };
  for (const [outcome, text] of Object.entries(expected)) {
    assert.equal(outcomeShort(outcome as TraceOutcome), text);
  }
  assert.equal(outcomeShort('changed', true), 'A changé');
  assert.equal(outcomeShort('inUse', true), 'En cours d’utilisation');
  assert.equal(outcomeLine({ browser: null, outcome: 'changed' }), 'A changé. Regarde de nouveau.');
  assert.equal(outcomeLine({ browser: 'Chrome', outcome: 'changed' }), 'Profil changé. Regarde de nouveau.');
});

test('la confirmation vaut tant que chacune de ses traces se coche encore', () => {
  const ask = askFor(FULL, ['chrome:0-h', 'edge:0-h']);
  assert.ok(ask !== null);
  assert.equal(askHolds(FULL, ask), true);
  assert.equal(askHolds(view([{ ...CHROME_MAIN, running: true }, EDGE, WINDOWS]), ask), false, 'navigateur ouvert depuis');
  assert.equal(askHolds(view([EDGE, WINDOWS]), ask), false, 'profil disparu');
  assert.equal(askHolds(view([{ ...CHROME_MAIN, items: [trace('chrome:0-h', 'history', { state: 'empty' })] }, EDGE]), ask), false, 'trace devenue vide');
  assert.equal(askHolds(view([{ ...CHROME_MAIN, items: [trace('chrome:0-h', 'history', { state: 'unreadable' })] }, EDGE]), ask), false, 'trace devenue illisible');
});

test('deux profils du même nom dans le même navigateur : le dossier s’ajoute, seulement alors', () => {
  const lea0 = browser('chrome', 0, 'Léa', { bytes: 1 });
  const lea3 = browser('chrome', 3, 'Léa', { bytes: 2 });
  const edgeLea = browser('edge', 0, 'Léa', { bytes: 3 });
  const both = view([lea0, lea3, edgeLea, WINDOWS]);
  assert.equal(profileLabel(lea0, both), 'Léa (Profil principal)');
  assert.equal(profileLabel(lea3, both), 'Léa (Profil 3)');
  assert.equal(profileLabel(edgeLea, both), 'Léa', 'un autre navigateur ne fait pas doublon');
  assert.equal(profileLabel(CHROME_MAIN, FULL), 'Léa', 'sans doublon, le nom seul');
  assert.equal(groupTitle(lea3, both), 'Chrome · Léa (Profil 3)');
  assert.equal(checkLabel(lea3, lea3.items[0] as TraceItem, both), 'Chrome, Léa (Profil 3), historique de navigation');
  const ask = askFor(both, ['chrome:0-h', 'chrome:3-h']);
  assert.deepEqual(ask?.blocks[0]?.traces.map((one) => one.name), [
    `Léa (Profil principal)${NBSP}: historique de navigation`,
    `Léa (Profil 3)${NBSP}: historique de navigation`,
  ]);
  const lines = lastLines(ask as NonNullable<typeof ask>, []);
  assert.deepEqual(lines.map((line) => line.block), ['Chrome · Léa (Profil principal)', 'Chrome · Léa (Profil 3)']);
  // La page : deux lignes, deux noms accessibles différents.
  const names = queryAll(page({}, both).tree, 'input[type="checkbox"]').map((box) => flat(box.attrs['aria-label'] ?? ''));
  assert.deepEqual(names.slice(0, 2), ['Chrome, Léa (Profil principal), historique de navigation', 'Chrome, Léa (Profil 3), historique de navigation']);
});

test('un nom de profil vide ou fait d’espaces est comme absent', () => {
  assert.equal(profileLabel(browser('chrome', 0, '', {})), 'Profil principal');
  assert.equal(profileLabel(browser('chrome', 5, '  \u00a0 ', {})), 'Profil 5');
});

test('« Ce qui reste » : une fois pour les navigateurs, la phrase d’Edge seulement si Edge peut réimporter Chrome, une pour Windows', () => {
  assert.deepEqual(remainsOfBrowsers(FULL), [fr.privacy.remainsBrowser, fr.privacy.remainsEdge]);
  assert.deepEqual(remainsOfBrowsers(view([EDGE, WINDOWS], { edgeImportsChrome: false })), [fr.privacy.remainsBrowser]);
  assert.deepEqual(remainsOfWindows(), [fr.privacy.remainsWindows]);
  assert.equal(hasBrowser(FULL), true);
  assert.equal(hasBrowser(view([WINDOWS])), false);
});

test('les profils d’un même navigateur sont réunis, dans l’ordre reçu', () => {
  const mixed = view([CHROME_MAIN, EDGE, CHROME_THREE, VIVALDI_OPEN, WINDOWS]);
  assert.deepEqual(
    browserSections(mixed).map((section) => [section.name, section.groups.map((one) => one.key), section.running]),
    [['Chrome', ['chrome:0', 'chrome:3'], false], ['Edge', ['edge:0'], false], ['Vivaldi', ['vivaldi:0'], true]],
  );
});

test('l’issue d’une trace : deux mots, et une aide qui nomme le navigateur ou l’app', () => {
  assert.equal(outcomeShort('inUse'), `En cours d’utilisation`);
  assert.equal(outcomeShort('changed'), 'Profil changé');
  assert.equal(outcomeHelp({ browser: 'Chrome', outcome: 'inUse' }), 'Ferme Chrome et recommence.');
  assert.equal(outcomeHelp({ browser: null, outcome: 'inUse' }), 'Ferme l’app et recommence.');
  assert.equal(outcomeHelp({ browser: null, outcome: 'changed' }), 'Regarde de nouveau.');
  assert.equal(outcomeHelp({ browser: null, outcome: 'partial' }), `Il en reste${NBSP}: regarde de nouveau.`);
  assert.equal(outcomeHelp({ browser: null, outcome: 'erased' }), null);
  assert.equal(outcomeLine({ browser: 'Edge', outcome: 'inUse' }), 'En cours d’utilisation. Ferme Edge et recommence.');
  assert.equal(outcomeLine({ browser: null, outcome: 'erased' }), 'Effacé');
});

test('les phrases de la conception sont reprises mot pour mot', () => {
  assert.equal(
    fr.privacy.browsers.intro,
    `Effacer l’historique d’un profil, c’est perdre${NBSP}: les pages visitées, la liste des téléchargements (pas les fichiers\u202f; un téléchargement interrompu ne pourra plus reprendre), les suggestions de la barre d’adresse. Les icônes de tes favoris reviennent à la prochaine visite.`,
  );
  assert.equal(
    fr.privacy.remainsBrowser,
    `Tes onglets ouverts et récemment fermés. Ce que la synchronisation garde dans ton compte en ligne, et peut faire revenir. Ce que tes extensions ont noté. Les cookies et les données des sites${NBSP}: pour eux, passe par le navigateur (Ctrl+Maj+Suppr).`,
  );
  assert.equal(fr.privacy.remainsEdge, 'Edge peut réimporter l’historique de Chrome à son lancement, si tu l’as réglé ainsi.');
  assert.equal(fr.privacy.firefox, `Firefox${NBSP}: efface depuis Firefox lui-même (Ctrl+Maj+Suppr).`);
  assert.equal(fr.privacy.cache, `Le cache des navigateurs se vide avec «${NBSP}Faire le vide${NBSP}», sur l’accueil.`);
  assert.equal(fr.privacy.confirm.direct, 'Elles sont supprimées directement, sans passer par la Corbeille.');
  assert.equal(fr.privacy.running('Chrome').startsWith(`Ferme Chrome pour effacer son historique.`), true);
  assert.equal(fr.privacy.intro, 'Des traces de ton activité sur ce PC, dans ton compte. Rien n’est coché d’avance.');
  assert.equal(fr.privacy.erase, 'Effacer ce qui est coché');
});

// ── Les textes ─────────────────────────────────────────────────────────────

/** Tous les textes du module, y compris ceux qui se calculent. */
function moduleTexts(group: object, path = 'fr.privacy'): Array<[string, string]> {
  return Object.entries(group).flatMap(([key, value]): Array<[string, string]> => {
    const name = `${path}.${key}`;
    if (typeof value === 'string') {
      return [[name, value]];
    }
    if (typeof value === 'function') {
      return [[name, (value as (...args: unknown[]) => string)('Chrome', 'Historique de navigation', 'Effacé')]];
    }
    return moduleTexts(value as object, name);
  });
}

const FORBIDDEN = /pour de bon|définitivement|sans laisser de trace|sécuris/i;

test('aucun texte du module ne promet ce que Shinku ne peut pas tenir', () => {
  const texts = moduleTexts(fr.privacy);
  assert.ok(texts.length > 40);
  for (const [name, text] of texts) {
    assert.doesNotMatch(text, FORBIDDEN, `${name} : « ${text} »`);
  }
  // Le texte d'attente de l'ancienne page « bientôt » a disparu, partout.
  for (const [name, text] of moduleTexts(fr, 'fr')) {
    assert.doesNotMatch(text, /cookies de suivi/i, `${name} : « ${text} »`);
  }
  assert.ok(!('privacy' in fr.soon));
});

// ── Les gabarits ───────────────────────────────────────────────────────────

interface Calls {
  asked: number;
  confirmed: number;
  dismissed: number;
  toggled: string[];
  windows: string[];
}

function actions(): { calls: Calls; actions: never } {
  const calls: Calls = { asked: 0, confirmed: 0, dismissed: 0, toggled: [], windows: [] };
  const stub = {
    askErase: () => (calls.asked += 1),
    confirmErase: () => (calls.confirmed += 1),
    dismissErase: () => (calls.dismissed += 1),
    toggleTrace: (id: string) => calls.toggled.push(id),
    openWindowsSettings: (page: string) => calls.windows.push(page),
    retryPrivacy: () => {},
  };
  return { calls, actions: stub as never };
}

function page(parts: Record<string, unknown> = {}, shown: PrivacyView | null = FULL) {
  const { calls, actions: stub } = actions();
  const state = { ...initialState(NOW), route: 'privacy', privacy: shown, ...parts } as never;
  const template = privacyView(state, stub);
  return { calls, template, tree: parse(`<main class="main">${markupOf(template)}</main>`), text: textOf(template) };
}

const flat = (text: string): string => text.replace(/[  \s]+/g, ' ').trim();

test('rien n’est coché d’avance : l’état de départ, et chaque case, sont vides', () => {
  assert.deepEqual(initialState(NOW).privacyChecked, []);
  const { template } = page();
  const written = htmlOf(template);
  const boxes = written.match(/type="checkbox"/g) ?? [];
  assert.equal(boxes.length, 6, 'six traces se cochent dans cet exemple');
  assert.equal((written.match(/\.checked=/g) ?? []).length, 6, 'chaque case lit son état');
  assert.equal((written.match(/\.checked=true/g) ?? []).length, 0);
  // Cochée par la personne : seule cette case l'est.
  const one = htmlOf(page({ privacyChecked: ['edge:0-h'] }).template);
  assert.equal((one.match(/\.checked=true/g) ?? []).length, 1);
});

test('chaque case a un nom accessible complet : le navigateur, le profil, la trace', () => {
  const { tree } = page();
  const names = queryAll(tree, 'input[type="checkbox"]').map((box) => flat(box.attrs['aria-label'] ?? ''));
  assert.deepEqual(names, [
    'Chrome, Léa, historique de navigation',
    'Chrome, Profil 3, historique de navigation',
    'Edge, Profil principal, historique de navigation',
    'Windows, Fichiers et dossiers récents',
    'Windows, Ce que tu as tapé dans Windows',
    'Windows, Presse-papiers',
  ]);
});

test('une section « Navigateurs » avec une carte par navigateur, puis Windows, puis « Dans Windows », puis le bloc replié', () => {
  const { tree } = page();
  const titles = queryAll(tree, 'h2').map((node) => flat(textOfNode(node)));
  assert.deepEqual(titles, [fr.privacy.browsers.title, 'Windows', fr.privacy.inWindows.title].map(flat));
  const cards = queryAll(tree, '.pbrowser');
  assert.deepEqual(cards.map((card) => flat(textOfNode(query(card, 'h3') as Node))), ['Chrome', 'Edge', 'Brave', 'Vivaldi']);
  // Une ligne par profil.
  assert.deepEqual(queryAll(cards[0] as Node, 'li').length, 3);
  // Chaque carte et chaque section a son titre pour les lecteurs d'écran.
  for (const labelled of [...queryAll(tree, 'section[aria-labelledby]'), ...cards]) {
    assert.ok(queryAll(tree, `[id="${labelled.attrs['aria-labelledby']}"]`).length === 1, labelled.attrs['aria-labelledby']);
  }
  const fold = query(tree, 'details');
  assert.ok(fold !== null);
  assert.ok(!('open' in fold.attrs), 'le bloc « Ce que Shinku ne touche pas » est replié');
  assert.equal(flat(textOfNode(query(fold, 'summary') as Node)), flat(fr.privacy.untouched.title));
  // Un seul profil dans un navigateur : la même forme (une carte, une ligne).
  const single = page({}, view([CHROME_MAIN, WINDOWS])).tree;
  const one = queryAll(single, '.pbrowser');
  assert.equal(one.length, 1);
  assert.equal(queryAll(one[0] as Node, 'li').length, 1);
});

const occurrences = (text: string, part: string): number => flat(text).split(flat(part)).length - 1;

test('ce qu’on perd et « Ce qui reste » s’écrivent une fois pour les navigateurs, jamais répétés par profil', () => {
  const { text, tree } = page();
  assert.equal(occurrences(text, fr.privacy.browsers.intro), 1, 'ce qu’on perd : une fois');
  assert.equal(occurrences(text, fr.privacy.remainsBrowser), 1, '« Ce qui reste » des navigateurs : une fois');
  assert.equal(occurrences(text, fr.privacy.remainsEdge), 1);
  assert.equal(occurrences(text, fr.privacy.remainsWindows), 1);
  assert.equal(occurrences(text, fr.privacy.cache), 1);
  // Windows garde ce qu'on perd, ligne par ligne.
  for (const kind of ['recent', 'typed', 'clipboard'] as const) {
    assert.ok(flat(text).includes(flat(fr.privacy.loses[kind])), kind);
  }
  assert.ok(flat(text).includes(flat(formatBytes(48.3 * 1024 ** 2))));
  assert.ok(flat(text).includes(flat(`23${NBSP}raccourcis`)));
  // Une ligne de profil : son nom en premier, « Historique de navigation » en second, le poids à droite.
  const first = queryAll(tree, '.pbrowser li')[0] as Node;
  assert.equal(flat(textOfNode(query(first, '.t1') as Node)), 'Léa');
  assert.equal(flat(textOfNode(query(first, '.t2') as Node)), flat(fr.privacy.trace.history));
  assert.equal(flat(textOfNode(query(first, '.val') as Node)), flat(formatBytes(48.3 * 1024 ** 2)));
  // Les phrases du bas sont dans le même conteneur : le même bord gauche.
  const remains = queryAll(tree, 'section.pblock')[0] as Node;
  const premain = queryAll(remains, '.premain');
  assert.equal(premain.length, 1);
  const inside = flat(textOfNode(premain[0] as Node));
  for (const part of [fr.privacy.remainsLabel, fr.privacy.remainsBrowser, fr.privacy.remainsEdge, fr.privacy.firefox, fr.privacy.cache]) {
    assert.ok(inside.includes(flat(part)), part);
  }
  const without = page({}, { ...FULL, edgeImportsChrome: false }).text;
  assert.ok(!flat(without).includes(flat(fr.privacy.remainsEdge)));
});

test('une trace vide dit « Rien à effacer » sans case ; une illisible le dit sans case ; un navigateur ouvert : une phrase en tête de sa carte, des lignes sans case', () => {
  const { tree, text } = page();
  const cards = queryAll(tree, '.pbrowser');
  const chrome = cards[0] as Node; // Léa (case), Profil 2 (vide), Profil 3 (case)
  assert.equal(queryAll(chrome, 'input').length, 2);
  const emptyRow = queryAll(chrome, 'li')[1] as Node;
  assert.equal(queryAll(emptyRow, 'input').length, 0);
  assert.ok(flat(textOfNode(emptyRow)).includes(flat(fr.privacy.nothing)));
  assert.ok(flat(textOfNode(emptyRow)).includes('Profil 2'));

  const brave = cards[2] as Node;
  assert.equal(queryAll(brave, 'input').length, 0);
  assert.ok(flat(textOfNode(brave)).includes(flat(fr.privacy.unreadable)));

  const open = cards[3] as Node; // Vivaldi, ouvert
  assert.equal(queryAll(open, 'input').length, 0);
  const said = flat(textOfNode(open));
  assert.ok(said.includes('Ferme Vivaldi pour effacer son historique.'), said);
  assert.ok(said.includes('zone de notification'));
  assert.ok(query(open, 'h3') !== null && (open.children[1] as Node).attrs['class']?.includes('pwarn'), 'la phrase est en tête de la carte, sous le nom');
  assert.ok(said.includes(flat(formatBytes(3 * 1024 ** 2))), 'la trace garde son poids : elle est seulement inerte');
  assert.equal(occurrences(text, 'Ferme Vivaldi pour effacer'), 1, 'la phrase n’est écrite qu’une fois');
  // Deux profils ouverts : toujours une seule phrase.
  const two = page({}, view([browser('chrome', 0, null, { bytes: 1 }, true), browser('chrome', 1, null, { bytes: 1 }, true), WINDOWS])).text;
  assert.equal(occurrences(two, 'Ferme Chrome pour effacer'), 1);
});

test('la ligne sur Firefox n’est là que si un dossier de profils Firefox existe, la phrase du cache que si un navigateur est là', () => {
  assert.ok(flat(page().text).includes(flat(fr.privacy.firefox)));
  assert.ok(!flat(page({}, view([CHROME_MAIN, WINDOWS])).text).includes(flat(fr.privacy.firefox)));
  assert.ok(flat(page().text).includes(flat(fr.privacy.cache)));
  assert.ok(!flat(page({}, view([WINDOWS], { firefox: true })).text).includes(flat(fr.privacy.cache)));
  assert.ok(flat(page({}, view([WINDOWS], { firefox: true })).text).includes(flat(fr.privacy.firefox)));
});

test('« Effacer ce qui est coché » est inerte tant que rien n’est coché, puis il ouvre la confirmation', () => {
  const none = page();
  const button = queryAll(none.tree, 'button.pill')[0] as Node;
  assert.equal(flat(textOfNode(button)), flat(fr.privacy.erase));
  assert.equal(button.attrs['aria-disabled'], 'true');
  assert.ok(flat(textOfNode(none.tree)).includes(flat(fr.privacy.eraseHint)));
  const inert = clicksOf(none.template).filter((click) => click.context.includes('class="pill small"'));
  assert.equal(inert.length, 1);
  inert[0]?.fn();
  assert.equal(none.calls.asked, 0, 'inerte : le geste ne part pas');

  const some = page({ privacyChecked: ['edge:0-h', 'w-clip'] });
  const live = queryAll(some.tree, 'button.pill');
  assert.equal(live[0]?.attrs['aria-disabled'], undefined);
  assert.ok(flat(textOfNode(some.tree)).includes(flat(fr.privacy.checked(2))));
  clicksOf(some.template).find((click) => click.context.includes('class="pill small"'))?.fn();
  assert.equal(some.calls.asked, 1);
  // Le même bouton, dans la barre du bas : pour ne pas remonter.
  assert.equal(queryAll(some.tree, '.pzone button.pill').length, 1);
  assert.equal(queryAll(none.tree, '.pzone').length, 0);
});

test('pendant l’effacement, la page est inerte et le dit', () => {
  const busy = page({ privacyBusy: true, privacyChecked: ['edge:0-h'] });
  assert.ok(flat(busy.text).includes(flat(fr.privacy.working)));
  for (const box of queryAll(busy.tree, 'input[type="checkbox"]')) {
    assert.equal(box.attrs['aria-disabled'], 'true');
  }
  assert.equal(queryAll(busy.tree, 'button.pill')[0]?.attrs['aria-disabled'], 'true');
  assert.equal(query(busy.tree, '.page.privacy')?.attrs['aria-busy'], 'true');
  assert.equal(flat(textOfNode(query(busy.tree, '[aria-live]') as Node)), flat(fr.privacy.working));
  // Le geste d'une case ou du bouton ne part pas.
  clicksOf(busy.template).filter((click) => click.context.includes('class="pill small"')).forEach((click) => click.fn());
  assert.equal(busy.calls.asked, 0);
  const idle = page();
  assert.equal(query(idle.tree, '.page.privacy')?.attrs['aria-busy'], undefined);
});

test('« Dans Windows » : trois boutons vers les pages de Windows, rien d’autre', () => {
  const { tree, calls, template } = page();
  assert.deepEqual([...WINDOWS_PAGES], ['activityHistory', 'searchPermissions', 'diagnostics']);
  const buttons = queryAll(tree, 'li.pwin button');
  assert.equal(buttons.length, 3);
  for (const [index, button] of buttons.entries()) {
    // Le nom accessible contient le texte visible, puis le nom de la page.
    const labelled = (button.attrs['aria-labelledby'] ?? '').split(' ');
    assert.equal(labelled[0], button.attrs['id']);
    const name = queryAll(tree, `[id="${labelled[1]}"]`)[0] as Node;
    assert.equal(flat(textOfNode(name)), flat(fr.privacy.inWindows.pages[WINDOWS_PAGES[index] as 'diagnostics'].name));
  }
  clicksOf(template)
    .filter((click) => click.context.includes('id="privacy-win-'))
    .forEach((click) => click.fn());
  assert.deepEqual(calls.windows, ['activityHistory', 'searchPermissions', 'diagnostics']);
  // La page de diagnostic ne promet pas le bouton de suppression.
  assert.ok(flat(fr.privacy.inWindows.pages.diagnostics.text).includes('Selon ton édition de Windows'));
});

test('« Ce que Shinku ne touche pas » nomme les mots de passe, les favoris, les extensions, les onglets, les cookies, les épinglés et les sauvegardes', () => {
  const said = flat(textOfNode(query(page().tree, 'details') as Node));
  for (const word of ['mots de passe', 'favoris', 'extensions', 'onglets', 'cookies', 'épinglés', 'sauvegardes', 'points de restauration', 'Opera']) {
    assert.ok(said.includes(word), word);
  }
});

test('après un effacement : une issue par ligne dans la carte, dans la région vivante, et sous la ligne qui reste', () => {
  const ask = askFor(FULL, ['chrome:0-h', 'edge:0-h', 'w-recent', 'w-typed', 'w-clip']);
  assert.ok(ask !== null);
  const outcomes: TraceOutcome[] = ['erased', 'inUse', 'changed', 'partial', 'failed'];
  const lines = lastLines(
    ask,
    ask.ids.map((id, index) => ({ id, outcome: outcomes[index] as TraceOutcome })),
  );
  const { tree, text } = page({ privacyLast: lines, privacyAnnounce: announceLines(lines) });
  const card = queryAll(tree, '.plast');
  assert.equal(card.length, 5);
  for (const [index, line] of card.entries()) {
    const outcome = outcomes[index] as TraceOutcome;
    assert.ok(flat(textOfNode(query(line, '.pres') as Node)) === flat(outcomeShort(outcome)));
    // Le nom d'abord, la trace ensuite ; jamais deux deux-points ni une majuscule au milieu d'une phrase.
    assert.ok(!/:.*:/.test(textOfNode(line)), textOfNode(line));
  }
  const first = card[0] as Node;
  assert.equal(flat(textOfNode(query(first, '.t1') as Node)), 'Chrome · Léa');
  assert.equal(flat(textOfNode(query(first, '.t2') as Node)), flat(fr.privacy.trace.history));
  const helps = card.map((line) => query(line, '.phelp')?.children.length === 0 ? flat(textOfNode(query(line, '.phelp') as Node)) : null);
  assert.deepEqual(helps, [null, 'Ferme Edge et recommence.', 'Regarde de nouveau.', 'Il en reste : regarde de nouveau.', null]);
  // La région vivante garde une phrase complète par trace.
  const spoken = flat(textOfNode(query(tree, '[aria-live]') as Node));
  assert.ok(spoken.startsWith('Effacement terminé.'));
  // L'écran et la région vivante disent la même chose : l'aide nomme le navigateur.
  assert.ok(spoken.includes('En cours d’utilisation. Ferme Edge et recommence.'), spoken);
  assert.ok(spoken.includes('Effacé en partie. Il en reste : regarde de nouveau.'), spoken);
  assert.ok(flat(text).includes(flat(fr.privacy.lastTitle)));
  // Une ligne qui n'a pas pu partir garde sa case et dit pourquoi, sous son nom, avec la même aide.
  const edge = queryAll(tree, '.pbrowser')[1] as Node;
  assert.equal(flat(textOfNode(query(edge, '.pout') as Node)), 'En cours d’utilisation. Ferme Edge et recommence.');
  assert.ok(query(edge, 'input[type="checkbox"]') !== null);
});

test('un refus du cœur est dit clairement : un grand vide en cours, trop de traces', () => {
  const busy = page({ privacyRefused: 'busy' }).text;
  assert.ok(flat(busy).includes(flat(fr.privacy.refused.busy)));
  assert.ok(flat(busy).includes('Rien n’a été effacé'));
  const many = page({ privacyRefused: 'tooMany' }).text;
  assert.ok(flat(many).includes(flat(fr.privacy.refused.tooMany)));
  assert.ok(!flat(page().text).includes('Rien n’a été effacé'));
});

test('la page sans vue dit « Chargement… », ou l’échec avec « Réessayer »', () => {
  assert.ok(flat(page({}, null).text).includes(flat(fr.common.loading)));
  const failed = page({ privacyFailed: true }, null);
  assert.ok(flat(failed.text).includes(flat(fr.toast.unexpected)));
  assert.ok(flat(failed.text).includes(flat(fr.privacy.retry)));
});

test('quand rien ne se peut effacer, la page le dit en tête', () => {
  const nothing = page({}, view([CHROME_TWO, { ...WINDOWS, items: WINDOWS.items.map((item) => ({ ...item, state: 'empty' as const })) }]));
  assert.ok(flat(textOfNode(query(nothing.tree, '.pline') as Node)).includes(flat(fr.privacy.nothingNow)));
  assert.equal(queryAll(nothing.tree, 'input').length, 0);
  // Chrome ouvert, Windows vide : des traces sont présentes mais aucune case n'existe, le texte ne dit pas « Coche… ».
  const onlyOpen = page({}, view([VIVALDI_OPEN, { ...WINDOWS, items: WINDOWS.items.map((item) => ({ ...item, state: 'empty' as const })) }]));
  assert.ok(flat(textOfNode(query(onlyOpen.tree, '.pline') as Node)).includes(flat(fr.privacy.nothingNow)));
  assert.ok(!flat(onlyOpen.text).includes(flat(fr.privacy.eraseHint)));
});

// ── La confirmation ────────────────────────────────────────────────────────

function dialog(parts: Record<string, unknown> = {}) {
  const { calls, actions: stub } = actions();
  const ask = askFor(FULL, ['chrome:0-h', 'edge:0-h', 'w-clip', 'w-typed']);
  const template = privacyDialog({ ...initialState(NOW), route: 'privacy', privacy: FULL, privacyAsk: ask, ...parts } as never, stub);
  return { calls, template, tree: parse(markupOf(template)), ask };
}

test('la confirmation liste les traces cochées par bloc, dit qu’elles ne passent pas par la Corbeille, et Annuler est par défaut', () => {
  const { tree, template, calls } = dialog();
  const box = query(tree, 'dialog.ask') as Node;
  assert.ok(box !== null);
  assert.equal(box.attrs['aria-labelledby'], 'privacy-ask-title');
  const names = queryAll(tree, '.askname').map((node) => flat(textOfNode(node)));
  assert.deepEqual(names, ['Chrome', 'Edge', 'Windows']);
  const chrome = queryAll(tree, '.askblock')[0] as Node;
  // Le nom est dans un <bdi> : l'aide d'arbre met le texte propre avant les enfants, on vérifie donc les deux morceaux.
  const bullet = queryAll(chrome, 'li')[0] as Node;
  assert.equal(queryAll(bullet, 'bdi').map((node) => flat(textOfNode(node))).join(), 'Léa');
  assert.equal(flat(bullet.text), flat(`${NBSP}: historique de navigation`).trim().replace(/^:/, ':'));
  const windows = queryAll(tree, '.askblock')[2] as Node;
  assert.deepEqual(queryAll(windows, 'li').map((node) => flat(textOfNode(node))), [flat(fr.privacy.trace.typed), flat(fr.privacy.trace.clipboard)]);
  assert.ok(flat(textOfNode(tree)).includes('Elles sont supprimées directement, sans passer par la Corbeille.'));

  const buttons = queryAll(tree, '.askbtns button');
  assert.deepEqual(buttons.map((button) => flat(textOfNode(button))), ['Annuler', 'Effacer']);
  assert.ok('autofocus' in (buttons[0] as Node).attrs, 'Annuler a le focus à l’ouverture');
  assert.ok(!('autofocus' in (buttons[1] as Node).attrs), 'Effacer ne l’a pas');
  const clicks = clicksOf(template);
  clicks.filter((click) => click.context.endsWith('autofocus @click=')).forEach((click) => click.fn());
  assert.equal(calls.dismissed, 1);
  assert.equal(calls.confirmed, 0, 'Annuler ne confirme rien');
});

test('sans confirmation à faire, la boîte n’existe pas', () => {
  assert.equal(htmlOf(dialog({ privacyAsk: null }).template), '');
});

test('la boîte s’adresse au cœur par ce que la confirmation a figé, pas par ce que la vue dit ensuite', () => {
  const { ask } = dialog();
  assert.deepEqual(ask?.ids, ['chrome:0-h', 'edge:0-h', 'w-typed', 'w-clip']);
});

test('aucun texte affiché, dans aucun état de la page, ne contient les mots interdits', () => {
  const states: Array<Record<string, unknown>> = [
    {},
    { privacyBusy: true },
    { privacyRefused: 'busy' },
    { privacyRefused: 'tooMany' },
    { privacyChecked: ['edge:0-h'] },
  ];
  const ask = askFor(FULL, checkableIds(FULL));
  assert.ok(ask !== null);
  const all: TraceOutcome[] = ['erased', 'nothing', 'inUse', 'changed', 'partial', 'failed'];
  const lines = lastLines(ask, ask.ids.map((id, index) => ({ id, outcome: all[index % all.length] as TraceOutcome })));
  states.push({ privacyLast: lines, privacyAnnounce: announceLines(lines) });
  for (const parts of states) {
    const shown = page(parts);
    assert.doesNotMatch(shown.text, FORBIDDEN, JSON.stringify(parts));
    assert.doesNotMatch(shown.text, /cookies de suivi/i);
  }
  assert.doesNotMatch(textOf(dialog().template), FORBIDDEN);
});

// ── La barre du bas ────────────────────────────────────────────────────────

test('la barre du bas est inerte pendant la confirmation et pendant l’effacement, et la page lui laisse sa place', () => {
  const ask = askFor(FULL, ['edge:0-h']);
  for (const parts of [{ privacyAsk: ask }, { privacyBusy: true }]) {
    const shown = page({ privacyChecked: ['edge:0-h'], ...parts });
    const bar = queryAll(shown.tree, '.pzone button.pill');
    assert.equal(bar.length, 1);
    assert.equal(bar[0]?.attrs['aria-disabled'], 'true', JSON.stringify(Object.keys(parts)));
    clicksOf(shown.template).filter((click) => click.context.includes('class="pill small"')).forEach((click) => click.fn());
    assert.equal(shown.calls.asked, 0);
  }
  // Sans confirmation ni effacement, elle répond.
  const live = page({ privacyChecked: ['edge:0-h'] });
  assert.equal(queryAll(live.tree, '.pzone button.pill')[0]?.attrs['aria-disabled'], undefined);
  // La zone qui défile se laisse assez de marge en bas pour que la dernière ligne passe au-dessus de la barre.
  assert.ok(query(live.tree, '.scroll.sayon') !== null);
  const css = readFileSync(new URL('../src/styles/privacy.css', import.meta.url), 'utf8');
  assert.match(css, /\.scroll\.sayon > \.page\.privacy \{\s*padding-bottom: (\d+)px;/);
  assert.ok(Number(/\.scroll\.sayon > \.page\.privacy \{\s*padding-bottom: (\d+)px;/.exec(css)?.[1]) >= 96);
});

test('les paragraphes d’introduction ont le même bord gauche que les titres et « Ce qui reste » (4 px, plus précis que `.startup .d`)', () => {
  const css = readFileSync(new URL('../src/styles/privacy.css', import.meta.url), 'utf8');
  assert.match(css, /\.privacy \.pintro \{\s*margin: 0 4px 12px;/);
  assert.match(css, /\.premain \{\s*margin: 10px 4px 0;/);
});

// ── Gestes de la boîte et de la page, noms posés en texte ──────────────────

/** Les fonctions liées à `nom=` (par exemple `@cancel=`) dans un gabarit, avec le HTML écrit avant chacune. */
function boundTo(value: unknown, name: string): Array<{ context: string; fn: (event: unknown) => unknown }> {
  const found: Array<{ context: string; fn: (event: unknown) => unknown }> = [];
  const walk = (inner: unknown): void => {
    if (Array.isArray(inner)) {
      inner.forEach(walk);
      return;
    }
    if (typeof inner !== 'object' || inner === null) {
      return;
    }
    if ('_$litDirective$' in inner && 'values' in inner) {
      const { values } = inner as unknown as { values: unknown[] };
      const last = values.at(-1);
      if (typeof last === 'function' && values.length > 1) {
        for (const [index, item] of [...(values[0] as Iterable<unknown>)].entries()) {
          walk((last as (item: unknown, index: number) => unknown)(item, index));
        }
      } else {
        walk(last);
      }
      return;
    }
    if ('strings' in inner && 'values' in inner) {
      const template = inner as { strings: readonly string[]; values: readonly unknown[] };
      let out = '';
      template.strings.forEach((text, index) => {
        out += text;
        if (index >= template.values.length) {
          return;
        }
        const one = template.values[index];
        if (out.endsWith(name) && typeof one === 'function') {
          found.push({ context: out, fn: one as (event: unknown) => unknown });
          out = out.slice(0, out.length - name.length);
          return;
        }
        walk(one);
        out += htmlOf(one);
      });
    }
  };
  walk(value);
  return found;
}

test('Échap referme la confirmation sans rien confirmer, et ne laisse pas le navigateur la fermer à sa façon', () => {
  const { template, calls } = dialog();
  const [cancel] = boundTo(template, '@cancel=');
  assert.ok(cancel !== undefined, 'la boîte écoute « cancel »');
  let prevented = 0;
  cancel.fn({ preventDefault: () => (prevented += 1) });
  assert.equal(prevented, 1);
  assert.equal(calls.dismissed, 1);
  assert.equal(calls.confirmed, 0);
});

test('pendant l’effacement, un clic sur une case est annulé (la case du DOM ne bascule pas) ; hors effacement, il passe', () => {
  for (const [busy, expected] of [[true, 1], [false, 0]] as const) {
    const shown = page({ privacyBusy: busy });
    const boxes = boundTo(shown.template, '@click=').filter((one) => one.context.includes('type="checkbox"'));
    assert.ok(boxes.length >= 1, 'les cases écoutent le clic');
    let prevented = 0;
    for (const one of boxes.slice(0, 1)) {
      one.fn({ preventDefault: () => (prevented += 1) });
    }
    assert.equal(prevented, expected, busy ? 'figée pendant l’effacement' : 'libre sinon');
  }
});

/** Vrai si une valeur du gabarit est passée par `unsafeHTML` (ce qui lirait un nom de profil comme du HTML). */
function usesUnsafeHtml(value: unknown): boolean {
  const marker = (unsafeHTML('x') as unknown as { _$litDirective$: unknown })._$litDirective$;
  const walk = (inner: unknown): boolean => {
    if (Array.isArray(inner)) {
      return inner.some(walk);
    }
    if (typeof inner !== 'object' || inner === null) {
      return false;
    }
    if ('_$litDirective$' in inner) {
      if ((inner as { _$litDirective$: unknown })._$litDirective$ === marker) {
        return true;
      }
      const { values } = inner as unknown as { values: unknown[] };
      const last = values.at(-1);
      return typeof last === 'function' && values.length > 1
        ? [...(values[0] as Iterable<unknown>)].some((item, index) => walk((last as (item: unknown, index: number) => unknown)(item, index)))
        : walk(last);
    }
    if ('values' in inner) {
      return (inner as { values: unknown[] }).values.some(walk);
    }
    return false;
  };
  return walk(value);
}

const HOSTILE = '<img src=x onerror=alert(1)>';

test('un nom de profil est posé en texte, jamais lu comme du HTML : ligne, confirmation, « Dernier effacement »', () => {
  const hostile = view([browser('chrome', 0, HOSTILE, { bytes: 5 }), WINDOWS]);
  const ask = askFor(hostile, ['chrome:0-h']);
  assert.ok(ask !== null);
  const lines = lastLines(ask, [{ id: 'chrome:0-h', outcome: 'inUse' }]);
  const shown = page({ privacyLast: lines }, hostile);
  assert.equal(usesUnsafeHtml(shown.template), false);
  assert.equal(usesUnsafeHtml(dialog({ privacy: hostile, privacyAsk: ask }).template), false);
  // Chaque nom est dans un <bdi> : un nom écrit de droite à gauche ne réordonne pas ce qui l'entoure.
  assert.ok(queryAll(shown.tree, '.row .t1 bdi').length > queryAll(shown.tree, '.plast bdi').length, 'le titre de la ligne isole le nom');
  assert.ok(queryAll(shown.tree, '.plast bdi').length >= 1, 'la carte « Dernier effacement » isole le nom');
  assert.ok(queryAll(dialog({ privacy: hostile, privacyAsk: ask }).tree, 'li bdi').length >= 1, 'la confirmation isole le nom');
  assert.ok(htmlOf(dialog({ privacy: hostile, privacyAsk: ask }).template).includes('<bdi>'));
  // Le détecteur sait voir un `unsafeHTML`.
  assert.equal(usesUnsafeHtml(html`<p>${unsafeHTML(HOSTILE)}</p>`), true);
});

test('une seule trace cochée : la confirmation parle au singulier ; plusieurs : au pluriel', () => {
  const one = dialog({ privacyAsk: askFor(FULL, ['edge:0-h']) });
  const said = flat(textOf(one.template));
  assert.ok(said.includes('Effacer cette trace ?'));
  assert.ok(said.includes('Elle est supprimée directement, sans passer par la Corbeille.'));
  assert.ok(!said.includes('Effacer ces traces'));
  const many = flat(textOf(dialog().template));
  assert.ok(many.includes('Effacer ces traces ?'));
  assert.ok(many.includes('Elles sont supprimées directement, sans passer par la Corbeille.'));
});

test('« Effacement en cours » n’est annoncé qu’une fois : une seule région vivante, la note visible n’en est pas une', () => {
  const busy = page({ privacyBusy: true });
  assert.equal(queryAll(busy.tree, '[aria-live]').length, 1);
  assert.equal(queryAll(busy.tree, '[role="status"]').length, 0);
  assert.ok(flat(textOfNode(query(busy.tree, '.pwait') as Node)).includes(flat(fr.privacy.working)));
});

test('les noms longs passent à la ligne : ligne, confirmation et « Dernier effacement »', () => {
  const css = readFileSync(new URL('../src/styles/privacy.css', import.meta.url), 'utf8');
  assert.match(css, /\.privacy \.row \.t1,\s*\.privacy \.plast \.t1,\s*\.asklist li \{\s*overflow-wrap: anywhere;/);
  assert.match(css, /\.privacy \.row \.tx \{\s*min-width: 0;/);
  assert.match(css, /\.privacy \.pblock \.sub \{[^}]*overflow-wrap: anywhere;/);
  assert.match(css, /\.askname \{[^}]*overflow-wrap: anywhere;/);
});

test('« Pas encore pris en charge » nomme aussi les fichiers récents d’Office et des autres apps', () => {
  assert.match(fr.privacy.untouched.outside, /fichiers récents d’Office et des autres apps/);
});

test('un refus « la liste a changé » est dit, sans promettre ce que Shinku n’a pas fait', () => {
  assert.equal(fr.privacy.refused.listChanged, 'La liste a changé : regarde de nouveau.'.replace(' :', `${NBSP}:`));
  const shown = page({ privacyRefused: 'listChanged' }).text;
  assert.ok(flat(shown).includes('La liste a changé : regarde de nouveau.'));
  assert.ok(fr.privacy.refused.busy.includes('mise à la corbeille de restes'));
});
