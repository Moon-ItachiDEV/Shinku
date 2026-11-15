// Le choix de l'exemplaire gardé d'un doublon : la règle que l'écran et la
// démo répètent après le cœur.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { demoOptions, mockBackend } from '../src/backend/mock.ts';
import type { DuplicateGroup } from '../src/backend/types.ts';
import { keepIn, sharedFolders } from '../src/model/duplicates.ts';

const MB = 1024 ** 2;

function group(digest: string, name: string, folders: string[], size = 10 * MB): DuplicateGroup {
  return {
    size,
    digest,
    reclaimable: size * (folders.length - 1),
    files: folders.map((folder, index) => ({
      path: `${folder}\\${name}`,
      onDisk: size,
      created: index,
      modified: 0,
      keep: index === 0,
      synced: false,
    })),
  };
}

const LOOSE = 'C:\\Users\\Léa\\Desktop\\Nouveau dossier (3)';
const SAFE = 'C:\\Users\\Léa\\Desktop\\a ne pas perdre';
const GROUPS = [
  group('aa', 'IMG_6104.MOV', [LOOSE, SAFE]),
  group('bb', 'IMG_4453.MOV', [LOOSE, SAFE]),
  group('cc', 'Facture.pdf', ['C:\\Users\\Léa\\Documents', 'C:\\Users\\Léa\\Downloads', SAFE]),
  group('dd', 'Notes.txt', ['C:\\Users\\Léa\\Documents', 'D:\\Sauvegarde']),
];

const kept = (groups: readonly DuplicateGroup[]): string[][] =>
  groups.map((entry) => entry.files.filter((file) => file.keep).map((file) => file.path));

test('choisir une ligne ne change que son groupe, qui garde un seul exemplaire', () => {
  const after = keepIn(GROUPS, { digest: 'cc', size: 10 * MB, index: 1, wholeFolder: false });
  assert.deepEqual(kept(after), [
    [`${LOOSE}\\IMG_6104.MOV`],
    [`${LOOSE}\\IMG_4453.MOV`],
    ['C:\\Users\\Léa\\Downloads\\Facture.pdf'],
    ['C:\\Users\\Léa\\Documents\\Notes.txt'],
  ]);
  assert.equal(after[2]?.reclaimable, 20 * MB);
});

test('garder un dossier garde son exemplaire dans chaque groupe qui en a un, et pas ailleurs', () => {
  const after = keepIn(GROUPS, { digest: 'aa', size: 10 * MB, index: 1, wholeFolder: true });
  assert.deepEqual(kept(after), [
    [`${SAFE}\\IMG_6104.MOV`],
    [`${SAFE}\\IMG_4453.MOV`],
    [`${SAFE}\\Facture.pdf`],
    ['C:\\Users\\Léa\\Documents\\Notes.txt'],
  ]);
});

test('garder un dossier ne remplace pas un exemplaire déjà choisi dans ce dossier', () => {
  // Deux exemplaires dans le même dossier, un troisième ailleurs.
  const photos = group('ee', 'photo.jpg', [LOOSE, SAFE, SAFE]);
  const files = photos.files.map((file, index) => (index === 2 ? { ...file, path: `${SAFE}\\photo (1).jpg` } : file));
  const twice = [group('aa', 'IMG_6104.MOV', [LOOSE, SAFE]), { ...photos, files }];
  const byHand = keepIn(twice, { digest: 'ee', size: 10 * MB, index: 2, wholeFolder: false });
  assert.deepEqual(kept(byHand)[1], [`${SAFE}\\photo (1).jpg`]);
  // Le groupe garde déjà ce dossier : le bouton le compte, et ne le touche pas.
  assert.deepEqual(sharedFolders(byHand, 4).find((entry) => entry.folder === SAFE)?.kept, 1);
  const folder = keepIn(byHand, { digest: 'aa', size: 10 * MB, index: 1, wholeFolder: true });
  assert.deepEqual(kept(folder), [[`${SAFE}\\IMG_6104.MOV`], [`${SAFE}\\photo (1).jpg`]]);
});

test('un choix qui ne désigne rien ne change rien', () => {
  assert.deepEqual(keepIn(GROUPS, { digest: 'zz', size: 10 * MB, index: 0, wholeFolder: true }), GROUPS);
  assert.deepEqual(keepIn(GROUPS, { digest: 'aa', size: 10 * MB, index: 7, wholeFolder: false }), GROUPS);
  // Même empreinte, autre taille : ce n'est pas le même groupe.
  assert.deepEqual(keepIn(GROUPS, { digest: 'aa', size: 11 * MB, index: 1, wholeFolder: false }), GROUPS);
});

test('les dossiers proposés sont ceux que plusieurs groupes partagent, les plus fournis d’abord', () => {
  const folders = sharedFolders(GROUPS, 4);
  assert.deepEqual(
    folders.map((entry) => [entry.folder, entry.groups, entry.kept]),
    [
      [SAFE, 3, 0],
      [LOOSE, 2, 2],
      ['C:\\Users\\Léa\\Documents', 2, 2],
    ],
  );
  // Le choix d'un dossier le garde partout où il a un exemplaire.
  const safe = folders[0];
  assert.ok(safe !== undefined);
  const after = sharedFolders(keepIn(GROUPS, safe.choice), 4);
  assert.deepEqual(after.find((entry) => entry.folder === SAFE)?.kept, 3);
  assert.equal(sharedFolders(GROUPS, 1).length, 1);
});

test('la démo retient le choix jusqu’à la prochaine analyse', async () => {
  const backend = mockBackend(demoOptions('?speed=100000'));
  const scan = async () => {
    await backend.startScan();
    for (let tries = 0; tries < 200 && (await backend.sessionView()).state !== 'results'; tries += 1) {
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    const page = await backend.duplicates(0, 50);
    assert.ok(page !== null && page.groups.length > 1);
    return page;
  };
  const before = await scan();
  const first = before.groups[0];
  assert.ok(first !== undefined);
  assert.equal(await backend.keepDuplicate({ digest: first.digest, size: first.size, index: 1, wholeFolder: false }), true);
  const after = await backend.duplicates(0, 50);
  assert.deepEqual(after?.groups[0]?.files.map((file) => file.keep).slice(0, 2), [false, true]);
  assert.equal(after?.groups[1]?.files[0]?.keep, true);
  assert.equal(await backend.keepDuplicate({ digest: 'inconnu', size: 1, index: 0, wholeFolder: false }), false);
  // Une nouvelle analyse repart du plus ancien.
  assert.equal((await scan()).groups[0]?.files[0]?.keep, true);
});
