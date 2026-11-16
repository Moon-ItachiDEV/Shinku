// Ce que la page Journal écrit : les phrases de chaque sorte de ligne (pluriels, durée, poids, date qui manque),
// le bouton « Oublier » seulement s'il y a une désinstallation, ce qui est montré tel quel et ce qui ne l'est
// jamais, et la durée qui ne s'écrit que si la fin est après le début.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import type { HistoryLine, HistoryView } from '../src/backend/types.ts';
import { DOM, EXPECTED } from '../scripts/essai-journal-dom.mjs';
import { demoOptions, mockBackend } from '../src/backend/mock.ts';
import { formatSpan } from '../src/format.ts';
import { fr } from '../src/i18n/fr.ts';
import { hasUninstalls, journalFocus, rowOf } from '../src/model/journal.ts';
import { initialState } from '../src/state.ts';
import { journalDialog, journalView } from '../src/views/journal.ts';
import { settingsFocus, settingsView } from '../src/views/settings.ts';
import { type Node, parse, query, queryAll, textOfNode } from './html-tree.ts';
import { fakeDocument, htmlOf, markupOf, textOf } from './template.ts';

fakeDocument();

const NOW = new Date(2026, 10, 16, 15, 0, 0).getTime();
const HOUR = 3_600_000;
const noActions = new Proxy({}, { get: () => () => {} }) as never;

const uninstall = (parts: Partial<Extract<HistoryLine, { kind: 'uninstall' }>> = {}): HistoryLine => ({
  kind: 'uninstall',
  atMs: NOW - HOUR,
  name: 'Essai Shinku App',
  leftovers: 3,
  leftoverBytes: 48_000_000,
  ...parts,
});

const game = (parts: Partial<Extract<HistoryLine, { kind: 'game' }>> = {}): HistoryLine => ({
  kind: 'game',
  startedMs: NOW - 3 * HOUR,
  endedMs: NOW - HOUR,
  name: 'Un jeu',
  powerRestored: true,
  appsRestored: 2,
  complete: true,
  ...parts,
});

const clean = (parts: Partial<Extract<HistoryLine, { kind: 'clean' }>> = {}): HistoryLine => ({
  kind: 'clean',
  startedMs: NOW - HOUR,
  finishedMs: NOW - HOUR + 60_000,
  state: 'completed',
  freedBytes: 3_400_000_000,
  recoverableBytes: 1_100_000_000,
  skippedFiles: 0,
  ...parts,
});

function sentence(line: HistoryLine): string {
  const row = rowOf(line, NOW);
  // Les espaces insécables de la typographie se lisent ici comme des espaces.
  return `${row.before}${row.name?.name ?? ''}${row.after}`.replace(/[\u00a0\u202f]/g, ' ');
}

function page(lines: HistoryLine[] | null, parts: Record<string, unknown> = {}): string {
  const history: HistoryView | null = lines === null ? null : { lines, more: false };
  return markupOf(journalView({ ...initialState(NOW), route: 'journal', history, ...parts } as never, noActions));
}

// ── Les phrases ────────────────────────────────────────────────────────────

test('une désinstallation : le nom, puis les restes au singulier ou au pluriel, avec leur poids s’il y en a un', () => {
  assert.equal(sentence(uninstall()), `Désinstallé : Essai Shinku App. 3 restes mis à la corbeille (45,8 Mo).`);
  assert.match(sentence(uninstall({ leftovers: 1, leftoverBytes: 0 })), /1 reste mis à la corbeille\.$/);
  assert.ok(!sentence(uninstall({ leftovers: 1, leftoverBytes: 0 })).includes('('), 'pas de poids quand il est nul');
  assert.equal(sentence(uninstall({ leftovers: 0, leftoverBytes: 0 })), 'Désinstallé : Essai Shinku App.');
});

test('un grand vide : les totaux, ce qui a été laissé, et les fins qui ne sont pas un bilan complet', () => {
  assert.match(sentence(clean()), /^Grand vide : .* libérés, .* mis à la corbeille\.$/);
  assert.match(sentence(clean({ skippedFiles: 2 })), /2 fichiers laissés en place\.$/);
  assert.match(sentence(clean({ skippedFiles: 1 })), /1 fichier laissé en place\.$/);
  assert.equal(sentence(clean({ state: 'running' })), fr.journal.clean.running);
  assert.match(sentence(clean({ state: 'cancelled' })), /arrêté en route/);
  assert.match(sentence(clean({ state: 'interrupted' })), /coupé avant la fin/);
  assert.match(sentence(clean({ freedBytes: 0, recoverableBytes: 0 })), /rien n’a été retiré/);
});

test('une partie : la durée seulement si la fin est après le début, et le bilan de la page Mode jeu', () => {
  assert.match(sentence(game()), /^Mode jeu : Un jeu, 2 h\. /);
  assert.ok(!/,/.test(sentence(game({ endedMs: NOW - 4 * HOUR }))), 'une fin avant le début : pas de durée');
  assert.ok(!/1 min/.test(sentence(game({ endedMs: NOW - 4 * HOUR }))));
  assert.ok(!/,/.test(sentence(game({ startedMs: null, endedMs: null }))));
  assert.ok(sentence(game()).endsWith(fr.journal.game.restored), 'le bilan est celui de la page Mode jeu');
  assert.ok(fr.game.lastOk('X', null).endsWith(fr.journal.game.restored), 'une seule formule pour les deux pages');
  assert.equal(rowOf(game(), NOW).link, null);
  assert.deepEqual(rowOf(game({ complete: false }), NOW).link?.route, 'gameMode');
});

test('une coupure du Démarrage : la formule de la page Démarrage, sans point doublé, et rien sans date', () => {
  const cut = (parts: Partial<Extract<HistoryLine, { kind: 'startupCut' }>>): HistoryLine => ({
    kind: 'startupCut',
    atMs: NOW - 48 * HOUR,
    name: 'Une app',
    backMs: null,
    ...parts,
  });
  assert.ok(!sentence(cut({})).includes('remise'));
  assert.ok(sentence(cut({ backMs: NOW })).includes('remise toute seule depuis'));
  assert.ok(!sentence(cut({ backMs: NOW })).includes('..'));
  assert.ok(!sentence(cut({ atMs: null, backMs: NOW })).includes('remise'));
  assert.equal(rowOf(cut({}), NOW).link?.route, 'startup');
});

test('la page Mode jeu écrit la durée par la même fonction, et se tait sans elle', () => {
  const source = readFileSync(new URL('../src/views/game.ts', import.meta.url), 'utf8');
  assert.ok(source.includes('formatSpan(last.started, last.ended)'));
  assert.equal(fr.game.lastOk('X', null).replace(/\u00a0/g, ' '), `Dernière partie : X. ${fr.journal.game.restored}`);
  assert.ok(fr.game.lastOk('X', '5 min').includes(', 5 min.'));
});

test('une date jusqu’à un jour dans le futur montre son vrai jour, pas une heure d’aujourd’hui', () => {
  const tomorrow = new Date(NOW + 20 * HOUR);
  const row = rowOf(uninstall({ atMs: tomorrow.getTime() }), NOW);
  assert.ok(row.date !== null);
  const shown = textOf(journalView({ ...initialState(NOW), history: { lines: [uninstall({ atMs: tomorrow.getTime() })], more: false } } as never, noActions));
  assert.ok(/le \d+\u00a0\S+, \d\d:\d\d/.test(shown), shown);
});

test('la durée n’existe que si la fin est après le début', () => {
  assert.equal(formatSpan(10, 10), null);
  assert.equal(formatSpan(20, 10), null);
  assert.equal(formatSpan(null, 10), null);
  assert.equal(formatSpan(10, null), null);
  assert.notEqual(formatSpan(0, 60_000), null);
});

test('chaque ligne a son chemin du retour, ou aucun', () => {
  assert.equal(rowOf(clean(), NOW).link?.route, 'bin');
  assert.equal(rowOf(clean({ recoverableBytes: 0 }), NOW).link, null);
  assert.equal(rowOf(clean({ state: 'running' }), NOW).link, null);
  assert.equal(rowOf(uninstall(), NOW).link?.route, 'bin');
  assert.equal(rowOf(uninstall({ leftovers: 0 }), NOW).link, null);
});

// ── La page ────────────────────────────────────────────────────────────────

test('« Oublier les désinstallations » n’existe que s’il y a une désinstallation', () => {
  assert.equal(hasUninstalls({ lines: [clean()], more: false }), false);
  assert.equal(hasUninstalls({ lines: [clean(), uninstall()], more: false }), true);
  assert.ok(page([uninstall()]).includes(fr.journal.forget));
  assert.ok(!page([clean(), game()]).includes(fr.journal.forget));
  assert.ok(!page([]).includes(fr.journal.forget));
  assert.ok(page([uninstall()]).includes(`data-fk="${journalFocus.forget}"`));
});

test('une ligne sans date le dit ; une ligne datée a un <time>', () => {
  const tree = parse(page([uninstall({ atMs: null }), uninstall()]));
  assert.equal(queryAll(tree, 'time').length, 1);
  assert.ok(textOfNode(tree).includes(fr.journal.unknownDate));
});

test('la page vide, en attente, en échec, et la note de fin', () => {
  assert.ok(page([]).includes(fr.journal.empty));
  assert.ok(page(null).includes(fr.journal.loading));
  const failed = page(null, { historyFailed: true });
  assert.ok(failed.includes(fr.journal.failed) && failed.includes(fr.journal.retry));
  assert.ok(page([clean()]).includes(fr.journal.note));
  assert.ok(!page([clean()]).includes(fr.journal.more));
  assert.ok(htmlOf(journalView({ ...initialState(NOW), history: { lines: [clean()], more: true } } as never, noActions)).includes(fr.journal.more));
});

test('un nom est du texte : jamais écrit comme du HTML, isolé par <bdi>, entier et sans title', () => {
  // Le faux gabarit des essais n'échappe pas ; c'est lit-html qui le fait, sauf `unsafeHTML`, que la page n'emploie pas.
  for (const file of ['../src/views/journal.ts', '../src/model/journal.ts']) {
    const source = readFileSync(new URL(file, import.meta.url), 'utf8');
    assert.ok(!/unsafeHTML|innerHTML|unsafe-html/.test(source), file);
  }
  const name = 'Un très long nom '.repeat(8);
  const shown: Node | null = query(parse(page([uninstall({ name })])), 'bdi.jname');
  assert.ok(shown !== null, 'le nom est isolé par <bdi>');
  assert.equal(shown?.attrs['title'], undefined, 'le nom est écrit en entier : pas de title');
  assert.ok(textOfNode(shown as Node).includes(name.trim()));
  // La feuille de style le laisse passer à la ligne : ni points de suspension, ni largeur bornée.
  const css = readFileSync(new URL('../src/styles/journal.css', import.meta.url), 'utf8');
  assert.ok(css.includes('overflow-wrap: anywhere'));
  assert.ok(!/text-overflow|inline-block|max-width: \d+ch/.test(css));
});

test('la confirmation de l’oubli : ses mots, et rien tant qu’elle n’est pas demandée', () => {
  assert.equal(htmlOf(journalDialog(initialState(NOW), noActions)), '');
  const written = textOf(journalDialog({ ...initialState(NOW), historyAsk: true } as never, noActions));
  assert.ok(written.includes(fr.journal.confirm.title) && written.includes(fr.journal.confirm.confirm));
  assert.ok(written.includes(fr.journal.confirm.cancel));
});

// ── « À propos », dans les Paramètres ──────────────────────────────────────

function settings(parts: Record<string, unknown> = {}): string {
  return markupOf(settingsView({ ...initialState(NOW), route: 'settings', ...parts } as never, noActions));
}

test('« À propos » : la phrase des promesses, le bouton des licences, et sa réponse seulement si le fichier ne s’est pas ouvert', () => {
  const normal = settings();
  assert.ok(normal.includes(fr.settings.about));
  assert.ok(normal.includes(fr.settings.aboutFree));
  assert.ok(normal.includes(fr.settings.licences));
  assert.ok(normal.includes(`data-fk="${settingsFocus.licences}"`));
  assert.ok(!normal.includes(fr.settings.licencesMissing) && !normal.includes(fr.settings.licencesFailed));
  assert.ok(settings({ licencesAnswer: 'missing' }).includes(fr.settings.licencesMissing));
  assert.ok(settings({ licencesAnswer: 'failed' }).includes(fr.settings.licencesFailed));
});

test('le faux cœur : une ligne de chaque sorte, et « Oublier » ne retire que les désinstallations', async () => {
  const backend = mockBackend(demoOptions(''));
  const first = await backend.historyView();
  assert.deepEqual([...new Set(first.lines.map((line) => line.kind))].sort(), ['clean', 'game', 'startupCut', 'uninstall']);
  // Comme le produit : une seule partie, pas de grand vide « en cours » vieux de plusieurs jours.
  assert.equal(first.lines.filter((line) => line.kind === 'game').length, 1);
  assert.ok(!first.lines.some((line) => line.kind === 'clean' && line.state === 'running'));
  assert.ok(first.lines.some((line) => line.kind === 'clean' && line.state === 'completed' && line.recoverableBytes > 0));
  assert.ok(first.lines.some((line) => line.kind === 'clean' && line.state === 'cancelled'));
  const uninstalls = first.lines.filter((line) => line.kind === 'uninstall');
  assert.equal(uninstalls.length, 2);
  assert.ok(uninstalls.some((line) => line.leftovers > 0) && uninstalls.some((line) => line.leftovers === 0));
  assert.ok(uninstalls.some((line) => line.name.length > 100));
  const cuts = first.lines.filter((line) => line.kind === 'startupCut');
  assert.equal(cuts.length, 2);
  assert.ok(cuts.some((line) => line.backMs !== null) && cuts.some((line) => line.atMs === null));
  const after = await backend.forgetUninstalls();
  assert.ok(after.lines.length > 0 && after.lines.every((line) => line.kind !== 'uninstall'));
  assert.equal(await backend.openLicences(), 'missing');
});

// ── L'essai de la fenêtre ──────────────────────────────────────────────────

test('l’essai de la fenêtre trouve dans la page ce qu’il cherche, et ses textes sont ceux de fr.ts', () => {
  const tree = parse(page([uninstall(), clean()]));
  assert.ok(query(tree, DOM.page) !== null);
  assert.equal(queryAll(tree, DOM.row).length, 2);
  const first = queryAll(tree, DOM.row)[0] as Node;
  assert.ok(query(first, DOM.sentence) !== null && query(first, DOM.date) !== null && query(first, DOM.link) !== null);
  assert.ok(query(tree, DOM.forget) !== null, 'le bouton « Oublier » est trouvé quand il existe');
  assert.ok(query(parse(page(null)), DOM.loading) !== null, 'l’attente est trouvée');
  const about = parse(settings());
  assert.ok(query(about, DOM.licences) !== null, 'le bouton des licences est trouvé');
  assert.equal(EXPECTED.title, fr.journal.title);
  assert.equal(EXPECTED.forget, fr.journal.forget);
  assert.equal(EXPECTED.licences, fr.settings.licences);
  assert.equal(EXPECTED.uninstalled, fr.journal.uninstall.before.trim().replace(/[\u00a0\u202f]/g, ' '));
  assert.ok(fr.journal.clean.completed('x').replace(/[\u00a0\u202f]/g, ' ').startsWith(EXPECTED.cleaned));
  const plain = (text: string): string => text.replace(/[\u00a0\u202f]/g, ' ');
  assert.ok(plain(fr.journal.clean.totals(1, 0)).includes(EXPECTED.freed), 'les totaux disent « libérés »');
  assert.ok(plain(fr.journal.clean.totals(0, 1)).includes(EXPECTED.binned), 'les totaux disent « mis à la corbeille »');
  assert.ok(fr.journal.uninstall.leftovers(3, 0).replace(/[\u00a0\u202f]/g, ' ').startsWith(EXPECTED.leftovers));
});

test('l’essai de la fenêtre ne clique ni sur « Voir leurs licences » ni sur « Oublier »', () => {
  const source = readFileSync(new URL('../scripts/essai-fenetre.mjs', import.meta.url), 'utf8');
  const start = source.indexOf('async function checkSettings');
  const end = source.indexOf('async function checkSingleWindow');
  const part = source.slice(start, end);
  assert.ok(part.includes('readAbout') && part.includes('checkJournal'));
  // Aucun clic sur ces boutons : pas de `click(` ni de `clickInPage` à leur sujet, ni de sélecteur de leurs boutons.
  assert.ok(!/click\([^)]*(licences|forget|JOURNAL_DOM)/i.test(part), 'aucun clic sur un bouton du Journal ou des licences');
  assert.ok(!/EXPECTED\.(licences|forget)[^;]*\bclick/.test(part));
  const journal = source.slice(source.indexOf('async function checkJournal'), source.indexOf('async function checkSingleWindow'));
  assert.ok(journal.includes('cleanStartedAt') && journal.includes('Date.parse(row.iso)'), 'les dates des lignes sont jugées');
  assert.ok(journal.includes('cleaned.sentence.startsWith(JOURNAL_EXPECTED.cleaned)'), 'la ligne doit commencer par « Grand vide : »');
  assert.ok(/await checkClean\(window, planted\);\s*\}\s*await checkJournal\(window\);\s*if \(onWindows\)/.test(source), 'le Journal vient après le grand vide');
});
