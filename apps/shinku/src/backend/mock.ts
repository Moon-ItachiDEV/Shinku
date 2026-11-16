// Un faux guichet, pour voir l'interface dans un navigateur sans Windows ni
// Tauri : `npm run demo`. Il n'est jamais embarqué dans l'application.
//
// Les chiffres sont ceux du canevas de design. L'analyse et le grand vide
// sont de simples animations : rien n'est lu sur le disque, rien n'y est
// supprimé.

import type {
  Backend,
  BackendError,
  CategorySummary,
  CleanPhase,
  CleanProgress,
  CleanReport,
  CleanRequest,
  CleanRun,
  CleanStop,
  DuplicateGroup,
  DuplicatesPage,
  GameOption,
  GameRefusal,
  GameSession,
  GameStatus,
  GameView,
  JunkReport,
  KnownGame,
  LargeFile,
  LargeFilesPage,
  LastGame,
  LeftOpen,
  LoadIssue,
  OpenApp,
  Overview,
  PowerOutcome,
  Phase,
  Progress,
  QuietApp,
  RecycleBin,
  SessionView,
  Settings,
  StartupEntry,
  StartupRefusal,
  StartupScope,
  StartupView,
  StepState,
  Summary,
  Theme,
} from './types.ts';

import { keepIn } from '../model/duplicates.ts';
import { type AppsDemo, appsBackend, appsDemoOptions } from './mock-apps.ts';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const DAY = 24 * 3600 * 1000;

/** Ce que la démo montre du mode jeu (`?game=…&agent=…&power=…&startup=…`). */
export interface GameDemo {
  /** Où en est la partie : éteint, en veille, en jeu, remis en place, remise en attente. */
  state: 'off' | 'idle' | 'playing' | 'released' | 'pending';
  agent: 'running' | 'stopped' | 'missing';
  power: 'available' | 'unavailable' | 'unsuited' | 'battery';
  startup: 'enabled' | 'disabled';
}

/** Ce que la démo montre du démarrage (`?boot=few|many|empty&bootfail=1&bootread=user|machine&bootload=fail&bootshinku=off`). */
export interface BootDemo {
  /** Les apps inscrites : quelques-unes, une quarantaine, ou aucune. */
  data: 'few' | 'many' | 'empty';
  /** Tout changement échoue. */
  fail: boolean;
  /** La portée dont une place n'a pas pu être lue, ou aucune. */
  read: 'all' | 'user' | 'machine';
  /** La première lecture de la vue échoue (`?bootload=fail`) ; « Réessayer » la réussit. */
  load: 'ok' | 'fail';
  /** Shinku est coupé dans Windows (`?bootshinku=off`). */
  shinku: 'on' | 'off';
}

export interface MockOptions {
  /** Accélère l'analyse simulée (1 : durée normale, environ 7 secondes). */
  speed: number;
  /** Simule un fichier de réglages abîmé au démarrage. */
  issue: 'none' | 'unreadable' | 'partial';
  /** Corbeille vide plutôt que pleine. */
  emptyBin: boolean;
  /** Mode d'affichage enregistré dans les faux réglages. */
  theme: Theme;
  /**
   * Comment se passe le grand vide simulé. `smooth` : sans histoire. `rough` :
   * une app est restée ouverte et des fichiers sont laissés en place. `halt` :
   * l'étape « Doublons » s'arrête d'elle-même, par sécurité.
   */
  clean: 'smooth' | 'rough' | 'halt';
  /** Annonce, à l'Accueil, un grand vide interrompu au lancement précédent. */
  interrupted: boolean;
  /** Ce que la démo montre du mode jeu. */
  game: GameDemo;
  /** Ce que la démo montre du démarrage. */
  boot: BootDemo;
  /** Ce que la démo montre de la désinstallation (voir mock-apps.ts). */
  apps: AppsDemo;
}

/**
 * Lit les options de la démo dans l'adresse de la page :
 * `?speed=4&theme=light&issue=unreadable&bin=empty&clean=rough&interrupted=1`.
 * Le mode jeu : `?game=off|idle|playing|released|pending`, `?agent=missing|stopped`,
 * `?power=unavailable|unsuited|battery`, `?startup=disabled`.
 * Le démarrage : `?boot=few|many|empty`, `?bootfail=1`, `?bootread=user|machine`, `?bootload=fail`, `?bootshinku=off`.
 * La désinstallation : `?apps=few|many|empty|partial`, `?appsload=fail`,
 * `?appsflow=launching|waiting|stuck|leftovers|none|family|unsure`, `?appsfail=broken|cancelled|bin|kept`.
 */
export function demoOptions(search: string): MockOptions {
  const query = new URLSearchParams(search);
  const speed = Number(query.get('speed') ?? '1');
  const issue = query.get('issue');
  const theme = query.get('theme');
  const clean = query.get('clean');
  const game = query.get('game');
  const agent = query.get('agent');
  const power = query.get('power');
  const boot = query.get('boot');
  const read = query.get('bootread');
  return {
    speed: Number.isFinite(speed) && speed > 0 ? speed : 1,
    issue: issue === 'unreadable' || issue === 'partial' ? issue : 'none',
    emptyBin: query.get('bin') === 'empty',
    theme: theme === 'light' || theme === 'dark' ? theme : 'system',
    clean: clean === 'rough' || clean === 'halt' ? clean : 'smooth',
    interrupted: query.get('interrupted') === '1',
    game: {
      state:
        game === 'idle' || game === 'playing' || game === 'released' || game === 'pending' ? game : 'off',
      agent: agent === 'missing' || agent === 'stopped' ? agent : 'running',
      power: power === 'unavailable' || power === 'unsuited' || power === 'battery' ? power : 'available',
      startup: query.get('startup') === 'disabled' ? 'disabled' : 'enabled',
    },
    boot: {
      data: boot === 'many' || boot === 'empty' ? boot : 'few',
      fail: query.get('bootfail') === '1',
      read: read === 'user' || read === 'machine' ? read : 'all',
      load: query.get('bootload') === 'fail' ? 'fail' : 'ok',
      shinku: query.get('bootshinku') === 'off' ? 'off' : 'on',
    },
    apps: appsDemoOptions(query),
  };
}

// ── Le mode jeu de la démo ─────────────────────────────────────────────────

/** Les apps que le cœur propose d'office : `on` est leur état d'origine. */
const BUILT_IN_APPS: ReadonlyArray<{ exe: string; label: string; on: boolean }> = [
  { exe: 'onedrive.exe', label: 'OneDrive', on: true },
  { exe: 'dropbox.exe', label: 'Dropbox', on: true },
  { exe: 'googledrivefs.exe', label: 'Google Drive', on: true },
  { exe: 'chrome.exe', label: 'Google Chrome', on: false },
  { exe: 'msedge.exe', label: 'Microsoft Edge', on: false },
  { exe: 'firefox.exe', label: 'Firefox', on: false },
  { exe: 'brave.exe', label: 'Brave', on: false },
  { exe: 'opera.exe', label: 'Opera', on: false },
  { exe: 'vivaldi.exe', label: 'Vivaldi', on: false },
];

const fileName = (path: string): string => path.slice(path.lastIndexOf('\\') + 1).toLowerCase();

/** Les apps qui tournent dans la démo, en plus des programmes ouverts ci-dessous. */
const RUNNING_APPS: ReadonlySet<string> = new Set(['onedrive.exe', 'chrome.exe', 'qbittorrent.exe']);

/**
 * Les programmes ouverts de la démo : quatre que le sélecteur propose (dont un
 * lanceur que le cœur refuse comme jeu et comme app en veilleuse), et
 * l'Explorateur de Windows, que le cœur ne propose jamais (nom protégé).
 */
const OPEN_PROGRAMS: ReadonlyArray<{ name: string; path: string }> = [
  { name: 'Roblox', path: 'C:\\Users\\Léa\\AppData\\Local\\Roblox\\Versions\\version-1a2b3c4d\\RobloxPlayerBeta.exe' },
  { name: 'Notepad++', path: 'C:\\Program Files\\Notepad++\\notepad++.exe' },
  { name: 'VLC media player', path: 'C:\\Program Files\\VideoLAN\\VLC\\vlc.exe' },
  { name: 'Battle.net', path: 'C:\\Program Files (x86)\\Battle.net\\Battle.net.exe' },
  { name: 'Explorateur Windows', path: 'C:\\Windows\\explorer.exe' },
];

/** Les noms de fichier que le cœur ne propose ni ne met jamais en veilleuse. */
const PROTECTED_EXES: ReadonlySet<string> = new Set(['explorer.exe']);

/** Les lanceurs de jeux : jamais mis en veilleuse (règle du cœur), refusés comme « protégés ». */
const LAUNCHER_EXES: ReadonlySet<string> = new Set(['steam.exe', 'epicgameslauncher.exe', 'battle.net.exe']);

/** Les bornes des listes du cœur (`MAX_GAME_PATHS` et `MAX_EXE_NAMES`, crates/shinku-core/src/settings.rs). */
export const GAME_LIST_LIMIT = 256;
export const QUIET_LIST_LIMIT = 64;

/** Les noms d'affichage que le cœur lit sur une app ouverte. */
const DISPLAY_NAMES: ReadonlyMap<string, string> = new Map([
  ['qbittorrent.exe', 'qBittorrent'],
  ...OPEN_PROGRAMS.map((program): [string, string] => [fileName(program.path), program.name]),
]);

/** Les programmes que le cœur ne laisse jamais ajouter comme jeu. */
const NEVER_GAMES: ReadonlySet<string> = new Set(['vlc.exe', 'battle.net.exe']);

const HOLLOW_PEAKS = 'D:\\SteamLibrary\\steamapps\\common\\Hollow Peaks\\hp.exe';
const MINECRAFT = 'C:\\Program Files\\Eclipse Adoptium\\jdk-17.0.9.9-hotspot\\bin\\javaw.exe';

const samePath = (left: string, right: string): boolean => left.toLowerCase() === right.toLowerCase();
// ── Le démarrage de la démo ────────────────────────────────────────────────

/**
 * L'identifiant d'une entrée : les deux lettres de la place, un tiret, seize
 * chiffres hexadécimaux. Il a la forme de celui du cœur (un FNV-1a 64 bits du
 * nom) ; la démo n'a pas besoin de la même valeur.
 */
function startupId(place: 'ur' | 'uw' | 'uf' | 'mr' | 'mw' | 'mf', name: string): string {
  let hash = 0xcbf29ce484222325n;
  for (let index = 0; index < name.length; index += 1) {
    hash = ((hash ^ BigInt(name.charCodeAt(index))) * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return `${place}-${hash.toString(16).padStart(16, '0')}`;
}

/** Ce que la démo écrit d'une entrée ; le reste (identifiant, portée, réglable) s'en déduit. */
interface BootSeed {
  place: 'ur' | 'uw' | 'uf' | 'mr' | 'mw' | 'mf';
  raw: string;
  name: string;
  alias?: string | undefined;
  publisher?: string | undefined;
  state?: StartupEntry['state'] | undefined;
  /** Il y a combien de jours Windows a noté la coupure. */
  cutDaysAgo?: number | undefined;
  /** La coupure a eu lieu à cette date (mois à partir de 0, jour, années avant maintenant). */
  cutOn?: { month: number; day: number; yearsAgo: number } | undefined;
  mark?: 'cutByShinku' | 'cameBack' | undefined;
  target?: StartupEntry['target'] | undefined;
  host?: string | undefined;
  sync?: boolean | undefined;
  own?: boolean | undefined;
}

function bootEntry(seed: BootSeed, now: number): StartupEntry {
  const machine = seed.place.startsWith('m');
  const state = seed.state ?? 'enabled';
  const cutAt =
    seed.cutOn !== undefined
      ? new Date(new Date(now).getFullYear() - seed.cutOn.yearsAgo, seed.cutOn.month, seed.cutOn.day, 9, 41).getTime()
      : seed.cutDaysAgo === undefined
        ? null
        : now - seed.cutDaysAgo * DAY;
  return {
    id: startupId(seed.place, seed.raw),
    name: seed.name,
    alias: seed.alias ?? null,
    // Le cœur ne lit l'éditeur que dans le fichier qu'il a trouvé.
    publisher: (seed.target ?? 'found') === 'found' ? (seed.publisher ?? null) : null,
    scope: machine ? 'machine' : 'user',
    origin: seed.place.endsWith('f') ? 'folder' : 'registry',
    state,
    disabledAt: state === 'disabled' ? cutAt : null,
    mark:
      seed.mark === 'cutByShinku' && cutAt !== null
        ? { kind: 'cutByShinku', at: cutAt }
        : seed.mark === 'cameBack' && cutAt !== null
          ? { kind: 'cameBack', cut: cutAt }
          : null,
    target: seed.target ?? 'found',
    host: seed.host ?? null,
    hint: seed.sync === true && (seed.target ?? 'found') === 'found' ? 'sync' : null,
    own: seed.own === true,
    adjustable: !machine && state !== 'unknown' && seed.own !== true,
  };
}

const FEW_BOOT: readonly BootSeed[] = [
  { place: 'ur', raw: 'Discord', name: 'Discord', publisher: 'Discord Inc.' },
  { place: 'ur', raw: 'Steam', name: 'Steam', publisher: 'Valve Corporation', state: 'disabled', mark: 'cutByShinku', cutDaysAgo: 3 },
  { place: 'ur', raw: 'OneDrive', name: 'Microsoft OneDrive', alias: 'OneDrive', publisher: 'Microsoft Corporation', sync: true },
  { place: 'ur', raw: 'Spotify', name: 'Spotify', publisher: 'Spotify AB', mark: 'cameBack', cutDaysAgo: 5 },
  { place: 'uf', raw: 'Adobe Updater Startup Utility.lnk', name: 'Mise à jour Adobe', target: 'missing' },
  { place: 'ur', raw: 'Lancement rapide', name: 'Lancement rapide', target: 'hosted', host: 'rundll32.exe' },
  { place: 'ur', raw: 'Shinku', name: 'Shinku', publisher: 'Shinku', own: true },
  { place: 'ur', raw: 'Ancien outil', name: 'Ancien outil', state: 'unknown' },
  { place: 'mr', raw: 'SecurityHealth', name: 'Sécurité Windows', alias: 'SecurityHealth', publisher: 'Microsoft Corporation' },
  { place: 'mr', raw: 'RtkAudUService', name: 'Realtek HD Audio Universal Service', publisher: 'Realtek Semiconductor' },
  { place: 'mr', raw: 'LGHUB', name: 'Logitech G HUB', publisher: 'Logitech, Inc.', state: 'disabled', cutOn: { month: 10, day: 2, yearsAgo: 1 } },
  { place: 'mw', raw: 'SunJavaUpdateSched', name: 'Java Update Scheduler', publisher: 'Oracle Corporation' },
];

const MANY_NAMES: readonly string[] = [
  'Adobe Creative Cloud', 'Battle.net', 'Brave', 'Dropbox', 'EA app', 'Epic Games Launcher', 'Figma', 'GeForce Experience',
  'Google Drive', 'iCloud', 'Logi Options+', 'Malwarebytes', 'Microsoft Teams', 'NordVPN', 'Notion', 'Opera GX',
  'Overwolf', 'Razer Synapse', 'Signal', 'Slack', 'Spotify', 'Steam', 'TeamViewer', 'Telegram', 'Ubisoft Connect',
  'Zoom', 'Corsair iCUE', 'Wacom Tablet', '7-Zip', '1Password', 'AMD Software', 'Bluetooth', 'Intel Driver Assistant',
  'Lenovo Vantage', 'OneNote', 'qBittorrent', 'Skype', 'VLC', 'Zotero', 'Visual Studio Code',
];

const MANY_PUBLISHERS: readonly string[] = [
  'Microsoft Corporation',
  'Adobe Inc.',
  'Google LLC',
  'Valve Corporation',
  'Razer Inc.',
  'Mozilla Foundation',
  'Logitech, Inc.',
];

function manyBoot(): BootSeed[] {
  return MANY_NAMES.map((name, index): BootSeed => {
    const machine = index % 4 === 3;
    const place = machine ? (index % 8 === 3 ? 'mr' : 'mf') : index % 7 === 0 ? 'uf' : index % 5 === 0 ? 'uw' : 'ur';
    const kind = index % 9;
    return {
      place,
      raw: `${name}${place === 'mf' || place === 'uf' ? '.lnk' : ''}`,
      name,
      publisher: kind === 5 ? undefined : MANY_PUBLISHERS[index % MANY_PUBLISHERS.length],
      state: kind === 1 || kind === 2 ? 'disabled' : kind === 7 && !machine ? 'unknown' : 'enabled',
      cutDaysAgo: kind === 1 ? index : kind === 2 ? 40 + index : kind === 6 ? 2 + (index % 9) : undefined,
      mark: kind === 1 ? 'cutByShinku' : kind === 6 ? 'cameBack' : undefined,
      target: kind === 3 ? 'missing' : kind === 4 ? 'hosted' : kind === 8 ? 'unknown' : 'found',
      host: kind === 4 ? 'javaw.exe' : undefined,
      alias: kind === 0 ? `${name.toLowerCase().replaceAll(' ', '')}.exe` : undefined,
      sync: index % 6 === 0,
    };
  });
}

const gameRefused = (reason: GameRefusal): Promise<never> => Promise.reject({ kind: 'gameRefused', reason });
const startupRefused = (reason: StartupRefusal): Promise<never> =>
  Promise.reject({ kind: 'startupRefused', reason });

/**
 * Comme le cœur (`startup/mod.rs`) : le compte d'abord, puis le PC ; dans chaque portée, par nom en
 * minuscules, puis nom d'inscription en minuscules (l'absence d'abord), puis identifiant. La
 * comparaison est celle des points de code, pas celle d'une langue.
 */
function sortedBoot(entries: readonly StartupEntry[]): StartupEntry[] {
  const rank = (scope: StartupScope): number => (scope === 'user' ? 0 : 1);
  const text = (left: string, right: string): number => (left < right ? -1 : left > right ? 1 : 0);
  return entries.toSorted(
    (left, right) =>
      rank(left.scope) - rank(right.scope) ||
      text(left.name.toLowerCase(), right.name.toLowerCase()) ||
      (left.alias === null
        ? right.alias === null
          ? 0
          : -1
        : right.alias === null
          ? 1
          : text(left.alias.toLowerCase(), right.alias.toLowerCase())) ||
      text(left.id, right.id),
  );
}

const JUNK: JunkReport = {
  bytes: 3.7 * GB,
  files: 18_412,
  sparedBytes: 320 * MB,
  sparedFiles: 1204,
  categories: [
    {
      category: 'appCaches',
      label: 'Caches des apps',
      bytes: 1.43 * GB,
      files: 9120,
      lines: [
        { label: 'Google Chrome', bytes: 612 * MB, files: 4210, rules: ['chrome.cache'], closeFirst: ['chrome.exe'] },
        { label: 'Discord', bytes: 388 * MB, files: 2140, rules: ['discord.cache'], closeFirst: ['Discord.exe'] },
        { label: 'Steam', bytes: 301 * MB, files: 1630, rules: ['steam.htmlcache'], closeFirst: ['steam.exe'] },
        { label: 'Microsoft Edge', bytes: 163 * MB, files: 1140, rules: ['edge.cache'], closeFirst: ['msedge.exe'] },
      ],
    },
    {
      category: 'tempFiles',
      label: 'Fichiers temporaires',
      bytes: 920 * MB,
      files: 6304,
      lines: [{ label: 'Dossier temporaire de Windows', bytes: 920 * MB, files: 6304, rules: ['windows.temp'], closeFirst: [] }],
    },
    {
      category: 'updateLeftovers',
      label: 'Restes de mises à jour',
      bytes: 1.05 * GB,
      files: 2210,
      lines: [
        { label: 'Installeurs de pilotes NVIDIA', bytes: 760 * MB, files: 1480, rules: ['nvidia.installer'], closeFirst: [] },
        { label: 'Restes de mises à jour d’apps', bytes: 315 * MB, files: 730, rules: ['squirrel.packages'], closeFirst: [] },
      ],
    },
    {
      category: 'logsAndReports',
      label: 'Journaux et rapports d’erreur',
      bytes: 300 * MB,
      files: 778,
      lines: [
        { label: 'Rapports d’erreur de Windows', bytes: 212 * MB, files: 96, rules: ['windows.wer'], closeFirst: [] },
        { label: 'Journaux des apps', bytes: 88 * MB, files: 682, rules: ['apps.logs'], closeFirst: [] },
      ],
    },
  ],
};

function largeFiles(now: number): LargeFile[] {
  const user = 'C:\\Users\\Léa';
  return [
    { path: `${user}\\Downloads\\Windows11_23H2.iso`, size: 6.2 * GB, onDisk: 6.2 * GB, modified: now - 243 * DAY, synced: false },
    { path: `${user}\\Videos\\Montage_été_2023.mp4`, size: 4.8 * GB, onDisk: 4.8 * GB, modified: now - 400 * DAY, synced: false },
    { path: `${user}\\VirtualBox VMs\\Linux\\Linux.vdi`, size: 4.2 * GB, onDisk: 4.2 * GB, modified: now - 152 * DAY, synced: false },
    { path: `${user}\\OneDrive\\Documents\\Sauvegarde_ancien_PC.zip`, size: 3.4 * GB, onDisk: 3.4 * GB, modified: now - 760 * DAY, synced: true },
  ];
}

function duplicateGroups(now: number): DuplicateGroup[] {
  const user = 'C:\\Users\\Léa';
  let rank = 0;
  const group = (name: string, size: number, folders: string[], synced: boolean[] = []): DuplicateGroup => ({
    size,
    // Une empreinte par groupe, comme dans le cœur.
    digest: `${(rank++).toString(16).padStart(2, '0')}1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262`,
    reclaimable: size * (folders.length - 1),
    files: folders.map((folder, index) => ({
      path: `${user}\\${folder}\\${name}`,
      onDisk: size,
      created: now - (900 - index * 200) * DAY,
      modified: now - 900 * DAY,
      keep: index === 0,
      synced: synced[index] ?? false,
    })),
  });
  return [
    group('Vacances_Bretagne_2022.mp4', 612 * MB, ['Videos', 'Downloads', 'OneDrive\\Vidéos'], [false, false, true]),
    group('Album_photos_mariage.zip', 384 * MB, ['Documents', 'Downloads']),
    group('Concert_live_integral.flac', 121 * MB, ['Music', 'Downloads\\Musique']),
    group('IMG_2048.heic', 6.4 * MB, ['Pictures\\2023', 'OneDrive\\Images'], [false, true]),
    group('Facture_électricité_mars.pdf', 1.2 * MB, ['Documents\\Factures', 'Downloads', 'Desktop']),
  ];
}

const delay = (milliseconds: number) => new Promise<void>((resolve) => setTimeout(resolve, milliseconds));

const IDLE_PROGRESS: Progress = { phase: 'preparing', folders: 0, files: 0, foundBytes: 0, workDone: 0, workTotal: 0 };

/** Les étapes du grand vide, dans l'ordre où le cœur les mène. */
const STEPS = ['recycleBin', 'junkFiles', 'duplicates'] as const;
type Step = (typeof STEPS)[number];

/** Durées du grand vide simulé, en millisecondes à vitesse normale. */
const STEP_TIME: Record<Step, number> = { recycleBin: 900, junkFiles: 3000, duplicates: 2400 };
/** Début de l'étape « Fichiers inutiles » : ils sont recomptés, le volume de travail n'est pas encore connu. */
const RECOUNT_TIME = 600;
/** Entre la fin de la dernière étape (ou la demande d'arrêt) et le bilan. */
const WRAP_UP_TIME = 200;
/** Dans le scénario `halt`, la part des doublons déjà partie quand l'étape s'arrête. */
const HALT_AT = 0.4;

/** Une étape du grand vide simulé, sur la ligne du temps. */
interface Stage {
  id: Step;
  from: number;
  to: number;
}

/** Un grand vide simulé : ce qu'il a à faire, et où il en est. */
interface Cleaning {
  request: CleanRequest;
  began: number;
  stages: Stage[];
  /** Instant de la demande d'arrêt, sur la ligne du temps du grand vide. */
  stopAt: number | null;
  bin: RecycleBin;
  junk: { bytes: number; files: number; leftOpen: LeftOpen[]; skipped: number };
  duplicates: { bytes: number; copies: number; unrecyclable: number; halts: CleanStop | null };
  report: CleanReport | null;
}

/** Part d'une étape déjà faite à cet instant, ou `null` si elle n'a pas commencé. */
function fraction(run: Cleaning, id: Step, time: number): number | null {
  const stage = run.stages.find((entry) => entry.id === id);
  // Une étape qui n'avait pas commencé à la demande d'arrêt ne commence pas.
  const reached = Math.min(time, run.stopAt ?? Infinity);
  if (stage === undefined || reached < stage.from) {
    return null;
  }
  if (id === 'recycleBin') {
    // La corbeille se vide d'un bloc : commencée, elle va au bout.
    return time >= stage.to ? 1 : 0;
  }
  const lead = id === 'junkFiles' ? RECOUNT_TIME : 0;
  const whole = STEP_TIME[id] - lead;
  const done = Math.max(0, Math.min(reached, stage.to) - stage.from - lead) / whole;
  return Math.min(1, done);
}

/** Instant où le grand vide rend son bilan. */
function endOf(run: Cleaning): number {
  const last = run.stages[run.stages.length - 1]?.to ?? 0;
  return (run.stopAt === null ? last : Math.min(last, run.stopAt)) + WRAP_UP_TIME;
}

/** Ce que les essais changent aux listes de départ du mode jeu (les paramètres d'adresse n'y touchent pas). */
export interface GameSeed {
  games?: readonly string[];
  quietAdded?: readonly string[];
}

export function mockBackend(options: MockOptions, seed: GameSeed = {}): Backend {
  const demo = options.game;
  let settings: Settings = {
    version: 1,
    launchWithWindows: true,
    language: 'fr',
    theme: options.theme,
    autoScan: 'weekly',
    notifyFromGb: 5,
    shaderCacheLimitGb: 5,
    recycleBinAlert: true,
    recycleBinThresholdGb: 5,
    recycleBinAutoEmptyDays: null,
    alwaysAsk: true,
    useRecycleBin: true,
    excludedFolders:
      options.issue === 'unreadable' ? [] : ['C:\\Users\\Léa\\Documents\\Projets', 'D:\\Sauvegardes'],
    gameModeAuto: demo.state !== 'off',
    gameModePower: true,
    gameModeQuietApps: true,
    gameModeQuietAdded: [...(seed.quietAdded ?? ['qbittorrent.exe'])],
    gameModeQuietRemoved: [],
    gameModeGames: [...(seed.games ?? [MINECRAFT])],
    gameModeNotGames: [],
  };
  let bootEntries: StartupEntry[] = sortedBoot(
    (options.boot.data === 'few' ? FEW_BOOT : options.boot.data === 'many' ? manyBoot() : []).map((seed) =>
      bootEntry(
        seed.own === true && options.boot.shinku === 'off'
          ? { ...seed, state: 'disabled', cutDaysAgo: 2 }
          : seed,
        Date.now(),
      ),
    ),
  );
  const bootView = (): StartupView => ({
    entries: structuredClone(bootEntries),
    unreadable: options.boot.read === 'all' ? [] : [options.boot.read],
  });
  let issue: LoadIssue | null =
    options.issue === 'unreadable'
      ? { kind: 'unreadable', reason: 'EOF while parsing a value at line 4 column 0' }
      : options.issue === 'partial'
        ? { kind: 'partial', fields: ['theme'] }
        : null;
  let bin: RecycleBin = options.emptyBin ? { bytes: 0, items: 0 } : { bytes: 6.8 * GB, items: 1284 };
  let free = 312 * GB;
  let interrupted: CleanRun | null = options.interrupted
    ? {
        startedMs: Date.now() - 2 * 3600 * 1000,
        finishedMs: null,
        state: 'interrupted',
        freedBytes: 2.1 * GB,
        recoverableBytes: 300 * MB,
        skippedFiles: 3,
      }
    : null;
  // Ce qu'un grand vide a emporté : l'analyse suivante ne le retrouve pas.
  let junkGone = false;
  let duplicatesGone = false;
  /** Le nombre d'apps actives, lu à la fin de la dernière analyse. */
  let startupCount: number | null = null;
  let bootLoadFails = options.boot.load === 'fail';
  let cleaning: Cleaning | null = null;

  /** L'app que le scénario `rough` laisse ouverte : ses fichiers ne partent pas. */
  const openApps = (): LeftOpen[] => (options.clean === 'rough' ? [{ label: 'Discord', bytes: 388 * MB }] : []);

  /** Les fichiers inutiles que l'analyse trouve : tout, ou ce qu'un grand vide a laissé. */
  function junkFound(): JunkReport {
    if (!junkGone) {
      return structuredClone(JUNK);
    }
    const left = new Set(openApps().map((app) => app.label));
    const categories = JUNK.categories.map((entry) => {
      const lines = entry.lines.filter((line) => left.has(line.label));
      return {
        ...entry,
        lines,
        bytes: lines.reduce((total, line) => total + line.bytes, 0),
        files: lines.reduce((total, line) => total + line.files, 0),
      };
    });
    return {
      bytes: categories.reduce((total, entry) => total + entry.bytes, 0),
      files: categories.reduce((total, entry) => total + entry.files, 0),
      sparedBytes: JUNK.sparedBytes,
      sparedFiles: JUNK.sparedFiles,
      categories,
    };
  }

  // L'analyse simulée : `started` est l'instant du lancement, ou `null`.
  let started: number | null = null;
  const WALK = 2600;
  const QUICK = 1500;
  const FULL = 2800;
  const elapsed = () => (started === null ? 0 : (Date.now() - started) * options.speed);

  const blocked = () =>
    issue !== null && (issue.kind === 'unreadable' || issue.fields.includes('excludedFolders'));

  const overview = (): Overview => ({
    version: '0.1.0',
    volumes: [
      { root: 'C:\\', label: '', fileSystem: 'NTFS', total: 931.5 * GB, free, fixed: true, seekPenalty: false, system: true },
    ],
    recycleBin: { ...bin },
    settings: structuredClone(settings),
    settingsIssue: issue,
    scanBlocked: blocked(),
    interruptedClean: interrupted,
  });


  // ── Le mode jeu simulé ───────────────────────────────────────────────────
  // Les réglages vivent dans `settings`, comme dans le cœur ; l'agent est
  // joué par `session`, `last` et `seen`.

  const startedAt = Date.now();
  const MINUTE = 60_000;
  /** Les jeux reconnus par bibliothèque déjà joués, le plus récent d'abord. */
  let seen: Array<{ path: string; name: string; lastPlayed: number }> = [
    { path: HOLLOW_PEAKS, name: 'Hollow Peaks', lastPlayed: demo.state === 'off' || demo.state === 'idle' ? startedAt - 130 * MINUTE : startedAt - 38 * MINUTE },
  ];
  /** Les noms que la démo connaît pour les chemins que la personne a ajoutés. */
  const names = new Map<string, string>([
    [MINECRAFT.toLowerCase(), 'Minecraft'],
    [HOLLOW_PEAKS.toLowerCase(), 'Hollow Peaks'],
  ]);
  let last: LastGame | null = {
    name: 'Hollow Peaks',
    // Deux heures dix de jeu, finies il y a deux heures dix.
    started: startedAt - 260 * MINUTE,
    ended: startedAt - 130 * MINUTE,
    powerRestored: true,
    appsRestored: 2,
    complete: true,
  };
  let session: { path: string; name: string; since: number; released: boolean; restorePending: boolean } | null =
    demo.state === 'playing' || demo.state === 'released' || demo.state === 'pending'
      ? {
          path: HOLLOW_PEAKS,
          name: 'Hollow Peaks',
          since: startedAt - 38 * MINUTE,
          released: demo.state === 'released',
          restorePending: demo.state === 'pending',
        }
      : null;
  /** La dernière liste d'apps ouvertes rendue : les `id` ne valent que pour elle. */
  let offered: readonly OpenApp[] = [];
  /**
   * Ce que l'agent fera à son pas suivant : le cœur poste l'ordre et répond
   * tout de suite, la partie ne change qu'à la vue demandée ensuite.
   */
  let releaseAsked = false;
  let endAsked = false;

  const isRunning = (exe: string): boolean =>
    RUNNING_APPS.has(exe) || OPEN_PROGRAMS.some((program) => fileName(program.path) === exe);

  /**
   * Le libellé d'une app ajoutée : son nom d'affichage quand elle tourne, sinon
   * son nom de fichier sans « .exe » (comme le cœur ; la démo ne ferme jamais
   * une app : « déjà vue pendant cette ouverture » revient à « ouverte »).
   */
  const addedLabel = (exe: string): string => {
    const stem = exe.replace(/\.exe$/, '');
    return isRunning(exe) ? (DISPLAY_NAMES.get(exe) ?? stem) : stem;
  };

  const gameApps = (): QuietApp[] => [
    ...BUILT_IN_APPS.map(
      (app): QuietApp => ({
        exe: app.exe,
        label: app.label,
        on: settings.gameModeQuietAdded.includes(app.exe) || (app.on && !settings.gameModeQuietRemoved.includes(app.exe)),
        running: isRunning(app.exe),
        builtIn: true,
      }),
    ),
    ...settings.gameModeQuietAdded
      .filter((exe) => !BUILT_IN_APPS.some((app) => app.exe === exe))
      .map((exe): QuietApp => ({ exe, label: addedLabel(exe), on: true, running: isRunning(exe), builtIn: false })),
  ];

  const powerSupport = (): GameView['powerSupport'] =>
    demo.power === 'unavailable' || demo.power === 'unsuited' ? demo.power : 'available';

  /** Ce que l'agent a fait de l'alimentation pour la partie en cours. */
  function powerOutcome(): PowerOutcome {
    if (!settings.gameModePower) {
      return 'off';
    }
    if (demo.power === 'unavailable' || demo.power === 'unsuited') {
      return demo.power;
    }
    return demo.power === 'battery' ? 'onBattery' : 'applied';
  }

  const quietedCount = (): number =>
    settings.gameModeQuietApps ? gameApps().filter((app) => app.on && app.running).length : 0;

  function gameSession(): GameSession | null {
    if (session === null) {
      return null;
    }
    const released = session.released;
    return {
      name: session.name,
      path: session.path,
      since: session.since,
      source: settings.gameModeGames.some((path) => samePath(path, session?.path ?? '')) ? 'added' : 'library',
      power: released ? 'off' : powerOutcome(),
      quietedApps: released ? 0 : quietedCount(),
      released,
      restorePending: session.restorePending,
    };
  }

  /** Les jeux connus : ajoutés (le dernier ajouté d'abord, comme le cœur), puis vus, sans ceux que la personne a écartés. */
  function gameList(): KnownGame[] {
    const added = settings.gameModeGames.toReversed().map(
      (path): KnownGame => ({ path, name: names.get(path.toLowerCase()) ?? fileName(path), source: 'added', lastPlayed: null }),
    );
    const known = seen
      .filter((entry) => ![...settings.gameModeGames, ...settings.gameModeNotGames].some((path) => samePath(path, entry.path)))
      .map((entry): KnownGame => ({ path: entry.path, name: entry.name, source: 'library', lastPlayed: entry.lastPlayed }));
    return [...added, ...known];
  }

  function gameView(): GameView {
    const auto = settings.gameModeAuto;
    return {
      auto,
      power: settings.gameModePower,
      quietApps: settings.gameModeQuietApps,
      agent: demo.agent === 'running' ? (auto || session !== null ? 'running' : 'stopped') : demo.agent,
      startupDisabled: demo.startup === 'disabled',
      session: gameSession(),
      last,
      powerSupport: powerSupport(),
      onBattery: demo.power === 'battery',
      apps: gameApps(),
      games: gameList(),
      notGames: settings.gameModeNotGames.map((path) => ({ path, name: names.get(path.toLowerCase()) ?? fileName(path) })),
    };
  }

  /** Le pas suivant de l'agent : il exécute les ordres postés depuis la dernière vue. */
  function agentStep(): void {
    if (session !== null) {
      if (endAsked) {
        endSession();
      } else if (releaseAsked) {
        session = { ...session, released: true, restorePending: false };
      }
    }
    releaseAsked = false;
    endAsked = false;
  }

  /** Fin de partie : tout est remis en place, le bilan est noté. */
  function endSession(): void {
    if (session === null) {
      return;
    }
    const outcome = powerOutcome();
    last = {
      name: session.name,
      started: session.since,
      ended: Date.now(),
      powerRestored: outcome === 'applied' && !session.released ? true : null,
      appsRestored: session.released ? 0 : quietedCount(),
      complete: true,
    };
    const known = seen.filter((entry) => !samePath(entry.path, session?.path ?? ''));
    seen = [{ path: session.path, name: session.name, lastPlayed: Date.now() }, ...known];
    session = null;
  }

  /** Les chemins que le cœur connaît : c'est tout ce qu'`set_game_status` accepte. */
  const knownGamePath = (path: string): string | undefined =>
    [...settings.gameModeGames, ...settings.gameModeNotGames, ...seen.map((entry) => entry.path), ...(session === null ? [] : [session.path])].find(
      (known) => samePath(known, path),
    );

  const without = (list: readonly string[], path: string): string[] => list.filter((other) => !samePath(other, path));

  // Les doublons de l'analyse en cours, avec les exemplaires que la personne a choisi de garder.
  let found: DuplicateGroup[] | null = null;
  const groups = (): DuplicateGroup[] => (duplicatesGone ? [] : (found ??= duplicateGroups(Date.now())));
  const duplicateBytes = () => groups().reduce((total, group) => total + group.reclaimable, 0);
  const duplicateCopies = () => groups().reduce((total, group) => total + group.files.length - 1, 0);

  function summary(complete: boolean, begun: number, finished: number): Summary {
    const large = largeFiles(finished);
    const junk = junkFound();
    const categories: CategorySummary[] = [
      { module: 'recycleBin', lot: 'optional', risk: 'D', bytes: bin.bytes, items: bin.items, state: 'ready' },
      { module: 'junkFiles', lot: 'auto', risk: 'A', bytes: junk.bytes, items: junk.files, state: 'ready' },
      {
        module: 'duplicates',
        lot: 'optional',
        risk: 'B',
        bytes: complete ? duplicateBytes() : 0,
        items: complete ? duplicateCopies() : 0,
        state: complete ? 'ready' : 'pending',
      },
      {
        module: 'largeFiles',
        lot: 'review',
        risk: 'B',
        bytes: large.reduce((total, file) => total + file.onDisk, 0),
        items: large.length,
        state: 'ready',
      },
      {
        module: 'startup',
        lot: 'review',
        risk: 'C',
        bytes: 0,
        // Lu une fois, à la fin de l'analyse : couper une app ensuite ne change pas ce compte.
        items: complete ? (startupCount ??= bootEntries.filter((entry) => entry.state === 'enabled' && !entry.own).length) : 0,
        // Une place illisible : le cœur ne mesure pas le démarrage.
        state: options.boot.read !== 'all' ? 'unavailable' : complete ? 'ready' : 'pending',
      },
    ];
    const autoBytes = categories
      .filter((entry) => entry.lot === 'auto')
      .reduce((total, entry) => total + entry.bytes, 0);
    return { complete, started: begun, finished, autoBytes, categories };
  }

  // ── Le grand vide simulé ─────────────────────────────────────────────────

  const cleanTime = (run: Cleaning) => (Date.now() - run.began) * options.speed;

  /** Ce qu'une étape est devenue, d'après la part qu'elle a faite. */
  function stepState(run: Cleaning, id: Step, done: number | null): StepState {
    if (!run.request[id]) {
      return 'notRequested';
    }
    return done !== null && done >= 1 ? 'done' : 'stopped';
  }

  /** Dresse le bilan d'un grand vide arrivé au bout, et en applique les effets au faux PC. */
  function conclude(run: Cleaning): CleanReport {
    const end = endOf(run);
    const part = (id: Step) => fraction(run, id, end);
    const [binPart, junkPart, copiesPart] = [part('recycleBin'), part('junkFiles'), part('duplicates')];

    const emptied = binPart !== null && binPart >= 1;
    const halt = run.duplicates.halts;
    const halted = halt !== null && copiesPart !== null && copiesPart >= HALT_AT;
    const copiesDone = halted ? HALT_AT : (copiesPart ?? 0);
    const junk = {
      state: stepState(run, 'junkFiles', junkPart),
      bytes: Math.round(run.junk.bytes * (junkPart ?? 0)),
      files: Math.round(run.junk.files * (junkPart ?? 0)),
      leftOpen: junkPart === null ? [] : run.junk.leftOpen,
    };
    const duplicates = {
      state: halted ? ('stopped' as const) : stepState(run, 'duplicates', copiesPart),
      bytes: Math.round(run.duplicates.bytes * copiesDone),
      copies: Math.round(run.duplicates.copies * copiesDone),
      stop: halted ? halt : null,
    };
    const recycleBin = {
      state: stepState(run, 'recycleBin', binPart),
      bytes: emptied ? run.bin.bytes : 0,
      items: emptied ? run.bin.items : 0,
    };
    // Sans l'avoir vidée d'abord, la corbeille du scénario `rough` était pleine.
    const kept = options.clean === 'rough' && !run.request.recycleBin ? 0.8 : 1;
    const cancelled =
      recycleBin.state === 'stopped' || junk.state === 'stopped' || (duplicates.state === 'stopped' && !halted);
    const report: CleanReport = {
      started: run.began,
      finished: Date.now(),
      outcome: cancelled ? 'cancelled' : 'completed',
      freedBytes: recycleBin.bytes + junk.bytes,
      recoverableBytes: Math.round(duplicates.bytes * kept),
      recycleBin,
      junk,
      duplicates,
      skipped: {
        inUse: Math.round(run.junk.skipped * 0.6 * (junkPart ?? 0)),
        changed: Math.round(run.junk.skipped * 0.4 * (junkPart ?? 0)),
        denied: 0,
        unrecyclable: copiesDone >= 1 ? run.duplicates.unrecyclable : 0,
        other: 0,
      },
      durationMs: Date.now() - run.began,
    };

    if (emptied) {
      bin = { bytes: 0, items: 0 };
    }
    bin = { bytes: bin.bytes + report.recoverableBytes, items: bin.items + duplicates.copies };
    free += report.freedBytes;
    junkGone ||= junk.state === 'done';
    duplicatesGone ||= duplicates.state === 'done';
    return report;
  }

  /** Conclut le grand vide simulé dès que son temps est écoulé. */
  function settle(): void {
    const run = cleaning;
    if (run !== null && run.report === null && cleanTime(run) >= endOf(run)) {
      run.report = conclude(run);
    }
  }

  /** Vrai tant qu'un grand vide simulé n'a pas rendu son bilan. */
  function cleanRunning(): boolean {
    settle();
    return cleaning !== null && cleaning.report === null;
  }

  function cleanProgress(run: Cleaning): CleanProgress {
    const time = Math.min(cleanTime(run), endOf(run));
    const part = (id: Step) => fraction(run, id, time) ?? 0;
    const halted = run.duplicates.halts !== null;
    const copiesPart = halted ? Math.min(HALT_AT, part('duplicates')) : part('duplicates');
    const freedBytes = Math.round(run.bin.bytes * part('recycleBin') + run.junk.bytes * part('junkFiles'));
    const recycledBytes = Math.round(run.duplicates.bytes * copiesPart);

    let phase: CleanPhase = 'preparing';
    if (run.report !== null) {
      phase = 'done';
    } else {
      // L'étape en cours ; après une demande d'arrêt, celle où il s'est arrêté.
      const reached = Math.min(time, run.stopAt ?? Infinity);
      for (const stage of run.stages) {
        if (reached >= stage.from) {
          phase = stage.id;
        }
      }
    }
    const work =
      phase === 'junkFiles' && part('junkFiles') > 0
        ? { workDone: Math.round(run.junk.bytes * part('junkFiles')), workTotal: run.junk.bytes }
        : phase === 'duplicates'
          ? { workDone: Math.round(run.duplicates.bytes * part('duplicates')), workTotal: run.duplicates.bytes }
          : { workDone: 0, workTotal: 0 };
    return {
      phase,
      freedBytes,
      recycledBytes,
      files: Math.round(run.junk.files * part('junkFiles') + run.duplicates.copies * copiesPart),
      skippedFiles: Math.round(run.junk.skipped * part('junkFiles')),
      ...work,
    };
  }

  // ── La session ───────────────────────────────────────────────────────────

  function view(): SessionView {
    settle();
    if (cleaning !== null) {
      return {
        state: cleaning.report === null ? 'cleaning' : 'cleaned',
        progress: IDLE_PROGRESS,
        summary: null,
        skipped: { folders: 0, refusedRoots: 0 },
        stale: false,
        clean: { request: cleaning.request, progress: cleanProgress(cleaning), report: cleaning.report },
      };
    }
    if (started === null) {
      return {
        state: 'idle',
        progress: IDLE_PROGRESS,
        summary: null,
        skipped: { folders: 0, refusedRoots: 0 },
        stale: false,
        clean: null,
      };
    }
    const time = elapsed();
    const walked = Math.min(1, time / WALK);
    const base = {
      folders: Math.round(27_320 * walked),
      files: Math.round(196_295 * walked),
      foundBytes: Math.round(junkFound().bytes * walked),
    };
    let phase: Phase = 'walking';
    let workDone = 0;
    let workTotal = 0;
    if (time >= WALK + QUICK + FULL) {
      phase = 'done';
    } else if (time >= WALK + QUICK) {
      phase = 'duplicatesFull';
      workTotal = 2.4 * GB;
      workDone = Math.round(workTotal * ((time - WALK - QUICK) / FULL));
    } else if (time >= WALK) {
      phase = 'duplicatesQuick';
      workTotal = 640 * MB;
      workDone = Math.round(workTotal * ((time - WALK) / QUICK));
    }
    const complete = phase === 'done';
    const finished = started + (complete ? WALK + QUICK + FULL : WALK) / options.speed;
    return {
      state: complete ? 'results' : 'scanning',
      progress: { phase, ...base, workDone, workTotal },
      summary: time >= WALK ? summary(complete, started, finished) : null,
      skipped: { folders: time >= WALK ? 3 : 0, refusedRoots: 0 },
      stale: Date.now() - finished > 10 * 60 * 1000,
      clean: null,
    };
  }

  const hasReport = () => view().summary !== null;
  const refuse = (error: BackendError): Promise<never> => Promise.reject(error);

  /** Les résultats sur lesquels un grand vide peut s'appuyer, ou la raison du refus : comme le cœur. */
  function cleanable(): Summary | Extract<BackendError, { kind: 'cleanRefused' }> {
    const current = view();
    if (current.state !== 'results' || current.summary === null || !current.summary.complete) {
      return { kind: 'cleanRefused', reason: 'noResults' };
    }
    return current.stale ? { kind: 'cleanRefused', reason: 'staleResults' } : current.summary;
  }

  return {
    overview: async () => {
      await delay(120);
      return overview();
    },
    acknowledgeSettingsIssue: async () => {
      issue = null;
      return overview();
    },
    startScan: async () => {
      if (blocked()) {
        return refuse({ kind: 'settingsUnchecked' });
      }
      if (cleanRunning()) {
        return refuse({ kind: 'cleanRunning' });
      }
      cleaning = null;
      interrupted = null;
      found = null;
      startupCount = null;
      started = Date.now();
      return view();
    },
    cancelScan: async () => {
      // Sans effet sur un grand vide en cours ; referme le bilan d'un grand vide terminé.
      if (!cleanRunning()) {
        cleaning = null;
        started = null;
      }
      return view();
    },
    sessionView: async () => view(),
    cleanPreview: async () => {
      const summary = cleanable();
      if ('kind' in summary) {
        return refuse(summary);
      }
      const junk = junkFound();
      const copies = groups().flatMap((group) => group.files.filter((file) => !file.keep));
      return {
        analysis: summary.started,
        recycleBin: { ...bin },
        junk: { bytes: junk.bytes, files: junk.files, openApps: junkGone ? [] : openApps() },
        duplicates: {
          bytes: duplicateBytes(),
          copies: copies.length,
          syncedCopies: copies.filter((file) => file.synced).length,
          otherDiskCopies: options.clean === 'rough' && copies.length > 0 ? 1 : 0,
        },
      };
    },
    startClean: async (request) => {
      if (blocked()) {
        return refuse({ kind: 'settingsUnchecked' });
      }
      if (cleanRunning()) {
        return refuse({ kind: 'cleanRunning' });
      }
      const summary = cleanable();
      if ('kind' in summary && summary.reason === 'noResults') {
        return refuse(summary);
      }
      if (!('kind' in summary) && summary.started !== request.analysis) {
        return refuse({ kind: 'cleanRefused', reason: 'otherAnalysis' });
      }
      if ('kind' in summary) {
        return refuse(summary);
      }
      const junk = junkFound();
      const wanted: CleanRequest = {
        analysis: request.analysis,
        recycleBin: request.recycleBin && bin.items > 0,
        junkFiles: request.junkFiles && junk.files > 0,
        duplicates: request.duplicates && duplicateCopies() > 0,
      };
      if (!wanted.recycleBin && !wanted.junkFiles && !wanted.duplicates) {
        return refuse({ kind: 'cleanRefused', reason: 'nothingToClean' });
      }

      // Les étapes demandées s'enchaînent sur la ligne du temps.
      const stages: Stage[] = [];
      let clock = 100;
      for (const id of STEPS) {
        if (wanted[id]) {
          stages.push({ id, from: clock, to: clock + STEP_TIME[id] });
          clock += STEP_TIME[id];
        }
      }
      const leftOpen = junkGone ? [] : openApps();
      const kept = groups()
        .find((group) => group.files.length > 2)
        ?.files.find((file) => file.keep);
      cleaning = {
        request,
        began: Date.now(),
        stages,
        stopAt: null,
        bin: { ...bin },
        junk: {
          bytes: junk.bytes - leftOpen.reduce((total, app) => total + app.bytes, 0),
          files: junk.files - (leftOpen.length > 0 ? 2140 : 0),
          leftOpen,
          skipped: options.clean === 'rough' ? 15 : 0,
        },
        duplicates: {
          bytes: duplicateBytes(),
          copies: duplicateCopies(),
          unrecyclable: options.clean === 'rough' ? 1 : 0,
          halts:
            options.clean === 'halt' && kept !== undefined ? { reason: 'keptCopyChanged', file: kept.path } : null,
        },
        report: null,
      };
      // Dès que le grand vide démarre, les résultats de l'analyse sont oubliés.
      started = null;
      return view();
    },
    cancelClean: async () => {
      if (cleanRunning() && cleaning !== null) {
        cleaning.stopAt ??= cleanTime(cleaning);
      }
      return view();
    },
    junk: async () => (hasReport() ? junkFound() : null),
    largeFiles: async (offset, limit): Promise<LargeFilesPage | null> => {
      if (!hasReport()) {
        return null;
      }
      const files = largeFiles(Date.now());
      return {
        threshold: 3 * GB,
        count: files.length,
        bytes: files.reduce((total, file) => total + file.onDisk, 0),
        listed: files.length,
        offset: Math.min(offset, files.length),
        files: files.slice(offset, offset + limit),
      };
    },
    duplicates: async (offset, limit): Promise<DuplicatesPage | null> => {
      const current = view();
      if (current.summary === null) {
        return null;
      }
      const all = current.summary.complete ? groups() : [];
      await delay(offset > 0 ? 300 : 0);
      return {
        state: current.summary.complete ? 'done' : 'pending',
        candidates: 4210,
        copies: current.summary.complete ? duplicateCopies() : 0,
        bytes: current.summary.complete ? duplicateBytes() : 0,
        skippedFiles: current.summary.complete ? 2 : 0,
        hardLinks: 0,
        groupCount: all.length,
        offset: Math.min(offset, all.length),
        groups: all.slice(offset, offset + limit),
      };
    },
    keepDuplicate: async (choice) => {
      const known = groups();
      const group = known.find((other) => other.digest === choice.digest && other.size === choice.size);
      const current = view();
      if (current.state !== 'results' || current.stale || group === undefined || choice.index >= group.files.length) {
        return false;
      }
      found = keepIn(known, choice);
      return true;
    },
    setTheme: async (theme) => {
      settings = { ...settings, theme };
      return structuredClone(settings);
    },
    addExcludedFolder: async () => {
      if (cleanRunning()) {
        return refuse({ kind: 'cleanRunning' });
      }
      await delay(250);
      const extra = 'D:\\Jeux\\Captures';
      if (!settings.excludedFolders.includes(extra)) {
        settings = { ...settings, excludedFolders: [...settings.excludedFolders, extra] };
      }
      started = null;
      return structuredClone(settings);
    },
    removeExcludedFolder: async (path) => {
      if (cleanRunning()) {
        return refuse({ kind: 'cleanRunning' });
      }
      settings = { ...settings, excludedFolders: settings.excludedFolders.filter((other) => other !== path) };
      started = null;
      return structuredClone(settings);
    },
    gameView: async () => {
      agentStep();
      return gameView();
    },
    setGameOption: async (option: GameOption, on) => {
      switch (option) {
        case 'auto':
          settings = { ...settings, gameModeAuto: on };
          if (!on) {
            // L'agent voit le réglage éteint : la partie se termine, tout est remis.
            endSession();
          }
          break;
        case 'power':
          settings = { ...settings, gameModePower: on };
          break;
        case 'quietApps':
          settings = { ...settings, gameModeQuietApps: on };
          break;
      }
      return gameView();
    },
    setQuietApp: async (exe, on) => {
      const known = gameApps().find((app) => app.exe === exe);
      if (known === undefined) {
        return gameRefused('unknownApp');
      }
      const added = settings.gameModeQuietAdded;
      if (on) {
        // Cochée d'office : on la retire des refusées ; sinon on l'ajoute aux choisies.
        const builtIn = BUILT_IN_APPS.find((app) => app.exe === exe);
        settings =
          builtIn?.on === true
            ? { ...settings, gameModeQuietRemoved: settings.gameModeQuietRemoved.filter((other) => other !== exe) }
            : { ...settings, gameModeQuietAdded: added.includes(exe) ? added : [...added, exe] };
      } else {
        const builtIn = BUILT_IN_APPS.find((app) => app.exe === exe);
        settings = {
          ...settings,
          gameModeQuietAdded: added.filter((other) => other !== exe),
          gameModeQuietRemoved:
            builtIn?.on === true && !settings.gameModeQuietRemoved.includes(exe)
              ? [...settings.gameModeQuietRemoved, exe]
              : settings.gameModeQuietRemoved,
        };
      }
      return gameView();
    },
    openApps: async () => {
      await delay(250);
      // Le cœur ne propose pas les noms protégés.
      offered = OPEN_PROGRAMS.filter((program) => !PROTECTED_EXES.has(fileName(program.path))).map(
        (program, index): OpenApp => ({ id: index + 1, ...program }),
      );
      return offered.map((app) => ({ ...app }));
    },
    addGame: async (id, path) => {
      const app = offered.find((entry) => entry.id === id);
      // Le chemin n'est que comparé, exactement (casse comprise, comme le cœur) : il doit être celui du numéro, dans la dernière liste du cœur.
      if (app === undefined || app.path !== path) {
        return gameRefused('unknownApp');
      }
      if (NEVER_GAMES.has(fileName(app.path))) {
        return gameRefused('notAGame');
      }
      if (!settings.gameModeGames.some((other) => samePath(other, app.path)) && settings.gameModeGames.length >= GAME_LIST_LIMIT) {
        return gameRefused('listFull');
      }
      names.set(app.path.toLowerCase(), app.name);
      settings = {
        ...settings,
        gameModeGames: [...without(settings.gameModeGames, app.path), app.path],
        gameModeNotGames: without(settings.gameModeNotGames, app.path),
      };
      return gameView();
    },
    addQuietApp: async (id, path) => {
      const app = offered.find((entry) => entry.id === id);
      if (app === undefined || app.path !== path) {
        return gameRefused('unknownApp');
      }
      const exe = fileName(app.path);
      if (PROTECTED_EXES.has(exe) || LAUNCHER_EXES.has(exe)) {
        return gameRefused('protectedApp');
      }
      const builtIn = BUILT_IN_APPS.find((entry) => entry.exe === exe);
      if (builtIn?.on === true) {
        settings = { ...settings, gameModeQuietRemoved: settings.gameModeQuietRemoved.filter((other) => other !== exe) };
      } else if (!settings.gameModeQuietAdded.includes(exe)) {
        if (settings.gameModeQuietAdded.length >= QUIET_LIST_LIMIT) {
          return gameRefused('listFull');
        }
        settings = { ...settings, gameModeQuietAdded: [...settings.gameModeQuietAdded, exe] };
      }
      return gameView();
    },
    setGameStatus: async (path: string, status: GameStatus) => {
      const known = knownGamePath(path);
      if (known === undefined) {
        return gameRefused('unknownGame');
      }
      if (status === 'notGame') {
        settings = {
          ...settings,
          gameModeGames: without(settings.gameModeGames, known),
          gameModeNotGames: [...without(settings.gameModeNotGames, known), known],
        };
        if (session !== null && samePath(session.path, known)) {
          // Ce n'est plus un jeu : l'agent finira la partie à son pas suivant.
          endAsked = true;
        }
      } else {
        settings = {
          ...settings,
          gameModeGames: without(settings.gameModeGames, known),
          gameModeNotGames: without(settings.gameModeNotGames, known),
        };
      }
      return gameView();
    },
    releaseGame: async () => {
      if (session !== null) {
        // L'agent remettra tout en place à son pas suivant ; la partie continue sans rien appliquer.
        releaseAsked = true;
      }
      return gameView();
    },
    startupView: async () => {
      if (bootLoadFails) {
        bootLoadFails = false;
        throw new Error('démo : la première lecture échoue');
      }
      return bootView();
    },
    setStartupEntry: async (id, enabled) => {
      // L'ordre du cœur : l'entrée inconnue, puis celle qu'on ne règle pas, puis l'échec.
      const entry = bootEntries.find((other) => other.id === id);
      if (entry === undefined) {
        return startupRefused('unknownEntry');
      }
      if (!entry.adjustable) {
        return startupRefused('notAdjustable');
      }
      if (options.boot.fail) {
        return startupRefused('failed');
      }
      if ((entry.state === 'enabled') !== enabled) {
        const now = Date.now();
        bootEntries = bootEntries.map(
          (other): StartupEntry =>
            other.id !== id
              ? other
              : enabled
                ? { ...other, state: 'enabled', disabledAt: null, mark: null }
                : { ...other, state: 'disabled', disabledAt: now, mark: { kind: 'cutByShinku', at: now } },
        );
      }
      return bootView();
    },
    ...appsBackend(options.apps),
    openWindowsSettings: async () => {},
    minimizeWindow: async () => {},
    toggleMaximizeWindow: async () => {},
    closeWindow: async () => {},
  };
}
