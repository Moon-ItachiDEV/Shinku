// Ce que la page Désinstallation tire d'une vue du cœur : la liste filtrée et
// triée, les textes d'une ligne, les phases, les restes cochés, et où va le
// focus quand la vue change.
//
// Ces fonctions sont pures : elles ne font que lire ce que rend le cœur. Elles
// sont testées sans navigateur (tests/uninstall.test.ts).

import type {
  AppRow,
  AppsView,
  FollowView,
  LeftoverRow,
  RemoveLeftovers,
  RemoveReason,
  UninstallRefusal,
} from '../backend/types.ts';
import { formatBytes, formatDay } from '../format.ts';
import { fr } from '../i18n/fr.ts';

/** Délai entre deux demandes de la vue tant qu'une désinstallation est en cours, en millisecondes. */
export const APPS_POLL_DELAY = 2000;
/** Retour de la fenêtre : une vue finie depuis moins de ce délai (ms) n'est pas redemandée. */
export const APPS_RELOAD_GAP = 500;
/** Après combien de secondes d'attente la page ajoute sa seconde phrase. */
export const LATE_AFTER = 20;
/** Au-delà de cette taille cochée, la légende prévient que la corbeille peut faire de la place. */
export const BIN_NOTE_FROM = 1024 ** 3;

// ── La liste ───────────────────────────────────────────────────────────────

export type AppsSort = 'name' | 'size' | 'date';
export const APPS_SORTS: readonly AppsSort[] = ['name', 'size', 'date'];

/** Minuscules, sans accents : la comparaison que fait le cœur pour trier, et celle du champ de recherche. */
export function fold(text: string): string {
  return text.normalize('NFD').replace(/\p{M}+/gu, '').toLocaleLowerCase('fr');
}

/** Les apps dont le nom ou l'éditeur contient le texte cherché (plié). Un texte vide les garde toutes. */
export function filterApps(apps: readonly AppRow[], query: string): AppRow[] {
  const wanted = fold(query.trim());
  if (wanted === '') {
    return [...apps];
  }
  return apps.filter((app) => fold(app.name).includes(wanted) || fold(app.publisher ?? '').includes(wanted));
}

/**
 * Le tri choisi. Le cœur rend les apps par nom : « Nom » ne change rien. « Taille » : les plus grosses
 * d'abord, les tailles inconnues à la fin. « Date » : les plus récentes d'abord, les dates inconnues à
 * la fin. À égalité, l'ordre du cœur (le tri est stable).
 */
export function sortApps(apps: readonly AppRow[], sort: AppsSort): AppRow[] {
  const list = [...apps];
  switch (sort) {
    case 'name':
      return list;
    case 'size':
      return list.sort((left, right) => (right.bytes ?? -1) - (left.bytes ?? -1));
    case 'date':
      return list.sort((left, right) => {
        const [a, b] = [left.installed ?? '', right.installed ?? ''];
        return a === b ? 0 : a === '' ? 1 : b === '' ? -1 : a < b ? 1 : -1;
      });
  }
}

/** Ce que la liste montre : filtrée par la recherche, puis triée. */
export function listedApps(view: AppsView, query: string, sort: AppsSort): AppRow[] {
  return sortApps(filterApps(view.apps, query), sort);
}

/** La lettre de la pastille ronde : le premier caractère alphanumérique du nom (lettre ou chiffre), en majuscule ; « # » s'il n'y en a aucun. */
export function initialOf(name: string): string {
  const first = [...name].find((character) => /^[\p{L}\p{N}]$/u.test(character));
  if (first === undefined) {
    return fr.uninstall.initialFallback;
  }
  const upper = [...first.toLocaleUpperCase('fr')][0];
  return upper === undefined ? fr.uninstall.initialFallback : upper;
}

/** « AAAA-MM-JJ » en date locale ; `null` si ce n'est pas une date. */
export function installedTime(installed: string | undefined): number | null {
  const match = installed === undefined ? null : /^(\d{4})-(\d{2})-(\d{2})$/.exec(installed);
  if (match === null) {
    return null;
  }
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const date = new Date(year, month - 1, day);
  return date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day ? date.getTime() : null;
}

/**
 * La seconde ligne : l'éditeur (« Éditeur inconnu » s'il manque), puis « version 1.2.3 » si elle existe.
 * Triée par date, la ligne dit aussi quand l'app a été installée : sans cela, le tri ne se voit pas.
 */
export function detailParts(app: AppRow, sort: AppsSort, now: number): string[] {
  const parts = [app.publisher ?? fr.uninstall.unknownPublisher];
  if (app.version !== undefined) {
    parts.push(fr.uninstall.version(app.version));
  }
  const installed = sort === 'date' ? installedTime(app.installed) : null;
  if (installed !== null) {
    parts.push(fr.uninstall.installedOn(formatDay(installed, now)));
  }
  return parts;
}

/** La pastille d'une ligne : un indice par le nom, dit comme une ressemblance. `null` : aucune. */
export function chipOf(app: AppRow): string | null {
  if (app.removal !== 'offered') {
    return null;
  }
  switch (app.kind) {
    case 'runtime':
      return fr.uninstall.runtimeChip;
    case 'driver':
      return fr.uninstall.driverChip;
    case 'plain':
      return null;
  }
}

/** La taille déclarée ; rien si elle est inconnue. */
export function sizeOf(app: AppRow): string | null {
  return app.bytes === undefined || app.bytes <= 0 ? null : formatBytes(app.bytes);
}

/** Ce que la fin d'une ligne montre : le bouton, ou la raison de son absence. */
/** La phrase longue d'une fin de ligne sans bouton, pour `title` ; `null` quand le texte visible se suffit. */
export function endTitle(end: RowEnd): string | null {
  switch (end) {
    case 'refused':
      return fr.uninstall.refusedTitle;
    case 'needed':
      return fr.uninstall.neededTitle;
    default:
      return null;
  }
}

export type RowEnd = 'button' | 'refused' | 'own' | 'needed';

export function rowEnd(app: AppRow): RowEnd {
  switch (app.removal) {
    case 'offered':
      return 'button';
    case 'refused':
      return 'refused';
    case 'own':
      return 'own';
    case 'needed':
      return 'needed';
  }
}

export function appById(view: AppsView | null, id: string): AppRow | undefined {
  return view?.apps.find((app) => app.id === id);
}

// ── La confirmation ────────────────────────────────────────────────────────

/** Ce que la boîte de confirmation sait de l'app : de quoi écrire ses phrases, rien de plus. */
export interface UninstallAsk {
  id: string;
  name: string;
  kind: AppRow['kind'];
  scope: AppRow['scope'];
}

export function askFor(app: AppRow): UninstallAsk {
  return { id: app.id, name: app.name, kind: app.kind, scope: app.scope };
}

/** Les phrases en plus de l'explication : ce que la ressemblance du nom ou la portée de l'app doit faire savoir. */
export function confirmExtras(ask: UninstallAsk): string[] {
  return [
    ...(ask.kind === 'runtime' ? [fr.uninstall.confirm.runtime] : []),
    ...(ask.kind === 'driver' ? [fr.uninstall.confirm.driver] : []),
    ...(ask.scope === 'machine' ? [fr.uninstall.confirm.machine] : []),
  ];
}

// ── Les phases ─────────────────────────────────────────────────────────────

/** Les trois écrans de la page : la liste, l'attente, les restes. */
export type Screen = 'list' | 'scene' | 'leftovers';

export function screenOf(follow: FollowView | null | undefined): Screen {
  switch (follow?.phase) {
    case 'launching':
    case 'waiting':
    case 'settling':
      return 'scene';
    case 'removed':
      return 'leftovers';
    default:
      return 'list';
  }
}

/** Vrai tant que la page doit interroger le cœur : `launching`, `waiting` et `settling`. */
export function isPolling(follow: FollowView | null | undefined): boolean {
  return screenOf(follow) === 'scene';
}

/** La phrase vivante d'un changement de phase. */
export function announcement(follow: FollowView): string {
  switch (follow.phase) {
    case 'idle':
      return fr.uninstall.announce.list;
    case 'launching':
      return fr.uninstall.announce.launching(follow.name);
    case 'waiting':
      return fr.uninstall.announce.waiting(follow.name);
    case 'settling':
      return fr.uninstall.announce.settling(follow.name);
    case 'removed':
      return fr.uninstall.announce.removed(follow.name, proposed(follow).length);
  }
}

/** La phrase de l'attente sous le titre : celle de la phase, puis la seconde après 20 s. */
export function sceneTexts(follow: FollowView): string[] {
  switch (follow.phase) {
    case 'launching':
      return [fr.uninstall.scene.launching];
    case 'waiting':
      return follow.seconds >= LATE_AFTER ? [fr.uninstall.scene.waiting, fr.uninstall.scene.waitingLate] : [fr.uninstall.scene.waiting];
    case 'settling':
      return [fr.uninstall.scene.settling];
    default:
      return [];
  }
}

/** Le nom de l'app que la vue suit, ou `null`. */
export function followedName(follow: FollowView): string | null {
  return follow.phase === 'idle' ? null : follow.name;
}

// ── Les restes ─────────────────────────────────────────────────────────────

/** Ce que la phase `removed` propose vraiment : rien si une app de la même famille reste, ou si la vérification est incomplète. */
export function proposed(follow: FollowView): LeftoverRow[] {
  return follow.phase === 'removed' && !follow.family && follow.complete ? follow.leftovers : [];
}

/** Pourquoi la phase `removed` ne propose rien, ou `null` si elle propose quelque chose. */
export type NoneBecause = 'family' | 'unsure' | 'none';

export function noneBecause(follow: FollowView): NoneBecause | null {
  if (follow.phase !== 'removed') {
    return null;
  }
  if (follow.family) {
    return 'family';
  }
  if (!follow.complete) {
    return 'unsure';
  }
  return follow.leftovers.length === 0 ? 'none' : null;
}

/** Le texte de la seconde ligne d'un reste. */
export function leftoverText(row: LeftoverRow): string {
  switch (row.kind) {
    case 'folder':
      return fr.uninstall.leftovers.folder;
    case 'data':
      return fr.uninstall.leftovers.data;
    case 'shortcut':
      return fr.uninstall.leftovers.shortcut;
  }
}

/** « 240 Mo · 1 204 fichiers » ; ce qui manque ne s'écrit pas, et rien ne s'écrit si rien n'est connu. */
export function leftoverSize(row: LeftoverRow): string | null {
  const bytes = row.bytes;
  const files = row.files;
  if (bytes !== undefined && files !== undefined) {
    return fr.uninstall.leftovers.size(bytes, files);
  }
  if (bytes !== undefined) {
    return formatBytes(bytes);
  }
  return files === undefined ? null : fr.uninstall.leftovers.files(files);
}

/** Les restes qu'on peut encore cocher : ceux que Windows a supprimés pour de bon n'ont plus rien à envoyer. */
export function checkable(rows: readonly LeftoverRow[], failures: Readonly<Record<string, Failure>>): LeftoverRow[] {
  return rows.filter((row) => failures[row.path] !== 'destroyed');
}

/** Les restes cochés, dans l'ordre de la liste : `checked` porte des chemins. */
export function checkedRows(rows: readonly LeftoverRow[], checked: readonly string[]): LeftoverRow[] {
  return rows.filter((row) => checked.includes(row.path));
}

/** Ce que l'orbe dira : le nombre d'éléments cochés et leur taille, dont on sait si elle est complète. */
export interface Tally {
  count: number;
  bytes: number;
  /** La taille d'un élément coché est inconnue : le total est un minimum. */
  atLeast: boolean;
}

export function tallyOf(rows: readonly LeftoverRow[]): Tally {
  return {
    count: rows.length,
    bytes: rows.reduce((total, row) => total + (row.bytes ?? 0), 0),
    atLeast: rows.some((row) => row.bytes === undefined),
  };
}

/** La légende sous l'orbe, et sa phrase de plus quand beaucoup est coché. */
export function dockTexts(tally: Tally): string[] {
  if (tally.count === 0) {
    return [fr.uninstall.leftovers.captionNothing];
  }
  const texts = [fr.uninstall.leftovers.caption(tally.count, tally.bytes, tally.atLeast)];
  if (tally.bytes > BIN_NOTE_FROM) {
    texts.push(fr.uninstall.leftovers.binNote);
  }
  return texts;
}

/** Ce qu'un échec de la corbeille dit : une des trois raisons du cœur, ou « autre ». */
export type Failure = RemoveReason | 'other';

export function failureText(failure: Failure): string {
  switch (failure) {
    case 'inUse':
      return fr.uninstall.leftovers.inUse;
    case 'refused':
      return fr.uninstall.leftovers.refused;
    case 'partial':
      return fr.uninstall.leftovers.partial;
    case 'destroyed':
      return fr.uninstall.leftovers.destroyed;
    case 'other':
      return fr.uninstall.leftovers.other;
  }
}

/** Ce que la page retient d'une réponse à « Mettre à la corbeille ». */
export interface Removal {
  /** Combien sont partis à la corbeille, et leur taille. */
  moved: number;
  bytes: number;
  /** Les échecs, par chemin : ils restent dans la liste avec leur raison. */
  failures: Record<string, Failure>;
  /** Supprimés pour de bon par Windows : ils ne sont plus là, la page les garde à l'écran, sans case. */
  destroyed: LeftoverRow[];
  /** Les chemins qui ont eu un résultat (partis, déjà partis, échoués). */
  settled: string[];
}

/** `before` : les restes tels que la page les montrait quand elle a envoyé ses clés. */
export function removalOf(before: readonly LeftoverRow[], answer: RemoveLeftovers): Removal {
  const removal: Removal = { moved: 0, bytes: answer.bytes, failures: {}, destroyed: [], settled: [] };
  for (const result of answer.results) {
    const row = before.find((one) => one.key === result.key);
    if (row === undefined) {
      continue;
    }
    removal.settled.push(row.path);
    if (result.outcome === 'removed') {
      removal.moved += 1;
    } else if (result.outcome === 'failed' || result.outcome === 'refused') {
      removal.failures[row.path] = result.outcome === 'failed' ? (result.reason ?? 'other') : 'other';
      if (result.outcome === 'failed' && result.reason === 'destroyed') {
        removal.destroyed.push(row);
      }
    }
  }
  return removal;
}

/** La phrase du toast qui suit une demande : ce qui est parti, ou ce qui n'a rien donné. */
export function removalToast(removal: Removal): string | null {
  if (removal.moved > 0) {
    return fr.uninstall.toast.removed(removal.moved, removal.bytes);
  }
  return Object.keys(removal.failures).length > 0 ? fr.uninstall.toast.nothingMoved : null;
}

/**
 * Les échecs qui restent à montrer : ceux d'avant, sauf pour les restes qui ont eu un résultat
 * depuis, plus les nouveaux ; seuls comptent les restes encore dans la liste.
 */
export function mergedFailures(
  known: Readonly<Record<string, Failure>>,
  removal: Removal,
  rows: readonly LeftoverRow[],
): Record<string, Failure> {
  const merged: Record<string, Failure> = {};
  for (const row of rows) {
    const now = removal.failures[row.path];
    const before = removal.settled.includes(row.path) ? undefined : known[row.path];
    const failure = now ?? before;
    if (failure !== undefined) {
      merged[row.path] = failure;
    }
  }
  return merged;
}

// ── Les refus de « Désinstaller » ──────────────────────────────────────────

/** Ce que la page fait d'un refus à l'ouverture. `null` : rien (la personne a dit non à Windows). */
export interface Refusal {
  toast: string | null;
  /** La liste est relue : l'app n'est plus là, ou la page ne sait plus où elle en est. */
  reload: boolean;
}

export function refusalOf(error: unknown, name: string): Refusal {
  const found = typeof error === 'object' && error !== null && 'kind' in error ? (error as { kind: unknown; reason?: unknown }) : null;
  const reason = found?.kind === 'uninstallRefused' ? (found.reason as UninstallRefusal) : undefined;
  switch (reason) {
    case 'cancelled':
      return { toast: null, reload: true };
    case 'broken':
      return { toast: fr.uninstall.toast.broken(name), reload: true };
    case 'vanished':
      return { toast: fr.uninstall.toast.vanished(name), reload: true };
    default:
      return { toast: fr.uninstall.toast.failed, reload: true };
  }
}

// ── Le focus ───────────────────────────────────────────────────────────────

/** Ce qui identifie un contrôle où le focus peut être rendu (`data-fk` dans la page). */
export const uninstallFocus = {
  title: 'uninstall-title',
  search: 'uninstall-search',
  retry: 'uninstall-retry',
  sort: (sort: AppsSort): string => `uninstall-sort:${sort}`,
  row: (id: string): string => `uninstall:${id}`,
  back: 'uninstall-back',
  leftover: (key: number): string => `uninstall-left:${key}`,
  orb: 'uninstall-orb',
  rescan: 'uninstall-rescan',
  done: 'uninstall-done',
};

/** La clé de focus du bouton « Désinstaller » d'une ligne, ou `null` si la ligne n'en a pas. */
function rowKey(app: AppRow): string | null {
  return rowEnd(app) === 'button' ? uninstallFocus.row(app.id) : null;
}

/** L'identifiant de la ligne d'une clé de focus de ligne ; `null` si la clé n'est pas celle d'un bouton de ligne. */
function rowOfKey(key: string): string | null {
  const prefix = 'uninstall:';
  return key.startsWith(prefix) ? key.slice(prefix.length) : null;
}

/**
 * Où va le focus quand la liste change sous le bouton `key` : le même bouton s'il existe encore, sinon
 * celui de la ligne suivante, sinon de la précédente, sinon le titre. `null` : `key` n'était pas un
 * bouton de ligne de `before`, rien à déplacer. `before` et `after` sont les lignes affichées (filtrées, triées).
 */
export function focusTarget(before: readonly AppRow[], after: readonly AppRow[], key: string): string | null {
  const id = rowOfKey(key);
  if (id === null) {
    return null;
  }
  const withButton = (rows: readonly AppRow[]): string[] => rows.flatMap((app) => rowKey(app) ?? []);
  const was = withButton(before);
  const is = withButton(after);
  if (!was.includes(key)) {
    return null;
  }
  if (is.includes(key)) {
    return key;
  }
  const index = was.indexOf(key);
  for (const next of was.slice(index + 1)) {
    if (is.includes(next)) {
      return next;
    }
  }
  for (const previous of was.slice(0, index).reverse()) {
    if (is.includes(previous)) {
      return previous;
    }
  }
  return uninstallFocus.title;
}

/** Où va le focus après « Mettre à la corbeille » : le premier reste encore là, sinon « Terminé ». */
export function focusAfterRemoval(rows: readonly LeftoverRow[]): string {
  const first = rows[0];
  return first === undefined ? uninstallFocus.done : uninstallFocus.leftover(first.key);
}
