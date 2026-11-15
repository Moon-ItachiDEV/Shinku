// L'état de l'interface, et les gestes que les écrans peuvent déclencher.
//
// Les écrans (src/views) sont des fonctions de cet état : ils ne gardent rien
// pour eux. Tout changement passe par l'application (src/app.ts).

import type {
  DuplicatesPage,
  JunkReport,
  LargeFilesPage,
  Overview,
  SessionView,
  Theme,
} from './backend/types.ts';
import { type CleanRowId, type Selection, UNTOUCHED } from './model/scan.ts';

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

/** Ce que montre l'entrée « Grand vide » : l'accueil, l'analyse ou ses résultats. */
export type HomePane = 'home' | 'scan' | 'results';

export interface AppState {
  /** `loading` tant que le cœur n'a pas répondu ; `failed` s'il ne répond pas. */
  boot: 'loading' | 'ready' | 'failed' | 'outside';
  route: Route;
  pane: HomePane;
  overview: Overview | null;
  session: SessionView;
  /** Les cases que la personne a changées sur l'écran Résultats. */
  selection: Selection;
  junk: JunkReport | null;
  large: LargeFilesPage | null;
  duplicates: DuplicatesPage | null;
  /** Une page supplémentaire est en cours de chargement. */
  loadingMore: boolean;
  /** Le sélecteur de dossier de Windows est ouvert. */
  pickingFolder: boolean;
  /** Message bref affiché en bas de l'écran ; son texte reste le temps qu'il s'efface. */
  toast: string;
  toastShown: boolean;
  /** L'heure qu'il est, rafraîchie de temps en temps : pour « il y a 8 mois ». */
  now: number;
}

export const IDLE_SESSION: SessionView = {
  state: 'idle',
  progress: { phase: 'preparing', folders: 0, files: 0, foundBytes: 0, workDone: 0, workTotal: 0 },
  summary: null,
  skipped: { folders: 0, refusedRoots: 0 },
  stale: false,
};

export function initialState(now: number): AppState {
  return {
    boot: 'loading',
    route: 'home',
    pane: 'home',
    overview: null,
    session: IDLE_SESSION,
    selection: UNTOUCHED,
    junk: null,
    large: null,
    duplicates: null,
    loadingMore: false,
    pickingFolder: false,
    toast: '',
    toastShown: false,
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
  loadMore(list: 'large' | 'duplicates'): void;
  setTheme(theme: Theme): void;
  addFolder(): void;
  removeFolder(path: string): void;
  acknowledgeIssue(): void;
  minimize(): void;
  toggleMaximize(): void;
  close(): void;
}
