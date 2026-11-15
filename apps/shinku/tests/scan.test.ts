// Ce que les écrans Analyse et Résultats tirent d'une vue de session.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { CategorySummary, Phase, SessionView, Summary } from '../src/backend/types.ts';
import {
  ALL_SELECTED,
  binOverThreshold,
  counterBytes,
  duplicatesPercent,
  reportKey,
  ringFraction,
  scanRows,
  segments,
  selectedBytes,
} from '../src/model/scan.ts';

const GB = 1024 ** 3;

function entry(module: CategorySummary['module'], bytes: number, state: CategorySummary['state'] = 'ready'): CategorySummary {
  return { module, lot: module === 'largeFiles' ? 'review' : 'auto', risk: 'A', bytes, items: bytes > 0 ? 3 : 0, state };
}

function summary(complete: boolean, categories: CategorySummary[]): Summary {
  const autoBytes = categories.filter((item) => item.lot === 'auto').reduce((total, item) => total + item.bytes, 0);
  return { complete, finished: 1_700_000_000_000, autoBytes, categories };
}

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

test('le total suit les catégories cochées', () => {
  const report = summary(true, [entry('recycleBin', 6 * GB), entry('junkFiles', 3 * GB), entry('duplicates', 2 * GB), entry('largeFiles', 18 * GB)]);
  assert.equal(selectedBytes(report, ALL_SELECTED), 11 * GB);
  assert.equal(selectedBytes(report, { ...ALL_SELECTED, recycleBin: false }), 5 * GB);
  assert.equal(selectedBytes(null, ALL_SELECTED), 0);
  // L'ordre des segments est celui de l'écran : fichiers inutiles, doublons, corbeille.
  assert.deepEqual(segments(report, ALL_SELECTED), [3 * GB, 2 * GB, 6 * GB]);
  assert.deepEqual(segments(report, { ...ALL_SELECTED, duplicates: false }), [3 * GB, 0, 6 * GB]);
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
