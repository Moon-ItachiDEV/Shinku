// Ce que les écrans du grand vide tirent d'une vue de session : la demande,
// la confirmation, l'avancement et le bilan.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type {
  CategorySummary,
  CleanPhase,
  CleanPreview,
  CleanReport,
  CleanRequest,
  CleanView,
  Lot,
  SessionView,
  Summary,
} from '../src/backend/types.ts';
import {
  binWasFull,
  CLEAN_STEPS,
  cleanFraction,
  cleanRequest,
  confirmedRequest,
  confirmSteps,
  countedBeyondGain,
  failedSteps,
  headline,
  readiness,
  reportParts,
  requestedSteps,
  skippedParts,
  skippedTotal,
  stepPercent,
} from '../src/model/clean.ts';
import { type Selection, toggled, UNTOUCHED } from '../src/model/scan.ts';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const ANALYSIS = 1_699_999_940_000;

/** Les lots tels que le cœur les range : seuls les fichiers inutiles et la corbeille sont automatiques. */
const LOTS: Partial<Record<CategorySummary['module'], Lot>> = { duplicates: 'optional', largeFiles: 'review' };

function entry(module: CategorySummary['module'], bytes: number, state: CategorySummary['state'] = 'ready'): CategorySummary {
  return { module, lot: LOTS[module] ?? 'auto', risk: 'A', bytes, items: bytes > 0 ? 3 : 0, state };
}

function summary(categories: CategorySummary[], complete = true): Summary {
  const autoBytes = categories.filter((item) => item.lot === 'auto').reduce((total, item) => total + item.bytes, 0);
  return { complete, started: ANALYSIS, finished: 1_700_000_000_000, autoBytes, categories };
}

/** Une analyse terminée : 6 Go à la corbeille, 3 Go de fichiers inutiles, 2 Go de doublons. */
const FOUND = summary([entry('recycleBin', 6 * GB), entry('junkFiles', 3 * GB), entry('duplicates', 2 * GB), entry('largeFiles', 18 * GB)]);

function results(report: Summary | null, state: SessionView['state'] = 'results', stale = false): SessionView {
  return {
    state,
    progress: { phase: 'done', folders: 10, files: 100, foundBytes: 0, workDone: 0, workTotal: 0 },
    summary: report,
    skipped: { folders: 0, refusedRoots: 0 },
    stale,
    clean: null,
  };
}

/** La personne coche aussi les doublons. */
const WITH_DUPLICATES = toggled(FOUND, UNTOUCHED, 'duplicates');

const flags = (request: CleanRequest) => requestedSteps(request).join(',');

// ── La demande ─────────────────────────────────────────────────────────────

test('sans y toucher, la demande ne contient que les cases d’office : jamais de fichiers personnels', () => {
  const request = cleanRequest(FOUND, UNTOUCHED);
  assert.deepEqual(request, { analysis: ANALYSIS, recycleBin: true, junkFiles: true, duplicates: false });
});

test('les doublons ne partent que si la personne les a cochés, dans cette analyse', () => {
  assert.equal(cleanRequest(FOUND, WITH_DUPLICATES).duplicates, true);
  // Le même choix, fait pour une autre analyse, ne vaut plus.
  const elsewhere: Selection = { analysis: ANALYSIS - 60_000, changed: { duplicates: true } };
  assert.equal(cleanRequest(FOUND, elsewhere).duplicates, false);
});

test('une case décochée sort de la demande', () => {
  const withoutBin = toggled(FOUND, UNTOUCHED, 'recycleBin');
  assert.equal(flags(cleanRequest(FOUND, withoutBin)), 'junkFiles');
});

test('une catégorie vide n’est jamais demandée, même cochée d’office', () => {
  // L'analyse a vu la corbeille vide : si elle s'est remplie depuis, personne n'a vu de quoi.
  const report = summary([entry('recycleBin', 0), entry('junkFiles', 3 * GB), entry('duplicates', 0)]);
  const everything: Selection = { analysis: ANALYSIS, changed: { recycleBin: true, duplicates: true } };
  assert.equal(flags(cleanRequest(report, everything)), 'junkFiles');
});

test('les étapes d’une demande sont dans l’ordre du grand vide : la corbeille d’abord', () => {
  assert.deepEqual(CLEAN_STEPS, ['recycleBin', 'junkFiles', 'duplicates']);
  assert.equal(flags(cleanRequest(FOUND, WITH_DUPLICATES)), 'recycleBin,junkFiles,duplicates');
});

test('« Faire le vide » n’est proposé que sur des résultats complets et récents, avec de quoi nettoyer', () => {
  assert.equal(readiness(results(FOUND), UNTOUCHED), 'ready');
  // L'analyse tourne encore : les doublons ne sont pas connus.
  const partial = summary([entry('recycleBin', 6 * GB), entry('junkFiles', 3 * GB), entry('duplicates', 0, 'pending')], false);
  assert.equal(readiness(results(partial, 'scanning'), UNTOUCHED), 'pending');
  assert.equal(readiness(results(FOUND, 'scanning'), UNTOUCHED), 'pending');
  assert.equal(readiness(results(null, 'idle'), UNTOUCHED), 'pending');
  // Plus de dix minutes : il faut réanalyser.
  assert.equal(readiness(results(FOUND, 'results', true), UNTOUCHED), 'stale');
  // Des résultats partiels ne sont jamais « trop anciens » : ils ne sont pas prêts, c'est tout.
  assert.equal(readiness(results(partial, 'scanning', true), UNTOUCHED), 'pending');
});

test('rien de coché et rien à nettoyer ne se disent pas de la même façon', () => {
  let nothing = toggled(FOUND, UNTOUCHED, 'recycleBin');
  nothing = toggled(FOUND, nothing, 'junkFiles');
  assert.equal(readiness(results(FOUND), nothing), 'unchecked');
  // Seuls des doublons, pas cochés : il reste de quoi nettoyer.
  const onlyCopies = summary([entry('recycleBin', 0), entry('junkFiles', 0), entry('duplicates', 2 * GB)]);
  assert.equal(readiness(results(onlyCopies), UNTOUCHED), 'unchecked');
  // Les gros fichiers ne partent jamais avec le grand vide : ils ne comptent pas.
  const clean = summary([entry('recycleBin', 0), entry('junkFiles', 0), entry('duplicates', 0), entry('largeFiles', 18 * GB)]);
  assert.equal(readiness(results(clean), UNTOUCHED), 'empty');
  // Une corbeille que l'analyse n'a pas pu mesurer ne se nettoie pas.
  const unknown = summary([entry('recycleBin', 6 * GB, 'unavailable'), entry('junkFiles', 0)]);
  assert.equal(readiness(results(unknown), UNTOUCHED), 'empty');
});

// ── La confirmation ────────────────────────────────────────────────────────

const PREVIEW: CleanPreview = {
  analysis: ANALYSIS,
  recycleBin: { bytes: 7 * GB, items: 1290 },
  junk: { bytes: 3 * GB, files: 18_412, openApps: [{ label: 'Discord', bytes: 388 * MB }] },
  duplicates: { bytes: 2 * GB, copies: 8, syncedCopies: 2, otherDiskCopies: 0 },
};

test('la confirmation annonce les chiffres du moment, pas ceux de l’analyse', () => {
  const request = confirmedRequest(PREVIEW, cleanRequest(FOUND, WITH_DUPLICATES));
  assert.deepEqual(confirmSteps(PREVIEW, request), [
    // La corbeille a grossi depuis l'analyse : c'est ce qu'elle contient maintenant qui partira.
    { id: 'recycleBin', bytes: 7 * GB, count: 1290 },
    { id: 'junkFiles', bytes: 3 * GB, count: 18_412 },
    { id: 'duplicates', bytes: 2 * GB, count: 8 },
  ]);
});

test('ce que la confirmation liste est exactement ce qui est envoyé', () => {
  const selections = [UNTOUCHED, WITH_DUPLICATES, toggled(FOUND, WITH_DUPLICATES, 'junkFiles'), toggled(FOUND, UNTOUCHED, 'recycleBin')];
  const previews: CleanPreview[] = [
    PREVIEW,
    { ...PREVIEW, recycleBin: { bytes: 0, items: 0 } },
    { ...PREVIEW, recycleBin: null },
    { ...PREVIEW, duplicates: { bytes: 0, copies: 0, syncedCopies: 0, otherDiskCopies: 0 } },
    { ...PREVIEW, junk: { bytes: 0, files: 0, openApps: [] } },
  ];
  for (const selection of selections) {
    for (const preview of previews) {
      const wanted = cleanRequest(FOUND, selection);
      const sent = confirmedRequest(preview, wanted);
      assert.deepEqual(confirmSteps(preview, sent).map((step) => step.id), requestedSteps(sent));
      for (const step of confirmSteps(preview, sent)) {
        // Rien n'est annoncé qui n'ait de quoi partir, et rien ne s'ajoute à ce qui était coché.
        assert.ok(step.count > 0, `${step.id} annoncé sans rien à faire`);
        assert.equal(wanted[step.id], true, `${step.id} envoyé sans avoir été coché`);
      }
      assert.equal(sent.analysis, wanted.analysis);
    }
  }
});

test('une corbeille vidée entre-temps, ou devenue illisible, sort de la demande', () => {
  const wanted = cleanRequest(FOUND, UNTOUCHED);
  assert.equal(flags(confirmedRequest({ ...PREVIEW, recycleBin: { bytes: 0, items: 0 } }, wanted)), 'junkFiles');
  assert.equal(flags(confirmedRequest({ ...PREVIEW, recycleBin: null }, wanted)), 'junkFiles');
  // Plus rien du tout : la demande est vide, et rien ne sera proposé.
  const gone: CleanPreview = { ...PREVIEW, recycleBin: null, junk: { bytes: 0, files: 0, openApps: [] } };
  assert.equal(flags(confirmedRequest(gone, wanted)), '');
});

// ── L'avancement ───────────────────────────────────────────────────────────

const EVERYTHING: CleanRequest = { analysis: ANALYSIS, recycleBin: true, junkFiles: true, duplicates: true };

function running(phase: CleanPhase, workDone = 0, workTotal = 0, request: CleanRequest = EVERYTHING): CleanView {
  return {
    request,
    progress: { phase, freedBytes: 0, recycledBytes: 0, files: 0, skippedFiles: 0, workDone, workTotal },
    report: null,
  };
}

test('l’avancement d’une étape n’est dit que quand son volume de travail est connu', () => {
  assert.equal(stepPercent(running('preparing')), null);
  // La corbeille se vide d'un bloc.
  assert.equal(stepPercent(running('recycleBin')), null);
  // Les fichiers inutiles sont d'abord recomptés.
  assert.equal(stepPercent(running('junkFiles')), null);
  assert.equal(stepPercent(running('junkFiles', 250, 1000)), 25);
  assert.equal(stepPercent(running('duplicates', 999, 1000)), 99);
  assert.equal(stepPercent(running('duplicates', 1000, 1000)), 100);
  assert.equal(stepPercent(running('done')), null);
});

test('l’anneau avance d’une part égale par étape demandée', () => {
  assert.equal(cleanFraction(running('preparing')), null);
  assert.equal(cleanFraction(running('recycleBin')), null);
  assert.equal(cleanFraction(running('junkFiles')), null);
  // Deuxième étape sur trois, à moitié faite.
  assert.equal(cleanFraction(running('junkFiles', 500, 1000)), 0.5);
  assert.equal(cleanFraction(running('duplicates', 0, 1000)), 2 / 3);
  assert.equal(cleanFraction(running('duplicates', 1000, 1000)), 1);
  assert.equal(cleanFraction(running('done')), 1);
  // Seuls les doublons étaient demandés : ils font tout l'anneau.
  const onlyCopies: CleanRequest = { ...EVERYTHING, recycleBin: false, junkFiles: false };
  assert.equal(cleanFraction(running('duplicates', 250, 1000, onlyCopies)), 0.25);
  // Un travail compté au-delà de son total ne fait pas déborder l'anneau.
  assert.equal(cleanFraction(running('duplicates', 4000, 1000, onlyCopies)), 1);
});

// ── Le bilan ───────────────────────────────────────────────────────────────

/** Un grand vide allé au bout : 6 Go de corbeille, 3 Go de fichiers inutiles, 2 Go de doublons à la corbeille. */
const REPORT: CleanReport = {
  started: 1_700_000_100_000,
  finished: 1_700_000_130_000,
  outcome: 'completed',
  freedBytes: 9 * GB,
  recoverableBytes: 2 * GB,
  recycleBin: { state: 'done', bytes: 6 * GB, items: 1284 },
  junk: { state: 'done', bytes: 3 * GB, files: 18_412, leftOpen: [] },
  duplicates: { state: 'done', bytes: 2 * GB, copies: 8, stop: null },
  skipped: { inUse: 0, changed: 0, denied: 0, unrecyclable: 0, other: 0 },
  durationMs: 30_000,
};

test('le grand chiffre du bilan ne mélange jamais libéré et récupérable', () => {
  assert.deepEqual(headline(REPORT), { kind: 'freed', bytes: 9 * GB, recoverable: 2 * GB });
  // Rien de libéré pour de bon : ce qui est parti attend dans la corbeille.
  const onlyCopies: CleanReport = {
    ...REPORT,
    freedBytes: 0,
    recycleBin: { state: 'notRequested', bytes: 0, items: 0 },
    junk: { state: 'notRequested', bytes: 0, files: 0, leftOpen: [] },
  };
  assert.deepEqual(headline(onlyCopies), { kind: 'recycled', bytes: 2 * GB });
  assert.deepEqual(headline({ ...onlyCopies, recoverableBytes: 0 }), { kind: 'nothing' });
});

test('le bilan détaille les étapes demandées, dans l’ordre et les couleurs des Résultats', () => {
  assert.deepEqual(reportParts(REPORT), [
    { id: 'junkFiles', bytes: 3 * GB },
    { id: 'duplicates', bytes: 2 * GB },
    { id: 'recycleBin', bytes: 6 * GB },
  ]);
  const withoutCopies: CleanReport = { ...REPORT, duplicates: { state: 'notRequested', bytes: 0, copies: 0, stop: null } };
  assert.deepEqual(reportParts(withoutCopies).map((part) => part.id), ['junkFiles', 'recycleBin']);
  // Une étape arrêtée en route garde sa ligne : ce qu'elle a fait partir est parti.
  const stopped: CleanReport = { ...REPORT, junk: { state: 'stopped', bytes: GB, files: 6000, leftOpen: [] } };
  assert.deepEqual(reportParts(stopped)[0], { id: 'junkFiles', bytes: GB });
});

test('les étapes qui n’ont pas pu se faire sont nommées', () => {
  assert.deepEqual(failedSteps(REPORT), []);
  const broken: CleanReport = {
    ...REPORT,
    recycleBin: { state: 'failed', bytes: 0, items: 0 },
    duplicates: { state: 'failed', bytes: 0, copies: 0, stop: null },
  };
  assert.deepEqual(failedSteps(broken), ['recycleBin', 'duplicates']);
  // Arrêtée n'est pas ratée.
  assert.deepEqual(failedSteps({ ...REPORT, junk: { ...REPORT.junk, state: 'stopped' } }), []);
});

test('les fichiers laissés en place se comptent raison par raison', () => {
  assert.deepEqual(skippedParts(REPORT), []);
  assert.equal(skippedTotal(REPORT), 0);
  const left: CleanReport = { ...REPORT, skipped: { inUse: 9, changed: 0, denied: 2, unrecyclable: 1, other: 0 } };
  assert.deepEqual(skippedParts(left), [
    { reason: 'inUse', files: 9 },
    { reason: 'denied', files: 2 },
    { reason: 'unrecyclable', files: 1 },
  ]);
  assert.equal(skippedTotal(left), 12);
});

test('une corbeille qui garde moins que ce qu’elle a reçu était pleine', () => {
  assert.equal(binWasFull(REPORT), false);
  assert.equal(binWasFull({ ...REPORT, recoverableBytes: 2 * GB - 1 }), true);
  // Aucun doublon envoyé : la question ne se pose pas.
  const none: CleanReport = { ...REPORT, recoverableBytes: 0, duplicates: { state: 'notRequested', bytes: 0, copies: 0, stop: null } };
  assert.equal(binWasFull(none), false);
});

test('un disque qui a gagné moins que le compte le fait savoir', () => {
  assert.equal(countedBeyondGain(REPORT), null);
  assert.equal(countedBeyondGain({ ...REPORT, freedBytes: 8 * GB }), 9 * GB);
});
