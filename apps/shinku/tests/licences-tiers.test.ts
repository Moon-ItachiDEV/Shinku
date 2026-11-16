// Les licences des composants tiers : le script `scripts/licences-tiers.mjs` et sa garde.
//
// Les fonctions pures du script sont appelées sur de faux `cargo tree`, de faux `cargo metadata` et de faux
// dossiers. Un dernier groupe d'essais lance la garde sur le vrai dépôt (cargo et node_modules doivent y être :
// l'essai ÉCHOUE sans eux, il ne saute pas). Hors ligne, `SHINKU_CARGO_HORS_LIGNE=1` ajoute --offline à cargo.

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
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
    assert.equal(args[args.indexOf('-e') + 1], 'normal,build');
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
    readLicenseFiles(dir).map((file) => file.name),
    ['Copying.txt', 'LICENCE.md', 'LICENSE', 'NOTICE', 'UNLICENSE', 'license-apache'],
  );
  assert.ok(LICENSE_FILE_NAME.test('COPYING.LESSER'));
});

test('les textes : un lien symbolique n\'est pas suivi, un fichier de plus de 256 Kio n\'est pas lu', () => {
  const outside = folder({ secret: 'ne doit pas être lu' });
  const dir = folder({ 'LICENSE-MIT': 'MIT ordinaire' });
  symlinkSync(join(outside, 'secret'), join(dir, 'LICENSE'));
  writeFileSync(join(dir, 'COPYING'), 'x'.repeat(MAX_TEXT_BYTES + 1));
  writeFileSync(join(dir, 'NOTICE'), 'y'.repeat(MAX_TEXT_BYTES));
  assert.equal(statSync(join(dir, 'COPYING')).size, MAX_TEXT_BYTES + 1);
  const files = readLicenseFiles(dir);
  assert.deepEqual(
    files.map((file) => file.name),
    ['LICENSE-MIT', 'NOTICE'],
    'le lien (LICENSE) et le fichier trop gros (COPYING) sont ignorés, celui de 256 Kio pile est lu',
  );
  assert.ok(!files.some((file) => file.content.includes('ne doit pas être lu')));
});

test('les textes : les fins de ligne, la marque d\'ordre des octets et les blancs de fin ne distinguent pas deux copies', () => {
  assert.equal(normalizeText('﻿ligne 1\r\nligne 2\r\n\r\n'), 'ligne 1\nligne 2');
  assert.equal(normalizeText('a\rb'), 'a\nb');
});

const withTexts = (one: Package, texts: string[]): PackageWithTexts => ({ ...one, texts: texts.map((content, at) => ({ name: `T${at}`, content })), complement: null });
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

test('la garde : un paquet sans aucun texte échoue et est nommé ; avec un complément, il passe', () => {
  const dir = folder({ 'README.md': 'pas de licence ici' });
  const one: Package = { ...rustOne('sanstexte', 'BSD-3-Clause'), dir };
  const bare = attachTexts([one], new Map());
  assert.deepEqual(bare.errors, []);
  assert.deepEqual(checkPackages(bare.packages), ['sanstexte 1.0.0 : aucun texte de licence (ni dans son dossier ni dans les compléments)']);
  const complements = new Map([['sanstexte', { files: [{ name: 'LICENSE', content: 'texte pris à la main' }], source: 'dépôt X, étiquette 1.0.0' }]]);
  const fixed = attachTexts([one], complements);
  assert.deepEqual(fixed.errors, []);
  assert.deepEqual(checkPackages(fixed.packages), []);
  assert.equal(fixed.packages[0]?.complement, 'dépôt X, étiquette 1.0.0');
  // Le complément s'ajoute aux textes du dossier, il ne les remplace pas.
  const dir2 = folder({ LICENSE: 'le sien' });
  const both = attachTexts([{ ...one, dir: dir2 }], complements).packages[0];
  assert.deepEqual(both?.texts.map((text) => text.content), ['le sien', 'texte pris à la main']);
});

test('un complément qui ne va à aucun paquet du graphe est une erreur (le dossier ne vieillit pas en silence)', () => {
  const complements = new Map([['orphelin', { files: [{ name: 'LICENSE', content: 'x' }], source: 's' }]]);
  const { errors } = attachTexts([withTexts(rustOne('autre'), ['t'])], complements, () => [{ name: 'LICENSE', content: 't' }]);
  assert.equal(errors.length, 1);
  assert.match(errors[0] ?? '', /complément orphelin : aucun paquet de ce nom/);
});

test('les compléments : un SOURCE.txt est exigé, et au moins un fichier de licence', () => {
  const dir = folder({
    'bon/LICENSE': 'texte',
    'bon/SOURCE.txt': 'dépôt, étiquette',
    'sanssource/LICENSE': 'texte',
    'sanstexte/SOURCE.txt': 'dépôt',
  });
  const { complements, errors } = readComplements(dir);
  assert.deepEqual([...complements.keys()], ['bon']);
  assert.equal(errors.length, 2);
  assert.ok(errors.some((line) => line.includes('sanssource') && line.includes('SOURCE.txt')));
  assert.ok(errors.some((line) => line.includes('sanstexte') && line.includes('aucun fichier de licence')));
  assert.equal(complements.get('bon')?.source, 'dépôt, étiquette');
  assert.ok(!complements.get('bon')?.files.some((file) => file.name === 'SOURCE.txt'), 'SOURCE.txt n\'est pas un texte de licence');
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

test('le vrai dépôt : la garde passe (licences permises, un texte par paquet, compléments tous utiles)', () => {
  assert.deepEqual(realBuild().errors, []);
});

test('le vrai dépôt : le fichier nomme tauri, serde, windows, lit-html, les deux polices et les paquets à complément', () => {
  const content = realBuild().content;
  for (const name of ['tauri', 'serde', 'windows', 'lit-html', '@fontsource-variable/m-plus-1', '@fontsource/orbitron', 'alloc-stdlib', 'selectors', 'webview2-com', 'webview2-com-macros', 'webview2-com-sys']) {
    assert.match(content, new RegExp(`^${name.replace(/[/@.-]/g, '\\$&')} \\d`, 'm'), `LICENCES-TIERS.txt ne nomme pas ${name}`);
  }
  // Aucun paquet du dépôt lui-même.
  assert.doesNotMatch(content, /^shinku(?:-core|-agent|-helper)? 0\.1\.0$/m);
  // Aucun chemin de la machine, aucune date, aucun retour chariot.
  assert.doesNotMatch(content, /\/home\/|\/root\/|[A-Za-z]:\\\\Users|\r/);
});

test('le vrai dépôt : les paquets sous MPL-2.0 portent la phrase du code source', () => {
  const content = realBuild().content;
  const mpl = (content.match(/^\S+ \S+\n(?: {2}.*\n)+/gm) ?? []).filter((block) => block.includes('\n  Licence : MPL-2.0\n'));
  assert.ok(mpl.length >= 5, `cinq paquets sous MPL-2.0 attendus, ${mpl.length} vus`);
  for (const block of mpl) assert.match(block, /Code source de cette version \(MPL-2\.0\) : https:\/\//);
});
