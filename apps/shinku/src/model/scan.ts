// Ce que les écrans Analyse et Résultats tirent d'une vue de session.
//
// Ces fonctions sont pures : elles ne font que lire la vue rendue par le cœur.
// Elles sont testées sans navigateur (tests/scan.test.ts).

import type { CategorySummary, Lot, ModuleId, SessionView, Summary, Volume } from '../backend/types.ts';
import { percent } from '../format.ts';

/** Les lignes de l'écran Analyse, dans l'ordre de l'écran. */
export const SCAN_ROWS = ['junkFiles', 'duplicates', 'recycleBin', 'largeFiles', 'startup'] as const;
export type ScanRowId = (typeof SCAN_ROWS)[number];

/** Les lignes à cocher de l'écran Résultats, dans l'ordre de l'écran. */
export const CLEAN_ROWS = ['junkFiles', 'duplicates', 'recycleBin'] as const;
export type CleanRowId = (typeof CLEAN_ROWS)[number];

/** Les cases que la personne a changées, et dans quel sens. */
export type Choices = Partial<Record<CleanRowId, boolean>>;

/**
 * Les cases que la personne a changées sur l'écran Résultats.
 *
 * Tant qu'elle n'y touche pas, une case suit le cœur : seul le lot automatique
 * est coché d'office, jamais des fichiers personnels. Une case qu'elle a
 * cochée ne vaut que pour l'analyse où elle l'a fait ; une case qu'elle a
 * décochée le reste à l'analyse suivante. Une nouvelle analyse ne coche donc
 * jamais rien de plus que les cases d'office.
 */
export interface Selection {
  /** Début de l'analyse où ces choix ont été faits (`Summary.started`), ou `null` s'il n'y en a aucun. */
  analysis: number | null;
  changed: Choices;
}

export const UNTOUCHED: Selection = { analysis: null, changed: {} };

export type ScanRow =
  /** Pas encore commencé. */
  | { id: ScanRowId; status: 'waiting' }
  /** En cours. `bytes` : chiffre provisoire. `percent` : avancement connu. */
  | { id: ScanRowId; status: 'running'; bytes?: number; percent?: number }
  /** Terminé. Seul le lot `auto` compte dans le grand chiffre de l'écran. */
  | { id: ScanRowId; status: 'done'; bytes: number; items: number; lot: Lot }
  /** Non mesuré dans cette analyse. */
  | { id: ScanRowId; status: 'unavailable' }
  /** Module à venir. */
  | { id: ScanRowId; status: 'soon' };

export function category(summary: Summary | null, module: ModuleId): CategorySummary | undefined {
  return summary?.categories.find((entry) => entry.module === module);
}

/** Avancement de la recherche de doublons, en pour cent de l'étape en cours. */
export function duplicatesPercent(view: SessionView): number {
  const { phase, workDone, workTotal } = view.progress;
  if (phase === 'duplicatesQuick' || phase === 'duplicatesFull') {
    return percent(workDone, workTotal);
  }
  return phase === 'done' ? 100 : 0;
}

export function scanRows(view: SessionView): ScanRow[] {
  const walking = view.state === 'scanning' && view.summary === null;
  return SCAN_ROWS.map((id): ScanRow => {
    // Le module Démarrage n'existe pas encore.
    if (id === 'startup') {
      return { id, status: 'soon' };
    }
    const entry = category(view.summary, id);
    if (entry === undefined) {
      if (!walking) {
        return { id, status: 'waiting' };
      }
      // Pendant le parcours : les fichiers inutiles et les gros fichiers se
      // comptent, les doublons et la corbeille viennent juste après.
      if (id === 'junkFiles') {
        return { id, status: 'running', bytes: view.progress.foundBytes };
      }
      return id === 'largeFiles' ? { id, status: 'running' } : { id, status: 'waiting' };
    }
    switch (entry.state) {
      case 'ready':
        return { id, status: 'done', bytes: entry.bytes, items: entry.items, lot: entry.lot };
      case 'pending':
        return { id, status: 'running', percent: duplicatesPercent(view) };
      case 'unavailable':
        return { id, status: 'unavailable' };
    }
  });
}

/** Le grand chiffre de l'écran Analyse : ce qui est déjà sûr de pouvoir partir. */
export function counterBytes(view: SessionView): number {
  return view.summary?.autoBytes ?? view.progress.foundBytes;
}

/**
 * Part de l'analyse déjà faite, entre 0 et 1, ou `null` quand on ne peut pas
 * le savoir : pendant le parcours, personne ne connaît la taille du disque à lire.
 */
export function ringFraction(view: SessionView): number | null {
  const { phase, workDone, workTotal } = view.progress;
  const part = workTotal > 0 ? Math.min(1, workDone / workTotal) : 0;
  switch (phase) {
    case 'preparing':
    case 'walking':
      return view.summary?.complete === true ? 1 : null;
    // La comparaison des extraits est rapide ; la lecture complète fait le gros du temps.
    case 'duplicatesQuick':
      return 0.05 + 0.25 * part;
    case 'duplicatesFull':
      return 0.3 + 0.7 * part;
    case 'done':
      return 1;
    case 'cancelled':
      return 0;
  }
}

/**
 * Vrai si la case de cette ligne peut être cochée : son chiffre est définitif,
 * et sa catégorie peut partir avec « Faire le vide ».
 */
export function selectable(summary: Summary | null, id: CleanRowId): boolean {
  const entry = category(summary, id);
  return entry !== undefined && entry.state === 'ready' && entry.lot !== 'review';
}

/** Ceux des choix de la personne qui valent pour ce rapport. */
function choices(summary: Summary, selection: Selection): Choices {
  if (selection.analysis === summary.started) {
    return selection.changed;
  }
  // Une autre analyse : seules les cases décochées suivent.
  const kept: Choices = {};
  for (const id of CLEAN_ROWS) {
    if (selection.changed[id] === false) {
      kept[id] = false;
    }
  }
  return kept;
}

/** Vrai si cette ligne est cochée : le choix de la personne, sinon la case d'office. */
export function selected(summary: Summary | null, selection: Selection, id: CleanRowId): boolean {
  if (summary === null || !selectable(summary, id)) {
    return false;
  }
  return choices(summary, selection)[id] ?? (category(summary, id)?.lot === 'auto');
}

/** La sélection après un clic sur la case de cette ligne. */
export function toggled(summary: Summary | null, selection: Selection, id: CleanRowId): Selection {
  if (summary === null || !selectable(summary, id)) {
    return selection;
  }
  const changed = { ...choices(summary, selection), [id]: !selected(summary, selection, id) };
  return { analysis: summary.started, changed };
}

/** Total des catégories cochées : ce que « Faire le vide » libérerait. */
export function selectedBytes(summary: Summary | null, selection: Selection): number {
  let total = 0;
  for (const id of CLEAN_ROWS) {
    if (selected(summary, selection, id)) {
      total += category(summary, id)?.bytes ?? 0;
    }
  }
  return total;
}

/** Largeurs relatives de la barre en trois segments. */
export function segments(summary: Summary | null, selection: Selection): number[] {
  return CLEAN_ROWS.map((id) => (selected(summary, selection, id) ? (category(summary, id)?.bytes ?? 0) : 0));
}

/** Une clé qui change dès que le rapport publié change. */
export function reportKey(view: SessionView): string {
  return view.summary === null ? '' : `${view.summary.finished}:${view.summary.complete}`;
}

/** Le disque à montrer : celui de Windows, sinon le premier disque interne. */
export function mainVolume(volumes: readonly Volume[]): Volume | undefined {
  return volumes.find((volume) => volume.system) ?? volumes.find((volume) => volume.fixed) ?? volumes[0];
}

/**
 * Vrai si la corbeille dépasse le seuil d'alerte, en Go binaires comme
 * l'Explorateur. Une corbeille vide ne déborde jamais, quel que soit le seuil.
 */
export function binOverThreshold(bytes: number, thresholdGb: number): boolean {
  return bytes > 0 && bytes >= thresholdGb * 1024 ** 3;
}
