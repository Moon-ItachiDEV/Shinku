// Ce que les écrans du grand vide tirent d'une vue de session : la demande à
// envoyer, ce que la confirmation annonce, l'avancement et le bilan.
//
// Ces fonctions sont pures : elles ne font que lire ce que rend le cœur. Elles
// sont testées sans navigateur (tests/clean.test.ts).

import type { CleanPreview, CleanReport, CleanRequest, CleanView, SessionView, Summary } from '../backend/types.ts';
import { percent } from '../format.ts';
import { category, CLEAN_ROWS, type CleanRowId, selectable, selected, type Selection } from './scan.ts';

/** Les étapes du grand vide, dans l'ordre où le cœur les mène. */
export const CLEAN_STEPS = ['recycleBin', 'junkFiles', 'duplicates'] as const satisfies readonly CleanRowId[];
export type CleanStepId = (typeof CLEAN_STEPS)[number];

// ── La demande ─────────────────────────────────────────────────────────────

/**
 * La demande que les cases cochées représentent.
 *
 * Une case ne compte que si sa catégorie a quelque chose à nettoyer : une
 * corbeille que l'analyse a vue vide n'est pas vidée, même si elle s'est
 * remplie depuis, parce que personne n'a vu ce qu'elle contient maintenant.
 */
export function cleanRequest(summary: Summary, selection: Selection): CleanRequest {
  const wanted = (id: CleanRowId): boolean =>
    selected(summary, selection, id) && (category(summary, id)?.items ?? 0) > 0;
  return {
    analysis: summary.started,
    recycleBin: wanted('recycleBin'),
    junkFiles: wanted('junkFiles'),
    duplicates: wanted('duplicates'),
  };
}

/** Les étapes qu'une demande contient, dans l'ordre où elles seront menées. */
export function requestedSteps(request: CleanRequest): CleanStepId[] {
  return CLEAN_STEPS.filter((id) => request[id]);
}

/**
 * Ce que le bouton « Faire le vide » peut faire de ces résultats.
 *
 * - `pending` : l'analyse n'est pas finie, doublons compris ;
 * - `stale` : les résultats sont trop anciens, il faut réanalyser ;
 * - `empty` : aucune catégorie n'a quoi que ce soit à nettoyer ;
 * - `unchecked` : il y a de quoi nettoyer, mais rien n'est coché ;
 * - `ready` : le grand vide peut être proposé.
 */
export type Readiness = 'pending' | 'stale' | 'empty' | 'unchecked' | 'ready';

export function readiness(view: SessionView, selection: Selection): Readiness {
  const summary = view.summary;
  if (view.state !== 'results' || summary === null || !summary.complete) {
    return 'pending';
  }
  if (view.stale) {
    return 'stale';
  }
  if (requestedSteps(cleanRequest(summary, selection)).length > 0) {
    return 'ready';
  }
  const cleanable = CLEAN_ROWS.some((id) => selectable(summary, id) && (category(summary, id)?.items ?? 0) > 0);
  return cleanable ? 'unchecked' : 'empty';
}

// ── La confirmation ────────────────────────────────────────────────────────

/** Une étape telle que la confirmation l'annonce : ce qui partirait maintenant. */
export interface ConfirmStep {
  id: CleanStepId;
  bytes: number;
  /** Éléments de la corbeille, fichiers inutiles ou copies en double. */
  count: number;
}

/**
 * La demande, une fois confrontée au disque tel qu'il est maintenant : une
 * étape qui n'a plus rien à faire en sort. C'est cette demande-là que la
 * confirmation affiche, et c'est elle qui part si la personne confirme.
 */
export function confirmedRequest(preview: CleanPreview, request: CleanRequest): CleanRequest {
  return {
    analysis: request.analysis,
    recycleBin: request.recycleBin && preview.recycleBin !== null && preview.recycleBin.items > 0,
    junkFiles: request.junkFiles && preview.junk.files > 0,
    duplicates: request.duplicates && preview.duplicates.copies > 0,
  };
}

/** Les lignes de la confirmation : une par étape de la demande, avec ses chiffres du moment. */
export function confirmSteps(preview: CleanPreview, request: CleanRequest): ConfirmStep[] {
  return requestedSteps(request).map((id): ConfirmStep => {
    switch (id) {
      case 'recycleBin':
        return { id, bytes: preview.recycleBin?.bytes ?? 0, count: preview.recycleBin?.items ?? 0 };
      case 'junkFiles':
        return { id, bytes: preview.junk.bytes, count: preview.junk.files };
      case 'duplicates':
        return { id, bytes: preview.duplicates.bytes, count: preview.duplicates.copies };
    }
  });
}

// ── L'avancement ───────────────────────────────────────────────────────────

/**
 * Avancement de l'étape en cours, en pour cent, ou `null` quand son volume de
 * travail n'est pas connu : la corbeille se vide d'un bloc, et les fichiers
 * inutiles sont d'abord recomptés sur le disque.
 */
export function stepPercent(clean: CleanView): number | null {
  const { phase, workDone, workTotal } = clean.progress;
  if ((phase !== 'junkFiles' && phase !== 'duplicates') || workTotal <= 0) {
    return null;
  }
  return percent(workDone, workTotal);
}

/**
 * Part du grand vide déjà faite, entre 0 et 1, ou `null` quand on ne peut pas
 * le savoir. Chaque étape demandée compte pour une part égale : personne ne
 * sait d'avance laquelle sera la plus longue.
 */
export function cleanFraction(clean: CleanView): number | null {
  const { phase, workDone, workTotal } = clean.progress;
  if (phase === 'done') {
    return 1;
  }
  const steps = requestedSteps(clean.request);
  const rank = phase === 'preparing' ? -1 : steps.indexOf(phase);
  if (rank < 0 || workTotal <= 0) {
    return null;
  }
  return (rank + Math.min(1, workDone / workTotal)) / steps.length;
}

// ── Le bilan ───────────────────────────────────────────────────────────────

/**
 * Le grand chiffre du bilan. Ce qui est libéré pour de bon et ce qui attend
 * dans la corbeille ne s'additionnent jamais.
 */
export type Headline =
  /** De la place a été libérée ; `recoverable` attend en plus dans la corbeille. */
  | { kind: 'freed'; bytes: number; recoverable: number }
  /** Rien n'est libéré pour de bon : tout ce qui est parti attend dans la corbeille. */
  | { kind: 'recycled'; bytes: number }
  /** Rien n'est parti. */
  | { kind: 'nothing' };

export function headline(report: CleanReport): Headline {
  if (report.freedBytes > 0) {
    return { kind: 'freed', bytes: report.freedBytes, recoverable: report.recoverableBytes };
  }
  if (report.recoverableBytes > 0) {
    return { kind: 'recycled', bytes: report.recoverableBytes };
  }
  return { kind: 'nothing' };
}

/** Une étape demandée, avec la place occupée par ce qu'elle a réellement fait partir. */
export interface ReportPart {
  id: CleanStepId;
  bytes: number;
}

/** Les étapes demandées, dans l'ordre de l'écran Résultats : mêmes lignes, mêmes couleurs. */
export function reportParts(report: CleanReport): ReportPart[] {
  const steps = { junkFiles: report.junk, duplicates: report.duplicates, recycleBin: report.recycleBin };
  return CLEAN_ROWS.filter((id) => steps[id].state !== 'notRequested').map((id) => ({ id, bytes: steps[id].bytes }));
}

/** Les étapes qui n'ont pas pu se faire, ou pas en entier, dans l'ordre du grand vide. */
export function failedSteps(report: CleanReport): CleanStepId[] {
  const states = {
    recycleBin: report.recycleBin.state,
    junkFiles: report.junk.state,
    duplicates: report.duplicates.state,
  };
  return CLEAN_STEPS.filter((id) => states[id] === 'failed');
}

export const SKIP_REASONS = ['inUse', 'changed', 'denied', 'unrecyclable', 'other'] as const;
export type SkipReason = (typeof SKIP_REASONS)[number];

/** Les fichiers laissés en place, raison par raison ; les raisons sans fichier n'y sont pas. */
export function skippedParts(report: CleanReport): Array<{ reason: SkipReason; files: number }> {
  return SKIP_REASONS.map((reason) => ({ reason, files: report.skipped[reason] })).filter((part) => part.files > 0);
}

export function skippedTotal(report: CleanReport): number {
  return SKIP_REASONS.reduce((total, reason) => total + report.skipped[reason], 0);
}

/**
 * Vrai si la corbeille garde moins que ce qui lui a été envoyé : pleine, elle
 * a déjà abandonné une partie de son contenu.
 */
export function binWasFull(report: CleanReport): boolean {
  return report.duplicates.bytes > report.recoverableBytes;
}

/**
 * Ce que la corbeille vidée et les fichiers inutiles supprimés occupaient, si
 * le disque a gagné moins que cela ; sinon `null`. Le grand chiffre du bilan
 * ne dépasse jamais ce que le disque a réellement gagné.
 */
export function countedBeyondGain(report: CleanReport): number | null {
  const counted = report.recycleBin.bytes + report.junk.bytes;
  return counted > report.freedBytes ? counted : null;
}
