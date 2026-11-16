// Le faux cœur du démarrage : il réagit vraiment aux gestes, et il rend ce que
// les paramètres de la démo annoncent.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { type BootDemo, demoOptions, mockBackend } from '../src/backend/mock.ts';
import type { Backend, StartupEntry, StartupRefusal, StartupView } from '../src/backend/types.ts';

const DAY = 24 * 3600 * 1000;

function demo(query = ''): Backend {
  return mockBackend(demoOptions(query));
}

const byName = (view: StartupView, name: string): StartupEntry => {
  const found = view.entries.find((entry) => entry.name === name);
  assert.ok(found !== undefined, name);
  return found;
};

async function refusal(promise: Promise<unknown>): Promise<StartupRefusal | null> {
  try {
    await promise;
    return null;
  } catch (error) {
    const refused = error as { kind?: string; reason?: StartupRefusal };
    assert.equal(refused.kind, 'startupRefused');
    return refused.reason ?? null;
  }
}

test('les paramètres d’adresse du démarrage sont lus, et le reste prend les valeurs par défaut', () => {
  const expected = (parts: Partial<BootDemo>): BootDemo => ({ data: 'few', fail: false, read: 'all', load: 'ok', shinku: 'on', ...parts });
  assert.deepEqual(demoOptions('').boot, expected({}));
  assert.deepEqual(demoOptions('?boot=many').boot, expected({ data: 'many' }));
  assert.deepEqual(demoOptions('?boot=empty').boot, expected({ data: 'empty' }));
  assert.deepEqual(demoOptions('?boot=few').boot, expected({}));
  assert.deepEqual(demoOptions('?bootfail=1').boot, expected({ fail: true }));
  assert.deepEqual(demoOptions('?bootread=user').boot, expected({ read: 'user' }));
  assert.deepEqual(demoOptions('?bootread=machine').boot, expected({ read: 'machine' }));
  assert.deepEqual(demoOptions('?boot=many&bootfail=1&bootread=machine').boot, expected({ data: 'many', fail: true, read: 'machine' }));
  // Une valeur inconnue ne change rien ; `bootfail` ne vaut que « 1 ».
  assert.deepEqual(demoOptions('?boot=xyz&bootfail=oui&bootread=xyz').boot, expected({}));
  assert.deepEqual(demoOptions('?bootfail=0').boot, expected({}));
  assert.deepEqual(demoOptions('?bootload=fail').boot, expected({ load: 'fail' }));
  assert.deepEqual(demoOptions('?bootshinku=off').boot, expected({ shinku: 'off' }));
  assert.deepEqual(demoOptions('?bootload=oui&bootshinku=non').boot, expected({}));
  // Le Mode jeu garde son `?startup=` : il ne touche pas au démarrage, et inversement.
  assert.deepEqual(demoOptions('?startup=disabled').boot, expected({}));
  assert.equal(demoOptions('?boot=many&bootfail=1').game.startup, 'enabled');
});

test('par défaut, la démo montre douze apps : huit du compte, quatre du PC, le compte d’abord', async () => {
  const view = await demo().startupView();
  assert.deepEqual(view.unreadable, []);
  assert.equal(view.entries.length, 12);
  assert.deepEqual(
    view.entries.map((entry) => entry.scope),
    [...Array<string>(8).fill('user'), ...Array<string>(4).fill('machine')],
  );
  // Triées par nom sans casse dans chaque portée.
  assert.deepEqual(
    view.entries.filter((entry) => entry.scope === 'user').map((entry) => entry.name),
    ['Ancien outil', 'Discord', 'Lancement rapide', 'Microsoft OneDrive', 'Mise à jour Adobe', 'Shinku', 'Spotify', 'Steam'],
  );
  const ids = view.entries.map((entry) => entry.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const id of ids) {
    assert.match(id, /^(ur|uw|uf|mr|mw|mf)-[0-9a-f]{16}$/);
  }
});

test('chaque ligne de la démo montre ce que le cahier des charges lui donne', async () => {
  const view = await demo().startupView();
  const now = Date.now();

  const discord = byName(view, 'Discord');
  assert.equal(discord.publisher, 'Discord Inc.');
  assert.equal(discord.state, 'enabled');
  assert.equal(discord.adjustable, true);

  const steam = byName(view, 'Steam');
  assert.equal(steam.publisher, 'Valve Corporation');
  assert.equal(steam.state, 'disabled');
  assert.equal(steam.mark?.kind, 'cutByShinku');
  assert.ok(Math.abs(now - 3 * DAY - (steam.disabledAt ?? 0)) < 5000, 'coupée il y a trois jours');
  assert.deepEqual(steam.mark, { kind: 'cutByShinku', at: steam.disabledAt });

  const onedrive = byName(view, 'Microsoft OneDrive');
  assert.equal(onedrive.alias, 'OneDrive');
  assert.equal(onedrive.publisher, 'Microsoft Corporation');
  assert.equal(onedrive.hint, 'sync');

  const spotify = byName(view, 'Spotify');
  assert.equal(spotify.state, 'enabled');
  assert.equal(spotify.mark?.kind, 'cameBack');
  assert.ok(spotify.mark?.kind === 'cameBack' && Math.abs(now - 5 * DAY - spotify.mark.cut) < 5000, 'coupure il y a cinq jours');

  const adobe = byName(view, 'Mise à jour Adobe');
  assert.equal(adobe.origin, 'folder');
  assert.equal(adobe.target, 'missing');
  assert.equal(adobe.adjustable, true, 'une entrée « missing » se règle comme les autres');

  const quick = byName(view, 'Lancement rapide');
  assert.equal(quick.target, 'hosted');
  assert.equal(quick.host, 'rundll32.exe');

  const own = byName(view, 'Shinku');
  assert.equal(own.own, true);
  assert.equal(own.adjustable, false);

  const old = byName(view, 'Ancien outil');
  assert.equal(old.state, 'unknown');
  assert.equal(old.adjustable, false);

  const machine = view.entries.filter((entry) => entry.scope === 'machine');
  assert.deepEqual(machine.map((entry) => entry.name), [
    'Java Update Scheduler',
    'Logitech G HUB',
    'Realtek HD Audio Universal Service',
    'Sécurité Windows',
  ]);
  assert.ok(machine.every((entry) => !entry.adjustable), 'aucune entrée du PC ne se règle dans ce lot');
  assert.equal(byName(view, 'Sécurité Windows').alias, 'SecurityHealth');
  assert.equal(byName(view, 'Sécurité Windows').publisher, 'Microsoft Corporation');
  assert.equal(byName(view, 'Realtek HD Audio Universal Service').publisher, 'Realtek Semiconductor');
  assert.equal(byName(view, 'Java Update Scheduler').publisher, 'Oracle Corporation');
  // Logitech : coupée le 2 nov. de l'an dernier, sans marque de Shinku.
  const logitech = byName(view, 'Logitech G HUB');
  assert.equal(logitech.publisher, 'Logitech, Inc.');
  assert.equal(logitech.state, 'disabled');
  assert.equal(logitech.mark, null);
  const cutOn = new Date(logitech.disabledAt ?? 0);
  assert.deepEqual([cutOn.getFullYear(), cutOn.getMonth(), cutOn.getDate()], [new Date(now).getFullYear() - 1, 10, 2]);
});

test('« many » montre quarante apps de toutes sortes, « empty » aucune', async () => {
  const many = await demo('?boot=many').startupView();
  assert.equal(many.entries.length, 40);
  assert.equal(new Set(many.entries.map((entry) => entry.id)).size, 40);
  const has = (test: (entry: StartupEntry) => boolean, what: string): void =>
    assert.ok(many.entries.some(test), `la démo « many » doit montrer ${what}`);
  has((entry) => entry.scope === 'user', 'le compte');
  has((entry) => entry.scope === 'machine', 'le PC');
  has((entry) => entry.state === 'disabled', 'une coupée');
  has((entry) => entry.state === 'unknown', 'une illisible');
  has((entry) => entry.mark?.kind === 'cutByShinku', 'une coupée par Shinku');
  has((entry) => entry.mark?.kind === 'cameBack', 'une remise toute seule');
  has((entry) => entry.target === 'missing', 'une désinstallée');
  has((entry) => entry.target === 'hosted', 'une lancée par un hôte');
  has((entry) => entry.target === 'unknown', 'une cible illisible');
  has((entry) => entry.origin === 'folder', 'un raccourci');
  has((entry) => entry.hint === 'sync', 'une synchronisation');
  has((entry) => entry.publisher === null && entry.target === 'found', 'un éditeur inconnu');
  // La règle du cœur tient aussi dans la démo : seules les entrées du compte, lues, se règlent.
  assert.ok(many.entries.every((entry) => entry.adjustable === (entry.scope === 'user' && entry.state !== 'unknown' && !entry.own)));

  const empty = await demo('?boot=empty').startupView();
  assert.deepEqual(empty, { entries: [], unreadable: [] });
});

test('« bootread » déclare une portée illisible, sans retirer ce qui a été lu', async () => {
  const user = await demo('?bootread=user').startupView();
  assert.deepEqual(user.unreadable, ['user']);
  assert.equal(user.entries.length, 12);
  assert.deepEqual((await demo('?bootread=machine').startupView()).unreadable, ['machine']);
  assert.deepEqual((await demo('?boot=empty&bootread=machine').startupView()).unreadable, ['machine']);
});

test('les interrupteurs fonctionnent en mémoire : couper, puis remettre', async () => {
  const backend = demo();
  const before = await backend.startupView();
  const discord = byName(before, 'Discord');

  const cut = await backend.setStartupEntry(discord.id, false);
  const now = Date.now();
  const after = byName(cut, 'Discord');
  assert.equal(after.state, 'disabled');
  assert.ok(Math.abs(now - (after.disabledAt ?? 0)) < 5000);
  assert.deepEqual(after.mark, { kind: 'cutByShinku', at: after.disabledAt });
  // Rien d'autre n'a bougé, et la vue suivante dit la même chose.
  assert.deepEqual(
    cut.entries.filter((entry) => entry.id !== discord.id),
    before.entries.filter((entry) => entry.id !== discord.id),
  );
  assert.deepEqual(await backend.startupView(), cut);

  // « Annuler » : le geste inverse remet la ligne comme elle était.
  const restored = await backend.setStartupEntry(discord.id, true);
  assert.deepEqual(restored, before);
});

test('remettre une entrée coupée par Shinku ou remise toute seule efface sa marque', async () => {
  const backend = demo();
  const view = await backend.startupView();
  const steam = await backend.setStartupEntry(byName(view, 'Steam').id, true);
  assert.equal(byName(steam, 'Steam').state, 'enabled');
  assert.equal(byName(steam, 'Steam').mark, null);
  assert.equal(byName(steam, 'Steam').disabledAt, null);
  // Spotify s'était remise seule ; la couper la marque coupée par Shinku, et plus remise.
  const spotify = await backend.setStartupEntry(byName(view, 'Spotify').id, false);
  assert.equal(byName(spotify, 'Spotify').mark?.kind, 'cutByShinku');
});

test('le même geste deux fois ne change rien et n’est pas refusé', async () => {
  const backend = demo();
  const view = await backend.startupView();
  const discord = byName(view, 'Discord');
  assert.deepEqual(await backend.setStartupEntry(discord.id, true), view);
  const cut = await backend.setStartupEntry(discord.id, false);
  assert.deepEqual(await backend.setStartupEntry(discord.id, false), cut, 'la date de la coupure ne bouge pas');
});

test('la vue rendue est une copie : la modifier ne change pas la démo', async () => {
  const backend = demo();
  const view = await backend.startupView();
  view.entries[0] = { ...view.entries[0]!, name: 'Modifié' };
  view.entries.pop();
  assert.equal((await backend.startupView()).entries.length, 12);
  assert.equal((await backend.startupView()).entries[0]?.name, 'Ancien outil');
});

test('le faux cœur refuse comme le vrai : entrée inconnue, entrée qui ne se règle pas', async () => {
  const backend = demo();
  const view = await backend.startupView();
  assert.equal(await refusal(backend.setStartupEntry('ur-0000000000000000', false)), 'unknownEntry');
  assert.equal(await refusal(backend.setStartupEntry('n’importe quoi', false)), 'unknownEntry');
  for (const name of ['Shinku', 'Ancien outil', 'Sécurité Windows', 'Logitech G HUB']) {
    assert.equal(await refusal(backend.setStartupEntry(byName(view, name).id, true)), 'notAdjustable', name);
  }
  assert.deepEqual(await backend.startupView(), view, 'un refus ne change rien');
});

test('« bootfail » fait échouer tout changement, la vue reste lisible et ne bouge pas', async () => {
  const backend = demo('?bootfail=1');
  const view = await backend.startupView();
  assert.equal(await refusal(backend.setStartupEntry(byName(view, 'Discord').id, false)), 'failed');
  assert.equal(await refusal(backend.setStartupEntry(byName(view, 'Steam').id, true)), 'failed');
  assert.deepEqual(await backend.startupView(), view);
});

test('le résumé d’analyse compte les apps actives hors Shinku, et se tait si une place est illisible', async () => {
  const pause = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));
  const summaryOf = async (backend: Backend) => {
    await backend.startScan();
    await pause(20);
    const results = await backend.sessionView();
    const found = results.summary?.categories.find((category) => category.module === 'startup');
    assert.ok(found !== undefined);
    return found;
  };
  const options = { ...demoOptions(''), speed: 1_000_000 };
  const backend = mockBackend(options);
  // Discord, OneDrive, Spotify, Adobe, Lancement rapide ; Sécurité Windows, Realtek, Java. Pas Shinku, pas l'illisible, pas les coupées.
  assert.deepEqual(await summaryOf(backend), { module: 'startup', lot: 'review', risk: 'C', bytes: 0, items: 8, state: 'ready' });
  await backend.setStartupEntry(byName(await backend.startupView(), 'Discord').id, false);
  assert.equal((await summaryOf(backend)).items, 7);

  const unreadable = await summaryOf(mockBackend({ ...options, boot: { data: 'few', fail: false, read: 'machine', load: 'ok', shinku: 'on' } }));
  assert.equal(unreadable.state, 'unavailable');
  assert.equal((await summaryOf(mockBackend({ ...options, boot: { data: 'empty', fail: false, read: 'all', load: 'ok', shinku: 'on' } }))).items, 0);
});

test('l’ordre des refus est celui du cœur : entrée inconnue, entrée qui ne se règle pas, puis l’échec', async () => {
  const backend = demo('?bootfail=1');
  const view = await backend.startupView();
  assert.equal(await refusal(backend.setStartupEntry('ur-0000000000000000', false)), 'unknownEntry');
  assert.equal(await refusal(backend.setStartupEntry(byName(view, 'Shinku').id, false)), 'notAdjustable');
  assert.equal(await refusal(backend.setStartupEntry(byName(view, 'Sécurité Windows').id, false)), 'notAdjustable');
  assert.equal(await refusal(backend.setStartupEntry(byName(view, 'Discord').id, false)), 'failed');
});

test('le tri est celui du cœur : compte d’abord, puis nom en minuscules, alias (l’absence d’abord), identifiant, au point de code', async () => {
  for (const query of ['', '?boot=many']) {
    const { entries } = await demo(query).startupView();
    const text = (left: string, right: string): number => (left < right ? -1 : left > right ? 1 : 0);
    for (const [index, right] of entries.entries()) {
      const left = entries[index - 1];
      if (left === undefined) {
        continue;
      }
      const order =
        (left.scope === right.scope ? 0 : left.scope === 'user' ? -1 : 1) ||
        text(left.name.toLowerCase(), right.name.toLowerCase()) ||
        (left.alias === null ? (right.alias === null ? 0 : -1) : right.alias === null ? 1 : text(left.alias.toLowerCase(), right.alias.toLowerCase())) ||
        text(left.id, right.id);
      assert.ok(order <= 0, `${left.name} avant ${right.name} (${query})`);
    }
  }
  // Au point de code, « É » vient après « z » : pas l'ordre d'une langue.
  const names = (await demo().startupView()).entries.filter((entry) => entry.scope === 'machine').map((entry) => entry.name);
  assert.equal(names.at(-1), 'Sécurité Windows');
});

test('une cible introuvable ou illisible n’a ni éditeur ni synchronisation, dans toutes les démos', async () => {
  for (const query of ['', '?boot=many']) {
    const { entries } = await demo(query).startupView();
    const notFound = entries.filter((entry) => entry.target !== 'found');
    assert.ok(notFound.length > 0, query);
    for (const entry of notFound) {
      assert.equal(entry.publisher, null, entry.name);
      assert.equal(entry.hint, null, entry.name);
    }
  }
});

test('« bootshinku=off » montre Shinku coupé dans Windows ; « bootload=fail » rate la première lecture seulement', async () => {
  const off = await demo('?bootshinku=off').startupView();
  const own = byName(off, 'Shinku');
  assert.equal(own.state, 'disabled');
  assert.equal(own.own, true);
  assert.equal(own.adjustable, false);
  assert.equal((await demo().startupView()).entries.find((entry) => entry.own)?.state, 'enabled');

  const backend = demo('?bootload=fail');
  await assert.rejects(backend.startupView());
  assert.equal((await backend.startupView()).entries.length, 12);
  assert.equal((await backend.startupView()).entries.length, 12);
});

test('le compte de l’analyse est lu une fois, à la fin : en route il n’est pas mesuré, et couper ensuite ne le change pas', async () => {
  const pause = (milliseconds: number) => new Promise((resolve) => setTimeout(resolve, milliseconds));
  const startupOf = async (backend: Backend) =>
    (await backend.sessionView()).summary?.categories.find((category) => category.module === 'startup');
  // Lente : la fin n'est pas atteinte, le démarrage attend comme les doublons.
  const slow = mockBackend({ ...demoOptions(''), speed: 100 });
  await slow.startScan();
  await pause(50);
  const early = await startupOf(slow);
  assert.ok(early !== undefined);
  assert.deepEqual({ items: early.items, state: early.state }, { items: 0, state: 'pending' });

  const backend = mockBackend({ ...demoOptions(''), speed: 1_000_000 });
  await backend.startScan();
  await pause(20);
  assert.equal((await startupOf(backend))?.items, 8);
  await backend.setStartupEntry(byName(await backend.startupView(), 'Discord').id, false);
  assert.equal((await startupOf(backend))?.items, 8, 'le compte de cette analyse ne bouge pas');
  await backend.startScan();
  await pause(20);
  assert.equal((await startupOf(backend))?.items, 7, 'une nouvelle analyse relit');
});
