// Ce que les écrans Analyse et Résultats tirent d'une vue de session.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { CategorySummary, Lot, Phase, SessionView, Summary } from '../src/backend/types.ts';
import {
  binOverThreshold,
  counterBytes,
  duplicatesPercent,
  reportKey,
  ringFraction,
  scanRows,
  segments,
  selectable,
  selected,
  selectedBytes,
  type Selection,
  toggled,
  UNTOUCHED,
} from '../src/model/scan.ts';

const GB = 1024 ** 3;

/** Les lots tels que le cœur les range : seuls les fichiers inutiles et la corbeille sont automatiques. */
const LOTS: Partial<Record<CategorySummary['module'], Lot>> = { duplicates: 'optional', largeFiles: 'review' };

function entry(module: CategorySummary['module'], bytes: number, state: CategorySummary['state'] = 'ready'): CategorySummary {
  return { module, lot: LOTS[module] ?? 'auto', risk: 'A', bytes, items: bytes > 0 ? 3 : 0, state };
}

function summary(complete: boolean, categories: CategorySummary[], started = 1_699_999_940_000): Summary {
  const autoBytes = categories.filter((item) => item.lot === 'auto').reduce((total, item) => total + item.bytes, 0);
  return { complete, started, finished: 1_700_000_000_000, autoBytes, categories };
}

/** Une analyse terminée : 3 Go de fichiers inutiles, 2 Go de doublons, 6 Go à la corbeille, 18 Go de gros fichiers. */
const FOUND = [entry('recycleBin', 6 * GB), entry('junkFiles', 3 * GB), entry('duplicates', 2 * GB), entry('largeFiles', 18 * GB)];

function view(
  state: SessionView['state'],
  phase: Phase,
  report: Summary | null,
  work: { done: number; total: number } = { done: 0, total: 0 },
): SessionView {
  return {
    state,
    progress: { phase, folders: 10, files: 100, foundBytes: 2 * GB, workDone: work.done, workTotal: work.total },
    summary: report,
    skipped: { folders: 0, refusedRoots: 0 },
    stale: false,
  };
}

const statuses = (session: SessionView) => scanRows(session).map((row) => `${row.id}:${row.status}`);

test('avant toute analyse, chaque ligne attend, et Démarrage est annoncé pour plus tard', () => {
  assert.deepEqual(statuses(view('idle', 'preparing', null)), [
    'junkFiles:waiting',
    'duplicates:waiting',
    'recycleBin:waiting',
    'largeFiles:waiting',
    'startup:soon',
  ]);
});

test('pendant le parcours, les fichiers inutiles et les gros fichiers se comptent', () => {
  const session = view('scanning', 'walking', null);
  assert.deepEqual(statuses(session), [
    'junkFiles:running',
    'duplicates:waiting',
    'recycleBin:waiting',
    'largeFiles:running',
    'startup:soon',
  ]);
  const junk = scanRows(session)[0];
  assert.deepEqual(junk, { id: 'junkFiles', status: 'running', bytes: 2 * GB });
  assert.equal(counterBytes(session), 2 * GB);
  assert.equal(ringFraction(session), null);
});

test('après le parcours, seuls les doublons sont encore en cours', () => {
  const report = summary(false, [
    entry('recycleBin', 6 * GB),
    entry('junkFiles', 3 * GB),
    entry('duplicates', 0, 'pending'),
    entry('largeFiles', 18 * GB),
  ]);
  const session = view('scanning', 'duplicatesFull', report, { done: 1, total: 4 });
  assert.deepEqual(statuses(session), [
    'junkFiles:done',
    'duplicates:running',
    'recycleBin:done',
    'largeFiles:done',
    'startup:soon',
  ]);
  assert.deepEqual(scanRows(session)[1], { id: 'duplicates', status: 'running', percent: 25 });
  assert.equal(duplicatesPercent(session), 25);
  // Le grand chiffre ne compte que ce qui est sûr : pas les gros fichiers.
  assert.equal(counterBytes(session), 9 * GB);
});

test('une catégorie non mesurée le dit, au lieu d’afficher zéro', () => {
  const report = summary(true, [entry('recycleBin', 0, 'unavailable'), entry('junkFiles', GB), entry('duplicates', 0, 'unavailable')]);
  const rows = scanRows(view('results', 'done', report));
  assert.equal(rows[1]?.status, 'unavailable');
  assert.equal(rows[2]?.status, 'unavailable');
  // Gros fichiers absents du rapport : la ligne attend, elle n'invente rien.
  assert.equal(rows[3]?.status, 'waiting');
});

test('l’anneau avance d’un bout à l’autre sans jamais reculer', () => {
  const steps: Array<[Phase, number, number]> = [
    ['duplicatesQuick', 0, 10],
    ['duplicatesQuick', 5, 10],
    ['duplicatesQuick', 10, 10],
    ['duplicatesFull', 0, 10],
    ['duplicatesFull', 5, 10],
    ['duplicatesFull', 10, 10],
    ['done', 0, 0],
  ];
  const fractions = steps.map(([phase, done, total]) => ringFraction(view('scanning', phase, null, { done, total })));
  for (let index = 1; index < fractions.length; index += 1) {
    assert.ok((fractions[index] ?? 0) >= (fractions[index - 1] ?? 0), `étape ${index}`);
  }
  assert.equal(fractions.at(0), 0.05);
  assert.equal(fractions.at(-1), 1);
  // Un total inconnu ne produit ni division par zéro, ni dépassement.
  assert.equal(ringFraction(view('scanning', 'duplicatesFull', null, { done: 5, total: 0 })), 0.3);
  assert.equal(ringFraction(view('scanning', 'duplicatesFull', null, { done: 50, total: 10 })), 1);
});

test('le grand chiffre de l’analyse ne compte que le lot automatique', () => {
  const session = view('results', 'done', summary(true, FOUND));
  assert.equal(counterBytes(session), 9 * GB);
  // Chaque ligne terminée porte son lot : l'écran dit pourquoi un chiffre ne compte pas.
  const lots = scanRows(session).map((row) => (row.status === 'done' ? `${row.id}:${row.lot}` : `${row.id}:${row.status}`));
  assert.deepEqual(lots, ['junkFiles:auto', 'duplicates:optional', 'recycleBin:auto', 'largeFiles:review', 'startup:soon']);
});

test('les doublons ne sont jamais cochés d’office', () => {
  const report = summary(true, FOUND);
  assert.equal(selected(report, UNTOUCHED, 'junkFiles'), true);
  assert.equal(selected(report, UNTOUCHED, 'recycleBin'), true);
  assert.equal(selected(report, UNTOUCHED, 'duplicates'), false);
  assert.equal(selectable(report, 'duplicates'), true);
  // Sans toucher à rien, l'écran Résultats annonce le même total que l'écran Analyse.
  assert.equal(selectedBytes(report, UNTOUCHED), 9 * GB);
  assert.equal(selectedBytes(report, UNTOUCHED), report.autoBytes);
  assert.deepEqual(segments(report, UNTOUCHED), [3 * GB, 0, 6 * GB]);
  assert.equal(selectedBytes(null, UNTOUCHED), 0);
});

test('le total suit les cases que la personne change', () => {
  const report = summary(true, FOUND);
  // Elle coche les doublons : ils entrent dans le total.
  const withDuplicates = toggled(report, UNTOUCHED, 'duplicates');
  assert.equal(selected(report, withDuplicates, 'duplicates'), true);
  assert.equal(selectedBytes(report, withDuplicates), 11 * GB);
  // L'ordre des segments est celui de l'écran : fichiers inutiles, doublons, corbeille.
  assert.deepEqual(segments(report, withDuplicates), [3 * GB, 2 * GB, 6 * GB]);
  // Elle décoche la corbeille, puis revient sur les doublons.
  const withoutBin = toggled(report, withDuplicates, 'recycleBin');
  assert.equal(selectedBytes(report, withoutBin), 5 * GB);
  const back = toggled(report, withoutBin, 'duplicates');
  assert.equal(selected(report, back, 'duplicates'), false);
  assert.deepEqual(segments(report, back), [3 * GB, 0, 0]);
  // Rien n'a changé pour qui n'a touché à rien.
  assert.deepEqual(UNTOUCHED, { analysis: null, changed: {} });
});

test('une nouvelle analyse ne coche jamais rien de plus que les cases d’office', () => {
  const first = summary(true, FOUND, 1_000);
  // La personne coche les doublons et décoche la corbeille.
  const chosen = toggled(first, toggled(first, UNTOUCHED, 'duplicates'), 'recycleBin');
  assert.equal(selectedBytes(first, chosen), 5 * GB);
  // Nouvelle analyse : les doublons cochés tout à l'heure ne le sont plus…
  const second = summary(true, FOUND, 2_000);
  assert.equal(selected(second, chosen, 'duplicates'), false);
  // … et la corbeille, qu'elle avait décochée, le reste.
  assert.equal(selected(second, chosen, 'recycleBin'), false);
  assert.equal(selectedBytes(second, chosen), 3 * GB);
  // Un clic sur la nouvelle analyse n'emporte avec lui que les cases décochées.
  assert.deepEqual(toggled(second, chosen, 'junkFiles'), { analysis: 2_000, changed: { recycleBin: false, junkFiles: false } });
  // Elle peut recocher la corbeille : ce choix-là ne vaut que pour cette analyse.
  const again = toggled(second, chosen, 'recycleBin');
  assert.equal(selectedBytes(second, again), 9 * GB);
  assert.equal(selected(summary(true, FOUND, 3_000), again, 'recycleBin'), true);
  assert.equal(selected(summary(true, FOUND, 3_000), again, 'duplicates'), false);
});

test('un choix fait sur les premiers résultats tient quand l’analyse se termine', () => {
  const partial = summary(false, [entry('recycleBin', 6 * GB), entry('junkFiles', 3 * GB), entry('duplicates', 0, 'pending'), entry('largeFiles', 18 * GB)]);
  // Tant que la recherche continue, on ne coche pas des doublons qu'on ne connaît pas encore.
  assert.equal(selectable(partial, 'duplicates'), false);
  assert.equal(toggled(partial, UNTOUCHED, 'duplicates'), UNTOUCHED);
  const withoutBin = toggled(partial, UNTOUCHED, 'recycleBin');
  assert.equal(selectedBytes(partial, withoutBin), 3 * GB);
  // Le rapport complet de la même analyse : la corbeille reste décochée, les doublons aussi.
  const complete = summary(true, FOUND);
  assert.equal(selected(complete, withoutBin, 'recycleBin'), false);
  assert.equal(selected(complete, withoutBin, 'duplicates'), false);
  assert.equal(selectedBytes(complete, withoutBin), 3 * GB);
});

test('une ligne non mesurée, ou à vérifier, ne se coche pas', () => {
  const report = summary(true, [entry('recycleBin', 0, 'unavailable'), entry('junkFiles', GB), entry('duplicates', 0, 'unavailable')]);
  assert.equal(selectable(report, 'recycleBin'), false);
  assert.equal(selected(report, UNTOUCHED, 'recycleBin'), false);
  assert.equal(toggled(report, UNTOUCHED, 'recycleBin'), UNTOUCHED);
  // Même un choix enregistré ne coche pas une ligne qui ne peut pas l'être.
  const forced: Selection = { analysis: report.started, changed: { recycleBin: true, duplicates: true } };
  assert.equal(selected(report, forced, 'recycleBin'), false);
  assert.equal(selected(report, forced, 'duplicates'), false);
  assert.equal(selectedBytes(report, forced), GB);
  // Une catégorie que le cœur range dans « À vérifier » ne part pas avec le grand vide.
  const moved = summary(true, [{ ...entry('duplicates', 2 * GB), lot: 'review' }]);
  assert.equal(selectable(moved, 'duplicates'), false);
  assert.equal(selectedBytes(moved, { analysis: moved.started, changed: { duplicates: true } }), 0);
});

test('la clé du rapport change quand le rapport change', () => {
  const partial = view('scanning', 'duplicatesQuick', summary(false, []));
  const complete = view('results', 'done', summary(true, []));
  assert.equal(reportKey(view('idle', 'preparing', null)), '');
  assert.notEqual(reportKey(partial), reportKey(complete));
  assert.equal(reportKey(complete), reportKey(view('results', 'done', summary(true, []))));
});

test('le seuil de la corbeille se compte en Go binaires', () => {
  assert.equal(binOverThreshold(5 * GB, 5), true);
  assert.equal(binOverThreshold(5 * GB - 1, 5), false);
  // Une corbeille vide ne déborde jamais.
  assert.equal(binOverThreshold(0, 0), false);
  assert.equal(binOverThreshold(1, 0), true);
});
