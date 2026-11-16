// Ce que la page Confidentialité tire d'une vue du cœur : les noms des blocs, ce qui
// se coche, la liste de la confirmation, l'issue de chaque trace.
//
// Ces fonctions sont pures : elles ne font que lire ce que rend le cœur. Elles sont
// testées sans navigateur (tests/privacy.test.ts). Elles ne produisent jamais autre
// chose qu'un identifiant que le cœur a fabriqué (`TraceItem.id`) pour ce qui part
// vers lui.

import type {
  EraseResult,
  PrivacyGroup,
  PrivacyView,
  TraceItem,
  TraceKind,
  TraceOutcome,
  WindowsPage,
} from '../backend/types.ts';
import { formatBytes } from '../format.ts';
import { fr } from '../i18n/fr.ts';

// ── Noms ───────────────────────────────────────────────────────────────────

/**
 * Le nom d'un profil : celui que le navigateur lui donne, inséré tel quel (jamais retouché par la typographie),
 * sinon « Profil principal » pour `Default` et « Profil N » pour `Profile N`.
 */
export function profileLabel(group: PrivacyGroup): string {
  if (group.profile !== null) {
    return group.profile;
  }
  return group.slot === 0 || group.slot === null ? fr.privacy.mainProfile : fr.privacy.numberedProfile(group.slot);
}

/** Le titre d'un bloc : « Chrome · Léa », ou « Windows ». */
export function groupTitle(group: PrivacyGroup): string {
  if (group.kind === 'windows' || group.app === null) {
    return fr.privacy.windows;
  }
  return fr.privacy.blockTitle(fr.privacy.browser[group.app], profileLabel(group));
}

/** Le nom court du navigateur d'un bloc (« Chrome »), pour la phrase « Ferme Chrome… ». */
export function browserName(group: PrivacyGroup): string {
  return group.app === null ? fr.privacy.windows : fr.privacy.browser[group.app];
}

export const traceName = (kind: TraceKind): string => fr.privacy.trace[kind];

/** Le nom accessible complet de la case : le bloc, puis la trace (« Chrome · Léa, Historique de navigation »). */
export function checkLabel(group: PrivacyGroup, item: TraceItem): string {
  if (group.kind === 'browser' && group.app !== null) {
    return fr.privacy.checkLabelProfile(
      fr.privacy.browser[group.app],
      profileLabel(group),
      traceName(item.kind).toLocaleLowerCase('fr'),
    );
  }
  return fr.privacy.checkLabel(groupTitle(group), traceName(item.kind));
}

// ── Les navigateurs : une carte par navigateur, une ligne par profil ───────

/** Un navigateur et ses profils, dans l'ordre reçu du cœur. */
export interface BrowserSection {
  app: NonNullable<PrivacyGroup['app']>;
  name: string;
  /** Au moins un de ses profils est ouvert : la phrase « Ferme… » s'écrit une fois, en tête de sa carte. */
  running: boolean;
  groups: PrivacyGroup[];
}

/** Les groupes de navigateurs réunis par navigateur ; l'ordre est celui de la première apparition. */
export function browserSections(view: PrivacyView): BrowserSection[] {
  const sections: BrowserSection[] = [];
  for (const group of view.groups) {
    if (group.kind !== 'browser' || group.app === null) {
      continue;
    }
    const found = sections.find((section) => section.app === group.app);
    if (found === undefined) {
      sections.push({ app: group.app, name: fr.privacy.browser[group.app], running: group.running, groups: [group] });
    } else {
      found.groups.push(group);
      found.running = found.running || group.running;
    }
  }
  return sections;
}

/** Les groupes qui ne sont pas des navigateurs (Windows), dans l'ordre reçu. */
export const otherGroups = (view: PrivacyView): PrivacyGroup[] => view.groups.filter((group) => group.kind !== 'browser');

// ── Les lignes ─────────────────────────────────────────────────────────────

/** Ce que la ligne est : cochable, vide, illisible, ou inerte parce que le navigateur est ouvert. */
export type RowKind = 'check' | 'empty' | 'unreadable' | 'running';

export function rowKind(group: PrivacyGroup, item: TraceItem): RowKind {
  if (item.state === 'empty') {
    return 'empty';
  }
  if (item.state === 'unreadable') {
    return 'unreadable';
  }
  return group.running ? 'running' : 'check';
}

/** Seule une trace présente d'un navigateur fermé se coche. */
export const isCheckable = (group: PrivacyGroup, item: TraceItem): boolean => rowKind(group, item) === 'check';

/** Le poids ou le nombre d'une trace présente ; `null` pour une trace vide ou illisible. */
export function traceSize(item: TraceItem): string | null {
  if (item.state !== 'present') {
    return null;
  }
  // Le presse-papiers n'a pas de mesure : le cœur ne sait pas ce que l'historique de Windows contient.
  if (item.kind === 'clipboard') {
    return null;
  }
  if (item.bytes !== null) {
    return formatBytes(item.bytes);
  }
  if (item.count !== null) {
    return fr.privacy.count[item.kind](item.count);
  }
  return null;
}

/** Les identifiants des traces qu'on peut cocher, dans l'ordre de la page. */
export function checkableIds(view: PrivacyView): string[] {
  return view.groups.flatMap((group) => group.items.filter((item) => isCheckable(group, item)).map((item) => item.id));
}

/** Rien n'est à effacer nulle part : la page le dit une fois en tête. */
export function nothingToErase(view: PrivacyView): boolean {
  return view.groups.every((group) => group.items.every((item) => item.state !== 'present'));
}

/** La trace et son bloc, par identifiant. */
export function findTrace(view: PrivacyView, id: string): { group: PrivacyGroup; item: TraceItem } | undefined {
  for (const group of view.groups) {
    const item = group.items.find((one) => one.id === id);
    if (item !== undefined) {
      return { group, item };
    }
  }
  return undefined;
}

// ── Les cases ──────────────────────────────────────────────────────────────

/** Ce qui reste coché quand la vue change : seulement ce qui se coche encore, dans l'ordre de la page. */
export function stillChecked(view: PrivacyView, checked: readonly string[]): string[] {
  return checkableIds(view).filter((id) => checked.includes(id));
}

/** Coche ou décoche une trace ; un identifiant que la vue ne propose pas, ou inerte, ne change rien. */
export function toggled(view: PrivacyView, checked: readonly string[], id: string): string[] {
  if (!checkableIds(view).includes(id)) {
    return [...checked];
  }
  return stillChecked(view, checked.includes(id) ? checked.filter((one) => one !== id) : [...checked, id]);
}

// ── La confirmation ────────────────────────────────────────────────────────

/** Une trace de la confirmation : ce que la puce dit, et de quoi nommer son issue plus tard. */
export interface AskTrace {
  id: string;
  /** La puce : « Léa : historique de navigation » pour un navigateur, « Presse-papiers » pour Windows. */
  name: string;
  /** Le nom de la trace seul. */
  trace: string;
  /** Le bloc de la trace : « Chrome · Léa », ou « Windows ». */
  label: string;
  /** Le nom court du navigateur, `null` pour Windows. */
  browser: string | null;
}

/** Un bloc de la confirmation : un navigateur (tous ses profils cochés), ou Windows. */
export interface AskBlock {
  key: string;
  title: string;
  traces: AskTrace[];
}

/** Ce que la confirmation annonce, figé à l'ouverture : c'est cette liste d'identifiants qui part, telle quelle. */
export interface PrivacyAsk {
  ids: string[];
  blocks: AskBlock[];
}

function askTrace(group: PrivacyGroup, item: TraceItem): AskTrace {
  const trace = traceName(item.kind);
  if (group.kind === 'browser' && group.app !== null) {
    return {
      id: item.id,
      name: fr.privacy.askLine(profileLabel(group), trace.toLocaleLowerCase('fr')),
      trace,
      label: groupTitle(group),
      browser: fr.privacy.browser[group.app],
    };
  }
  return { id: item.id, name: trace, trace, label: groupTitle(group), browser: null };
}

/** La confirmation de ce qui est coché : un bloc par navigateur, puis Windows ; `null` si rien de cochable n'est coché. */
export function askFor(view: PrivacyView, checked: readonly string[]): PrivacyAsk | null {
  const wanted = stillChecked(view, checked);
  if (wanted.length === 0) {
    return null;
  }
  const picked = (group: PrivacyGroup): AskTrace[] =>
    group.items.filter((item) => isCheckable(group, item) && wanted.includes(item.id)).map((item) => askTrace(group, item));
  const blocks: AskBlock[] = [];
  for (const section of browserSections(view)) {
    const traces = section.groups.flatMap(picked);
    if (traces.length > 0) {
      blocks.push({ key: section.app, title: section.name, traces });
    }
  }
  for (const group of otherGroups(view)) {
    const traces = picked(group);
    if (traces.length > 0) {
      blocks.push({ key: group.key, title: groupTitle(group), traces });
    }
  }
  return { ids: blocks.flatMap((block) => block.traces.map((trace) => trace.id)), blocks };
}

// ── Après l'effacement ─────────────────────────────────────────────────────

/** L'issue d'une trace, avec de quoi la nommer même si la vue relue n'a plus la même forme. */
export interface LastLine {
  id: string;
  /** « Chrome · Léa », ou « Windows » : en premier dans la ligne. */
  block: string;
  trace: string;
  /** Le nom court du navigateur pour une trace de navigateur, sinon `null`. */
  browser: string | null;
  outcome: TraceOutcome;
}

/** Une issue par trace demandée, dans l'ordre de la confirmation ; une issue pour un identifiant qu'on n'a pas demandé est ignorée. */
export function lastLines(ask: PrivacyAsk, results: readonly EraseResult[]): LastLine[] {
  const lines: LastLine[] = [];
  for (const block of ask.blocks) {
    for (const trace of block.traces) {
      const found = results.find((result) => result.id === trace.id);
      if (found !== undefined) {
        lines.push({ id: trace.id, block: trace.label, trace: trace.trace, browser: trace.browser, outcome: found.outcome });
      }
    }
  }
  return lines;
}

/** L'issue en phrase entière : pour les lecteurs d'écran. */
export const outcomeText = (outcome: TraceOutcome): string => fr.privacy.outcome[outcome];

/** L'issue en deux mots : « En cours d'utilisation », « Profil changé »… */
export const outcomeShort = (outcome: TraceOutcome): string => fr.privacy.outcomeShort[outcome];

/** L'issue sous une trace : « En cours d'utilisation. Ferme Chrome et recommence. » */
export function outcomeLine(line: Pick<LastLine, 'browser' | 'outcome'>): string {
  const help = outcomeHelp(line);
  return help === null ? outcomeShort(line.outcome) : `${outcomeShort(line.outcome)}. ${help}`;
}

/** La phrase d'aide sous une issue qui demande quelque chose ; `null` quand il n'y a rien à ajouter. */
export function outcomeHelp(line: Pick<LastLine, 'browser' | 'outcome'>): string | null {
  switch (line.outcome) {
    case 'inUse':
      return line.browser === null ? fr.privacy.help.closeApp : fr.privacy.help.closeBrowser(line.browser);
    case 'changed':
      return fr.privacy.help.lookAgain;
    case 'partial':
      return fr.privacy.help.someLeft;
    default:
      return null;
  }
}

/** La phrase de la région vivante : toutes les issues, une par trace. */
export function announceLines(lines: readonly LastLine[]): string {
  return fr.privacy.announce(
    lines.map((line) => fr.privacy.lastLine(line.block, line.trace, outcomeText(line.outcome))).join('. '),
  );
}

// ── Les phrases « Ce qui reste » ───────────────────────────────────────────

/** Ce qui reste après l'effacement de l'historique d'un navigateur : une fois pour tous, sous les cartes. */
export function remainsOfBrowsers(view: PrivacyView): string[] {
  return [fr.privacy.remainsBrowser, ...(view.edgeImportsChrome ? [fr.privacy.remainsEdge] : [])];
}

/** Ce qui reste après les traces de Windows. */
export const remainsOfWindows = (): string[] => [fr.privacy.remainsWindows];

/** Au moins un navigateur est là : la phrase sur le cache s'écrit une fois, sous les blocs. */
export const hasBrowser = (view: PrivacyView): boolean => view.groups.some((group) => group.kind === 'browser');

// ── Dans Windows ───────────────────────────────────────────────────────────

/** Les pages de Windows que la page propose, dans l'ordre. */
export const WINDOWS_PAGES: readonly Extract<WindowsPage, 'activityHistory' | 'searchPermissions' | 'diagnostics'>[] = [
  'activityHistory',
  'searchPermissions',
  'diagnostics',
];

// ── Le focus ───────────────────────────────────────────────────────────────

export const TITLE_KEY = 'privacy-title';

export const privacyFocus = {
  title: TITLE_KEY,
  /** « Réessayer », sous le message d'erreur de la première vue. */
  retry: 'privacy-retry',
  erase: 'privacy-erase',
  check: (id: string): string => `privacy-check:${id}`,
};

/** Retour de la fenêtre : une vue finie depuis moins de ce délai (ms) n'est pas redemandée. */
export const PRIVACY_RELOAD_GAP = 500;
