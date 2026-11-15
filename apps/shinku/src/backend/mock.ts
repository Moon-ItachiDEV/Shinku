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
  StepState,
  Summary,
  Theme,
} from './types.ts';

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
}

/**
 * Lit les options de la démo dans l'adresse de la page :
 * `?speed=4&theme=light&issue=unreadable&bin=empty&clean=rough&interrupted=1`.
 * Le mode jeu : `?game=off|idle|playing|released|pending`, `?agent=missing|stopped`,
 * `?power=unavailable|unsuited|battery`, `?startup=disabled`.
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

/** Les apps qui tournent dans la démo. */
const RUNNING_APPS: ReadonlySet<string> = new Set(['onedrive.exe', 'chrome.exe']);

/** Ce que le sélecteur propose : quatre apps fictives, dont un lanceur que le cœur refuse comme jeu. */
const OPEN_APPS: readonly OpenApp[] = [
  { id: 1, name: 'Roblox', path: 'C:\\Users\\Léa\\AppData\\Local\\Roblox\\Versions\\version-1a2b3c4d\\RobloxPlayerBeta.exe' },
  { id: 2, name: 'Notepad++', path: 'C:\\Program Files\\Notepad++\\notepad++.exe' },
  { id: 3, name: 'VLC media player', path: 'C:\\Program Files\\VideoLAN\\VLC\\vlc.exe' },
  { id: 4, name: 'Battle.net', path: 'C:\\Program Files (x86)\\Battle.net\\Battle.net.exe' },
];

/** Les programmes que le cœur ne laisse jamais ajouter comme jeu. */
const NEVER_GAMES: ReadonlySet<string> = new Set(['vlc.exe', 'battle.net.exe']);

const HOLLOW_PEAKS = 'D:\\SteamLibrary\\steamapps\\common\\Hollow Peaks\\hp.exe';
const MINECRAFT = 'C:\\Program Files\\Eclipse Adoptium\\jdk-17.0.9.9-hotspot\\bin\\javaw.exe';

const fileName = (path: string): string => path.slice(path.lastIndexOf('\\') + 1).toLowerCase();
const samePath = (left: string, right: string): boolean => left.toLowerCase() === right.toLowerCase();
const gameRefused = (reason: GameRefusal): Promise<never> => Promise.reject({ kind: 'gameRefused', reason });

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
  const group = (name: string, size: number, folders: string[], synced: boolean[] = []): DuplicateGroup => ({
    size,
    digest: 'af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262',
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

export function mockBackend(options: MockOptions): Backend {
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
    gameModeQuietAdded: ['qbittorrent.exe'],
    gameModeQuietRemoved: [],
    gameModeGames: [MINECRAFT],
    gameModeNotGames: [],
  };
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
  const labels = new Map<string, string>([['qbittorrent.exe', 'qBittorrent']]);
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

  const gameApps = (): QuietApp[] => [
    ...BUILT_IN_APPS.map(
      (app): QuietApp => ({
        exe: app.exe,
        label: app.label,
        on: settings.gameModeQuietAdded.includes(app.exe) || (app.on && !settings.gameModeQuietRemoved.includes(app.exe)),
        running: RUNNING_APPS.has(app.exe),
        builtIn: true,
      }),
    ),
    ...settings.gameModeQuietAdded
      .filter((exe) => !BUILT_IN_APPS.some((app) => app.exe === exe))
      .map((exe): QuietApp => ({ exe, label: labels.get(exe) ?? exe, on: true, running: RUNNING_APPS.has(exe), builtIn: false })),
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

  /** Les jeux connus : ajoutés d'abord, puis vus, sans ceux que la personne a écartés. */
  function gameList(): KnownGame[] {
    const added = settings.gameModeGames.map(
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

  const groups = () => (duplicatesGone ? [] : duplicateGroups(Date.now()));
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
      { module: 'startup', lot: 'review', risk: 'C', bytes: 0, items: 0, state: 'unavailable' },
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
      const kept = groups().find((group) => group.files.length > 2)?.files[0];
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
    gameView: async () => gameView(),
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
      offered = OPEN_APPS;
      return OPEN_APPS.map((app) => ({ ...app }));
    },
    addGame: async (id) => {
      const app = offered.find((entry) => entry.id === id);
      if (app === undefined) {
        return gameRefused('unknownApp');
      }
      if (NEVER_GAMES.has(fileName(app.path))) {
        return gameRefused('notAGame');
      }
      names.set(app.path.toLowerCase(), app.name);
      settings = {
        ...settings,
        gameModeGames: [...without(settings.gameModeGames, app.path), app.path],
        gameModeNotGames: without(settings.gameModeNotGames, app.path),
      };
      return gameView();
    },
    addQuietApp: async (id) => {
      const app = offered.find((entry) => entry.id === id);
      if (app === undefined) {
        return gameRefused('unknownApp');
      }
      const exe = fileName(app.path);
      if (!gameApps().some((known) => known.exe === exe)) {
        labels.set(exe, app.name);
      }
      const builtIn = BUILT_IN_APPS.find((entry) => entry.exe === exe);
      if (builtIn?.on === true) {
        settings = { ...settings, gameModeQuietRemoved: settings.gameModeQuietRemoved.filter((other) => other !== exe) };
      } else if (!settings.gameModeQuietAdded.includes(exe)) {
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
          // Ce n'est plus un jeu : la partie se termine, tout est remis.
          endSession();
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
        // Tout est remis en place ; la partie continue sans rien appliquer.
        session = { ...session, released: true, restorePending: false };
      }
      return gameView();
    },
    openWindowsSettings: async () => {},
    minimizeWindow: async () => {},
    toggleMaximizeWindow: async () => {},
    closeWindow: async () => {},
  };
}
