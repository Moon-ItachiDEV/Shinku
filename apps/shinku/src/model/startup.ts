// Ce que la page Démarrage tire d'une vue du cœur : les comptes de l'en-tête,
// les morceaux et les pastilles d'une ligne, les textes de retour, et les
// gestes optimistes.
//
// Ces fonctions sont pures : elles ne font que lire ce que rend le cœur. Elles
// sont testées sans navigateur (tests/startup.test.ts).

import type {
  BackendError,
  StartupEntry,
  StartupScope,
  StartupView,
} from '../backend/types.ts';
import { formatDay } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import { APPS_POLL_DELAY } from './uninstall.ts';

/** Combien de temps la zone de retour garde sa phrase, en millisecondes. */
export const SAY_DELAY = 12_000;
/** Retour de la fenêtre : une vue finie depuis moins de ce délai (ms) n'est pas redemandée. */
export const STARTUP_RELOAD_GAP = 500;

/** La relecture tant que le cœur attend la personne : celle de la Désinstallation, la même minuterie. */
export const STARTUP_POLL_DELAY = APPS_POLL_DELAY;

// ── L'en-tête ──────────────────────────────────────────────────────────────

export interface StartupCounts {
  /** Toutes les entrées listées, coupées comprises. */
  total: number;
  /** Celles que Shinku lit actives. */
  running: number;
  /** Celles que Shinku lit coupées. */
  cut: number;
}

export function startupCounts(view: StartupView): StartupCounts {
  return {
    total: view.entries.length,
    running: view.entries.filter((entry) => entry.state === 'enabled').length,
    cut: view.entries.filter((entry) => entry.state === 'disabled').length,
  };
}

/**
 * La phrase sous le titre. Rien quand il n'y a aucune entrée, ni quand aucune
 * n'est lue ni active ni coupée : « aucune ne se lance, aucune n'est coupée »
 * promettrait plus que ce que Shinku lit.
 */
export function startupSummary(counts: StartupCounts): string | null {
  return counts.running + counts.cut === 0 ? null : fr.startup.summary(counts.running, counts.cut);
}

// ── Les deux cartes ────────────────────────────────────────────────────────

/** Les entrées d'une portée, dans l'ordre que le cœur a donné. */
export function entriesOf(view: StartupView, scope: StartupScope): StartupEntry[] {
  return view.entries.filter((entry) => entry.scope === scope);
}

/** Une portée vide et lisible n'a pas de carte ; une portée illisible en a une, avec son avertissement. */
export function showsScope(view: StartupView, scope: StartupScope): boolean {
  return entriesOf(view, scope).length > 0 || view.unreadable.includes(scope);
}

// ── Une ligne ──────────────────────────────────────────────────────────────

/** La lettre de la pastille ronde : le premier caractère alphanumérique du nom (lettre ou chiffre), en majuscule ; « # » s'il n'y en a aucun. */
export function initialOf(name: string): string {
  const first = [...name].find((character) => /^[\p{L}\p{N}]$/u.test(character));
  if (first === undefined) {
    return fr.startup.initialFallback;
  }
  const upper = [...first.toLocaleUpperCase('fr')][0];
  return upper === undefined ? fr.startup.initialFallback : upper;
}

/**
 * Les morceaux de la seconde ligne, dans l'ordre : l'éditeur (ou « Éditeur
 * inconnu » si le fichier est là sans société), le programme porteur, le nom
 * sous lequel l'app est inscrite, le raccourci du dossier Démarrage.
 */
export function detailParts(entry: StartupEntry): string[] {
  const parts: string[] = [];
  if (entry.publisher !== null) {
    parts.push(entry.publisher);
  } else if (entry.target === 'found') {
    parts.push(fr.startup.unknownPublisher);
  }
  if (entry.target === 'hosted' && entry.host !== null) {
    parts.push(fr.startup.hostedBy(entry.host));
  }
  // Une ligne du PC dit son nom d'inscription sur sa propre ligne (`registered`) : pas deux fois.
  if (entry.alias !== null && entry.registered === null) {
    parts.push(fr.startup.alias(entry.alias));
  }
  if (entry.origin === 'folder') {
    parts.push(fr.startup.folder);
  }
  return parts;
}

/** Au plus deux pastilles par ligne, pastille de la fin de ligne comprise. */
export const MAX_TAGS = 2;

/** Combien de pastilles la fin de ligne porte déjà : l'état d'une ligne du PC, « Coupée dans Windows » pour Shinku. */
function endTags(entry: StartupEntry): number {
  if (rowEnd(entry) === 'machine') {
    return machineChip(entry, 0) === null ? 0 : 1;
  }
  return entry.own && entry.state === 'disabled' ? 1 : 0;
}

/**
 * Les pastilles de gauche d'une ligne, dans l'ordre de priorité : semble
 * désinstallée, remise toute seule, coupée par Shinku, coupée (avec sa date si
 * Windows l'a notée), puis « Synchronise tes fichiers » tant que l'app est
 * active. Une ligne du PC, l'entrée de Shinku et une entrée dont l'état n'est
 * pas lu n'ont pas de pastille d'état ici : la fin de ligne la porte, ou rien
 * ne peut la dire. Avec la pastille de fin de ligne, deux au plus.
 */
export function rowTags(entry: StartupEntry, now: number): string[] {
  const tags: string[] = [];
  if (entry.target === 'missing') {
    tags.push(fr.startup.missing);
  }
  // Une ligne à interrupteur (du compte, ou du PC quand Shinku peut la régler) dit sa coupure ici ; l'autre, en fin de ligne.
  if ((entry.scope === 'user' || rowEnd(entry) === 'switch') && !entry.own && entry.state !== 'unknown') {
    if (entry.mark?.kind === 'cameBack') {
      tags.push(fr.startup.cameBack(formatDay(entry.mark.cut, now)));
    } else if (entry.mark?.kind === 'cutByShinku') {
      tags.push(fr.startup.cutByShinku(formatDay(entry.mark.at, now)));
    } else if (entry.state === 'disabled') {
      tags.push(entry.disabledAt === null ? fr.startup.cut : fr.startup.cutOn(formatDay(entry.disabledAt, now)));
    }
  }
  if (entry.hint === 'sync' && entry.state === 'enabled') {
    tags.push(fr.startup.sync);
  }
  return tags.slice(0, MAX_TAGS - endTags(entry));
}

/**
 * Ce que la fin de la ligne montre. Dans la carte du compte : un interrupteur,
 * sauf pour l'entrée de Shinku lui-même (son mode jeu la règle), pour une
 * entrée dont Shinku ne sait pas lire l'état, et pour une entrée lue qu'il ne
 * peut pas régler (nom illisible, identifiant en double). Dans la carte du PC : jamais
 * d'interrupteur, seulement l'état.
 */
export type RowEnd = 'switch' | 'own' | 'unknown' | 'unadjustable' | 'machine';

export function rowEnd(entry: StartupEntry): RowEnd {
  if (entry.scope === 'machine') {
    // Une ligne du PC que Shinku peut régler a l'interrupteur du compte ; sinon, seulement son état.
    return entry.adjustable && entry.state !== 'unknown' ? 'switch' : 'machine';
  }
  if (entry.own) {
    return 'own';
  }
  if (entry.state === 'unknown') {
    return 'unknown';
  }
  return entry.adjustable ? 'switch' : 'unadjustable';
}

// ── Le PC et l'attente ─────────────────────────────────────────────────────

/** Shinku a-t-il un moyen de régler le PC ? */
export function hasPcAccess(view: StartupView): boolean {
  return view.pc !== 'unavailable';
}

/** Un changement du PC attend la personne (ou l'assistant) : tous les interrupteurs sont inertes. */
export function isWaiting(view: StartupView | null): boolean {
  return view !== null && view.pending !== null;
}

/** La ligne qui attend, et ce qu'elle dit ; `null` : rien n'attend, ou la ligne n'est plus dans la liste. */
export function waitingText(view: StartupView | null): { id: string; text: string } | null {
  if (view === null || view.pending === null) {
    return null;
  }
  const entry = entryById(view, view.pending.id);
  return entry === undefined ? null : { id: entry.id, text: fr.startup.waiting(entry.registered ?? entry.name) };
}

/** Faut-il relire la vue toutes les deux secondes ? Oui tant que le cœur attend, sauf si c'est ce geste-ci qui attend. */
export function shouldPoll(view: StartupView | null, ownCall: boolean): boolean {
  return isWaiting(view) && !ownCall;
}

/** Le texte de l'unique pastille d'état d'une ligne du PC ; `null` : Shinku ne sait pas lire l'état. */
export function machineChip(entry: StartupEntry, now: number): string | null {
  switch (entry.state) {
    case 'enabled':
      return fr.startup.active;
    case 'disabled':
      return entry.disabledAt === null ? fr.startup.cut : fr.startup.cutOn(formatDay(entry.disabledAt, now));
    case 'unknown':
      return null;
  }
}

// ── Les gestes en vol et le focus ──────────────────────────────────────────

/** Ce qui identifie un interrupteur où le focus peut être rendu (`data-fk` dans la page). */
export const startupFocus = {
  /** L'interrupteur d'une ligne. */
  entry: (id: string): string => `startup:${id}`,
  /** Le bouton de fin d'une ligne sans interrupteur (« Ouvrir dans Windows », « Voir le Mode jeu »). */
  end: (id: string): string => `startup-end:${id}`,
  /** « Réessayer », sous le message d'erreur de la première vue. */
  retry: 'startup-retry',
};

// ── Les gestes optimistes ──────────────────────────────────────────────────

export function entryById(view: StartupView, id: string): StartupEntry | undefined {
  return view.entries.find((entry) => entry.id === id);
}

/**
 * La vue telle qu'elle sera quand le cœur aura accepté le geste : coupée par
 * Shinku à l'instant, ou remise (le cœur retire alors sa marque).
 */
export function withEntryEnabled(view: StartupView, id: string, enabled: boolean, now: number): StartupView {
  return {
    ...view,
    entries: view.entries.map(
      (entry): StartupEntry =>
        entry.id !== id
          ? entry
          : enabled
            ? { ...entry, state: 'enabled', disabledAt: null, mark: null }
            : { ...entry, state: 'disabled', disabledAt: now, mark: { kind: 'cutByShinku', at: now } },
    ),
  };
}

/** Remet une entrée dans l'état où elle était avant le geste refusé. */
export function withEntry(view: StartupView, before: StartupEntry): StartupView {
  return { ...view, entries: view.entries.map((entry) => (entry.id === before.id ? before : entry)) };
}

// ── La zone de retour ──────────────────────────────────────────────────────

/** Ce que la zone de retour dit, et le geste inverse que son bouton « Annuler » refait (`null` : pas de bouton). */
export interface Say {
  text: string;
  undo: { id: string; enabled: boolean } | null;
}

/** Après une coupure ou une remise réussie : la phrase, et de quoi la défaire. */
export function sayDone(before: StartupEntry, enabled: boolean): Say {
  const pc = before.scope === 'machine';
  return {
    text: enabled
      ? pc
        ? fr.startup.say.restoredPc(before.name)
        : fr.startup.say.restored(before.name)
      : pc
        ? fr.startup.say.cutPc(before.name, before.hint === 'sync')
        : fr.startup.say.cut(before.name, before.hint === 'sync'),
    undo: { id: before.id, enabled: !enabled },
  };
}

/**
 * Ce que dit un refus. `null` : rien (le cœur traitait déjà un autre
 * changement, le geste est ignoré). Toute autre erreur est un changement qui
 * n'a pas pu être fait.
 */
export function sayRefused(error: unknown): Say | null {
  const refusal =
    typeof error === 'object' && error !== null && 'kind' in error ? (error as BackendError) : null;
  if (refusal?.kind === 'startupRefused') {
    switch (refusal.reason) {
      case 'busy':
        return null;
      case 'unknownEntry':
        return { text: fr.startup.say.unknownEntry, undo: null };
      case 'notAdjustable':
        return { text: fr.startup.say.notAdjustable, undo: null };
      case 'failed':
        return { text: fr.startup.say.failed, undo: null };
      case 'cancelled':
        return { text: fr.startup.say.cancelled, undo: null };
      case 'needsAdmin':
        return { text: fr.startup.say.needsAdmin, undo: null };
    }
  }
  return { text: fr.startup.say.failed, undo: null };
}

// ── Les noms accessibles ───────────────────────────────────────────────────

/**
 * Le rang de chaque ligne parmi celles qui ont le même nom et le même alias
 * dans la liste donnée : 1 pour la première, 2 pour la deuxième, dans l'ordre
 * de la liste. Le nom accessible d'un interrupteur le porte quand il dépasse 1.
 */
export function labelRanks(entries: readonly StartupEntry[]): Map<string, number> {
  const seen = new Map<string, number>();
  const ranks = new Map<string, number>();
  for (const entry of entries) {
    // Un rang ne distingue que des interrupteurs : une ligne sans interrupteur n'en prend pas.
    if (rowEnd(entry) !== 'switch') {
      continue;
    }
    const key = JSON.stringify([entry.name, entry.alias]);
    const rank = (seen.get(key) ?? 0) + 1;
    seen.set(key, rank);
    ranks.set(entry.id, rank);
  }
  return ranks;
}

/** L'identifiant de la seconde ligne d'une ligne, que son interrupteur cite en description. */
export const detailId = (id: string): string => `startup-${id}-d`;
/** L'identifiant de la ligne « Inscrite sous… » d'une ligne du PC. */
export const registeredId = (id: string): string => `startup-${id}-r`;

// ── Le focus après un changement de liste ──────────────────────────────────

/** Le titre de la page : là où le focus va quand la ligne qui l'avait n'a pas de voisine. */
export const TITLE_KEY = 'startup-title';

/**
 * Où va le focus quand la ligne `key` n'est plus dans la liste : la ligne qui la
 * suivait et qui existe encore, sinon celle qui la précédait, sinon le titre.
 * `before` : les clés de la liste d'avant, dans l'ordre ; `after` : celles de la liste d'après.
 * Si la ligne existe encore, c'est elle.
 */
export function focusAfter(before: readonly string[], after: readonly string[], key: string): string {
  if (after.includes(key)) {
    return key;
  }
  const index = before.indexOf(key);
  if (index >= 0) {
    for (const next of before.slice(index + 1)) {
      if (after.includes(next)) {
        return next;
      }
    }
    for (const previous of before.slice(0, index).reverse()) {
      if (after.includes(previous)) {
        return previous;
      }
    }
  }
  return TITLE_KEY;
}

/** La clé de focus du contrôle d'une ligne (interrupteur ou bouton de fin) ; `null` : la ligne n'a rien à focaliser (le PC). */
export function controlKey(entry: StartupEntry): string | null {
  switch (rowEnd(entry)) {
    case 'switch':
      return startupFocus.entry(entry.id);
    case 'machine':
      return null;
    default:
      return startupFocus.end(entry.id);
  }
}

/** L'identifiant de la ligne d'une clé de focus de ligne ; `null` si la clé n'est pas celle d'un contrôle de ligne. */
function rowOfKey(key: string): string | null {
  for (const prefix of ['startup:', 'startup-end:']) {
    if (key.startsWith(prefix)) {
      return key.slice(prefix.length);
    }
  }
  return null;
}

/**
 * Où va le focus quand la vue change sous le contrôle de ligne `key` : la même ligne si elle a encore
 * un contrôle (son contrôle d'aujourd'hui, quel qu'il soit), sinon la suivante qui en a un, sinon la
 * précédente, sinon le titre. `null` : `key` n'était pas un contrôle de ligne de `before`, rien à déplacer.
 */
export function focusTarget(before: StartupView | null, after: StartupView, key: string): string | null {
  const id = rowOfKey(key);
  if (id === null || before === null) {
    return null;
  }
  const withControl = (view: StartupView): string[] =>
    view.entries.filter((entry) => controlKey(entry) !== null).map((entry) => entry.id);
  const beforeIds = withControl(before);
  if (!beforeIds.includes(id)) {
    return null;
  }
  const target = focusAfter(beforeIds, withControl(after), id);
  const entry = target === TITLE_KEY ? undefined : entryById(after, target);
  return entry === undefined ? TITLE_KEY : (controlKey(entry) ?? TITLE_KEY);
}

/** Les clés de focus des lignes d'une vue, dans l'ordre affiché (le compte, puis le PC) : seules les lignes à interrupteur en ont. */
export function entryKeys(view: StartupView): string[] {
  return view.entries.filter((entry) => rowEnd(entry) === 'switch').map((entry) => startupFocus.entry(entry.id));
}

/**
 * La phrase de la zone de retour parle-t-elle d'un état que la vue démentirait ? Oui si
 * l'entrée dont « Annuler » défait le geste n'existe plus, ou n'est plus dans l'état que la phrase annonce.
 */
export function sayStale(say: Say, view: StartupView): boolean {
  if (say.undo === null) {
    return false;
  }
  const found = entryById(view, say.undo.id);
  // « Annuler » remet `undo.enabled` : la phrase annonçait donc l'état contraire.
  return found === undefined || (found.state === 'enabled') === say.undo.enabled;
}
