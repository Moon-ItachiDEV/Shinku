// Les données échangées avec le cœur de Shinku.
//
// Ces types décrivent le JSON que rend le guichet (crates/shinku-core,
// modules backend, session, scan::report, clean et settings). Les noms sont
// ceux du JSON : toute évolution côté Rust se reporte ici.

export type Theme = 'system' | 'light' | 'dark';

export interface Settings {
  version: number;
  launchWithWindows: boolean;
  language: 'fr' | 'en';
  theme: Theme;
  autoScan: 'never' | 'weekly' | 'monthly';
  notifyFromGb: number;
  shaderCacheLimitGb: number | null;
  recycleBinAlert: boolean;
  recycleBinThresholdGb: number;
  recycleBinAutoEmptyDays: number | null;
  alwaysAsk: boolean;
  useRecycleBin: boolean;
  /** « Dossiers jamais touchés », sous leur forme habituelle (`C:\…`). */
  excludedFolders: string[];
  /** Mode jeu automatique : éteint par défaut. */
  gameModeAuto: boolean;
  /** Alimentation « Performances élevées » pendant la partie. */
  gameModePower: boolean;
  /** Apps d'arrière-plan en veilleuse pendant la partie. */
  gameModeQuietApps: boolean;
  /** Apps cochées par la personne (noms d'exécutables). */
  gameModeQuietAdded: string[];
  /** Apps cochées d'office que la personne a décochées. */
  gameModeQuietRemoved: string[];
  /** Jeux ajoutés par la personne (chemins d'exécutables). */
  gameModeGames: string[];
  /** « Ce n'est pas un jeu » : programmes jamais traités comme des jeux. */
  gameModeNotGames: string[];
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
  /**
   * Le grand vide que Shinku n'a pas pu conclure la dernière fois (PC éteint,
   * Shinku fermé de force). Annoncé jusqu'à la prochaine analyse.
   */
  interruptedClean: CleanRun | null;
}

export type RunState = 'running' | 'completed' | 'cancelled' | 'interrupted';

/** Un grand vide tel que le journal le garde : des dates et des totaux, aucun chemin. */
export interface CleanRun {
  startedMs: number;
  finishedMs: number | null;
  state: RunState;
  freedBytes: number;
  recoverableBytes: number;
  skippedFiles: number;
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

export type SessionState = 'idle' | 'scanning' | 'results' | 'failed' | 'cleaning' | 'cleaned';

export interface SessionView {
  state: SessionState;
  progress: Progress;
  summary: Summary | null;
  skipped: { folders: number; refusedRoots: number };
  stale: boolean;
  /** Le grand vide en cours ou terminé. Dès qu'il démarre, il n'y a plus de `summary`. */
  clean: CleanView | null;
}

// ── Le grand vide ──────────────────────────────────────────────────────────

/**
 * Tout ce que l'interface envoie pour faire le vide : trois cases et la date
 * de l'analyse affichée (`Summary.started`). Aucun chemin, aucune liste : ce
 * qui part est décidé par le cœur.
 */
export interface CleanRequest {
  analysis: number;
  recycleBin: boolean;
  junkFiles: boolean;
  duplicates: boolean;
}

/** Une app ouverte : ses fichiers inutiles ne sont pas touchés tant qu'elle le reste. */
export interface LeftOpen {
  label: string;
  bytes: number;
}

/** Ce que l'écran de confirmation doit dire, vu du disque tel qu'il est maintenant. */
export interface CleanPreview {
  analysis: number;
  /** La corbeille telle qu'elle est maintenant. La vider est sans retour. */
  recycleBin: RecycleBin | null;
  junk: { bytes: number; files: number; openApps: LeftOpen[] };
  duplicates: {
    bytes: number;
    copies: number;
    /** Copies rangées dans un dossier synchronisé : elles partiront aussi du service en ligne. */
    syncedCopies: number;
    /** Copies rangées sur un autre disque que l'exemplaire gardé. */
    otherDiskCopies: number;
  };
}

export type CleanPhase = 'preparing' | 'recycleBin' | 'junkFiles' | 'duplicates' | 'done';

export interface CleanProgress {
  phase: CleanPhase;
  /** Place libérée pour de bon jusqu'ici. */
  freedBytes: number;
  /** Place occupée par ce qui est parti à la corbeille jusqu'ici. */
  recycledBytes: number;
  files: number;
  skippedFiles: number;
  workDone: number;
  /** 0 : le volume de travail de l'étape n'est pas connu. */
  workTotal: number;
}

export type CleanOutcome = 'completed' | 'cancelled';
export type StepState = 'notRequested' | 'done' | 'stopped' | 'failed';
export type StopReason = 'notInRecycleBin' | 'keptCopyChanged';

/** L'arrêt de sécurité de l'étape « Doublons ». */
export interface CleanStop {
  reason: StopReason;
  file: string;
}

/** Le bilan d'un grand vide. « Libéré » et « récupérable » ne se mélangent jamais. */
export interface CleanReport {
  started: number;
  finished: number;
  outcome: CleanOutcome;
  /** Libéré pour de bon : corbeille vidée et fichiers inutiles. */
  freedBytes: number;
  /** En attente dans la corbeille : récupérable, pas encore libéré. */
  recoverableBytes: number;
  recycleBin: { state: StepState; bytes: number; items: number };
  junk: { state: StepState; bytes: number; files: number; leftOpen: LeftOpen[] };
  duplicates: { state: StepState; bytes: number; copies: number; stop: CleanStop | null };
  /** Fichiers laissés en place, par raison. */
  skipped: { inUse: number; changed: number; denied: number; unrecyclable: number; other: number };
  durationMs: number;
}

export interface CleanView {
  request: CleanRequest;
  progress: CleanProgress;
  report: CleanReport | null;
}

export type CleanRefusal = 'noResults' | 'staleResults' | 'otherAnalysis' | 'nothingToClean';

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

/**
 * L'exemplaire à garder dans un groupe de doublons. Le groupe est désigné par
 * son empreinte et sa taille, l'exemplaire par son rang : aucun chemin ne part
 * de l'interface.
 */
export interface KeepChoice {
  digest: string;
  size: number;
  index: number;
  /** Garder aussi, dans les autres groupes, l'exemplaire rangé dans le même dossier. */
  wholeFolder: boolean;
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

// ── Le mode jeu ────────────────────────────────────────────────────────────

/** Les trois interrupteurs du mode jeu. */
export type GameOption = 'auto' | 'power' | 'quietApps';
/** Le programme de veille (`shinku-agent.exe`) : en marche, arrêté, ou absent de l'installation. */
export type AgentState = 'running' | 'stopped' | 'missing';
export type PowerOutcome =
  | 'applied'
  | 'alreadyFast'
  | 'onBattery'
  | 'unavailable'
  | 'unsuited'
  | 'custom'
  | 'refused'
  | 'off';
export type PowerSupport = 'available' | 'unavailable' | 'unsuited';
export type GameSource = 'added' | 'library';
export type WindowsPage =
  | 'gameMode'
  | 'power'
  | 'startupApps'
  | 'apps'
  | 'activityHistory'
  | 'searchPermissions'
  | 'diagnostics';
export type GameStatus = 'notGame' | 'forgotten';

/** La partie en cours, telle que l'agent l'a notée. */
export interface GameSession {
  name: string;
  path: string;
  since: number;
  source: GameSource;
  power: PowerOutcome;
  /** Apps distinctes en veilleuse en ce moment. */
  quietedApps: number;
  released: boolean;
  restorePending: boolean;
}

/** Le bilan de la dernière partie terminée. */
export interface LastGame {
  name: string;
  started: number;
  ended: number;
  /** `null` : l'alimentation n'avait pas été changée. */
  powerRestored: boolean | null;
  appsRestored: number;
  complete: boolean;
}

export interface QuietApp {
  exe: string;
  label: string;
  on: boolean;
  running: boolean;
  builtIn: boolean;
}

export interface KnownGame {
  path: string;
  name: string;
  source: GameSource;
  lastPlayed: number | null;
}

export interface GameView {
  auto: boolean;
  power: boolean;
  quietApps: boolean;
  agent: AgentState;
  /** Désactivé par la personne dans les apps de démarrage de Windows. */
  startupDisabled: boolean;
  session: GameSession | null;
  last: LastGame | null;
  powerSupport: PowerSupport;
  onBattery: boolean;
  /** Apps intégrées et apps ajoutées. */
  apps: QuietApp[];
  /** Jeux ajoutés, puis jeux vus (sans ceux de `notGames`), le plus récent d'abord. */
  games: KnownGame[];
  notGames: Array<{ path: string; name: string }>;
}

/** Une app ouverte que la personne peut choisir. L'`id` ne vaut que pour la dernière liste rendue. */
export interface OpenApp {
  id: number;
  name: string;
  path: string;
}

export type GameRefusal = 'unknownApp' | 'unknownGame' | 'notAGame' | 'protectedApp' | 'listFull';

/** Une demande refusée par le cœur. */
export type BackendError =
  | { kind: 'settingsUnchecked' }
  | { kind: 'scanNotStarted'; reason: string }
  | { kind: 'settingsNotSaved'; reason: string }
  | { kind: 'folderRefused'; reason: FolderRefusal }
  | { kind: 'cleanRunning' }
  | { kind: 'cleanRefused'; reason: CleanRefusal }
  | { kind: 'cleanNotStarted'; reason: string }
  | { kind: 'gameRefused'; reason: GameRefusal }
  | { kind: 'startupRefused'; reason: StartupRefusal }
  | { kind: 'uninstallRefused'; reason: UninstallRefusal }
  | { kind: 'privacyRefused'; reason: PrivacyRefusal };

// ── Le démarrage ───────────────────────────────────────────────────────────

/** Le compte (réglable ici), ou tout le PC (en lecture seule dans ce lot). */
export type StartupScope = 'user' | 'machine';
/** `unknown` : Shinku ne sait pas lire le réglage de Windows, la ligne n'a pas d'interrupteur. */
export type StartupState = 'enabled' | 'disabled' | 'unknown';
export type StartupTarget = 'found' | 'missing' | 'hosted' | 'unknown';
export type StartupOrigin = 'registry' | 'folder';
export type StartupMark =
  /** Coupée par Shinku, à cette date. */
  | { kind: 'cutByShinku'; at: number }
  /** Coupée par Shinku, puis remise par l'app elle-même ; `cut` : la date de la coupure. */
  | { kind: 'cameBack'; cut: number };

export interface StartupEntry {
  /** L'identifiant que le cœur a donné : c'est tout ce que la page lui renvoie. */
  id: string;
  name: string;
  /** Le nom sous lequel l'app est inscrite, s'il diffère de `name`. */
  alias: string | null;
  /** Ce que le fichier déclare, pas une identité vérifiée. */
  publisher: string | null;
  scope: StartupScope;
  origin: StartupOrigin;
  state: StartupState;
  /** La date de la coupure, lue dans Windows, si elle est plausible. */
  disabledAt: number | null;
  mark: StartupMark | null;
  target: StartupTarget;
  /** Le programme qui porte la commande, si `target` vaut `hosted`. */
  host: string | null;
  hint: 'sync' | null;
  /** L'entrée de Shinku lui-même (mode jeu automatique). */
  own: boolean;
  /** Pour une entrée du PC, vrai si Shinku peut la régler (`StartupView.pc` n'est pas `unavailable`, et la ligne s'y prête). */
  adjustable: boolean;
  /**
   * Pour une entrée du PC : le nom sous lequel elle est inscrite, déjà mis en forme par le cœur (les caractères
   * douteux y sont montrés `⟨U+00A0⟩`, jamais retirés) ; la page l'affiche tel quel. `null` pour une entrée du compte.
   */
  registered: string | null;
}

/**
 * Comment Windows laisse Shinku régler le PC : `consent` (une fenêtre d'accord), `password` (le mot de passe d'un
 * administrateur), `direct` (Windows ne demande rien : Shinku fait confirmer), `unavailable` (aucun moyen).
 */
export type StartupPc = 'consent' | 'password' | 'direct' | 'unavailable';

/** Le changement du PC que l'assistant attend : la page relit la vue tant qu'il est là. */
export interface StartupPending {
  id: string;
  enabled: boolean;
}

export interface StartupView {
  /** Le compte d'abord, puis le PC ; triées. */
  entries: StartupEntry[];
  /** Les portées dont une place n'a pas pu être lue. */
  unreadable: StartupScope[];
  pc: StartupPc;
  pending: StartupPending | null;
}

export type StartupRefusal = 'unknownEntry' | 'notAdjustable' | 'busy' | 'failed' | 'cancelled' | 'needsAdmin';

// ── La désinstallation ─────────────────────────────────────────────────────

/** Le compte, ou tout le PC (Windows demandera alors son accord). */
export type AppScope = 'account' | 'machine';
/** Un indice par le nom, pas une certitude : `runtime` et `driver` sont des ressemblances. */
export type AppKind = 'plain' | 'runtime' | 'driver';
/**
 * Ce que Shinku propose pour cette app : `offered` (le bouton « Désinstaller »), `refused` (Windows
 * cache lui-même son bouton), `own` (c'est Shinku), `needed` (Shinku s'affiche grâce à elle).
 */
export type AppRemoval = 'offered' | 'refused' | 'own' | 'needed';

export interface AppRow {
  /** L'identifiant que le cœur a donné : c'est tout ce que la page lui renvoie. */
  id: string;
  name: string;
  version?: string;
  publisher?: string;
  /** La taille que l'app déclare ; absente si elle ne la déclare pas. */
  bytes?: number;
  /** « AAAA-MM-JJ ». */
  installed?: string;
  scope: AppScope;
  kind: AppKind;
  removal: AppRemoval;
}

export type LeftoverKind = 'folder' | 'data' | 'shortcut';

export interface LeftoverRow {
  /** Opaque : son rang dans la liste que le cœur garde. C'est tout ce que la page lui renvoie. */
  key: number;
  kind: LeftoverKind;
  /** À montrer, jamais à renvoyer. */
  path: string;
  bytes?: number;
  files?: number;
}

export type FollowView =
  | { phase: 'idle' }
  | { phase: 'launching'; id: string; name: string }
  | { phase: 'waiting'; id: string; name: string; seconds: number }
  | { phase: 'settling'; id: string; name: string }
  | { phase: 'removed'; id: string; name: string; leftovers: LeftoverRow[]; family: boolean; complete: boolean };

export interface AppsView {
  /** Triées par nom, sans casse ni accents. */
  apps: AppRow[];
  /** Faux si une source n'a pas pu être lue en entier. */
  complete: boolean;
  follow: FollowView;
  /** Dit une seule fois : l'installeur de Windows est sorti, et l'app est restée. */
  outcome?: 'kept';
}

export type UninstallRefusal =
  | 'busy'
  | 'vanished'
  | 'unreadable'
  | 'notOffered'
  | 'broken'
  | 'cancelled'
  | 'failed';

export type RemoveOutcome = 'removed' | 'alreadyGone' | 'refused' | 'failed';
/** `destroyed` : Windows a supprimé l'élément pour de bon au lieu de le garder dans la corbeille ; il n'est plus là. */
export type RemoveReason = 'inUse' | 'refused' | 'partial' | 'destroyed';

export interface RemoveResult {
  key: number;
  outcome: RemoveOutcome;
  reason?: RemoveReason;
}

/** Ce que rend `remove_leftovers` : la vue, ce qui est arrivé à chaque reste demandé, et la taille mise à la corbeille. */
export interface RemoveLeftovers {
  view: AppsView;
  results: RemoveResult[];
  bytes: number;
}

// ── La confidentialité ─────────────────────────────────────────────────────

/** Ce que Shinku sait effacer : l'historique d'un profil de navigateur, ou une trace de Windows. */
export type TraceKind = 'history' | 'recent' | 'typed' | 'clipboard';
/** Ce qui est arrivé à une trace que la personne avait cochée. */
export type TraceOutcome = 'erased' | 'nothing' | 'inUse' | 'changed' | 'partial' | 'failed';
export type TraceState = 'present' | 'empty' | 'unreadable';
export type BrowserApp = 'chrome' | 'edge' | 'brave' | 'vivaldi';

/** Une trace : jamais son contenu, seulement qu'elle existe, son poids ou son nombre d'éléments. */
export interface TraceItem {
  /** Fabriqué par le cœur, opaque pour la page : c'est la seule chose qu'elle lui renvoie. */
  id: string;
  kind: TraceKind;
  state: TraceState;
  /** Le poids des fichiers, sinon `null`. */
  bytes: number | null;
  /** Le nombre de valeurs ou de raccourcis, sinon `null`. */
  count: number | null;
}

/** Un navigateur et son profil, ou le bloc « Windows ». */
export interface PrivacyGroup {
  key: string;
  kind: 'browser' | 'windows';
  app: BrowserApp | null;
  /** Le nom du profil lu par le cœur, déjà nettoyé : à montrer tel quel. */
  profile: string | null;
  /** `null` : le bloc Windows ; `0` : le profil `Default` ; `N` : `Profile N`. */
  slot: number | null;
  /** Le navigateur est ouvert : ses traces ne s'effacent pas. */
  running: boolean;
  items: TraceItem[];
}

export interface PrivacyView {
  groups: PrivacyGroup[];
  /** Un dossier de profils Firefox existe : une ligne d'aide. */
  firefox: boolean;
  /** Chrome et Edge sont tous deux là : Edge peut réimporter l'historique de Chrome. */
  edgeImportsChrome: boolean;
}

export interface EraseResult {
  id: string;
  outcome: TraceOutcome;
}

/** Ce que rend `erase_traces` : la vue relue, et ce qui est arrivé à chaque trace demandée. */
export interface ErasedTraces {
  view: PrivacyView;
  results: EraseResult[];
}

/** `busy` : un grand vide ou un autre effacement est en cours ; `tooMany` : plus de 64 traces d'un coup. */
export type PrivacyRefusal = 'busy' | 'tooMany';

/** Ce que l'interface peut demander. Deux réalisations : Tauri, et la démo. */
export interface Backend {
  overview(): Promise<Overview>;
  acknowledgeSettingsIssue(): Promise<Overview>;
  startScan(): Promise<SessionView>;
  /** Arrête l'analyse et oublie ses résultats, ou referme le bilan d'un grand vide terminé. */
  cancelScan(): Promise<SessionView>;
  sessionView(): Promise<SessionView>;
  cleanPreview(): Promise<CleanPreview>;
  startClean(request: CleanRequest): Promise<SessionView>;
  /** Demande l'arrêt : le fichier en cours se termine, puis le bilan partiel arrive. */
  cancelClean(): Promise<SessionView>;
  junk(): Promise<JunkReport | null>;
  largeFiles(offset: number, limit: number): Promise<LargeFilesPage | null>;
  duplicates(offset: number, limit: number): Promise<DuplicatesPage | null>;
  /** Choisit l'exemplaire gardé d'un doublon. Faux si le choix ne désigne plus rien. */
  keepDuplicate(choice: KeepChoice): Promise<boolean>;
  setTheme(theme: Theme): Promise<Settings>;
  /** Ouvre le sélecteur de dossier de Windows. `null` si l'utilisateur annule. */
  addExcludedFolder(): Promise<Settings | null>;
  removeExcludedFolder(path: string): Promise<Settings>;
  /** Le mode jeu : réglages, état de l'agent, partie en cours, jeux et apps connus. */
  gameView(): Promise<GameView>;
  setGameOption(option: GameOption, on: boolean): Promise<GameView>;
  /** `exe` n'est que comparé à la liste connue du cœur. */
  setQuietApp(exe: string, on: boolean): Promise<GameView>;
  /** Les apps ouvertes ; les `id` rendus ne valent que pour cette liste. */
  openApps(): Promise<OpenApp[]>;
  /** `path` n'est que comparé : le cœur vérifie qu'il est bien celui du numéro `id` dans sa dernière liste. */
  addGame(id: number, path: string): Promise<GameView>;
  addQuietApp(id: number, path: string): Promise<GameView>;
  /** `path` n'est que comparé aux chemins que le cœur connaît. */
  setGameStatus(path: string, status: GameStatus): Promise<GameView>;
  /** « Tout remettre maintenant ». */
  releaseGame(): Promise<GameView>;
  /** Le démarrage : les apps inscrites, du compte et du PC. */
  startupView(): Promise<StartupView>;
  /** `id` n'est que comparé aux entrées que le cœur connaît ; il rend la vue entière. */
  setStartupEntry(id: string, enabled: boolean): Promise<StartupView>;
  /** Les apps installées, et où en est la désinstallation en cours ; relit aussi son avancement. */
  appsView(): Promise<AppsView>;
  /** `id` n'est que comparé aux apps que le cœur connaît : il relit lui-même la commande dans le registre. */
  startUninstall(id: string): Promise<AppsView>;
  /** Revient à la liste. Ne tue rien, ne touche à rien. */
  stopUninstall(): Promise<AppsView>;
  /** Cherche de nouveau les restes : les cases repartent vides. */
  rescanLeftovers(): Promise<AppsView>;
  /** Met des restes à la corbeille, jamais plus loin. `keys` : les clés que le cœur a données. */
  removeLeftovers(keys: number[]): Promise<RemoveLeftovers>;
  /** Les traces effaçables : navigateurs par profil, puis Windows. Rien n'est lu au-delà de leur existence et de leur poids. */
  privacyView(): Promise<PrivacyView>;
  /** `ids` : seulement ceux que le cœur a fabriqués. Il refuse au-delà de 64, ou pendant un grand vide. */
  eraseTraces(ids: string[]): Promise<ErasedTraces>;
  openWindowsSettings(page: WindowsPage): Promise<void>;
  minimizeWindow(): Promise<void>;
  toggleMaximizeWindow(): Promise<void>;
  closeWindow(): Promise<void>;
}
