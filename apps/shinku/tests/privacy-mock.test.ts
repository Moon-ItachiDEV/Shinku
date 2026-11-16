// Le faux cœur de la confidentialité : il rend ce que les paramètres de la démo annoncent, et il suit
// les règles du cœur (identifiants opaques, `tooMany` au-delà de 64, issues par trace, vue relue après).

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { demoOptions, MAX_ERASE, mockBackend, type PrivacyDemo } from '../src/backend/mock.ts';
import type { Backend, EraseResult, PrivacyGroup, PrivacyRefusal, PrivacyView, TraceItem } from '../src/backend/types.ts';

/** Vite : la démo attend un peu pour qu'on voie la page inerte ; les essais n'attendent presque pas. */
function demo(query = '', speed = 1000): Backend {
  return mockBackend({ ...demoOptions(query), speed });
}

const items = (view: PrivacyView): TraceItem[] => view.groups.flatMap((group) => group.items);
const present = (view: PrivacyView): string[] => items(view).filter((item) => item.state === 'present').map((item) => item.id);
const named = (view: PrivacyView, key: string): PrivacyGroup => {
  const found = view.groups.find((group) => group.key === key);
  assert.ok(found !== undefined, key);
  return found;
};

async function refusal(promise: Promise<unknown>): Promise<PrivacyRefusal | null> {
  try {
    await promise;
    return null;
  } catch (error) {
    const refused = error as { kind?: string; reason?: PrivacyRefusal };
    assert.equal(refused.kind, 'privacyRefused');
    return refused.reason ?? null;
  }
}

const outcomes = (results: EraseResult[]): string[] => results.map((result) => result.outcome);

test('les paramètres d’adresse de la confidentialité sont lus, et le reste prend les valeurs par défaut', () => {
  const expected = (parts: Partial<PrivacyDemo>): PrivacyDemo => ({ data: 'few', flow: 'done', ...parts });
  assert.deepEqual(demoOptions('').privacy, expected({}));
  for (const data of ['few', 'many', 'none', 'running'] as const) {
    assert.deepEqual(demoOptions(`?privacy=${data}`).privacy, expected({ data }));
  }
  for (const flow of ['done', 'inuse', 'changed', 'partial', 'fail'] as const) {
    assert.deepEqual(demoOptions(`?privacyflow=${flow}`).privacy, expected({ flow }));
  }
  assert.deepEqual(demoOptions('?privacy=many&privacyflow=partial').privacy, expected({ data: 'many', flow: 'partial' }));
  // Une valeur inconnue ne change rien.
  assert.deepEqual(demoOptions('?privacy=xyz&privacyflow=oui').privacy, expected({}));
  // Les autres modules gardent leurs paramètres.
  assert.equal(demoOptions('?privacy=many').boot.data, 'few');
  assert.equal(demoOptions('?boot=many').privacy.data, 'few');
});

test('?privacy=few : Chrome et Edge, une trace vide, Windows ; Edge peut réimporter Chrome', async () => {
  const view = await demo('?privacy=few').privacyView();
  assert.deepEqual(
    view.groups.map((group) => [group.key, group.kind, group.app, group.profile, group.slot, group.running]),
    [
      ['chrome:0', 'browser', 'chrome', 'Léa', 0, false],
      ['chrome:2', 'browser', 'chrome', null, 2, false],
      ['edge:0', 'browser', 'edge', null, 0, false],
      ['windows', 'windows', null, null, null, false],
    ],
  );
  assert.equal(named(view, 'chrome:2').items[0]?.state, 'empty');
  assert.deepEqual(named(view, 'windows').items.map((item) => item.kind), ['recent', 'typed', 'clipboard']);
  assert.equal(view.edgeImportsChrome, true);
  assert.equal(view.firefox, false);
});

test('?privacy=many : plusieurs profils, une trace illisible, une vide, Firefox', async () => {
  const view = await demo('?privacy=many').privacyView();
  assert.ok(view.groups.length >= 8);
  assert.ok(items(view).some((item) => item.state === 'unreadable'));
  assert.ok(items(view).some((item) => item.state === 'empty'));
  assert.ok(view.groups.some((group) => group.app === 'brave') && view.groups.some((group) => group.app === 'vivaldi'));
  assert.equal(view.firefox, true);
  assert.equal(view.edgeImportsChrome, true);
  assert.ok(view.groups.filter((group) => group.app === 'chrome').length >= 3);
  assert.ok(present(view).length <= MAX_ERASE, 'la démo ne dépasse pas ce que le cœur accepte');
});

test('?privacy=none : rien à effacer nulle part, pas de navigateur', async () => {
  const view = await demo('?privacy=none').privacyView();
  assert.deepEqual(view.groups.map((group) => group.key), ['windows']);
  assert.deepEqual(present(view), []);
  assert.equal(view.firefox, false);
  assert.equal(view.edgeImportsChrome, false);
});

test('?privacy=running : Chrome est ouvert (tous ses profils), Edge ne l’est pas', async () => {
  const view = await demo('?privacy=running').privacyView();
  assert.deepEqual(
    view.groups.map((group) => [group.key, group.running]),
    [['chrome:0', true], ['chrome:2', true], ['edge:0', false], ['windows', false]],
  );
  // Les traces d'un navigateur ouvert sont bien là : elles sont seulement inertes.
  assert.equal(named(view, 'chrome:0').items[0]?.state, 'present');
});

test('les identifiants sont opaques, uniques et stables d’une lecture à l’autre ; ils ne disent rien du contenu', async () => {
  const backend = demo('?privacy=many');
  const first = await backend.privacyView();
  const second = await backend.privacyView();
  assert.deepEqual(items(first).map((item) => item.id), items(second).map((item) => item.id));
  const ids = items(first).map((item) => item.id);
  assert.equal(new Set(ids).size, ids.length);
  for (const id of ids) {
    assert.match(id, /^tr-[0-9a-f]{8}$/);
  }
  // Une lecture ne rend pas l'état interne : la modifier ne change rien.
  (first.groups[0] as PrivacyGroup).items = [];
  assert.ok(((await backend.privacyView()).groups[0] as PrivacyGroup).items.length > 0);
});

test('effacer (done) : issue par trace, la vue est relue, les traces effacées sont vides', async () => {
  const backend = demo('?privacy=few');
  const view = await backend.privacyView();
  const wanted = [named(view, 'chrome:0').items[0]?.id as string, named(view, 'windows').items[2]?.id as string];
  const answer = await backend.eraseTraces(wanted);
  assert.deepEqual(answer.results, wanted.map((id) => ({ id, outcome: 'erased' })));
  assert.equal(named(answer.view, 'chrome:0').items[0]?.state, 'empty');
  assert.equal(named(answer.view, 'chrome:0').items[0]?.bytes, null);
  assert.equal(named(answer.view, 'windows').items[2]?.state, 'empty');
  // Ce qui n'était pas demandé n'a pas bougé.
  assert.equal(named(answer.view, 'edge:0').items[0]?.state, 'present');
  assert.deepEqual(await backend.privacyView(), answer.view, 'la vue relue ensuite est celle de la réponse');
  // Les effacer de nouveau : rien à effacer.
  assert.deepEqual(outcomes((await backend.eraseTraces(wanted)).results), ['nothing', 'nothing']);
});

test('?privacyflow=inuse : les navigateurs sont en cours d’utilisation, rien n’est touché ; Windows s’efface', async () => {
  const backend = demo('?privacy=few&privacyflow=inuse');
  const view = await backend.privacyView();
  const answer = await backend.eraseTraces(present(view));
  const byId = new Map(answer.results.map((result) => [result.id, result.outcome]));
  assert.equal(byId.get(named(view, 'chrome:0').items[0]?.id as string), 'inUse');
  assert.equal(byId.get(named(view, 'edge:0').items[0]?.id as string), 'inUse');
  assert.equal(byId.get(named(view, 'windows').items[0]?.id as string), 'erased');
  assert.equal(named(answer.view, 'chrome:0').items[0]?.state, 'present');
  assert.equal(named(answer.view, 'chrome:0').items[0]?.bytes, named(view, 'chrome:0').items[0]?.bytes);
});

test('?privacyflow=changed : le profil a changé, rien n’est touché', async () => {
  const backend = demo('?privacy=few&privacyflow=changed');
  const view = await backend.privacyView();
  const answer = await backend.eraseTraces(present(view));
  const byId = new Map(answer.results.map((result) => [result.id, result.outcome]));
  assert.equal(byId.get(named(view, 'edge:0').items[0]?.id as string), 'changed');
  assert.equal(named(answer.view, 'edge:0').items[0]?.state, 'present');
  assert.equal(byId.get(named(view, 'windows').items[1]?.id as string), 'erased');
});

test('?privacyflow=partial : les récents et ce qu’on a tapé dans Windows ne sont effacés qu’en partie, il en reste', async () => {
  const backend = demo('?privacy=few&privacyflow=partial');
  const view = await backend.privacyView();
  const [recent, typed, clipboard] = named(view, 'windows').items as [TraceItem, TraceItem, TraceItem];
  const answer = await backend.eraseTraces([recent.id, typed.id, clipboard.id]);
  assert.deepEqual(outcomes(answer.results), ['partial', 'partial', 'erased']);
  const after = named(answer.view, 'windows').items as [TraceItem, TraceItem, TraceItem];
  for (const [index, before] of [recent, typed].entries()) {
    const now = after[index] as TraceItem;
    assert.equal(now.state, 'present');
    assert.ok((now.count ?? 99) < (before.count ?? 0), 'il en reste moins qu’avant');
  }
  assert.equal(after[2].state, 'empty');
});

test('?privacyflow=fail : toutes les traces demandées échouent, rien ne change', async () => {
  const backend = demo('?privacy=few&privacyflow=fail');
  const view = await backend.privacyView();
  const answer = await backend.eraseTraces(present(view));
  assert.ok(answer.results.every((result) => result.outcome === 'failed'));
  assert.deepEqual(answer.view, view);
});

test('un navigateur ouvert : « en cours d’utilisation », rien n’est touché, même si la page l’avait coché', async () => {
  const backend = demo('?privacy=running');
  const view = await backend.privacyView();
  const open = named(view, 'chrome:0').items[0]?.id as string;
  const answer = await backend.eraseTraces([open]);
  assert.deepEqual(answer.results, [{ id: open, outcome: 'inUse' }]);
  assert.equal(named(answer.view, 'chrome:0').items[0]?.state, 'present');
});

test('un identifiant que le cœur n’a pas fabriqué est refusé, rien n’est touché ; un doublon n’est traité qu’une fois', async () => {
  const backend = demo('?privacy=few');
  const view = await backend.privacyView();
  const id = named(view, 'edge:0').items[0]?.id as string;
  const answer = await backend.eraseTraces(['../../Login Data', id, id]);
  assert.deepEqual(answer.results, [
    { id: '../../Login Data', outcome: 'changed' },
    { id, outcome: 'erased' },
  ]);
  assert.equal(named(answer.view, 'chrome:0').items[0]?.state, 'present');
});

test('au-delà de 64 traces, tout est refusé (tooMany), même ce qui existe ; 64 passent', async () => {
  const backend = demo('?privacy=few');
  const view = await backend.privacyView();
  const real = present(view);
  const many = Array.from({ length: MAX_ERASE + 1 }, (_, index) => real[index % real.length] as string);
  assert.equal(await refusal(backend.eraseTraces(many)), 'tooMany');
  assert.deepEqual(await backend.privacyView(), view, 'rien n’a été effacé');
  const ok = Array.from({ length: MAX_ERASE }, (_, index) => `x${index}`);
  assert.equal((await backend.eraseTraces(ok)).results.length, MAX_ERASE);
});

test('un effacement en cours refuse le second (busy), puis la machine est libre de nouveau', async () => {
  const backend = demo('?privacy=few');
  const view = await backend.privacyView();
  const ids = present(view);
  const first = backend.eraseTraces([ids[0] as string]);
  assert.equal(await refusal(backend.eraseTraces([ids[1] as string])), 'busy');
  await first;
  assert.deepEqual(outcomes((await backend.eraseTraces([ids[1] as string])).results), ['erased']);
});

test('busy passe avant tooMany : 65 traces pendant un effacement en cours sont refusées comme busy ; au repos, tooMany', async () => {
  const backend = demo('?privacy=few');
  const view = await backend.privacyView();
  const ids = present(view);
  const many = Array.from({ length: MAX_ERASE + 1 }, (_, index) => `x${index}`);
  const first = backend.eraseTraces([ids[0] as string]);
  assert.equal(await refusal(backend.eraseTraces(many)), 'busy');
  await first;
  assert.equal(await refusal(backend.eraseTraces(many)), 'tooMany');
});

test('un grand vide en cours refuse l’effacement (busy)', async () => {
  const backend = demo('', 40);
  await backend.startScan();
  await new Promise((resolve) => setTimeout(resolve, 400));
  const results = await backend.sessionView();
  assert.equal(results.state, 'results');
  assert.ok(results.summary !== null);
  await backend.startClean({ analysis: results.summary.started, recycleBin: true, junkFiles: true, duplicates: true });
  const view = await backend.privacyView();
  assert.equal(await refusal(backend.eraseTraces([present(view)[0] as string])), 'busy');
});
