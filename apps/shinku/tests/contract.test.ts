// Le contrat entre le cœur et l'interface.
//
// `contract/contract.json` est écrit par les tests du cœur
// (crates/shinku-core/tests/ui_contract.rs) : il note la forme de tout ce que
// le guichet peut rendre, et les valeurs de chaque énumération. Ce fichier
// vérifie que l'interface attend exactement cela :
//
// - les énumérations, par les types eux-mêmes (voir `allOf`) ;
// - les formes, par le faux cœur de la démo, dont les réponses sont tenues par
//   les types de `src/backend/types.ts`.
//
// Si un test échoue ici après un changement du cœur, c'est l'interface qu'il
// faut adapter : types, faux cœur, puis écrans.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mock, test } from 'node:test';

import { mockBackend, type MockOptions } from '../src/backend/mock.ts';
import type {
  AgentState,
  AppKind,
  AppRemoval,
  AppScope,
  AppsView,
  FollowView,
  BackendError,
  CategoryState,
  CategorySummary,
  CleanOutcome,
  CleanPhase,
  CleanRefusal,
  CleanRequest,
  DuplicatesPage,
  KeepChoice,
  FolderRefusal,
  GameOption,
  GameRefusal,
  GameSource,
  GameStatus,
  JunkCategory,
  LeftoverKind,
  LoadIssue,
  Lot,
  ModuleId,
  Phase,
  PowerOutcome,
  PowerSupport,
  RemoveOutcome,
  RemoveReason,
  RunState,
  SessionState,
  Settings,
  StartupEntry,
  StartupMark,
  StartupOrigin,
  StartupPc,
  StartupRefusal,
  StartupScope,
  StartupState,
  StartupTarget,
  StepState,
  StopReason,
  Theme,
  UninstallRefusal,
  WindowsPage,
} from '../src/backend/types.ts';

type Shape = string | Shape[] | { [field: string]: Shape };

interface Contract {
  enums: Record<string, string[]>;
  shapes: Record<string, Shape>;
}

const contract = JSON.parse(readFileSync(new URL('./contract/contract.json', import.meta.url), 'utf8')) as Contract;

// ── Énumérations ───────────────────────────────────────────────────────────

/**
 * Une liste qui nomme toutes les valeurs d'un type, et rien d'autre : oublier
 * une valeur, ou en écrire une de trop, est une erreur de compilation.
 */
function allOf<Union extends string>() {
  return <const List extends readonly Union[]>(list: List & ([Union] extends [List[number]] ? unknown : never)): List =>
    list;
}

const enums: Record<string, readonly string[]> = {
  phase: allOf<Phase>()(['preparing', 'walking', 'duplicatesQuick', 'duplicatesFull', 'done', 'cancelled']),
  sessionState: allOf<SessionState>()(['idle', 'scanning', 'results', 'failed', 'cleaning', 'cleaned']),
  cleanPhase: allOf<CleanPhase>()(['preparing', 'recycleBin', 'junkFiles', 'duplicates', 'done']),
  cleanRefusal: allOf<CleanRefusal>()(['noResults', 'staleResults', 'otherAnalysis', 'nothingToClean']),
  cleanOutcome: allOf<CleanOutcome>()(['completed', 'cancelled']),
  stepState: allOf<StepState>()(['notRequested', 'done', 'stopped', 'failed']),
  stopReason: allOf<StopReason>()(['notInRecycleBin', 'keptCopyChanged']),
  runState: allOf<RunState>()(['running', 'completed', 'cancelled', 'interrupted']),
  moduleId: allOf<ModuleId>()([
    'junkFiles',
    'duplicates',
    'largeFiles',
    'recycleBin',
    'startup',
    'gameMode',
    'uninstall',
    'privacy',
  ]),
  lot: allOf<Lot>()(['auto', 'optional', 'review']),
  risk: allOf<CategorySummary['risk']>()(['A', 'B', 'C', 'D']),
  categoryState: allOf<CategoryState>()(['ready', 'pending', 'unavailable']),
  junkCategory: allOf<JunkCategory>()(['appCaches', 'tempFiles', 'updateLeftovers', 'logsAndReports']),
  duplicatesState: allOf<DuplicatesPage['state']>()(['done', 'pending', 'off']),
  theme: allOf<Theme>()(['system', 'light', 'dark']),
  autoScan: allOf<Settings['autoScan']>()(['never', 'weekly', 'monthly']),
  language: allOf<Settings['language']>()(['fr', 'en']),
  folderRefusal: allOf<FolderRefusal>()(['notAbsolute', 'notAFolder', 'notUnicode']),
  backendError: allOf<BackendError['kind']>()([
    'settingsUnchecked',
    'scanNotStarted',
    'settingsNotSaved',
    'folderRefused',
    'cleanRunning',
    'cleanRefused',
    'cleanNotStarted',
    'gameRefused',
    'startupRefused',
    'uninstallRefused',
  ]),
  agentState: allOf<AgentState>()(['running', 'stopped', 'missing']),
  powerOutcome: allOf<PowerOutcome>()([
    'applied',
    'alreadyFast',
    'onBattery',
    'unavailable',
    'unsuited',
    'custom',
    'refused',
    'off',
  ]),
  powerSupport: allOf<PowerSupport>()(['available', 'unavailable', 'unsuited']),
  gameSource: allOf<GameSource>()(['added', 'library']),
  windowsPage: allOf<WindowsPage>()(['gameMode', 'power', 'startupApps', 'apps']),
  gameStatus: allOf<GameStatus>()(['notGame', 'forgotten']),
  gameOption: allOf<GameOption>()(['auto', 'power', 'quietApps']),
  gameRefusal: allOf<GameRefusal>()(['unknownApp', 'unknownGame', 'notAGame', 'protectedApp', 'listFull']),
  loadIssue: allOf<LoadIssue['kind']>()(['unreadable', 'partial']),
  startupScope: allOf<StartupScope>()(['user', 'machine']),
  startupState: allOf<StartupState>()(['enabled', 'disabled', 'unknown']),
  startupTarget: allOf<StartupTarget>()(['found', 'missing', 'hosted', 'unknown']),
  startupOrigin: allOf<StartupOrigin>()(['registry', 'folder']),
  startupMark: allOf<StartupMark['kind']>()(['cutByShinku', 'cameBack']),
  startupHint: allOf<NonNullable<StartupEntry['hint']>>()(['sync']),
  startupRefusal: allOf<StartupRefusal>()(['unknownEntry', 'notAdjustable', 'busy', 'failed', 'cancelled', 'needsAdmin']),
  startupAccess: allOf<StartupPc>()(['consent', 'password', 'direct', 'unavailable']),
  appsScope: allOf<AppScope>()(['account', 'machine']),
  appsKind: allOf<AppKind>()(['plain', 'runtime', 'driver']),
  appsRemoval: allOf<AppRemoval>()(['offered', 'refused', 'own', 'needed']),
  appsOutcome: allOf<NonNullable<AppsView['outcome']>>()(['kept']),
  leftoverKind: allOf<LeftoverKind>()(['folder', 'data', 'shortcut']),
  removeOutcome: allOf<RemoveOutcome>()(['removed', 'alreadyGone', 'refused', 'failed']),
  removeReason: allOf<RemoveReason>()(['inUse', 'refused', 'partial', 'destroyed']),
  uninstallRefusal: allOf<UninstallRefusal>()(['busy', 'vanished', 'unreadable', 'notOffered', 'broken', 'cancelled', 'failed']),
  followPhase: allOf<FollowView['phase']>()(['idle', 'launching', 'waiting', 'settling', 'removed']),
};

test('les énumérations de l’interface sont celles du cœur', () => {
  assert.deepEqual(Object.keys(enums).sort(), Object.keys(contract.enums).sort());
  for (const [name, values] of Object.entries(enums)) {
    assert.deepEqual([...values].sort(), [...(contract.enums[name] ?? [])].sort(), `énumération ${name}`);
  }
});

// ── Formes ─────────────────────────────────────────────────────────────────

/** La forme d'une valeur : ses champs et leur nature, sans leur contenu. */
function shape(value: unknown): Shape {
  if (value === null) {
    return 'null';
  }
  if (Array.isArray(value)) {
    const merged = value.map(shape).reduce<Shape | null>((known, next) => (known === null ? next : merge(known, next)), null);
    return merged === null ? [] : [merged];
  }
  if (typeof value === 'object') {
    return Object.fromEntries(Object.entries(value).map(([field, inner]) => [field, shape(inner)]));
  }
  return typeof value;
}

/** Réunit deux formes observées pour la même valeur ; une valeur absente ou une liste vide n'apprend rien. */
function merge(left: Shape, right: Shape): Shape {
  if (left === 'null') {
    return right;
  }
  if (right === 'null') {
    return left;
  }
  if (Array.isArray(left) && Array.isArray(right)) {
    const [first, second] = [left[0], right[0]];
    return first === undefined ? right : second === undefined ? left : [merge(first, second)];
  }
  if (typeof left === 'object' && typeof right === 'object' && !Array.isArray(left) && !Array.isArray(right)) {
    const merged: { [field: string]: Shape } = { ...left };
    for (const [field, inner] of Object.entries(right)) {
      const known = merged[field];
      merged[field] = known === undefined ? inner : merge(known, inner);
    }
    return merged;
  }
  return left === right ? left : 'mixed';
}

/** Ce qui sépare la forme attendue par le cœur de celle que l'interface connaît. */
function differences(expected: Shape, actual: Shape, path: string): string[] {
  // Une valeur absente d'un côté, ou une liste vide, ne contredit rien.
  if (expected === 'null' || actual === 'null') {
    return [];
  }
  if (Array.isArray(expected) && Array.isArray(actual)) {
    const [first, second] = [expected[0], actual[0]];
    return first === undefined || second === undefined ? [] : differences(first, second, `${path}[]`);
  }
  if (typeof expected === 'object' && typeof actual === 'object' && !Array.isArray(expected) && !Array.isArray(actual)) {
    const found: string[] = [];
    for (const field of new Set([...Object.keys(expected), ...Object.keys(actual)])) {
      const [first, second] = [expected[field], actual[field]];
      if (first === undefined) {
        found.push(`${path}.${field} : inconnu du cœur`);
      } else if (second === undefined) {
        found.push(`${path}.${field} : inconnu de l’interface`);
      } else {
        found.push(...differences(first, second, `${path}.${field}`));
      }
    }
    return found;
  }
  return expected === actual ? [] : [`${path} : ${JSON.stringify(expected)} côté cœur, ${JSON.stringify(actual)} côté interface`];
}

/**
 * Le faux cœur le plus bavard : un grand vide interrompu à annoncer, une app
 * restée ouverte, des fichiers laissés en place. Chaque champ a ainsi une
 * valeur à montrer.
 */
const OPTIONS: MockOptions = {
  speed: 1_000_000,
  issue: 'none',
  emptyBin: false,
  theme: 'system',
  clean: 'rough',
  interrupted: true,
  game: { state: 'playing', agent: 'running', power: 'available', startup: 'enabled' },
  boot: { data: 'few', fail: false, read: 'all', load: 'ok', shinku: 'on', pc: 'consent', pcflow: 'wait' },
  apps: { data: 'few', load: 'ok', flow: 'idle', fail: 'none' },
};
const pause = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));

/** Mène le faux cœur jusqu'à des résultats complets, et les rend. */
async function scanned(backend: ReturnType<typeof mockBackend>) {
  await backend.startScan();
  await pause(20);
  const results = await backend.sessionView();
  assert.equal(results.state, 'results');
  assert.ok(results.summary !== null);
  return { results, analysis: results.summary.started };
}

/**
 * La désinstallation, déroulée par le faux cœur avec une horloge simulée : chaque phase, le bilan, le rescan,
 * l'arrêt, la corbeille et un refus. Le contrat note la plupart de ces vues avec des lignes sans champ facultatif
 * (taille, éditeur, version, date) : `lean` ne garde que ce que le cœur y met.
 */
async function appsSamples(): Promise<Record<string, unknown>> {
  mock.timers.enable({ apis: ['setTimeout', 'Date'], now: Date.now() });
  try {
    const tick = <T>(promise: Promise<T>, milliseconds = 1000): Promise<T> => {
      mock.timers.tick(milliseconds);
      return promise;
    };
    const demoOf = (flow: 'idle' | 'leftovers' | 'launching' | 'waiting', fail: 'none' | 'bin' | 'kept' | 'broken' = 'none') =>
      mockBackend({ ...OPTIONS, apps: { ...OPTIONS.apps, flow, fail } });
    const lean = (view: AppsView): AppsView => ({
      ...view,
      apps: view.apps.map(({ id, kind, name, removal, scope }) => ({ id, kind, name, removal, scope })),
    });
    /** Lance la désinstallation de Discord et attend, par pas, la vue qui répond à `until`. */
    const reach = async (backend: ReturnType<typeof mockBackend>, until: (view: AppsView) => boolean): Promise<AppsView> => {
      await tick(backend.startUninstall('ac:Discord'), 800);
      for (let step = 0; step < 60; step += 1) {
        const view = await tick(backend.appsView(), 500);
        if (until(view)) {
          return view;
        }
      }
      assert.fail('le faux cœur n’arrive pas à la vue attendue');
    };

    const idle = await tick(demoOf('idle').appsView());
    const launching = await tick(demoOf('launching').appsView());
    const waiting = await tick(demoOf('waiting').appsView());
    const settling = await reach(demoOf('idle'), (view) => view.follow.phase === 'settling');
    const removedBackend = demoOf('leftovers');
    const removed = await tick(removedBackend.appsView());
    assert.equal(removed.follow.phase, 'removed');
    const rescanned = await tick(removedBackend.rescanLeftovers());
    const stopped = await tick(removedBackend.stopUninstall());
    const kept = await reach(demoOf('idle', 'kept'), (view) => view.outcome === 'kept');

    const binBackend = demoOf('leftovers', 'bin');
    const bilan = await tick(binBackend.appsView());
    assert.equal(bilan.follow.phase, 'removed');
    const keys = bilan.follow.phase === 'removed' ? bilan.follow.leftovers.map((row) => row.key) : [];
    const trashed = await tick(binBackend.removeLeftovers(keys));
    const refusal: unknown = await tick(demoOf('idle', 'broken').startUninstall('ac:Discord'), 800).then(
      () => null,
      (error: unknown) => error,
    );
    assert.deepEqual(refusal, { kind: 'uninstallRefused', reason: 'broken' });
    return {
      'appsView.idle': idle,
      'appsView.launching': launching,
      'appsView.waiting': waiting,
      'appsView.settling': lean(settling),
      'appsView.removed': lean(removed),
      'appsView.rescanned': lean(rescanned),
      'appsView.stopped': lean(stopped),
      'appsView.kept': lean(kept),
      removeLeftovers: { ...trashed, view: lean(trashed.view) },
    };
  } finally {
    mock.timers.reset();
  }
}

/** Les mêmes échantillons que ceux du cœur, pris cette fois dans le faux cœur de la démo. */
async function samples(): Promise<Record<string, unknown>> {
  const backend = mockBackend(OPTIONS);
  const idle = await backend.sessionView();
  const overview = await backend.overview();
  assert.ok(overview.interruptedClean !== null);
  const settings = await backend.setTheme('dark');
  const { results, analysis } = await scanned(backend);
  const lists = {
    junk: await backend.junk(),
    largeFiles: await backend.largeFiles(0, 10),
    duplicates: await backend.duplicates(0, 10),
  };

  // Le choix de l'exemplaire gardé d'un doublon.
  const group = lists.duplicates?.groups[0];
  assert.ok(group !== undefined);
  const keepChoice: KeepChoice = { digest: group.digest, size: group.size, index: 0, wholeFolder: true };
  assert.equal(await backend.keepDuplicate(keepChoice), true);

  // Le grand vide : sa confirmation, sa demande, puis son bilan.
  const cleanPreview = await backend.cleanPreview();
  const cleanRequest: CleanRequest = { analysis, recycleBin: true, junkFiles: true, duplicates: true };
  await backend.startClean(cleanRequest);
  await pause(20);
  const cleaned = await backend.sessionView();
  assert.equal(cleaned.state, 'cleaned');

  // L'arrêt de sécurité de l'étape « Doublons ».
  const halting = mockBackend({ ...OPTIONS, clean: 'halt' });
  await halting.startClean({ ...cleanRequest, analysis: (await scanned(halting)).analysis });
  await pause(20);
  const cleanStop = (await halting.sessionView()).clean?.report?.duplicates.stop;
  assert.ok(cleanStop != null);

  // Le mode jeu : la page en veille, puis en partie ; la liste des apps ouvertes ; un refus.
  const gameIdle = await mockBackend({ ...OPTIONS, game: { ...OPTIONS.game, state: 'idle' } }).gameView();
  const gaming = mockBackend(OPTIONS);
  const gamePlaying = await gaming.gameView();
  assert.ok(gamePlaying.session !== null);
  const openApps = await gaming.openApps();
  const refusal: unknown = await gaming.setGameStatus('Z:\\inconnu.exe', 'notGame').then(
    () => null,
    (error: unknown) => error,
  );

  // Le démarrage : la vue (une ligne de chaque sorte), et un refus.
  const booting = mockBackend(OPTIONS);
  const startupView = await booting.startupView();
  assert.ok(startupView.entries.some((entry) => entry.mark?.kind === 'cutByShinku'));
  assert.ok(startupView.entries.some((entry) => entry.mark?.kind === 'cameBack'));
  // Une vue relue pendant l'attente d'une entrée du PC : son champ `pending`.
  const machineEntry = startupView.entries.find((entry) => entry.scope === 'machine' && entry.adjustable);
  assert.ok(machineEntry !== undefined);
  const changing = mockBackend(OPTIONS);
  const waiting = changing.setStartupEntry(machineEntry.id, false);
  const startupPending = (await changing.startupView()).pending;
  assert.ok(startupPending !== null);
  await waiting;
  const unreadableBoot = await mockBackend({ ...OPTIONS, boot: { ...OPTIONS.boot, read: 'user' } }).startupView();
  assert.deepEqual(unreadableBoot.unreadable, ['user']);
  const startupRefusal: unknown = await booting.setStartupEntry('ur-0000000000000000', false).then(
    () => null,
    (error: unknown) => error,
  );

  const unreadable = (await mockBackend({ ...OPTIONS, issue: 'unreadable' }).overview()).settingsIssue;
  const partial = (await mockBackend({ ...OPTIONS, issue: 'partial' }).overview()).settingsIssue;

  const errors: { [Kind in BackendError['kind']]: Extract<BackendError, { kind: Kind }> } = {
    settingsUnchecked: { kind: 'settingsUnchecked' },
    scanNotStarted: { kind: 'scanNotStarted', reason: 'raison' },
    settingsNotSaved: { kind: 'settingsNotSaved', reason: 'raison' },
    folderRefused: { kind: 'folderRefused', reason: 'notAFolder' },
    cleanRunning: { kind: 'cleanRunning' },
    cleanRefused: { kind: 'cleanRefused', reason: 'staleResults' },
    cleanNotStarted: { kind: 'cleanNotStarted', reason: 'raison' },
    gameRefused: { kind: 'gameRefused', reason: 'unknownGame' },
    startupRefused: { kind: 'startupRefused', reason: 'unknownEntry' },
    uninstallRefused: { kind: 'uninstallRefused', reason: 'vanished' },
  };
  assert.deepEqual(refusal, errors.gameRefused, 'le faux cœur refuse un jeu inconnu comme le cœur');
  assert.deepEqual(startupRefusal, errors.startupRefused, 'le faux cœur refuse une entrée inconnue comme le cœur');

  return {
    overview,
    settings,
    'sessionView.idle': idle,
    'sessionView.results': results,
    'sessionView.cleaned': cleaned,
    ...lists,
    cleanPreview,
    cleanRequest,
    keepChoice,
    cleanStop,
    'gameView.idle': gameIdle,
    'gameView.playing': gamePlaying,
    openApps,
    'loadIssue.unreadable': unreadable,
    'loadIssue.partial': partial,
    'error.settingsUnchecked': errors.settingsUnchecked,
    'error.scanNotStarted': errors.scanNotStarted,
    'error.settingsNotSaved': errors.settingsNotSaved,
    'error.folderRefused': errors.folderRefused,
    'error.cleanRunning': errors.cleanRunning,
    'error.cleanRefused': errors.cleanRefused,
    'error.cleanNotStarted': errors.cleanNotStarted,
    'error.gameRefused': errors.gameRefused,
    startupView,
    startupPending,
    'error.startupRefused': errors.startupRefused,
    'error.uninstallRefused': errors.uninstallRefused,
    ...(await appsSamples()),
  };
}

test('tout ce que rend le cœur a la forme que l’interface attend', async () => {
  const known = await samples();
  assert.deepEqual(Object.keys(known).sort(), Object.keys(contract.shapes).sort());
  for (const [name, sample] of Object.entries(known)) {
    assert.notEqual(sample, null, `échantillon ${name} manquant`);
    const expected = contract.shapes[name];
    assert.ok(expected !== undefined);
    assert.deepEqual(differences(expected, shape(sample), name), [], `forme de ${name}`);
  }
});

test('la comparaison des formes sait voir un écart', () => {
  const core = shape({ recycleBin: { bytes: 1, items: 2 }, volumes: [{ root: 'C:\\' }], issue: null });
  assert.deepEqual(differences(core, shape({ recycleBin: { bytes: 1, items: 2 }, volumes: [], issue: { kind: 'x' } }), 'x'), []);
  assert.deepEqual(differences(core, shape({ recycleBin: { bytes: 1, count: 2 }, volumes: [], issue: null }), 'x'), [
    'x.recycleBin.items : inconnu de l’interface',
    'x.recycleBin.count : inconnu du cœur',
  ]);
  assert.deepEqual(differences(core, shape({ recycleBin: { bytes: '1', items: 2 }, volumes: [{ root: 3 }], issue: null }), 'x'), [
    'x.recycleBin.bytes : "number" côté cœur, "string" côté interface',
    'x.volumes[].root : "string" côté cœur, "number" côté interface',
  ]);
});
