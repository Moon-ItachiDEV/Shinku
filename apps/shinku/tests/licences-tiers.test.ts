// Les licences des composants tiers : le script `scripts/licences-tiers.mjs` et sa garde.
//
// Les fonctions pures du script sont appelées sur de faux `cargo tree`, de faux `cargo metadata` et de faux
// dossiers. Un dernier groupe d'essais lance la garde sur le vrai dépôt (cargo et node_modules doivent y être :
// l'essai ÉCHOUE sans eux, il ne saute pas). Hors ligne, `SHINKU_CARGO_HORS_LIGNE=1` ajoute --offline à cargo.

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { after, test } from 'node:test';

import {
  ALLOWED_LICENSES,
  LICENSE_FILE_NAME,
  MAX_TEXT_BYTES,
  PAGE_DEV_PACKAGES,
  type Package,
  type PackageWithTexts,
  attachTexts,
  build,
  cargoCalls,
  checkExpression,
  checkPackages,
  composeFile,
  licenseTexts,
  runCli,
  sha256,
  writeAtomically,
  normalizeText,
  numberTexts,
  parseArguments,
  parseCargoTree,
  parseExpression,
  readComplements,
  readLicenseFiles,
  resolvePagePackages,
  resolveRustPackages,
  unionPackages,
} from '../scripts/licences-tiers.mjs';

const root = new URL('../', import.meta.url);
const scratch = mkdtempSync(join(tmpdir(), 'shinku-licences-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

let counter = 0;
/** Un dossier neuf, sous le dossier de travail des essais. */
function folder(files: Record<string, string> = {}): string {
  counter += 1;
  const dir = join(scratch, `dossier-${counter}`);
  mkdirSync(dir, { recursive: true });
  for (const [name, content] of Object.entries(files)) {
    mkdirSync(join(dir, name, '..'), { recursive: true });
    writeFileSync(join(dir, name), content);
  }
  return dir;
}

// ── La garde : les expressions de licence ──

test('la garde : une expression passe quand la liste permise la satisfait (OR : une suffit, AND : toutes, WITH : un seul nom)', () => {
  for (const ok of [
    'MIT',
    'MIT OR GPL-3.0',
    'Apache-2.0 WITH LLVM-exception',
    'MIT/Apache-2.0',
    'Apache-2.0 / MIT',
    'Unlicense/MIT',
    '(MIT OR Apache-2.0) AND Unicode-3.0',
    'CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception',
    'BSD-3-Clause AND MIT',
    '(MIT)',
    'GPL-3.0 OR (MIT AND Zlib)',
  ]) {
    assert.equal(checkExpression(ok), null, `« ${ok} » devrait passer`);
  }
});

test('la garde : GPL, une conjonction avec une licence refusée, une exception inconnue et la parenthèse qui change tout échouent', () => {
  for (const bad of ['GPL-3.0', 'MIT AND GPL-3.0', 'GPL-3.0 OR AGPL-3.0', 'MIT WITH Classpath-exception-2.0', '(MIT OR GPL-3.0) AND GPL-3.0', 'LGPL-2.1-or-later']) {
    const problem = checkExpression(bad);
    assert.ok(problem !== null && problem.includes('pas dans la liste permise'), `« ${bad} » devrait échouer : ${String(problem)}`);
  }
  // `WITH` fait partie du nom : « Apache-2.0 » seul n'autorise pas une autre exception, mais le nom complet de la liste passe.
  assert.ok(ALLOWED_LICENSES.includes('Apache-2.0 WITH LLVM-exception'));
  assert.notEqual(checkExpression('Apache-2.0 WITH Autre-exception'), null);
});

test('la garde : une expression illisible ou absente échoue, avec une raison', () => {
  for (const bad of ['MIT OR', 'OR MIT', '(MIT', 'MIT)', 'MIT AND AND Zlib', 'Apache-2.0 WITH', 'MIT Zlib', '()', '/', 'MIT //Zlib']) {
    const problem = checkExpression(bad);
    assert.ok(problem !== null && problem.includes('illisible'), `« ${bad} » devrait être illisible : ${String(problem)}`);
  }
  for (const missing of [null, undefined, '', '   ']) {
    assert.equal(checkExpression(missing), 'aucune licence déclarée');
  }
  assert.throws(() => parseExpression(''));
});

test("la garde : l'arbre garde la structure (AND l'emporte sur OR, les parenthèses sont lues)", () => {
  assert.deepEqual(parseExpression('A OR B AND C'), {
    kind: 'or',
    items: [
      { kind: 'id', name: 'A' },
      { kind: 'and', items: [{ kind: 'id', name: 'B' }, { kind: 'id', name: 'C' }] },
    ],
  });
  assert.deepEqual(parseExpression('Apache-2.0 WITH LLVM-exception'), { kind: 'id', name: 'Apache-2.0 WITH LLVM-exception' });
  // Sans parenthèses, `MIT OR GPL-3.0 AND GPL-3.0` = MIT OR (GPL AND GPL) : passe ; avec, non.
  assert.equal(checkExpression('MIT OR GPL-3.0 AND GPL-3.0'), null);
  assert.notEqual(checkExpression('(MIT OR GPL-3.0) AND GPL-3.0'), null);
});

// ── Les paquets Rust : cargo tree, cargo metadata ──

test('cargo : les deux commandes de la fabrication, --locked partout, la cible Windows, --offline seulement sur demande', () => {
  const calls = cargoCalls();
  assert.equal(calls.trees.length, 2);
  assert.deepEqual(calls.trees[0]?.slice(-2), ['-p', 'shinku']);
  assert.deepEqual(calls.trees[1]?.slice(-4), ['-p', 'shinku-agent', '-p', 'shinku-helper']);
  for (const args of [...calls.trees, calls.metadata]) {
    assert.ok(args.includes('--locked'), `${args.join(' ')} : sans --locked`);
    assert.ok(!args.includes('--offline'), 'pas de --offline en dur : la CI a le réseau');
  }
  for (const args of calls.trees) {
    assert.deepEqual(args.slice(0, 1), ['tree']);
    assert.equal(args[args.indexOf('-e') + 1], 'normal,no-proc-macro', 'les paquets liés seulement : ni outils de fabrication ni macros');
    assert.equal(args[args.indexOf('--target') + 1], 'x86_64-pc-windows-msvc');
    assert.equal(args[args.indexOf('--prefix') + 1], 'none');
  }
  assert.deepEqual(calls.metadata.slice(0, 3), ['metadata', '--format-version', '1']);
  for (const args of [...cargoCalls({ offline: true }).trees, cargoCalls({ offline: true }).metadata]) {
    assert.ok(args.includes('--offline'));
  }
  assert.equal(parseArguments(['--hors-ligne']).offline, true);
  assert.equal(parseArguments(['--verifier']).verify, true);
  assert.throws(() => parseArguments(['--autre']));
});

/** Ce que `cargo tree --prefix none` écrit : une ligne par paquet, des doublons marqués « (*) », un paquet local. */
const TREE_WINDOW = `shinku v0.1.0 (/home/x/shinku/apps/shinku/src-tauri)
tauri v2.12.5
serde v1.0.229
serde_derive v1.0.229 (proc-macro)
serde v1.0.229 (*)
shinku-core v0.1.0 (/home/x/shinku/crates/shinku-core)
`;
const TREE_AGENT = `shinku-agent v0.1.0 (/home/x/shinku/crates/shinku-agent)
shinku-core v0.1.0 (/home/x/shinku/crates/shinku-core)
windows-sys v0.61.2
serde v1.0.229
`;

const metaPackage = (name: string, version: string, license: string | null, extra: Record<string, unknown> = {}) => ({
  name,
  version,
  source: 'registry+https://github.com/rust-lang/crates.io-index',
  license,
  repository: `https://example.org/${name}`,
  manifest_path: `/registre/${name}-${version}/Cargo.toml`,
  ...extra,
});
const METADATA = {
  packages: [
    metaPackage('tauri', '2.12.5', 'Apache-2.0 OR MIT'),
    metaPackage('serde', '1.0.229', 'MIT OR Apache-2.0'),
    metaPackage('serde_derive', '1.0.229', 'MIT OR Apache-2.0'),
    metaPackage('windows-sys', '0.61.2', 'MIT OR Apache-2.0'),
    metaPackage('serde', '0.9.0', 'MIT OR Apache-2.0'), // une autre version, hors du graphe
    metaPackage('shinku', '0.1.0', null, { source: null }),
    metaPackage('shinku-agent', '0.1.0', null, { source: null }),
    metaPackage('shinku-core', '0.1.0', null, { source: null }),
  ],
};

test('cargo tree : une ligne par paquet, la version lue, les marques « (*) » et « (proc-macro) » ignorées', () => {
  assert.deepEqual(
    parseCargoTree(TREE_WINDOW).map((one) => `${one.name} ${one.version}`),
    ['shinku 0.1.0', 'tauri 2.12.5', 'serde 1.0.229', 'serde_derive 1.0.229', 'serde 1.0.229', 'shinku-core 0.1.0'],
  );
  assert.throws(() => parseCargoTree('quelque chose qui n\'est pas un paquet'), /illisible/);
});

test("l'union des deux commandes : chaque paquet une fois, trié, et ceux d'une seule des deux y sont", () => {
  const union = unionPackages([parseCargoTree(TREE_WINDOW), parseCargoTree(TREE_AGENT)]).map((one) => `${one.name} ${one.version}`);
  assert.deepEqual(union, ['serde 1.0.229', 'serde_derive 1.0.229', 'shinku 0.1.0', 'shinku-agent 0.1.0', 'shinku-core 0.1.0', 'tauri 2.12.5', 'windows-sys 0.61.2']);
  // « windows-sys » n'est que dans la seconde liste, « tauri » que dans la première : les deux sont gardés.
  assert.ok(union.includes('windows-sys 0.61.2') && union.includes('tauri 2.12.5'));
});

test('les paquets du dépôt lui-même (sans `source`) sont écartés ; la version hors du graphe aussi', () => {
  const { packages, errors } = resolveRustPackages([parseCargoTree(TREE_WINDOW), parseCargoTree(TREE_AGENT)], METADATA);
  assert.deepEqual(errors, []);
  assert.deepEqual(
    packages.map((one) => `${one.name} ${one.version}`),
    ['serde 1.0.229', 'serde_derive 1.0.229', 'tauri 2.12.5', 'windows-sys 0.61.2'],
  );
  assert.equal(packages[0]?.dir, '/registre/serde-1.0.229');
});

test('un paquet de cargo tree introuvable dans cargo metadata est une erreur', () => {
  const { errors } = resolveRustPackages([[{ name: 'fantome', version: '1.0.0' }]], METADATA);
  assert.equal(errors.length, 1);
  assert.match(errors[0] ?? '', /fantome 1\.0\.0 : dans cargo tree, introuvable dans cargo metadata/);
});

// ── Les paquets de la page ──

test('la page : les dépendances du verrou, plus les polices de PAGE_DEV_PACKAGES ; le reste du développement est écarté', () => {
  const lock = {
    packages: {
      '': { version: '0.1.0' },
      'node_modules/lit-html': { version: '3.3.3', license: 'BSD-3-Clause' },
      'node_modules/vite': { version: '8.0.0', license: 'MIT', dev: true },
      'node_modules/@fontsource/orbitron': { version: '5.3.0', license: 'OFL-1.1', dev: true },
      'node_modules/@fontsource-variable/m-plus-1': { version: '5.3.1', license: 'OFL-1.1', dev: true },
    },
  };
  const { packages, errors } = resolvePagePackages(lock, '/racine');
  assert.deepEqual(errors, []);
  assert.deepEqual(
    packages.map((one) => one.name),
    ['@fontsource-variable/m-plus-1', '@fontsource/orbitron', 'lit-html'],
  );
  assert.equal(packages[2]?.dir, '/racine/node_modules/lit-html');
  // Un nom de la liste qui n'est plus dans le verrou : erreur.
  assert.match(resolvePagePackages(lock, '/racine', ['absent']).errors[0] ?? '', /absent : cité dans PAGE_DEV_PACKAGES/);
});

/** Les paquets que la page importe, lus dans le code et le CSS de `src`. */
function importedPackages(): Set<string> {
  const found = new Set<string>();
  const walk = (dir: URL): void => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const next = new URL(entry.name + (entry.isDirectory() ? '/' : ''), dir);
      if (entry.isDirectory()) walk(next);
      else if (/\.(ts|css)$/.test(entry.name)) scan(readFileSync(next, 'utf8'));
    }
  };
  const nameOf = (specifier: string): string => specifier.split('/').slice(0, specifier.startsWith('@') ? 2 : 1).join('/');
  const scan = (text: string): void => {
    for (const match of text.matchAll(/(?:\bfrom|\bimport)\s*\(?\s*['"]([^'"]+)['"]/g)) {
      const specifier = match[1] ?? '';
      if (!specifier.startsWith('.') && !specifier.startsWith('/') && !specifier.startsWith('node:')) found.add(nameOf(specifier));
    }
    for (const match of text.matchAll(/url\(\s*['"]?([^'")]+)['"]?\s*\)/g)) {
      const specifier = match[1] ?? '';
      if (/^@?[a-z0-9][\w.-]*\//i.test(specifier) && !specifier.includes(':')) found.add(nameOf(specifier));
    }
  };
  walk(new URL('src/', root));
  return found;
}

test('la page : tout paquet importé par src (code ou CSS) est dans la liste des paquets de la page', () => {
  const lock = JSON.parse(readFileSync(new URL('package-lock.json', root), 'utf8')) as Parameters<typeof resolvePagePackages>[0];
  const { packages } = resolvePagePackages(lock, '/racine');
  const known = new Set(packages.map((one) => one.name));
  const imported = importedPackages();
  assert.ok(imported.has('lit-html') && imported.has('@tauri-apps/api') && imported.has('@fontsource/orbitron'), `le balayage ne voit plus les imports : ${[...imported].join(', ')}`);
  for (const name of imported) assert.ok(known.has(name), `src importe « ${name} », qui n'est dans aucune des deux listes (dependencies, PAGE_DEV_PACKAGES) : sa licence manquerait au fichier`);
  for (const name of PAGE_DEV_PACKAGES) assert.ok(imported.has(name), `${name} est dans PAGE_DEV_PACKAGES mais src ne l'importe plus`);
});

// ── Les textes ──

test('les textes : à la racine du paquet seulement, sans tenir compte de la casse, les autres fichiers ignorés', () => {
  const dir = folder({
    LICENSE: 'MIT A',
    'license-apache': 'Apache',
    'Copying.txt': 'C',
    NOTICE: 'N',
    UNLICENSE: 'U',
    'LICENCE.md': 'FR',
    'README.md': 'pas une licence',
    'src/LICENSE': 'dans un sous-dossier : ignoré',
    'licensing.rs': 'commence par « licens » mais pas « licen[cs]e »',
  });
  assert.deepEqual(
    readLicenseFiles(dir).files.map((file) => file.name),
    ['Copying.txt', 'LICENCE.md', 'LICENSE', 'NOTICE', 'UNLICENSE', 'license-apache'],
  );
  assert.ok(LICENSE_FILE_NAME.test('COPYING.LESSER'));
});

test("les textes : rien n'est écarté en silence : un lien, un fichier trop gros, un fichier vide et un dossier sont rendus avec leur raison", () => {
  const outside = folder({ secret: 'ne doit pas être lu' });
  const dir = folder({ 'LICENSE-MIT': 'MIT ordinaire', 'LICENSE-VIDE': ' \r\n\t', 'LICENSE-BOM': '﻿', 'LICENSES/MIT': 'dans un dossier' });
  symlinkSync(join(outside, 'secret'), join(dir, 'LICENSE'));
  writeFileSync(join(dir, 'COPYING'), 'x'.repeat(MAX_TEXT_BYTES + 1));
  writeFileSync(join(dir, 'NOTICE'), 'y'.repeat(MAX_TEXT_BYTES));
  assert.equal(statSync(join(dir, 'COPYING')).size, MAX_TEXT_BYTES + 1);
  const { files, rejected } = readLicenseFiles(dir);
  assert.deepEqual(
    files.map((file) => file.name),
    ['LICENSE-MIT', 'NOTICE'],
    'celui de 256 Kio pile est lu',
  );
  assert.ok(!files.some((file) => file.content.includes('ne doit pas être lu')));
  assert.deepEqual(
    rejected.map((one) => `${one.name}:${one.kind}`),
    ['COPYING:large', 'LICENSE:link', 'LICENSE-BOM:empty', 'LICENSE-VIDE:empty', 'LICENSES:directory'],
  );
});

test('la garde : un fichier au nom de licence écarté est une erreur qui nomme le paquet et le fichier, sauf un dossier quand le paquet a par ailleurs un texte', () => {
  const dir = folder({ 'LICENSE-MIT': 'MIT ordinaire', 'LICENSE-VIDE': '', 'LICENSES/MIT': 'dossier' });
  const one: Package = { ...rustOne('paquet', 'MIT'), dir };
  assert.deepEqual(checkPackages(attachTexts([one], new Map()).packages), ['paquet 1.0.0 : fichier « LICENSE-VIDE » écarté (vide)']);
  // Un lien et un fichier trop gros : erreur, même si un autre fichier fait un texte.
  const outside = folder({ secret: 's' });
  const dir2 = folder({ LICENSE: 'MIT' });
  symlinkSync(join(outside, 'secret'), join(dir2, 'LICENSE-LIEN'));
  writeFileSync(join(dir2, 'COPYING'), 'x'.repeat(MAX_TEXT_BYTES + 1));
  const errors2 = checkPackages(attachTexts([{ ...one, dir: dir2 }], new Map()).packages);
  assert.equal(errors2.length, 2, errors2.join(' | '));
  assert.ok(errors2.some((line) => line.startsWith('paquet 1.0.0 : fichier « COPYING » écarté (plus de 256 Kio')));
  assert.ok(errors2.some((line) => line.startsWith('paquet 1.0.0 : fichier « LICENSE-LIEN » écarté (un lien')));
  // Un dossier LICENSES/ seul : le paquet n'a aucun texte, et l'erreur le dit aussi.
  const dir3 = folder({ 'LICENSES/MIT': 'dossier' });
  assert.deepEqual(
    checkPackages(attachTexts([{ ...one, dir: dir3 }], new Map()).packages).map((line) => line.split(' : ')[1]),
    ["fichier « LICENSES » écarté (un dossier, qui n'est pas lu)", "aucun texte de licence (ni dans son dossier ni dans les compléments ; un NOTICE n'en est pas un)"],
  );
});

test("la garde : un NOTICE est recopié mais ne compte pas comme texte de licence : un paquet qui n'a qu'un NOTICE échoue", () => {
  const one: Package = { ...rustOne('apache', 'Apache-2.0'), dir: folder({ NOTICE: 'Copyright des auteurs' }) };
  const attached = attachTexts([one], new Map()).packages;
  assert.deepEqual(attached[0]?.texts.map((text) => text.name), ['NOTICE'], 'le NOTICE est recopié');
  assert.equal(licenseTexts(attached[0]?.texts ?? []).length, 0);
  assert.match(checkPackages(attached)[0] ?? '', /^apache 1\.0\.0 : aucun texte de licence/);
  // Avec un vrai texte à côté, le paquet passe, et le NOTICE paraît dans le fichier.
  const both = attachTexts([{ ...one, dir: folder({ NOTICE: 'Copyright des auteurs', LICENSE: 'Apache texte' }) }], new Map()).packages;
  assert.deepEqual(checkPackages(both), []);
  assert.match(composeFile({ fixed: 'f', packages: both }), /Copyright des auteurs/);
});

test('les textes : les fins de ligne, la marque d\'ordre des octets et les blancs de fin ne distinguent pas deux copies', () => {
  assert.equal(normalizeText('﻿ligne 1\r\nligne 2\r\n\r\n'), 'ligne 1\nligne 2');
  assert.equal(normalizeText('a\rb'), 'a\nb');
});

const withTexts = (one: Package, texts: string[]): PackageWithTexts => ({ ...one, texts: texts.map((content, at) => ({ name: `T${at}`, content })), rejected: [], complement: null });
const rustOne = (name: string, license: string | null = 'MIT', version = '1.0.0'): Package => ({ ecosystem: 'rust', name, version, license, repository: `https://example.org/${name}`, dir: `/registre/${name}` });

test('les textes identiques ne sont écrits qu\'une fois, et chaque paquet renvoie aux siens par un numéro', () => {
  const packages = [withTexts(rustOne('a'), ['MIT\ncommun', 'Apache']), withTexts(rustOne('b'), ['MIT\ncommun']), withTexts(rustOne('c'), ['Autre', 'MIT\ncommun'])];
  const { texts, perPackage } = numberTexts(packages);
  assert.deepEqual(texts, ['MIT\ncommun', 'Apache', 'Autre']);
  assert.deepEqual(perPackage, [[1, 2], [1], [1, 3]]);
  const file = composeFile({ fixed: 'section fixe', packages });
  assert.equal(file.split('\nMIT\ncommun\n').length - 1, 1, 'le texte commun paraît une seule fois');
  assert.match(file, /^a 1\.0\.0\n {2}Licence : MIT\n {2}Dépôt : https:\/\/example\.org\/a\n {2}Textes : \[1\] \[2\]$/m);
  assert.match(file, /^c 1\.0\.0\n(?: {2}.*\n)* {2}Textes : \[1\] \[3\]$/m);
});

const complementOf = (name: string, version: string, content = 'texte pris à la main', source = 'dépôt X, étiquette 1.0.0') => ({
  name,
  version,
  files: [{ name: 'LICENSE', content }],
  source,
});
const own =
  (...names: string[]) =>
  (): { files: { name: string; content: string; notice: boolean }[]; rejected: [] } => ({
    files: names.map((name) => ({ name, content: `contenu de ${name}`, notice: /^notice/i.test(name) })),
    rejected: [],
  });

test('la garde : un paquet sans aucun texte échoue et est nommé ; avec un complément de sa version, il passe', () => {
  const one: Package = { ...rustOne('sanstexte', 'BSD-3-Clause'), dir: folder({ 'README.md': 'pas de licence ici' }) };
  const bare = attachTexts([one], new Map());
  assert.deepEqual(bare.errors, []);
  assert.deepEqual(checkPackages(bare.packages), ["sanstexte 1.0.0 : aucun texte de licence (ni dans son dossier ni dans les compléments ; un NOTICE n'en est pas un)"]);
  const fixed = attachTexts([one], new Map([['sanstexte@1.0.0', complementOf('sanstexte', '1.0.0')]]));
  assert.deepEqual(fixed.errors, []);
  assert.deepEqual(checkPackages(fixed.packages), []);
  assert.equal(fixed.packages[0]?.complement, 'dépôt X, étiquette 1.0.0');
});

test("un complément ne vaut que pour la version vérifiée : une autre version du paquet ne l'a pas, et le complément devient orphelin", () => {
  const complements = new Map([['sanstexte@1.0.0', complementOf('sanstexte', '1.0.0')]]);
  const next: Package = { ...rustOne('sanstexte', 'BSD-3-Clause', '1.1.0'), dir: folder() };
  const { packages, errors } = attachTexts([next], complements);
  assert.equal(errors.length, 1);
  assert.match(errors[0] ?? '', /complément sanstexte@1\.0\.0 : aucun paquet de ce nom et de cette version dans les dépendances \(à supprimer, ou à revérifier/);
  assert.equal(packages[0]?.complement, null);
  assert.equal(checkPackages(packages).length, 1, 'et le paquet de la nouvelle version retombe sans texte');
  // Les deux versions dans le graphe : seule celle du complément le reçoit.
  const both = attachTexts([{ ...next, version: '1.0.0' }, next], complements);
  assert.deepEqual(both.errors, []);
  assert.deepEqual(both.packages.map((one) => one.complement !== null), [true, false]);
});

test('un complément pour un paquet qui a déjà ses propres textes de licence est une erreur ; un NOTICE seul ne la déclenche pas', () => {
  const complements = new Map([['a@1.0.0', complementOf('a', '1.0.0')]]);
  const useless = attachTexts([rustOne('a')], complements, own('LICENSE-MIT'));
  assert.deepEqual(useless.errors, ['complément a@1.0.0 : inutile, le paquet a ses propres textes de licence (à supprimer)']);
  const noticeOnly = attachTexts([rustOne('a')], complements, own('NOTICE'));
  assert.deepEqual(noticeOnly.errors, []);
  assert.deepEqual(checkPackages(noticeOnly.packages), []);
});

test('un complément qui ne va à aucun paquet du graphe est une erreur (le dossier ne vieillit pas en silence)', () => {
  const complements = new Map([['orphelin@1.0.0', complementOf('orphelin', '1.0.0')]]);
  const { errors } = attachTexts([withTexts(rustOne('autre'), ['t'])], complements, own('LICENSE'));
  assert.equal(errors.length, 1);
  assert.match(errors[0] ?? '', /complément orphelin@1\.0\.0 : aucun paquet de ce nom et de cette version/);
});

test("les compléments : <nom>@<version>, un SOURCE.txt, au moins un vrai texte de licence, et rien d'écarté en silence", () => {
  const dir = folder({
    'bon@1.2.3/LICENSE': 'texte',
    'bon@1.2.3/SOURCE.txt': 'dépôt, étiquette',
    'sanssource@1.0.0/LICENSE': 'texte',
    'sanstexte@1.0.0/SOURCE.txt': 'dépôt',
    'noticeseul@1.0.0/NOTICE': 'notice',
    'noticeseul@1.0.0/SOURCE.txt': 'dépôt',
    'sansversion/LICENSE': 'texte',
    'sansversion/SOURCE.txt': 'dépôt',
    'vide@1.0.0/LICENSE': '',
    'vide@1.0.0/LICENSE-MIT': 'texte',
    'vide@1.0.0/SOURCE.txt': 'dépôt',
  });
  const { complements, errors } = readComplements(dir);
  assert.deepEqual([...complements.keys()], ['bon@1.2.3', 'vide@1.0.0']);
  assert.deepEqual(errors.sort(), [
    'complément noticeseul@1.0.0 : aucun fichier de licence',
    "complément sansversion : le dossier doit s'appeler <nom>@<version> (le texte n'est valable que pour une version)",
    "complément sanssource@1.0.0 : SOURCE.txt manque (il dit d'où vient le texte)",
    'complément sanstexte@1.0.0 : aucun fichier de licence',
    'complément vide@1.0.0 : fichier « LICENSE » écarté (vide)',
  ].sort());
  assert.equal(complements.get('bon@1.2.3')?.source, 'dépôt, étiquette');
  assert.equal(complements.get('bon@1.2.3')?.version, '1.2.3');
  assert.ok(!complements.get('bon@1.2.3')?.files.some((file) => file.name === 'SOURCE.txt'), "SOURCE.txt n'est pas un texte de licence");
});

test("la garde : une licence refusée, un dépôt qui manque à une MPL-2.0, une licence absente : chaque paquet fautif est nommé", () => {
  const packages = [
    withTexts(rustOne('bon', 'MIT OR GPL-3.0'), ['t']),
    withTexts(rustOne('gpl', 'GPL-3.0'), ['t']),
    withTexts(rustOne('muet', null), ['t']),
    withTexts({ ...rustOne('mpl', 'MPL-2.0'), repository: null }, ['t']),
    withTexts({ ...rustOne('mplok', 'MPL-2.0'), repository: 'https://example.org/mplok' }, ['t']),
  ];
  const errors = checkPackages(packages);
  assert.equal(errors.length, 3, errors.join(' | '));
  assert.ok(errors[0]?.startsWith('gpl 1.0.0 : la licence « GPL-3.0 » n\'est pas dans la liste permise'));
  assert.ok(errors[1]?.startsWith('muet 1.0.0 : aucune licence déclarée'));
  assert.ok(errors[2]?.startsWith('mpl 1.0.0 : sous MPL-2.0 et sans adresse de dépôt'));
});

// ── Le fichier ──

test('le fichier : chaque paquet MPL-2.0 dit où obtenir le code source de sa version', () => {
  const mpl = withTexts({ ...rustOne('selectif', 'MPL-2.0', '0.38.0'), repository: 'https://example.org/stylo' }, ['MPL texte']);
  const file = composeFile({ fixed: 'fixe', packages: [mpl, withTexts(rustOne('mit'), ['MIT'])] });
  assert.match(file, /^selectif 0\.38\.0\n(?: {2}.*\n)* {2}Code source de cette version \(MPL-2\.0\) : https:\/\/example\.org\/stylo, version 0\.38\.0$/m);
  assert.equal(file.match(/Code source de cette version/g)?.length, 1, 'seul le paquet sous MPL-2.0 porte la phrase');
});

test('le fichier : la section fixe est recopiée en tête, avant les paquets Rust, ceux de la page, puis les textes', () => {
  const page: PackageWithTexts = withTexts({ ...rustOne('lit-html', 'BSD-3-Clause', '3.3.3'), ecosystem: 'npm', repository: null }, ['BSD']);
  const file = composeFile({ fixed: 'SECTION-FIXE-RECOPIEE', packages: [page, withTexts(rustOne('serde'), ['MIT'])] });
  const at = (text: string): number => file.indexOf(text);
  assert.ok(at('SECTION-FIXE-RECOPIEE') > 0, 'la section fixe est recopiée');
  assert.ok(at('SECTION-FIXE-RECOPIEE') < at('\nserde 1.0.0\n'));
  assert.ok(at('\nserde 1.0.0\n') < at('\nlit-html 3.3.3\n'));
  assert.ok(at('\nlit-html 3.3.3\n') < at('4. TEXTES DES LICENCES'));
  assert.ok(file.startsWith('LICENCES DES COMPOSANTS TIERS DE SHINKU\n'));
});

test('le fichier : deux passages donnent le même fichier, octet pour octet ; fins de ligne \\n, pas de marque d\'ordre, pas de date ni de chemin', () => {
  const make = () => composeFile({ fixed: 'fixe\r\nsuite', packages: [withTexts(rustOne('b'), ['x\r\ny']), withTexts(rustOne('a'), ['x\ny'])].reverse() });
  const first = make();
  assert.equal(first, make());
  assert.ok(!first.includes('\r'));
  assert.ok(!first.startsWith('﻿'));
  assert.ok(first.endsWith('\n'));
  assert.doesNotMatch(first, /\/registre\/|\b20\d\d-\d\d-\d\d\b/);
  // Triée : a avant b, quel que soit l'ordre d'entrée.
  assert.ok(first.indexOf('\na 1.0.0\n') < first.indexOf('\nb 1.0.0\n'));
});

// ── Le vrai dépôt ──

const offline = process.env.SHINKU_CARGO_HORS_LIGNE === '1';
let real: ReturnType<typeof build> | undefined;
const realBuild = () => (real ??= build({ offline }));

test('le vrai dépôt : cargo répond (l\'essai échoue, il ne saute pas, si cargo manque)', () => {
  assert.doesNotThrow(() => realBuild(), 'cargo tree ou cargo metadata a échoué : cargo doit être installé, et les sources des paquets téléchargées (cargo fetch)');
});

test('le vrai dépôt : la garde passe (licences permises, un texte de licence par paquet, rien d\'écarté, compléments tous utiles)', () => {
  assert.deepEqual(realBuild().errors, []);
});

/** Ce qui ne doit jamais paraître dans le fichier fabriqué : un chemin de la machine ou un retour chariot. */
const MACHINE = /\/home\/|\/root\/|[A-Za-z]:\\Users|\r/;

test('le détecteur de chemins de la machine voit un chemin Linux, un chemin Windows et un retour chariot, et laisse passer un texte ordinaire', () => {
  for (const bad of ['voir /home/quelquun/x', 'dans /root/.cargo', 'C:\\Users\\quelquun\\x', 'd:\\Users', 'a\r\nb']) assert.match(bad, MACHINE, bad);
  for (const good of ['Copyright (c) 2014 Alex Crichton', 'le dossier Users de la doc', 'https://example.org/Users']) assert.doesNotMatch(good, MACHINE, good);
});

test('le vrai dépôt : le fichier nomme tauri, serde, windows, lit-html, les deux polices et les paquets à complément', () => {
  const content = realBuild().content;
  for (const name of ['tauri', 'serde', 'windows', 'lit-html', '@fontsource-variable/m-plus-1', '@fontsource/orbitron', 'alloc-stdlib', 'webview2-com', 'webview2-com-sys']) {
    assert.match(content, new RegExp(`^${name.replace(/[/@.-]/g, '\\$&')} \\d`, 'm'), `LICENCES-TIERS.txt ne nomme pas ${name}`);
  }
  // Aucun paquet du dépôt lui-même.
  assert.doesNotMatch(content, /^shinku(?:-core|-agent|-helper)? 0\.1\.0$/m);
  assert.doesNotMatch(content, MACHINE);
});

test('le vrai dépôt : seuls les paquets liés sont listés : ni les outils de fabrication (tauri-build) ni les macros (serde_derive, webview2-com-macros)', () => {
  const names = new Set(realBuild().packages.map((one) => one.name));
  for (const absent of ['tauri-build', 'tauri-winres', 'embed-resource', 'serde_derive', 'webview2-com-macros', 'selectors']) {
    assert.ok(!names.has(absent), `${absent} n'est pas lié dans les programmes : il ne doit pas être listé`);
  }
  for (const present of ['serde', 'tauri', 'windows', 'webview2-com-sys']) assert.ok(names.has(present), present);
});

test('le vrai dépôt : la provenance de chaque complément est dans le fichier, sous son paquet', () => {
  const content = realBuild().content;
  for (const [name = '', version = ''] of [['alloc-stdlib', '0.3.0'], ['webview2-com', '0.39.1'], ['webview2-com-sys', '0.39.1']] as const) {
    const block = new RegExp(`^${name} ${version.replace(/\./g, '\\.')}\\n(?: {2}.*\\n)+`, 'm').exec(content)?.[0] ?? '';
    assert.match(block, /Texte ajouté à la main\. Sa provenance :\n {4}Fichier LICENSE à la racine de https:\/\/github\.com\//, `${name} : sa provenance manque sous le paquet`);
  }
});

test('le vrai dépôt : les paquets sous MPL-2.0 portent la phrase du code source', () => {
  const content = realBuild().content;
  const mpl = (content.match(/^\S+ \S+\n(?: {2}.*\n)+/gm) ?? []).filter((block) => block.includes('\n  Licence : MPL-2.0\n'));
  assert.ok(mpl.length >= 1, 'le balayage ne voit plus de paquet sous MPL-2.0');
  for (const block of mpl) assert.match(block, /Code source de cette version \(MPL-2\.0\) : https:\/\//);
});

/** Les paquets que la section fixe cite entre guillemets français après « paquet » ou « paquets ». */
function packagesQuoted(text: string): string[] {
  const found: string[] = [];
  for (const match of text.matchAll(/paquets?\s+«\s*([^»]+?)\s*»(?:\s+et\s+«\s*([^»]+?)\s*»)?/g)) {
    found.push(match[1] ?? '');
    if (match[2] !== undefined) found.push(match[2]);
  }
  return found;
}

test('la section fixe : le relevé des paquets cités voit « paquet » et « paquets », pas « paquet Debian »', () => {
  assert.deepEqual(packagesQuoted('ceux du paquet « cfg-if »)'), ['cfg-if']);
  assert.deepEqual(packagesQuoted('ceux des paquets « cfg-if » et\n« hashbrown »)'), ['cfg-if', 'hashbrown']);
  assert.deepEqual(packagesQuoted('du paquet Debian « nsis » 3.09'), []);
});

test('la section fixe : chaque paquet qu\'elle cite (« cfg-if », « hashbrown », « icu_collections »…) est dans la liste fabriquée et y a au moins un texte', () => {
  const fixed = readFileSync(new URL('src-tauri/licences/section-fixe.txt', root), 'utf8');
  const quoted = packagesQuoted(fixed);
  for (const wanted of ['cfg-if', 'hashbrown', 'icu_collections']) assert.ok(quoted.includes(wanted), `la section fixe ne cite plus « ${wanted} » : le relevé a changé`);
  const packages = realBuild().packages;
  for (const name of quoted) {
    const found = packages.filter((one) => one.name === name);
    assert.ok(found.length > 0, `la section fixe renvoie au paquet « ${name} », qui n'est pas dans la liste fabriquée`);
    assert.ok(found.every((one) => licenseTexts(one.texts).length > 0), `« ${name} » n'a aucun texte de licence`);
  }
});

// ── La ligne de commande ──

/** Un faux dépôt : package-lock.json, node_modules, section fixe, un paquet Rust avec son texte. */
function fakeApp(options: { complements?: Record<string, string>; fixed?: string | null; nodeModules?: boolean } = {}): { appDir: string; crate: string; output: string } {
  const appDir = folder({
    'package-lock.json': JSON.stringify({
      packages: {
        '': {},
        'node_modules/lit-html': { version: '3.3.3', license: 'BSD-3-Clause' },
        'node_modules/@fontsource/orbitron': { version: '5.3.0', license: 'OFL-1.1', dev: true },
        'node_modules/@fontsource-variable/m-plus-1': { version: '5.3.1', license: 'OFL-1.1', dev: true },
      },
    }),
    'node_modules/lit-html/LICENSE': 'BSD texte',
    'node_modules/@fontsource/orbitron/LICENSE': 'OFL texte',
    'node_modules/@fontsource-variable/m-plus-1/LICENSE': 'OFL texte',
    ...(options.fixed === null ? {} : { 'src-tauri/licences/section-fixe.txt': options.fixed ?? 'section fixe' }),
    ...Object.fromEntries(Object.entries(options.complements ?? {}).map(([name, content]) => [`src-tauri/licences/complements/${name}`, content])),
  });
  if (options.nodeModules === false) rmSync(join(appDir, 'node_modules'), { recursive: true });
  const crate = folder({ LICENSE: 'MIT texte du paquet' });
  return { appDir, crate, output: join(appDir, 'src-tauri', 'licences', 'LICENCES-TIERS.txt') };
}

/** Un faux cargo : `tree` rend `tree`, `metadata` rend les paquets ; ou il échoue. */
function fakeCargo(crate: string, license: string | null = 'MIT', fails = false): (args: string[]) => string {
  return (args) => {
    if (fails) throw new Error('cargo : échec simulé');
    if (args[0] === 'tree') return 'shinku v0.1.0 (/x)\nserde v1.0.0\n';
    return JSON.stringify({
      packages: [
        { name: 'shinku', version: '0.1.0', source: null, license: null, repository: null, manifest_path: '/x/Cargo.toml' },
        { name: 'serde', version: '1.0.0', source: 'registry+x', license, repository: 'https://example.org/serde', manifest_path: join(crate, 'Cargo.toml') },
      ],
    });
  };
}

const NO_ENV = {};
const cli = (argv: string[], app: { appDir: string }, run: (args: string[]) => string, env: Record<string, string | undefined> = NO_ENV) => {
  const lines: string[] = [];
  const code = runCli(argv, { run, appDir: app.appDir, env, log: (line) => lines.push(line) });
  return { code, lines };
};

test('la ligne de commande : la garde passe, code 0, le fichier est écrit, avec les comptes et son empreinte sur la dernière ligne', () => {
  const app = fakeApp();
  const { code, lines } = cli([], app, fakeCargo(app.crate));
  assert.equal(code, 0, lines.join(' | '));
  const written = readFileSync(app.output, 'utf8');
  assert.match(written, /^serde 1\.0\.0\n/m);
  assert.match(lines[0] ?? '', /^licences-tiers : fichier écrit \(1 paquets Rust liés, 3 paquets de l'interface, 3 textes distincts\)\.$/);
  assert.equal(lines[lines.length - 1], `licences-tiers : sha256 ${sha256(written)}`);
  assert.match(sha256(written), /^[0-9a-f]{64}$/);
  assert.deepEqual(readdirSync(join(app.appDir, 'src-tauri', 'licences')).filter((name) => name.endsWith('.tmp')), [], 'aucun fichier temporaire ne reste');
});

test('la ligne de commande : deux passages écrivent le même fichier, octet pour octet, avec la même empreinte', () => {
  const app = fakeApp();
  const first = cli([], app, fakeCargo(app.crate));
  const one = readFileSync(app.output);
  const second = cli([], app, fakeCargo(app.crate));
  assert.deepEqual(readFileSync(app.output), one);
  assert.equal(first.lines.at(-1), second.lines.at(-1));
});

test('la ligne de commande : erreurs de la garde, code 1, aucun fichier écrit', () => {
  const app = fakeApp();
  const { code, lines } = cli([], app, fakeCargo(app.crate, 'GPL-3.0'));
  assert.equal(code, 1);
  assert.match(lines.join('\n'), /serde 1\.0\.0 : la licence « GPL-3\.0 » n'est pas dans la liste permise/);
  assert.equal(existsSync(app.output), false, 'rien ne doit être écrit quand la garde échoue');
  assert.ok(!lines.some((line) => line.includes('sha256')));
});

test("la ligne de commande : erreurs de la garde, un fichier fabriqué d'avant n'est pas modifié", () => {
  const app = fakeApp();
  mkdirSync(join(app.appDir, 'src-tauri', 'licences'), { recursive: true });
  writeFileSync(app.output, 'ancien contenu');
  assert.equal(cli([], app, fakeCargo(app.crate, 'GPL-3.0')).code, 1);
  assert.equal(readFileSync(app.output, 'utf8'), 'ancien contenu');
});

test("la ligne de commande : --verifier ne crée ni ne modifie rien, rend 0 et donne l'empreinte de ce qui serait fabriqué", () => {
  const app = fakeApp();
  const verify = cli(['--verifier'], app, fakeCargo(app.crate));
  assert.equal(verify.code, 0);
  assert.equal(existsSync(app.output), false);
  assert.match(verify.lines[0] ?? '', /^licences-tiers : la garde passe \(/);
  const make = cli([], app, fakeCargo(app.crate));
  assert.equal(verify.lines.at(-1), make.lines.at(-1), 'la même empreinte que la fabrication');
  const before = readFileSync(app.output);
  cli(['--verifier'], app, fakeCargo(app.crate));
  assert.deepEqual(readFileSync(app.output), before);
});

test('la ligne de commande : --sortie écrit ailleurs, --hors-ligne ajoute --offline aux appels de cargo', () => {
  const app = fakeApp();
  const target = join(scratch, `sortie-${counter}.txt`);
  const seen: string[][] = [];
  const run = (args: string[]) => (seen.push(args), fakeCargo(app.crate)(args));
  assert.equal(cli(['--sortie', target, '--hors-ligne'], app, run).code, 0);
  assert.ok(existsSync(target));
  assert.equal(existsSync(app.output), false);
  assert.ok(seen.length >= 3 && seen.every((args) => args.includes('--offline')));
  const env = cli([], app, (args) => (seen.push(args), fakeCargo(app.crate)(args)), { SHINKU_CARGO_HORS_LIGNE: '1' });
  assert.equal(env.code, 0);
  assert.ok(seen.slice(-3).every((args) => args.includes('--offline')));
});

test("la ligne de commande : si l'écriture échoue (la sortie est un dossier), code 1 et le message", () => {
  const app = fakeApp();
  const target = folder();
  const { code, lines } = cli(['--sortie', target], app, fakeCargo(app.crate));
  assert.equal(code, 1);
  assert.match(lines.join('\n'), /licences-tiers : écriture impossible : /);
  assert.ok(!lines.some((line) => line.includes('sha256')));
});

test('la ligne de commande : un argument inconnu rend 2 et ne lance pas cargo', () => {
  const app = fakeApp();
  let launched = false;
  const { code, lines } = cli(['--nimporte'], app, () => ((launched = true), ''));
  assert.equal(code, 2);
  assert.match(lines.join('\n'), /argument inconnu : --nimporte/);
  assert.equal(launched, false);
  assert.equal(existsSync(app.output), false);
});

test('la ligne de commande : si cargo échoue, code 1, le message de cargo, rien d\'écrit', () => {
  const app = fakeApp();
  const { code, lines } = cli([], app, fakeCargo(app.crate, 'MIT', true));
  assert.equal(code, 1);
  assert.match(lines.join('\n'), /licences-tiers : cargo : échec simulé/);
  assert.equal(existsSync(app.output), false);
});

test('la ligne de commande : section-fixe.txt manquant ou vide est une erreur, rien n\'est écrit', () => {
  for (const fixed of [null, '', '  \n\t\n']) {
    const app = fakeApp({ fixed });
    const { code, lines } = cli([], app, fakeCargo(app.crate));
    assert.equal(code, 1, `section fixe ${JSON.stringify(fixed)}`);
    assert.match(lines.join('\n'), /section-fixe\.txt manque ou est vide/);
    assert.equal(existsSync(app.output), false);
  }
});

test('la ligne de commande : node_modules manquant a son propre message', () => {
  const app = fakeApp({ nodeModules: false });
  const { code, lines } = cli([], app, fakeCargo(app.crate));
  assert.equal(code, 1);
  assert.match(lines.join('\n'), /node_modules manque : lance `npm ci` avant/);
});

test('la ligne de commande : la provenance d\'un complément est dans le fichier écrit, sous son paquet', () => {
  const app = fakeApp({ complements: { 'serde@1.0.0/LICENSE': 'texte à la main', 'serde@1.0.0/SOURCE.txt': 'PROVENANCE-DU-COMPLEMENT\nsuite' } });
  rmSync(join(app.crate, 'LICENSE'));
  const { code, lines } = cli([], app, fakeCargo(app.crate));
  assert.equal(code, 0, lines.join(' | '));
  assert.match(readFileSync(app.output, 'utf8'), /^serde 1\.0\.0\n(?: {2}.*\n)* {2}Texte ajouté à la main\. Sa provenance :\n {4}PROVENANCE-DU-COMPLEMENT\n {4}suite\n {2}Textes : \[\d+\]$/m);
});

test("l'écriture : un fichier temporaire puis un renommage, et rien ne reste si l'écriture échoue", () => {
  const dir = folder();
  const path = join(dir, 'sous', 'LICENCES-TIERS.txt');
  writeAtomically(path, 'contenu');
  assert.equal(readFileSync(path, 'utf8'), 'contenu');
  assert.deepEqual(readdirSync(join(dir, 'sous')), ['LICENCES-TIERS.txt']);
  // Le chemin est un dossier : le renommage échoue, le fichier temporaire est retiré.
  mkdirSync(join(dir, 'cible'));
  writeFileSync(join(dir, 'cible', 'x'), 'x');
  assert.throws(() => writeAtomically(join(dir, 'cible'), 'contenu'));
  assert.deepEqual(readdirSync(dir).filter((name) => name.endsWith('.tmp')), []);
  // Un fichier déjà là n'est jamais écrit à moitié : si le fichier temporaire ne peut pas être créé, il reste intact.
  const existing = join(dir, 'existant.txt');
  writeFileSync(existing, 'ancien contenu');
  mkdirSync(`${existing}.${process.pid}.tmp`);
  assert.throws(() => writeAtomically(existing, 'nouveau contenu'));
  assert.equal(readFileSync(existing, 'utf8'), 'ancien contenu');
});

test('lancé directement, le script écrit ; importé, il ne fait rien (les deux chemins sont comparés après realpath)', () => {
  const script = fileURLToPath(new URL('scripts/licences-tiers.mjs', root));
  const link = join(scratch, 'lien-vers-le-script.mjs');
  symlinkSync(script, link);
  const run = (file: string) => spawnSync(process.execPath, [file, '--argument-inconnu'], { encoding: 'utf8' });
  assert.equal(run(script).status, 2, 'lancé par son chemin');
  assert.equal(run(link).status, 2, 'lancé par un lien : le script se reconnaît');
  const imported = spawnSync(process.execPath, ['--input-type=module', '-e', `import ${JSON.stringify(pathToFileURL(script).href)}`, '--', '--argument-inconnu'], { encoding: 'utf8' });
  assert.equal(imported.status, 0, 'importé, il ne lance pas la ligne de commande');
});
