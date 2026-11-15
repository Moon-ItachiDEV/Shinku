// Un faux guichet, pour voir l'interface dans un navigateur sans Windows ni
// Tauri : `npm run demo`. Il n'est jamais embarqué dans l'application.
//
// Les chiffres sont ceux du canevas de design. L'analyse est une simple
// animation : rien n'est lu sur le disque.

import type {
  Backend,
  BackendError,
  CategorySummary,
  DuplicateGroup,
  DuplicatesPage,
  JunkReport,
  LargeFile,
  LargeFilesPage,
  LoadIssue,
  Overview,
  Phase,
  SessionView,
  Settings,
  Summary,
  Theme,
} from './types.ts';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const DAY = 24 * 3600 * 1000;

export interface MockOptions {
  /** Accélère l'analyse simulée (1 : durée normale, environ 7 secondes). */
  speed: number;
  /** Simule un fichier de réglages abîmé au démarrage. */
  issue: 'none' | 'unreadable' | 'partial';
  /** Corbeille vide plutôt que pleine. */
  emptyBin: boolean;
  /** Mode d'affichage enregistré dans les faux réglages. */
  theme: Theme;
}

/**
 * Lit les options de la démo dans l'adresse de la page :
 * `?speed=4&theme=light&issue=unreadable&bin=empty`.
 */
export function demoOptions(search: string): MockOptions {
  const query = new URLSearchParams(search);
  const speed = Number(query.get('speed') ?? '1');
  const issue = query.get('issue');
  const theme = query.get('theme');
  return {
    speed: Number.isFinite(speed) && speed > 0 ? speed : 1,
    issue: issue === 'unreadable' || issue === 'partial' ? issue : 'none',
    emptyBin: query.get('bin') === 'empty',
    theme: theme === 'light' || theme === 'dark' ? theme : 'system',
  };
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

export function mockBackend(options: MockOptions): Backend {
  let settings: Settings = {
    version: 1,
    launchWithWindows: true,
    language: 'fr',
    theme: options.theme,
    autoScan: 'weekly',
    notifyFromGb: 5,
    gameModeOnLaunch: true,
    shaderCacheLimitGb: 5,
    recycleBinAlert: true,
    recycleBinThresholdGb: 5,
    recycleBinAutoEmptyDays: null,
    alwaysAsk: true,
    useRecycleBin: true,
    excludedFolders:
      options.issue === 'unreadable' ? [] : ['C:\\Users\\Léa\\Documents\\Projets', 'D:\\Sauvegardes'],
  };
  let issue: LoadIssue | null =
    options.issue === 'unreadable'
      ? { kind: 'unreadable', reason: 'EOF while parsing a value at line 4 column 0' }
      : options.issue === 'partial'
        ? { kind: 'partial', fields: ['theme'] }
        : null;
  const bin = options.emptyBin ? { bytes: 0, items: 0 } : { bytes: 6.8 * GB, items: 1284 };

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
      { root: 'C:\\', label: '', fileSystem: 'NTFS', total: 931.5 * GB, free: 312 * GB, fixed: true, seekPenalty: false, system: true },
    ],
    recycleBin: bin,
    settings: structuredClone(settings),
    settingsIssue: issue,
    scanBlocked: blocked(),
  });

  const groups = () => duplicateGroups(Date.now());
  const duplicateBytes = () => groups().reduce((total, group) => total + group.reclaimable, 0);
  const duplicateCopies = () => groups().reduce((total, group) => total + group.files.length - 1, 0);

  function summary(complete: boolean, finished: number): Summary {
    const large = largeFiles(finished);
    const categories: CategorySummary[] = [
      { module: 'recycleBin', lot: 'auto', risk: 'D', bytes: bin.bytes, items: bin.items, state: 'ready' },
      { module: 'junkFiles', lot: 'auto', risk: 'A', bytes: JUNK.bytes, items: JUNK.files, state: 'ready' },
      {
        module: 'duplicates',
        lot: 'auto',
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
    return { complete, finished, autoBytes, categories };
  }

  function view(): SessionView {
    if (started === null) {
      return {
        state: 'idle',
        progress: { phase: 'preparing', folders: 0, files: 0, foundBytes: 0, workDone: 0, workTotal: 0 },
        summary: null,
        skipped: { folders: 0, refusedRoots: 0 },
        stale: false,
      };
    }
    const time = elapsed();
    const walked = Math.min(1, time / WALK);
    const base = {
      folders: Math.round(27_320 * walked),
      files: Math.round(196_295 * walked),
      foundBytes: Math.round(JUNK.bytes * walked),
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
      summary: time >= WALK ? summary(complete, finished) : null,
      skipped: { folders: time >= WALK ? 3 : 0, refusedRoots: 0 },
      stale: Date.now() - finished > 10 * 60 * 1000,
    };
  }

  const hasReport = () => view().summary !== null;
  const refuse = (error: BackendError): Promise<never> => Promise.reject(error);

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
      started = Date.now();
      return view();
    },
    cancelScan: async () => {
      started = null;
      return view();
    },
    sessionView: async () => view(),
    junk: async () => (hasReport() ? structuredClone(JUNK) : null),
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
      await delay(250);
      const extra = 'D:\\Jeux\\Captures';
      if (!settings.excludedFolders.includes(extra)) {
        settings = { ...settings, excludedFolders: [...settings.excludedFolders, extra] };
      }
      started = null;
      return structuredClone(settings);
    },
    removeExcludedFolder: async (path) => {
      settings = { ...settings, excludedFolders: settings.excludedFolders.filter((other) => other !== path) };
      started = null;
      return structuredClone(settings);
    },
    minimizeWindow: async () => {},
    toggleMaximizeWindow: async () => {},
    closeWindow: async () => {},
  };
}
