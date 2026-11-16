// Le faux cœur de la désinstallation : il suit un vrai déroulement, et il rend
// ce que les paramètres de la démo annoncent.

import assert from 'node:assert/strict';
import { afterEach, beforeEach, mock, test } from 'node:test';

import { type AppsDemo, appsDemoOptions, DEFAULT_APPS_DEMO } from '../src/backend/mock-apps.ts';
import { demoOptions, mockBackend } from '../src/backend/mock.ts';
import type { AppsView, Backend, UninstallRefusal } from '../src/backend/types.ts';

const NOW = new Date(2026, 10, 15, 12).getTime();

beforeEach(() => {
  mock.timers.enable({ apis: ['setTimeout', 'Date'], now: NOW });
});

afterEach(() => {
  mock.timers.reset();
});

function demo(query = ''): Backend {
  return mockBackend(demoOptions(query));
}

/** Attend la réponse d'une demande du faux cœur, en laissant passer le temps qu'elle prend. */
async function answer<T>(promise: Promise<T>, milliseconds = 1000): Promise<T> {
  mock.timers.tick(milliseconds);
  return promise;
}

/** Fait avancer l'horloge, puis relit la vue comme le fait la page. */
async function poll(backend: Backend, milliseconds: number): Promise<AppsView> {
  mock.timers.tick(milliseconds);
  return answer(backend.appsView(), 200);
}

async function refusal(promise: Promise<unknown>, milliseconds = 1000): Promise<UninstallRefusal | null> {
  try {
    await answer(promise, milliseconds);
    return null;
  } catch (error) {
    const refused = error as { kind?: string; reason?: UninstallRefusal };
    assert.equal(refused.kind, 'uninstallRefused');
    return refused.reason ?? null;
  }
}

const DISCORD = 'ac:Discord';

test('les paramètres d’adresse de la désinstallation sont lus, et le reste prend les valeurs par défaut', () => {
  const read = (query: string): AppsDemo => appsDemoOptions(new URLSearchParams(query));
  const expected = (parts: Partial<AppsDemo>): AppsDemo => ({ ...DEFAULT_APPS_DEMO, ...parts });
  assert.deepEqual(read(''), expected({}));
  assert.deepEqual(DEFAULT_APPS_DEMO, { data: 'few', load: 'ok', flow: 'idle', fail: 'none' });
  for (const data of ['few', 'many', 'empty', 'partial'] as const) {
    assert.deepEqual(read(`?apps=${data}`), expected({ data }));
  }
  assert.deepEqual(read('?appsload=fail'), expected({ load: 'fail' }));
  for (const flow of ['launching', 'waiting', 'stuck', 'leftovers', 'none', 'family', 'unsure'] as const) {
    assert.deepEqual(read(`?appsflow=${flow}`), expected({ flow }));
  }
  for (const fail of ['broken', 'cancelled', 'bin', 'destroyed', 'kept'] as const) {
    assert.deepEqual(read(`?appsfail=${fail}`), expected({ fail }));
  }
  assert.deepEqual(read('?apps=many&appsflow=family&appsfail=bin&appsload=fail'), {
    data: 'many',
    load: 'fail',
    flow: 'family',
    fail: 'bin',
  });
  // Une valeur inconnue ne change rien.
  assert.deepEqual(read('?apps=xyz&appsload=oui&appsflow=xyz&appsfail=xyz'), expected({}));
  // Elles passent par les options de la démo, et ne touchent ni au démarrage ni au mode jeu.
  assert.deepEqual(demoOptions('?appsflow=waiting').apps, expected({ flow: 'waiting' }));
  assert.deepEqual(demoOptions('?boot=many').apps, expected({}));
  assert.equal(demoOptions('?appsflow=waiting').boot.data, 'few');
});

test('par défaut, la démo montre quinze apps, triées par nom sans casse ni accents', async () => {
  const view = await answer(demo().appsView());
  assert.equal(view.apps.length, 15);
  assert.equal(view.complete, true);
  assert.deepEqual(view.follow, { phase: 'idle' });
  assert.equal(view.outcome, undefined);
  const fold = (text: string): string => text.normalize('NFD').replace(/\p{M}+/gu, '').toLocaleLowerCase('fr');
  const names = view.apps.map((app) => fold(app.name));
  assert.deepEqual(names, names.toSorted());
  const ids = view.apps.map((app) => app.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const app of view.apps) {
    assert.match(app.id, app.scope === 'account' ? /^ac:.+/ : /^m6:\{[0-9a-f]{8}-0000-4000-8000-[0-9a-f]{12}\}$/);
  }
});

test('chaque sorte de ligne est là : proposée, refusée, Shinku, nécessaire, ressemblances, tailles et dates inconnues', async () => {
  const { apps } = await answer(demo().appsView());
  const named = (name: string) => {
    const found = apps.find((app) => app.name.startsWith(name));
    assert.ok(found !== undefined, name);
    return found;
  };
  assert.equal(named('Discord').removal, 'offered');
  assert.equal(named('Discord').scope, 'account');
  assert.equal(named('Git').scope, 'machine');
  assert.equal(named('Microsoft Edge').removal, 'refused');
  assert.equal(named('Shinku').removal, 'own');
  assert.equal(named('Microsoft Edge WebView2').removal, 'needed');
  assert.equal(named('Microsoft Visual C++').kind, 'runtime');
  assert.equal(named('Realtek').kind, 'driver');
  assert.equal(named('Ancien outil').publisher, undefined, 'un éditeur absent est absent, pas vide');
  assert.equal('bytes' in named('Spotify'), false, 'une taille inconnue est absente');
  assert.equal('installed' in named('Notepad++'), false);
  assert.match(named('Discord').installed ?? '', /^\d{4}-\d{2}-\d{2}$/);
  assert.ok(apps.some((app) => app.name.length > 80), 'un nom très long pour l’essai des débordements');
});

test('trois cents apps pour essayer la fluidité, aucune pour la page vide, une source illisible pour l’avertissement', async () => {
  const many = await answer(demo('?apps=many').appsView());
  assert.ok(many.apps.length >= 300);
  assert.equal(new Set(many.apps.map((app) => app.id)).size, many.apps.length);
  const empty = await answer(demo('?apps=empty').appsView());
  assert.deepEqual(empty.apps, []);
  assert.equal(empty.complete, true);
  const partial = await answer(demo('?apps=partial').appsView());
  assert.equal(partial.complete, false);
  assert.equal(partial.apps.length, 15);
});

test('la première lecture échoue avec ?appsload=fail ; la suivante réussit', async () => {
  const backend = demo('?appsload=fail');
  await assert.rejects(answer(backend.appsView()));
  assert.equal((await answer(backend.appsView())).apps.length, 15);
});

test('« Désinstaller » : launching pendant la réponse à Windows, puis waiting, settling, removed', async () => {
  const backend = demo();
  const starting = backend.startUninstall(DISCORD);
  // `apps_view` répond pendant que `start_uninstall` attend : la phase est `launching`.
  const during = await poll(backend, 300);
  assert.deepEqual(during.follow, { phase: 'launching', id: DISCORD, name: 'Discord' });
  const started = await answer(starting, 600);
  assert.deepEqual(started.follow, { phase: 'waiting', id: DISCORD, name: 'Discord', seconds: 0 });
  assert.ok(started.apps.some((app) => app.id === DISCORD), 'l’app est encore là tant que son inscription l’est');

  const later = await poll(backend, 2000);
  assert.equal(later.follow.phase, 'waiting');
  assert.equal(later.follow.phase === 'waiting' && later.follow.seconds, 2);

  const settling = await poll(backend, 1200);
  assert.deepEqual(settling.follow, { phase: 'settling', id: DISCORD, name: 'Discord' });
  assert.ok(!settling.apps.some((app) => app.id === DISCORD), 'l’inscription a disparu');

  const done = await poll(backend, 1600);
  assert.equal(done.follow.phase, 'removed');
  assert.ok(done.follow.phase === 'removed' && done.follow.leftovers.length === 3);
  assert.ok(done.follow.phase === 'removed' && done.follow.complete && !done.follow.family);
  // Rien ne se rejoue : « removed » rend la même chose tant que la personne n'a pas fini.
  assert.deepEqual((await poll(backend, 10_000)).follow, done.follow);
});

test('les restes : un dossier, des données, un raccourci ; des clés qui sont des rangs ; jamais de chemin à renvoyer', async () => {
  const backend = demo('?appsflow=leftovers');
  const view = await answer(backend.appsView());
  assert.equal(view.follow.phase, 'removed');
  const leftovers = view.follow.phase === 'removed' ? view.follow.leftovers : [];
  assert.deepEqual(leftovers.map((row) => [row.key, row.kind]), [[0, 'folder'], [1, 'data'], [2, 'shortcut']]);
  for (const row of leftovers) {
    assert.match(row.path, /^C:\\Users\\/);
    assert.ok((row.bytes ?? 0) > 0 && (row.files ?? 0) > 0);
  }
  assert.ok(!view.apps.some((app) => app.name === 'Discord'));
});

test('« Revenir à la liste » et « Terminé » : retour à idle, sans rien toucher', async () => {
  const backend = demo();
  const starting = backend.startUninstall(DISCORD);
  await answer(starting, 800);
  const stopped = await answer(backend.stopUninstall());
  assert.deepEqual(stopped.follow, { phase: 'idle' });
  assert.ok(stopped.apps.some((app) => app.id === DISCORD), 'l’app est toujours là');
  assert.deepEqual((await poll(backend, 10_000)).follow, { phase: 'idle' });
  // Rien en cours pendant le lancement : le cœur refuse de s'arrêter à ce moment-là.
  const launching = backend.startUninstall(DISCORD);
  assert.equal(await refusal(backend.stopUninstall(), 10), 'busy');
  await answer(launching, 800);
});

test('les refus de « Désinstaller », dans l’ordre du cœur', async () => {
  const backend = demo();
  assert.equal(await refusal(backend.startUninstall('ac:Inconnue')), 'vanished');
  const { apps } = await answer(backend.appsView());
  const edge = apps.find((app) => app.removal === 'refused');
  const shinku = apps.find((app) => app.removal === 'own');
  assert.equal(await refusal(backend.startUninstall(edge?.id ?? '')), 'notOffered');
  assert.equal(await refusal(backend.startUninstall(shinku?.id ?? '')), 'notOffered');
  // Un seul à la fois.
  const first = backend.startUninstall(DISCORD);
  assert.equal(await refusal(backend.startUninstall('ac:Spotify'), 10), 'busy');
  await answer(first, 800);
  assert.equal(await refusal(backend.startUninstall('ac:Spotify'), 10), 'busy');
});

test('?appsfail=broken refuse tout de suite ; ?appsfail=cancelled refuse après la fenêtre de Windows et revient à idle', async () => {
  const broken = demo('?appsfail=broken');
  assert.equal(await refusal(broken.startUninstall(DISCORD), 10), 'broken');
  assert.deepEqual((await answer(broken.appsView())).follow, { phase: 'idle' });

  const cancelled = demo('?appsfail=cancelled');
  assert.equal(await refusal(cancelled.startUninstall(DISCORD), 800), 'cancelled');
  assert.deepEqual((await answer(cancelled.appsView())).follow, { phase: 'idle' });
});

test('?appsfail=kept : l’installeur sort, l’app reste, et la vue le dit une seule fois', async () => {
  const backend = demo('?appsfail=kept');
  await answer(backend.startUninstall(DISCORD), 800);
  const view = await poll(backend, 3200);
  assert.deepEqual(view.follow, { phase: 'idle' });
  assert.equal(view.outcome, 'kept');
  assert.ok(view.apps.some((app) => app.id === DISCORD));
  const after = await poll(backend, 1000);
  assert.equal(after.outcome, undefined, 'dit une seule fois');
});

test('les pages qui s’ouvrent dans une phase : launching, waiting, stuck', async () => {
  const launching = await answer(demo('?appsflow=launching').appsView());
  assert.deepEqual(launching.follow, { phase: 'launching', id: DISCORD, name: 'Discord' });
  // Elle y reste : la démo montre la phase, elle ne l'avance pas.
  assert.equal((await poll(demo('?appsflow=launching'), 60_000)).follow.phase, 'launching');

  const waiting = demo('?appsflow=waiting');
  const first = await answer(waiting.appsView());
  assert.equal(first.follow.phase === 'waiting' && first.follow.seconds <= 1, true);
  const later = await poll(waiting, 30_000);
  assert.equal(later.follow.phase === 'waiting' && later.follow.seconds >= 30, true, 'elle compte les secondes, et ne finit pas seule');

  const stuck = await answer(demo('?appsflow=stuck').appsView());
  assert.equal(stuck.follow.phase === 'waiting' && stuck.follow.seconds >= 25, true);
});

test('les pages qui s’ouvrent sur un bilan : des restes, rien, une app de la même famille, une vérification impossible', async () => {
  const none = (await answer(demo('?appsflow=none').appsView())).follow;
  assert.ok(none.phase === 'removed' && none.leftovers.length === 0 && !none.family && none.complete);
  const family = (await answer(demo('?appsflow=family').appsView())).follow;
  assert.ok(family.phase === 'removed' && family.family && family.leftovers.length === 0);
  const unsure = (await answer(demo('?appsflow=unsure').appsView())).follow;
  assert.ok(unsure.phase === 'removed' && !unsure.complete && unsure.leftovers.length === 0);
});

test('« Désinstaller » finit sur le même bilan que celui que ?appsflow= montre', async () => {
  for (const flow of ['none', 'family', 'unsure'] as const) {
    const backend = demo(`?appsflow=${flow}`);
    await answer(backend.stopUninstall());
    await answer(backend.startUninstall('ac:Spotify'), 800);
    await poll(backend, 3100);
    const done = (await poll(backend, 1600)).follow;
    assert.ok(done.phase === 'removed', flow);
    assert.equal(done.name, 'Spotify');
    assert.equal(done.family, flow === 'family');
    assert.equal(done.complete, flow !== 'unsure');
    assert.equal(done.leftovers.length, 0);
  }
});

test('la corbeille : les restes demandés partent, leur taille est rendue, les clés se renumérotent', async () => {
  const backend = demo('?appsflow=leftovers');
  const answered = await answer(backend.removeLeftovers([0, 2]), 400);
  assert.deepEqual(answered.results, [
    { key: 0, outcome: 'removed' },
    { key: 2, outcome: 'removed' },
  ]);
  assert.equal(answered.bytes, 1.1 * 1024 ** 3 + 1.2 * 1024);
  const follow = answered.view.follow;
  assert.ok(follow.phase === 'removed');
  // Le reste qui n'est pas parti est maintenant le premier : son rang a changé.
  assert.deepEqual(follow.leftovers.map((row) => [row.key, row.kind]), [[0, 'data']]);
  // Une nouvelle recherche ne retrouve pas ce qui est parti.
  const again = await answer(backend.rescanLeftovers());
  assert.ok(again.follow.phase === 'removed' && again.follow.leftovers.length === 1);
});

test('une clé inconnue est refusée ; la corbeille qui refuse (?appsfail=bin) laisse tout en place, avec les trois raisons', async () => {
  const backend = demo('?appsflow=leftovers');
  const unknown = await answer(backend.removeLeftovers([9]), 400);
  assert.deepEqual(unknown.results, [{ key: 9, outcome: 'refused' }]);
  assert.equal(unknown.bytes, 0);

  const bin = demo('?appsflow=leftovers&appsfail=bin');
  const failed = await answer(bin.removeLeftovers([0, 1, 2]), 400);
  assert.deepEqual(failed.results, [
    { key: 0, outcome: 'failed', reason: 'inUse' },
    { key: 1, outcome: 'failed', reason: 'refused' },
    { key: 2, outcome: 'failed', reason: 'partial' },
  ]);
  assert.equal(failed.bytes, 0);
  assert.ok(failed.view.follow.phase === 'removed' && failed.view.follow.leftovers.length === 3, 'ce qui n’est pas parti reste dans la liste');
});

test('la corbeille et « Chercher à nouveau » ne valent qu’après la désinstallation', async () => {
  const backend = demo();
  assert.equal(await refusal(backend.removeLeftovers([0]), 400), 'busy');
  assert.equal(await refusal(backend.rescanLeftovers(), 10), 'busy');
});

test('le faux cœur ne renvoie jamais un objet qu’on pourrait modifier de l’extérieur', async () => {
  const backend = demo();
  const first = await answer(backend.appsView());
  first.apps.length = 0;
  assert.equal((await answer(backend.appsView())).apps.length, 15);
});

test('?appsfail=destroyed : la ligne supprimée pour de bon reste dans la vue, un second envoi dit « déjà parti », « Chercher à nouveau » ne la retrouve plus', async () => {
  const backend = demo('?appsflow=leftovers&appsfail=destroyed');
  const before = await answer(backend.appsView());
  assert.equal(before.follow.phase, 'removed');
  const rows = before.follow.phase === 'removed' ? before.follow.leftovers : [];
  const result = await answer(backend.removeLeftovers([rows[0]?.key ?? 0]), 400);
  assert.deepEqual(result.results, [{ key: rows[0]?.key, outcome: 'failed', reason: 'destroyed' }]);
  assert.equal(result.bytes, 0);
  const kept = result.view.follow.phase === 'removed' ? result.view.follow.leftovers : [];
  assert.deepEqual(kept.map((row) => row.path), rows.map((row) => row.path), 'le vrai cœur garde tout ce qui n’est pas parti');
  // Renvoyée, elle est « déjà partie » et quitte la vue.
  const again = await answer(backend.removeLeftovers([rows[0]?.key ?? 0]), 400);
  assert.deepEqual(again.results, [{ key: rows[0]?.key, outcome: 'alreadyGone' }]);
  const left = again.view.follow.phase === 'removed' ? again.view.follow.leftovers : [];
  assert.equal(left.length, rows.length - 1);

  // Une recherche neuve ne retrouve pas ce qui a été supprimé.
  const other = demo('?appsflow=leftovers&appsfail=destroyed');
  const first = await answer(other.appsView());
  const paths = first.follow.phase === 'removed' ? first.follow.leftovers.map((row) => row.path) : [];
  await answer(other.removeLeftovers([0]), 400);
  const rescanned = await answer(other.rescanLeftovers());
  const found = rescanned.follow.phase === 'removed' ? rescanned.follow.leftovers.map((row) => row.path) : [];
  assert.deepEqual(found, paths.slice(1));
});
