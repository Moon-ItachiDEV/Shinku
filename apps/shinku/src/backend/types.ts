// Les données échangées avec le cœur de Shinku.
//
// Ces types décrivent le JSON que rend le guichet (crates/shinku-core,
// modules backend, session, scan::report et settings). Les noms sont ceux du
// JSON : toute évolution côté Rust se reporte ici.

export type Theme = 'system' | 'light' | 'dark';

export interface Settings {
  version: number;
  launchWithWindows: boolean;
  language: 'fr' | 'en';
  theme: Theme;
  autoScan: 'never' | 'weekly' | 'monthly';
  notifyFromGb: number;
  gameModeOnLaunch: boolean;
  shaderCacheLimitGb: number | null;
  recycleBinAlert: boolean;
  recycleBinThresholdGb: number;
  recycleBinAutoEmptyDays: number | null;
  alwaysAsk: boolean;
  useRecycleBin: boolean;
  /** « Dossiers jamais touchés », sous leur forme habituelle (`C:\…`). */
  excludedFolders: string[];
}

/** Ce qui n'a pas pu être relu dans le fichier des réglages. */
export type LoadIssue = { kind: 'unreadable'; reason: string } | { kind: 'partial'; fields: string[] };

export interface Volume {
  root: string;
  label: string;
  fileSystem: string;
  total: number;
  free: number;
  fixed: boolean;
  seekPenalty: boolean | null;
  system: boolean;
}

export interface RecycleBin {
  bytes: number;
  items: number;
}

export interface Overview {
  version: string;
  volumes: Volume[];
  recycleBin: RecycleBin | null;
  settings: Settings;
  settingsIssue: LoadIssue | null;
  /** Vrai tant que l'utilisateur n'a pas lu un avertissement qui touche aux dossiers exclus. */
  scanBlocked: boolean;
}

export type Phase = 'preparing' | 'walking' | 'duplicatesQuick' | 'duplicatesFull' | 'done' | 'cancelled';

export interface Progress {
  phase: Phase;
  folders: number;
  files: number;
  /** Fichiers inutiles déjà trouvés, en octets (chiffre provisoire). */
  foundBytes: number;
  workDone: number;
  workTotal: number;
}

export type ModuleId =
  | 'junkFiles'
  | 'duplicates'
  | 'largeFiles'
  | 'recycleBin'
  | 'startup'
  | 'gameMode'
  | 'uninstall'
  | 'privacy';

export type CategoryState = 'ready' | 'pending' | 'unavailable';

/**
 * Le lot d'une catégorie du grand vide. `auto` : cochée d'office. `optional` :
 * proposée, jamais cochée d'office (des fichiers personnels). `review` : à
 * vérifier dans son module.
 */
export type Lot = 'auto' | 'optional' | 'review';

export interface CategorySummary {
  module: ModuleId;
  lot: Lot;
  risk: 'A' | 'B' | 'C' | 'D';
  bytes: number;
  items: number;
  state: CategoryState;
}

export interface Summary {
  complete: boolean;
  /** Début de l'analyse (millisecondes Unix) : le même pour ses résultats partiels et complets. */
  started: number;
  /** Moment où ces résultats ont été établis (millisecondes Unix). */
  finished: number;
  /** Total du lot automatique : ce que « Faire le vide » libère si aucune case n'est changée. */
  autoBytes: number;
  categories: CategorySummary[];
}

export type SessionState = 'idle' | 'scanning' | 'results' | 'failed';

export interface SessionView {
  state: SessionState;
  progress: Progress;
  summary: Summary | null;
  skipped: { folders: number; refusedRoots: number };
  stale: boolean;
}

export type JunkCategory = 'appCaches' | 'tempFiles' | 'updateLeftovers' | 'logsAndReports';

export interface JunkLine {
  label: string;
  bytes: number;
  files: number;
  rules: string[];
  closeFirst: string[];
}

export interface JunkCategoryReport {
  category: JunkCategory;
  label: string;
  bytes: number;
  files: number;
  lines: JunkLine[];
}

export interface JunkReport {
  bytes: number;
  files: number;
  sparedBytes: number;
  sparedFiles: number;
  categories: JunkCategoryReport[];
}

export interface LargeFile {
  path: string;
  size: number;
  onDisk: number;
  modified: number;
  synced: boolean;
}

export interface LargeFilesPage {
  threshold: number;
  count: number;
  bytes: number;
  listed: number;
  offset: number;
  files: LargeFile[];
}

export interface DuplicateFile {
  path: string;
  onDisk: number;
  created: number;
  modified: number;
  keep: boolean;
  synced: boolean;
}

export interface DuplicateGroup {
  size: number;
  digest: string;
  reclaimable: number;
  files: DuplicateFile[];
}

export interface DuplicatesPage {
  state: 'done' | 'pending' | 'off';
  candidates: number;
  copies: number;
  bytes: number;
  skippedFiles: number;
  hardLinks: number;
  groupCount: number;
  offset: number;
  groups: DuplicateGroup[];
}

export type FolderRefusal = 'notAbsolute' | 'notAFolder' | 'notUnicode';

/** Une demande refusée par le cœur. */
export type BackendError =
  | { kind: 'settingsUnchecked' }
  | { kind: 'scanNotStarted'; reason: string }
  | { kind: 'settingsNotSaved'; reason: string }
  | { kind: 'folderRefused'; reason: FolderRefusal };

/** Ce que l'interface peut demander. Deux réalisations : Tauri, et la démo. */
export interface Backend {
  overview(): Promise<Overview>;
  acknowledgeSettingsIssue(): Promise<Overview>;
  startScan(): Promise<SessionView>;
  cancelScan(): Promise<SessionView>;
  sessionView(): Promise<SessionView>;
  junk(): Promise<JunkReport | null>;
  largeFiles(offset: number, limit: number): Promise<LargeFilesPage | null>;
  duplicates(offset: number, limit: number): Promise<DuplicatesPage | null>;
  setTheme(theme: Theme): Promise<Settings>;
  /** Ouvre le sélecteur de dossier de Windows. `null` si l'utilisateur annule. */
  addExcludedFolder(): Promise<Settings | null>;
  removeExcludedFolder(path: string): Promise<Settings>;
  minimizeWindow(): Promise<void>;
  toggleMaximizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
}
