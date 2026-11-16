// Le Journal : de quoi écrire chaque ligne. Aucune chaîne n'est écrite ici (elles sont dans i18n/fr.ts) ; ce
// fichier choisit seulement quelle phrase va avec quelle ligne, et où elle mène.
//
// Chaque phrase est au passé et ne dit que ce qui a été noté : le Journal ne dit jamais dans quel état est le PC
// maintenant. Il ne fait rien lui-même : pour revenir en arrière, il mène à la page où le geste existe déjà.

import type { HistoryLine, HistoryView } from '../backend/types.ts';
import { formatDay, formatSpan } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import type { Route } from '../state.ts';
import type { IconName } from '../views/art.ts';

/** Un nom à montrer tel quel (c'est du texte), coupé par le style s'il est long, entier dans `title`. */
export interface JournalName {
  name: string;
}

/** Où mène une ligne : la page où le geste existe, et le texte du bouton. */
export interface JournalLink {
  route: Route;
  label: string;
}

export interface JournalRow {
  icon: IconName;
  /** Le début de la phrase, le nom s'il y en a un, la fin. */
  before: string;
  name: JournalName | null;
  after: string;
  /** Quand : `null` pour une date que le cœur a écartée (la page écrit « date inconnue »). */
  date: number | null;
  link: JournalLink | null;
}

/** Les repères de focus de la page. */
export const journalFocus = {
  title: 'journal-title',
  retry: 'journal-retry',
  forget: 'journal-forget',
};

/** « 3,2 Go libérés, 1,1 Go mis à la corbeille », puis les fichiers laissés en place s'il y en a. */
function cleanSentence(line: Extract<HistoryLine, { kind: 'clean' }>): string {
  if (line.state === 'running') {
    return fr.journal.clean.running;
  }
  const totals = fr.journal.clean.totals(line.freedBytes, line.recoverableBytes);
  const head =
    line.state === 'completed'
      ? fr.journal.clean.completed(totals)
      : line.state === 'cancelled'
        ? fr.journal.clean.cancelled(totals)
        : fr.journal.clean.interrupted(totals);
  return line.skippedFiles > 0 ? `${head} ${fr.journal.clean.skipped(line.skippedFiles)}` : head;
}

/** La date qui range la ligne, et que la page écrit : la fin d'un grand vide ou d'une partie, à défaut son début. */
export function dateOf(line: HistoryLine): number | null {
  switch (line.kind) {
    case 'clean':
      return line.finishedMs ?? line.startedMs;
    case 'game':
      return line.endedMs ?? line.startedMs;
    case 'startupCut':
    case 'uninstall':
      return line.atMs;
  }
}

/** La ligne telle que la page l'écrit. `now` : pour « le 15 nov. » dans la phrase d'une entrée remise toute seule. */
export function rowOf(line: HistoryLine, now: number): JournalRow {
  const date = dateOf(line);
  switch (line.kind) {
    case 'clean':
      return {
        icon: 'home',
        before: cleanSentence(line),
        name: null,
        after: '',
        date,
        link: line.state !== 'running' && line.recoverableBytes > 0 ? { route: 'bin', label: fr.journal.seeBin } : null,
      };
    case 'startupCut': {
      // La formule de la page Démarrage, reprise telle quelle : « S'est remise toute seule depuis le 15 nov. ».
      // La date est celle de la coupure, comme sur cette page ; sans elle, la phrase se tait.
      const back = line.backMs !== null && line.atMs !== null ? fr.startup.cameBack(formatDay(line.atMs, now)) : '';
      // « nov. » porte déjà son point : on n'en ajoute pas un second.
      const cameBack = back === '' ? '' : ` ${back}${back.endsWith('.') ? '' : '.'}`;
      return {
        icon: 'startup',
        before: fr.journal.startupCut.before,
        name: { name: line.name },
        after: `${fr.journal.startupCut.after}${cameBack}`,
        date,
        link: { route: 'startup', label: fr.journal.seeStartup },
      };
    }
    case 'game': {
      // La durée seulement si la fin est après le début ; puis le bilan de la page Mode jeu, mot pour mot.
      const duration = formatSpan(line.startedMs, line.endedMs);
      const report = line.complete ? fr.journal.game.restored : fr.journal.game.notRestored;
      return {
        icon: 'gameMode',
        before: fr.journal.game.before,
        name: { name: line.name },
        after: `${fr.journal.game.after(duration)} ${report}`,
        date,
        link: line.complete ? null : { route: 'gameMode', label: fr.journal.seeGame },
      };
    }
    case 'uninstall':
      return {
        icon: 'uninstall',
        before: fr.journal.uninstall.before,
        name: { name: line.name },
        after:
          line.leftovers > 0
            ? `${fr.journal.uninstall.after} ${fr.journal.uninstall.leftovers(line.leftovers, line.leftoverBytes)}`
            : fr.journal.uninstall.after,
        date,
        link: line.leftovers > 0 ? { route: 'bin', label: fr.journal.seeBin } : null,
      };
  }
}

/** Il y a une ligne de désinstallation : le bouton « Oublier les désinstallations » existe. */
export function hasUninstalls(view: HistoryView | null): boolean {
  return view?.lines.some((line) => line.kind === 'uninstall') ?? false;
}

/**
 * La date de la ligne en `datetime` pour `<time>`, ou `undefined` si elle n'a pas de sens. Le cœur n'envoie que des
 * dates entre 1970 et demain ; la garde protège la page d'un faux cœur ou d'un cœur plus récent.
 */
export function isoOf(date: number | null): string | undefined {
  if (date === null || !Number.isFinite(date)) {
    return undefined;
  }
  const value = new Date(date);
  return Number.isNaN(value.getTime()) ? undefined : value.toISOString();
}
