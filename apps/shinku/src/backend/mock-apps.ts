// La désinstallation de la démo : un faux cœur qui suit un vrai déroulement
// (le désinstalleur s'ouvre, l'app disparaît, des restes sont proposés) sans
// rien lire ni rien supprimer.
//
// Paramètres d'adresse (voir `appsDemoOptions`) :
//   ?apps=few|many|empty|partial   les apps montrées (partial : une source illisible)
//   ?appsload=fail                 la première lecture de la vue échoue ; « Réessayer » la réussit
//   ?appsflow=launching|waiting|stuck|leftovers|none|family|unsure
//                                  la page s'ouvre dans cette phase ; « Désinstaller » finit sur le même bilan
//   ?appsfail=broken|cancelled|bin|kept
//                                  « Désinstaller » est refusé (broken, cancelled), la corbeille refuse
//                                  (bin), ou l'installeur sort sans retirer l'app (kept)

import type {
  AppRow,
  AppsView,
  Backend,
  FollowView,
  LeftoverRow,
  RemoveLeftovers,
  RemoveReason,
  RemoveResult,
  UninstallRefusal,
} from './types.ts';

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const KB = 1024;

export interface AppsDemo {
  /** Les apps installées : quelques-unes, 300, aucune, ou quelques-unes avec une source illisible. */
  data: 'few' | 'many' | 'empty' | 'partial';
  /** La première lecture de la vue échoue (`?appsload=fail`). */
  load: 'ok' | 'fail';
  /** Où en est la page à son ouverture, et sur quel bilan finit « Désinstaller ». */
  flow: 'idle' | 'launching' | 'waiting' | 'stuck' | 'leftovers' | 'none' | 'family' | 'unsure';
  /** Ce qui se passe mal, ou `none`. */
  fail: 'none' | 'broken' | 'cancelled' | 'bin' | 'kept';
}

export const DEFAULT_APPS_DEMO: AppsDemo = { data: 'few', load: 'ok', flow: 'idle', fail: 'none' };

/** Lit les paramètres de la désinstallation ; une valeur inconnue ne change rien. */
export function appsDemoOptions(query: URLSearchParams): AppsDemo {
  const data = query.get('apps');
  const flow = query.get('appsflow');
  const fail = query.get('appsfail');
  return {
    data: data === 'many' || data === 'empty' || data === 'partial' ? data : 'few',
    load: query.get('appsload') === 'fail' ? 'fail' : 'ok',
    flow:
      flow === 'launching' ||
      flow === 'waiting' ||
      flow === 'stuck' ||
      flow === 'leftovers' ||
      flow === 'none' ||
      flow === 'family' ||
      flow === 'unsure'
        ? flow
        : 'idle',
    fail: fail === 'broken' || fail === 'cancelled' || fail === 'bin' || fail === 'kept' ? fail : 'none',
  };
}

// ── Les apps ───────────────────────────────────────────────────────────────

/** Un identifiant à la forme de celui du cœur : « ac:… » pour le compte, « m6:{GUID} » pour le PC. */
function appId(scope: AppRow['scope'], key: string): string {
  if (scope === 'account') {
    return `ac:${key}`;
  }
  let hash = 0x811c9dc5;
  for (let index = 0; index < key.length; index += 1) {
    hash = Math.imul(hash ^ key.charCodeAt(index), 0x01000193) >>> 0;
  }
  const hex = hash.toString(16).padStart(8, '0');
  return `m6:{${hex}-0000-4000-8000-${hex}${hex.slice(0, 4)}}`;
}

interface Seed {
  name: string;
  publisher?: string;
  version?: string;
  bytes?: number;
  installed?: string;
  scope?: AppRow['scope'];
  kind?: AppRow['kind'];
  removal?: AppRow['removal'];
}

function row(seed: Seed): AppRow {
  const scope = seed.scope ?? 'machine';
  return {
    id: appId(scope, seed.name),
    name: seed.name,
    ...(seed.version === undefined ? {} : { version: seed.version }),
    ...(seed.publisher === undefined ? {} : { publisher: seed.publisher }),
    ...(seed.bytes === undefined ? {} : { bytes: seed.bytes }),
    ...(seed.installed === undefined ? {} : { installed: seed.installed }),
    scope,
    kind: seed.kind ?? 'plain',
    removal: seed.removal ?? 'offered',
  };
}

const FEW: readonly Seed[] = [
  { name: 'Discord', publisher: 'Discord Inc.', version: '1.0.9163', bytes: 388 * MB, installed: '2024-11-02', scope: 'account' },
  { name: 'Git', publisher: 'The Git Development Community', version: '2.42.0', bytes: 360 * MB, installed: '2024-01-20' },
  { name: 'Mozilla Firefox (x64 fr)', publisher: 'Mozilla', version: '121.0', bytes: 214 * MB, installed: '2024-02-14' },
  {
    name: 'Microsoft Visual C++ 2015-2022 Redistributable (x64)',
    publisher: 'Microsoft Corporation',
    version: '14.38.33135',
    bytes: 20.5 * MB,
    installed: '2023-12-09',
    kind: 'runtime',
  },
  {
    name: 'Realtek High Definition Audio Driver',
    publisher: 'Realtek Semiconductor Corp.',
    version: '6.0.9549.1',
    bytes: 96 * MB,
    installed: '2023-09-30',
    kind: 'driver',
  },
  { name: 'Microsoft Edge WebView2 Runtime', publisher: 'Microsoft Corporation', version: '120.0.2210.144', bytes: 640 * MB, removal: 'needed', kind: 'plain' },
  { name: 'Shinku', publisher: 'Shinku', version: '0.1.0', bytes: 18 * MB, installed: '2025-03-01', removal: 'own' },
  { name: 'Microsoft Edge', publisher: 'Microsoft Corporation', version: '120.0.2210.144', installed: '2023-06-12', removal: 'refused' },
  { name: 'Python 3.12.1 (64-bit)', publisher: 'Python Software Foundation', version: '3.12.1150.0', bytes: 108 * MB, installed: '2024-03-08', scope: 'account' },
  { name: 'VLC media player', publisher: 'VideoLAN', version: '3.0.20', bytes: 2.1 * GB, installed: '2024-05-17' },
  { name: 'Spotify', publisher: 'Spotify AB', installed: '2025-01-11', scope: 'account' },
  { name: '7-Zip 23.01 (x64)', publisher: 'Igor Pavlov', version: '23.01', bytes: 5.4 * MB, installed: '2023-11-21' },
  { name: 'Notepad++ (64-bit x64)', publisher: 'Notepad++ Team', version: '8.6.2', bytes: 12.7 * MB },
  { name: 'Ancien outil' },
  {
    name: 'Application avec un nom vraiment très long pour vérifier que la ligne ne déborde jamais de la carte',
    publisher: 'Un éditeur au nom lui aussi assez long, avec ses initiales et sa forme juridique SAS',
    version: '12.0.1234.5678-beta.3+build.20250101',
    bytes: 3 * GB,
    installed: '2022-02-02',
  },
];

const MANY_NAMES: readonly string[] = [
  'Adobe Acrobat', 'Audacity', 'Battle.net', 'Blender', 'Brave', 'CCleaner', 'Citrix Workspace', 'Dropbox', 'EA app', 'Epic Games Launcher',
  'Figma', 'FileZilla', 'GIMP', 'GeForce Experience', 'Google Chrome', 'HandBrake', 'Inkscape', 'iTunes', 'Java', 'KeePassXC',
  'Krita', 'LibreOffice', 'Malwarebytes', 'Microsoft Teams', 'MongoDB Compass', 'NordVPN', 'Notion', 'OBS Studio', 'Opera GX', 'Overwolf',
  'PuTTY', 'Razer Synapse', 'Signal', 'Slack', 'SumatraPDF', 'TeamViewer', 'Telegram Desktop', 'Thunderbird', 'Ubisoft Connect', 'Unity Hub',
  'WinRAR', 'WinSCP', 'Wireshark', 'Zoom', 'Zotero',
];
const MANY_PUBLISHERS: readonly (string | undefined)[] = [
  'Microsoft Corporation', 'Adobe Inc.', 'Google LLC', 'Valve Corporation', 'Mozilla Foundation', undefined, 'Logitech, Inc.', 'JetBrains s.r.o.',
];

function many(): Seed[] {
  const seeds: Seed[] = [...FEW];
  for (let index = 0; index < 285; index += 1) {
    const base = MANY_NAMES[index % MANY_NAMES.length] ?? 'App';
    const round = Math.floor(index / MANY_NAMES.length);
    const publisher = MANY_PUBLISHERS[index % MANY_PUBLISHERS.length];
    seeds.push({
      name: round === 0 ? base : `${base} ${round + 1}`,
      ...(publisher === undefined ? {} : { publisher }),
      ...(index % 7 === 0 ? {} : { version: `${1 + (index % 9)}.${index % 23}.${(index * 37) % 1000}` }),
      ...(index % 5 === 0 ? {} : { bytes: ((index * 7919) % 900 + 1) * MB }),
      ...(index % 11 === 0 ? {} : { installed: `20${20 + (index % 6)}-${String(1 + (index % 12)).padStart(2, '0')}-${String(1 + (index % 28)).padStart(2, '0')}` }),
      scope: index % 3 === 0 ? 'account' : 'machine',
      kind: index % 41 === 0 ? 'runtime' : index % 53 === 0 ? 'driver' : 'plain',
    });
  }
  return seeds;
}

/** Comme le cœur : par nom, sans casse ni accents, puis identifiant. */
function sorted(rows: readonly AppRow[]): AppRow[] {
  const fold = (text: string): string => text.normalize('NFD').replace(/\p{M}+/gu, '').toLocaleLowerCase('fr');
  return rows.toSorted((left, right) => {
    const [a, b] = [fold(left.name), fold(right.name)];
    return a < b ? -1 : a > b ? 1 : left.id < right.id ? -1 : left.id > right.id ? 1 : 0;
  });
}

// ── Les restes ─────────────────────────────────────────────────────────────

/** Ce que l'app laisse : son dossier, ses réglages, un raccourci. Les tailles sont celles du canevas. */
function leftoversOf(appName: string): Array<Omit<LeftoverRow, 'key'>> {
  // Le nom nu : sans parenthèses ni numéro de version, comme le cœur le calcule.
  const name = appName.replace(/\s*\([^)]*\)\s*$/u, '').replace(/\s+v?[\d.]+$/u, '');
  return [
    { kind: 'folder', path: `C:\\Users\\Léa\\AppData\\Local\\${name}`, bytes: 1.1 * GB, files: 8912 },
    { kind: 'data', path: `C:\\Users\\Léa\\AppData\\Roaming\\${name.toLowerCase()}`, bytes: 240 * MB, files: 1204 },
    { kind: 'shortcut', path: `C:\\Users\\Léa\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\${name}.lnk`, bytes: 1.2 * KB, files: 1 },
  ];
}

const REASONS: readonly RemoveReason[] = ['inUse', 'refused', 'partial'];

/** Ce qui s'écoule entre deux étapes du faux désinstalleur, en millisecondes. */
const LAUNCH_MS = 700;
const WAIT_MS = 3000;
const SETTLE_MS = 1500;
const delay = (milliseconds: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, milliseconds));

const refuse = (reason: UninstallRefusal): Promise<never> => Promise.reject({ kind: 'uninstallRefused', reason });

type Step =
  | { kind: 'idle' }
  | { kind: 'launching'; app: AppRow; frozen: boolean }
  | { kind: 'waiting'; app: AppRow; since: number; frozen: boolean }
  | { kind: 'settling'; app: AppRow; since: number }
  | { kind: 'removed'; app: AppRow };

/** Le faux cœur de la désinstallation : cinq commandes, qui s'ajoutent à celles du reste de la démo. */
export function appsBackend(
  demo: AppsDemo,
): Pick<Backend, 'appsView' | 'startUninstall' | 'stopUninstall' | 'rescanLeftovers' | 'removeLeftovers'> {
  let apps = sorted((demo.data === 'empty' ? [] : demo.data === 'many' ? many() : FEW).map(row));
  const complete = demo.data !== 'partial';
  let loadFails = demo.load === 'fail';
  let step: Step = { kind: 'idle' };
  let kept: Array<Omit<LeftoverRow, 'key'>> = [];
  // Les restes déjà partis à la corbeille : une nouvelle recherche ne les retrouve pas.
  const gone = new Set<string>();

  /** Le bilan que « Désinstaller » rend : des restes, rien, une app de la même famille, ou une vérification impossible. */
  const verdict = demo.flow === 'none' || demo.flow === 'family' || demo.flow === 'unsure' ? demo.flow : 'leftovers';

  function search(app: AppRow): void {
    kept = verdict === 'leftovers' ? leftoversOf(app.name).filter((one) => !gone.has(one.path)) : [];
  }

  function release(app: AppRow): void {
    apps = apps.filter((one) => one.id !== app.id);
  }

  // Une page qui s'ouvre dans une phase : la démo la montre telle quelle.
  const first = apps.find((app) => app.name === 'Discord') ?? apps.find((app) => app.removal === 'offered');
  if (first !== undefined) {
    const start = Date.now();
    switch (demo.flow) {
      case 'launching':
        step = { kind: 'launching', app: first, frozen: true };
        break;
      case 'waiting':
        step = { kind: 'waiting', app: first, since: start, frozen: true };
        break;
      case 'stuck':
        step = { kind: 'waiting', app: first, since: start - 25_000, frozen: true };
        break;
      case 'leftovers':
      case 'none':
      case 'family':
      case 'unsure':
        release(first);
        search(first);
        step = { kind: 'removed', app: first };
        break;
      case 'idle':
        break;
    }
  }

  /** Ce que fait `poll()` du cœur : fait avancer le déroulement avec l'horloge. */
  function advance(): { kept: boolean } {
    const now = Date.now();
    if (step.kind === 'waiting' && !step.frozen && now - step.since >= WAIT_MS) {
      if (demo.fail === 'kept') {
        step = { kind: 'idle' };
        return { kept: true };
      }
      release(step.app);
      step = { kind: 'settling', app: step.app, since: now };
    } else if (step.kind === 'settling' && now - step.since >= SETTLE_MS) {
      search(step.app);
      step = { kind: 'removed', app: step.app };
    }
    return { kept: false };
  }

  function follow(): FollowView {
    switch (step.kind) {
      case 'idle':
        return { phase: 'idle' };
      case 'launching':
        return { phase: 'launching', id: step.app.id, name: step.app.name };
      case 'waiting':
        return { phase: 'waiting', id: step.app.id, name: step.app.name, seconds: Math.floor((Date.now() - step.since) / 1000) };
      case 'settling':
        return { phase: 'settling', id: step.app.id, name: step.app.name };
      case 'removed':
        return {
          phase: 'removed',
          id: step.app.id,
          name: step.app.name,
          // Les clés sont des rangs dans la liste gardée : elles se renumérotent à chaque retrait.
          leftovers: kept.map((one, key): LeftoverRow => ({ ...one, key })),
          family: verdict === 'family',
          complete: verdict !== 'unsure',
        };
    }
  }

  function view(outcome = false): AppsView {
    return { apps: structuredClone(apps), complete, follow: follow(), ...(outcome ? { outcome: 'kept' as const } : {}) };
  }

  return {
    appsView: async () => {
      await delay(120);
      if (loadFails) {
        loadFails = false;
        throw new Error('démo : la première lecture échoue');
      }
      return view(advance().kept);
    },
    startUninstall: async (id) => {
      if (step.kind !== 'idle') {
        return refuse('busy');
      }
      const app = apps.find((one) => one.id === id);
      if (app === undefined) {
        return refuse('vanished');
      }
      if (app.removal !== 'offered') {
        return refuse('notOffered');
      }
      if (demo.fail === 'broken') {
        return refuse('broken');
      }
      // Le cœur passe en `launching` avant de lancer : `ShellExecuteExW` ne rend la main qu'après la réponse à Windows.
      step = { kind: 'launching', app, frozen: false };
      await delay(LAUNCH_MS);
      if (demo.fail === 'cancelled') {
        step = { kind: 'idle' };
        return refuse('cancelled');
      }
      step = { kind: 'waiting', app, since: Date.now(), frozen: false };
      return view();
    },
    stopUninstall: async () => {
      if (step.kind === 'launching') {
        return refuse('busy');
      }
      step = { kind: 'idle' };
      kept = [];
      return view();
    },
    rescanLeftovers: async () => {
      if (step.kind !== 'removed') {
        return refuse('busy');
      }
      search(step.app);
      return view();
    },
    removeLeftovers: async (keys): Promise<RemoveLeftovers> => {
      if (step.kind !== 'removed') {
        return refuse('busy');
      }
      await delay(300);
      const current = kept.map((one, key) => ({ ...one, key }));
      const results: RemoveResult[] = [];
      let bytes = 0;
      for (const key of keys) {
        const found = current.find((one) => one.key === key);
        if (found === undefined) {
          results.push({ key, outcome: 'refused' });
        } else if (demo.fail === 'bin') {
          results.push({ key, outcome: 'failed', reason: REASONS[key % REASONS.length] ?? 'refused' });
        } else {
          results.push({ key, outcome: 'removed' });
          bytes += found.bytes ?? 0;
          gone.add(found.path);
        }
      }
      kept = kept.filter((one) => !gone.has(one.path));
      return { view: view(), results, bytes };
    },
  };
}
