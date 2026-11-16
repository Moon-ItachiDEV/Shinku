// L'état de l'interface, et les gestes que les écrans peuvent déclencher.
//
// Les écrans (src/views) sont des fonctions de cet état : ils ne gardent rien
// pour eux. Tout changement passe par l'application (src/app.ts).

import type {
  AppsView,
  CleanPreview,
  CleanRequest,
  DuplicatesPage,
  GameOption,
  GameStatus,
  GameView,
  KeepChoice,
  JunkReport,
  LargeFilesPage,
  LeftoverRow,
  OpenApp,
  Overview,
  PrivacyRefusal,
  PrivacyView,
  SessionView,
  StartupView,
  Theme,
  WindowsPage,
} from './backend/types.ts';
import type { Leaving } from './model/game.ts';
import type { LastLine, PrivacyAsk } from './model/privacy.ts';
import { type CleanRowId, type Selection, UNTOUCHED } from './model/scan.ts';
import type { Say } from './model/startup.ts';
import type { AppsSort, Failure, UninstallAsk } from './model/uninstall.ts';

/** Les entrées du menu latéral. */
export type Route =
  | 'home'
  | 'junk'
  | 'duplicates'
  | 'large'
  | 'bin'
  | 'startup'
  | 'gameMode'
  | 'uninstall'
  | 'privacy'
  | 'settings';

/**
 * Ce que la personne a choisi de voir dans l'entrée « Grand vide » : l'accueil,
 * l'analyse ou ses résultats. Un grand vide en cours ou terminé passe devant
 * ce choix (voir `shownPane`, src/views/home.ts).
 */
export type HomePane = 'home' | 'scan' | 'results';

/** La confirmation du grand vide : ce qu'elle annonce, et la demande qui partira telle quelle. */
export interface CleanConfirmation {
  preview: CleanPreview;
  request: CleanRequest;
}

/** Le sélecteur du mode jeu : un jeu ou une app à choisir parmi celles qui sont ouvertes. */
export interface Picker {
  kind: 'game' | 'app';
  /** `null` tant que le cœur n'a pas rendu la liste. */
  apps: OpenApp[] | null;
}

export interface AppState {
  /** `loading` tant que le cœur n'a pas répondu ; `failed` s'il ne répond pas. */
  boot: 'loading' | 'ready' | 'failed' | 'outside';
  route: Route;
  pane: HomePane;
  overview: Overview | null;
  session: SessionView;
  /** Les cases que la personne a changées sur l'écran Résultats. */
  selection: Selection;
  /** La confirmation du grand vide, tant qu'elle est ouverte. */
  confirm: CleanConfirmation | null;
  /** Une demande de grand vide est partie : ses boutons attendent la réponse. */
  cleanBusy: boolean;
  /** La personne a demandé l'arrêt du grand vide en cours. */
  stopping: boolean;
  /** La jauge du disque a été relue depuis la fin du grand vide affiché. */
  diskAfterClean: boolean;
  junk: JunkReport | null;
  large: LargeFilesPage | null;
  duplicates: DuplicatesPage | null;
  /** Une page supplémentaire est en cours de chargement. */
  loadingMore: boolean;
  /** Le choix d'un exemplaire à garder attend la réponse du cœur. */
  keeping: boolean;
  /** Le sélecteur de dossier de Windows est ouvert. */
  pickingFolder: boolean;
  /** Message bref affiché en bas de l'écran ; son texte reste le temps qu'il s'efface. */
  toast: string;
  toastShown: boolean;
  /** La dernière vue du mode jeu ; `null` tant que le cœur n'a pas répondu. */
  game: GameView | null;
  /** Depuis quand l'agent du mode jeu est vu arrêté, le mode automatique allumé, ou `null`. */
  gameStoppedSince: number | null;
  /** Le chemin de la partie dont « Ce n'est pas un jeu » est parti : l'en-tête attend que l'agent la termine. */
  gameLeaving: Leaving | null;
  /** Plusieurs demandes de suite de la vue du mode jeu ont échoué : la page le dit au lieu de « Chargement… ». */
  gameFailed: boolean;
  /** Les gestes du mode jeu en vol (voir `busyKey`) : leurs boutons attendent la réponse. */
  gameBusy: readonly string[];
  /** Le sélecteur de jeu ou d'app, tant qu'il est ouvert. */
  picker: Picker | null;
  /** La dernière vue de la page Démarrage ; `null` tant que le cœur n'a pas répondu. */
  startup: StartupView | null;
  /** La vue du démarrage n'a pas pu être lue : la page le dit au lieu de « Chargement… ». */
  startupFailed: boolean;
  /** L'entrée dont le changement est en vol : tous les interrupteurs attendent sa réponse, `null` si aucun. */
  startupBusy: string | null;
  /** La zone de retour : ce que le dernier geste a donné, ou `null`. */
  startupSay: Say | null;
  /** La dernière vue de la page Désinstallation ; `null` tant que le cœur n'a pas répondu. */
  apps: AppsView | null;
  /** La vue des apps n'a pas pu être lue : la page le dit au lieu de « Chargement… ». */
  appsFailed: boolean;
  /** Le texte cherché et le tri choisi : ils vivent ici, pas dans les réglages. */
  appsQuery: string;
  appsSort: AppsSort;
  /** Le geste en vol : tous les boutons de la page attendent sa réponse, `null` si aucun. */
  appsBusy: 'start' | 'stop' | 'rescan' | 'remove' | null;
  /** La confirmation de « Désinstaller », tant qu'elle est ouverte. */
  appsConfirm: UninstallAsk | null;
  /** Les chemins des restes cochés : jamais cochés d'avance. */
  appsChecked: readonly string[];
  /** Les restes que la corbeille n'a pas pris, par chemin, avec leur raison. */
  appsFailures: Readonly<Record<string, Failure>>;
  /** Supprimés pour de bon par Windows : plus là, mais gardés à l'écran avec leur raison, sans case. */
  appsDestroyed: readonly LeftoverRow[];
  /** Tout ce qui était proposé est parti : la page ne dit plus « rien trouvé ». */
  appsEmptied: boolean;
  /** La phrase de la région vivante : un changement de phase, annoncé une fois. */
  appsAnnounce: string;
  /** La dernière vue de la page Confidentialité ; `null` tant que le cœur n'a pas répondu. */
  privacy: PrivacyView | null;
  /** La vue des traces n'a pas pu être lue : la page le dit au lieu de « Chargement… ». */
  privacyFailed: boolean;
  /** Les identifiants des traces cochées : jamais cochées d'avance, vidées après un effacement et à chaque retour sur la page. */
  privacyChecked: readonly string[];
  /** La confirmation de « Effacer ce qui est coché », tant qu'elle est ouverte : la liste qui partira telle quelle. */
  privacyAsk: PrivacyAsk | null;
  /** Un effacement est parti : la page est inerte jusqu'à sa réponse. */
  privacyBusy: boolean;
  /** Le dernier refus du cœur (`busy`, `tooMany`), dit sur la page ; rien n'a été effacé. */
  privacyRefused: PrivacyRefusal | null;
  /** L'issue de chaque trace du dernier effacement. */
  privacyLast: readonly LastLine[];
  /** La phrase de la région vivante : les issues du dernier effacement, annoncées une fois. */
  privacyAnnounce: string;
  /** L'heure qu'il est, rafraîchie de temps en temps : pour « il y a 8 mois ». */
  now: number;
}

export const IDLE_SESSION: SessionView = {
  state: 'idle',
  progress: { phase: 'preparing', folders: 0, files: 0, foundBytes: 0, workDone: 0, workTotal: 0 },
  summary: null,
  skipped: { folders: 0, refusedRoots: 0 },
  stale: false,
  clean: null,
};

export function initialState(now: number): AppState {
  return {
    boot: 'loading',
    route: 'home',
    pane: 'home',
    overview: null,
    session: IDLE_SESSION,
    selection: UNTOUCHED,
    confirm: null,
    cleanBusy: false,
    stopping: false,
    diskAfterClean: false,
    junk: null,
    large: null,
    duplicates: null,
    loadingMore: false,
    keeping: false,
    pickingFolder: false,
    toast: '',
    toastShown: false,
    game: null,
    gameStoppedSince: null,
    gameLeaving: null,
    gameFailed: false,
    gameBusy: [],
    picker: null,
    startup: null,
    startupFailed: false,
    startupBusy: null,
    startupSay: null,
    apps: null,
    appsFailed: false,
    appsQuery: '',
    appsSort: 'name',
    appsBusy: null,
    appsConfirm: null,
    appsChecked: [],
    appsFailures: {},
    appsDestroyed: [],
    appsEmptied: false,
    appsAnnounce: '',
    privacy: null,
    privacyFailed: false,
    privacyChecked: [],
    privacyAsk: null,
    privacyBusy: false,
    privacyRefused: null,
    privacyLast: [],
    privacyAnnounce: '',
    now,
  };
}

export interface Actions {
  navigate(route: Route): void;
  startScan(): void;
  cancelScan(): void;
  showScan(): void;
  showResults(): void;
  showHome(): void;
  toggle(row: CleanRowId): void;
  /** « Faire le vide » : prépare la confirmation. Rien ne part encore. */
  askClean(): void;
  /** La personne confirme : le grand vide démarre. */
  confirmClean(): void;
  /** La personne referme la confirmation : rien ne part. */
  dismissClean(): void;
  stopClean(): void;
  /** Referme le bilan d'un grand vide terminé. */
  closeReport(): void;
  loadMore(list: 'large' | 'duplicates'): void;
  /** Choisit l'exemplaire gardé d'un doublon. Rien n'est supprimé. */
  keepDuplicate(choice: KeepChoice): void;
  setTheme(theme: Theme): void;
  addFolder(): void;
  removeFolder(path: string): void;
  acknowledgeIssue(): void;
  setGameOption(option: GameOption, on: boolean): void;
  setQuietApp(exe: string, on: boolean): void;
  /** « Tout remettre maintenant ». */
  releaseGame(): void;
  setGameStatus(path: string, status: GameStatus): void;
  openPicker(kind: 'game' | 'app'): void;
  closePicker(): void;
  /** La personne a choisi une app ouverte dans le sélecteur. */
  pickOpenApp(id: number): void;
  /** Coupe ou remet une entrée du compte. */
  setStartupEntry(id: string, enabled: boolean): void;
  /** « Annuler » dans la zone de retour : le geste inverse du dernier. */
  undoStartup(): void;
  /** La barre de retour a (ou n'a plus) le focus ou le survol : tant que oui, elle ne s'efface pas. */
  holdStartupSay(by: 'focus' | 'hover', on: boolean): void;
  /** « Réessayer » : la première vue du démarrage n'est pas venue. */
  retryStartup(): void;
  /** Le texte cherché dans la liste des apps. */
  setAppsQuery(query: string): void;
  setAppsSort(sort: AppsSort): void;
  /** « Réessayer » : la liste des apps n'est pas venue, ou pas en entier. */
  retryApps(): void;
  /** « Désinstaller » sur une ligne : ouvre la confirmation. Rien ne part encore. */
  askUninstall(id: string): void;
  /** La personne confirme : le cœur ouvre le désinstalleur de l'app. */
  confirmUninstall(): void;
  /** La personne referme la confirmation : rien ne part. */
  dismissUninstall(): void;
  /** « Revenir à la liste » et « Terminé ». */
  stopUninstall(): void;
  rescanLeftovers(): void;
  /** Coche ou décoche un reste (par son chemin). */
  toggleLeftover(path: string): void;
  /** L'orbe : met à la corbeille les restes cochés. */
  removeLeftovers(): void;
  /** Coche ou décoche une trace (par son identifiant). */
  toggleTrace(id: string): void;
  /** « Effacer ce qui est coché » : ouvre la confirmation. Rien ne part encore. */
  askErase(): void;
  /** La personne confirme : les identifiants cochés partent au cœur. */
  confirmErase(): void;
  /** La personne referme la confirmation : rien ne part. */
  dismissErase(): void;
  /** « Réessayer » : la première vue des traces n'est pas venue. */
  retryPrivacy(): void;
  openWindowsSettings(page: WindowsPage): void;
  minimize(): void;
  toggleMaximize(): void;
  close(): void;
}
