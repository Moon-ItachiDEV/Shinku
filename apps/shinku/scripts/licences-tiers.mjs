// Fabrique LICENCES-TIERS.txt : les licences de tout ce que Shinku embarque et qui n'est pas de lui, et vérifie
// qu'une nouvelle dépendance ne peut pas entrer sans que quelqu'un l'ait décidé.
//
//   node scripts/licences-tiers.mjs                   fabrique src-tauri/licences/LICENCES-TIERS.txt (ignoré par git)
//   node scripts/licences-tiers.mjs --verifier        ne fabrique rien : lance seulement la garde
//   node scripts/licences-tiers.mjs --sortie <fichier>   écrit ailleurs
//   node scripts/licences-tiers.mjs --hors-ligne      ajoute --offline aux appels de cargo
//                                                     (ou : SHINKU_CARGO_HORS_LIGNE=1)
//
// Les paquets Rust : l'union de ce que `cargo tree` donne pour les deux commandes que la fabrication lance
// vraiment (la fenêtre ; l'agent et l'assistant ensemble), moins les paquets du dépôt lui-même ; leurs détails
// viennent de `cargo metadata`. Les paquets de la page : ceux de `dependencies` de package-lock.json, plus la liste
// nommée PAGE_DEV_PACKAGES (les polices : elles sont en `devDependencies` mais leur contenu part dans la page).
// Chaque paquet donne ses fichiers de licence (racine du paquet seulement), ceux du dossier
// src-tauri/licences/complements/<nom>/ s'y ajoutent (textes pris à la main dans le dépôt d'origine, avec leur
// SOURCE.txt). La section fixe, src-tauri/licences/section-fixe.txt, est recopiée en tête.
//
// Les fonctions pures sont exportées : tests/licences-tiers.test.ts les appelle sans lancer cargo.
// Node 22, aucune dépendance.

import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

/** Les licences que Shinku accepte d'embarquer sans question. Une autre expression n'entre pas sans décision. */
export const ALLOWED_LICENSES = [
  'MIT',
  'MIT-0',
  'Apache-2.0',
  'Apache-2.0 WITH LLVM-exception',
  'BSD-2-Clause',
  'BSD-3-Clause',
  'ISC',
  'Zlib',
  '0BSD',
  'Unlicense',
  'CC0-1.0',
  'Unicode-3.0',
  'Unicode-DFS-2016',
  'BSL-1.0',
  'MPL-2.0',
  'OFL-1.1',
];

/** Les paquets de `devDependencies` dont le contenu part quand même dans la page : les deux polices. */
export const PAGE_DEV_PACKAGES = ['@fontsource-variable/m-plus-1', '@fontsource/orbitron'];

/** Les deux commandes que la fabrication lance : `tauri build` (la fenêtre) et `beforeBuildCommand` (les deux autres). */
export const TREE_TARGET = 'x86_64-pc-windows-msvc';
export const TREE_SELECTIONS = [['shinku'], ['shinku-agent', 'shinku-helper']];

/** Un fichier de licence plus gros que cela n'en est pas un : il n'est pas lu. */
export const MAX_TEXT_BYTES = 256 * 1024;

/** Les noms de fichiers de licence, à la racine d'un paquet, sans tenir compte de la casse. */
export const LICENSE_FILE_NAME = /^(licen[cs]e|copying|notice|unlicense)/i;

// ── La licence d'un paquet : lecture et garde ──────────────────────────────

/**
 * Lit une expression de licence (SPDX, et l'ancien `/` des paquets plus vieux) :
 * `OR` et `/` (une suffit), `AND` (toutes), parenthèses, `WITH` (fait partie du nom).
 * Renvoie un arbre, ou lève une erreur si l'expression est illisible.
 *
 * @param {string} text
 * @returns {{ kind: 'id', name: string } | { kind: 'or' | 'and', items: unknown[] }}
 */
export function parseExpression(text) {
  if (typeof text !== 'string' || text.trim() === '') throw new Error('expression vide');
  const tokens = text.match(/\(|\)|\/|[^\s()/]+/g) ?? [];
  let at = 0;
  const peek = () => tokens[at];
  const take = () => tokens[at++];

  const isWord = (token) => token !== undefined && token !== '(' && token !== ')' && token !== '/' && token !== 'AND' && token !== 'OR' && token !== 'WITH';

  function parseAtom() {
    const token = take();
    if (token === '(') {
      const inner = parseOr();
      if (take() !== ')') throw new Error('parenthèse non fermée');
      return inner;
    }
    if (!isWord(token)) throw new Error(`« ${token ?? 'fin'} » n'est pas un nom de licence`);
    if (peek() === 'WITH') {
      take();
      const exception = take();
      if (!isWord(exception)) throw new Error(`exception manquante après WITH (« ${token} WITH »)`);
      return { kind: 'id', name: `${token} WITH ${exception}` };
    }
    return { kind: 'id', name: token };
  }

  function parseAnd() {
    const items = [parseAtom()];
    while (peek() === 'AND') {
      take();
      items.push(parseAtom());
    }
    return items.length === 1 ? items[0] : { kind: 'and', items };
  }

  function parseOr() {
    const items = [parseAnd()];
    while (peek() === 'OR' || peek() === '/') {
      take();
      items.push(parseAnd());
    }
    return items.length === 1 ? items[0] : { kind: 'or', items };
  }

  const tree = parseOr();
  if (at < tokens.length) throw new Error(`« ${tokens[at]} » est de trop`);
  return tree;
}

/** Les noms de licence qui paraissent dans un arbre, dans l'ordre. */
export function licenseNames(tree) {
  return tree.kind === 'id' ? [tree.name] : tree.items.flatMap(licenseNames);
}

/** L'arbre est-il satisfait par la liste permise ? `OR` : une suffit ; `AND` : toutes. */
export function isSatisfied(tree, allowed = ALLOWED_LICENSES) {
  if (tree.kind === 'id') return allowed.includes(tree.name);
  return tree.kind === 'or' ? tree.items.some((item) => isSatisfied(item, allowed)) : tree.items.every((item) => isSatisfied(item, allowed));
}

/**
 * La garde sur une expression : `null` si elle passe, sinon la raison, en français.
 *
 * @param {string | null | undefined} expression
 * @returns {string | null}
 */
export function checkExpression(expression, allowed = ALLOWED_LICENSES) {
  if (expression === null || expression === undefined || String(expression).trim() === '') return 'aucune licence déclarée';
  let tree;
  try {
    tree = parseExpression(expression);
  } catch (error) {
    return `expression de licence illisible « ${expression} » (${error.message})`;
  }
  return isSatisfied(tree, allowed) ? null : `la licence « ${expression} » n'est pas dans la liste permise`;
}

// ── Les paquets Rust ───────────────────────────────────────────────────────

/** Les arguments de cargo : un tableau par appel. `offline` ajoute --offline (les essais hors ligne). */
export function cargoCalls({ offline = false } = {}) {
  const extra = offline ? ['--offline'] : [];
  const trees = TREE_SELECTIONS.map((packages) => [
    'tree',
    '--locked',
    ...extra,
    '-e',
    'normal,build',
    '--target',
    TREE_TARGET,
    '--prefix',
    'none',
    ...packages.flatMap((name) => ['-p', name]),
  ]);
  const metadata = ['metadata', '--format-version', '1', '--locked', ...extra];
  return { trees, metadata };
}

/**
 * Lit la sortie de `cargo tree --prefix none` : une ligne par paquet (`nom vX.Y.Z`, suivi au besoin de
 * `(proc-macro)`, de `(*)` ou du chemin d'un paquet local).
 *
 * @param {string} text
 * @returns {{ name: string, version: string }[]}
 */
export function parseCargoTree(text) {
  const found = [];
  for (const line of text.split(/\r?\n/)) {
    if (line.trim() === '') continue;
    const match = /^(\S+) v(\S+)/.exec(line);
    if (match === null) throw new Error(`ligne de cargo tree illisible : « ${line} »`);
    found.push({ name: match[1], version: match[2] });
  }
  return found;
}

/** L'union de plusieurs listes de paquets, sans doublon, triée par nom puis par version. */
export function unionPackages(lists) {
  const seen = new Map();
  for (const list of lists) for (const one of list) seen.set(`${one.name}\u0000${one.version}`, one);
  return [...seen.values()].sort(comparePackages);
}

function compareText(a, b) {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** Nom puis version : un ordre à nous, sans langue ni locale, pour que deux passages donnent le même fichier. */
export function comparePackages(a, b) {
  return compareText(a.name, b.name) || compareText(a.version, b.version);
}

/**
 * Les paquets Rust tiers : l'union des listes de `cargo tree`, sans les paquets du dépôt (sans `source`), avec
 * leurs détails venus de `cargo metadata`.
 *
 * @param {{ name: string, version: string }[][]} lists  une liste par appel de `cargo tree`
 * @param {{ packages: { name: string, version: string, source: string | null, license: string | null, repository: string | null, manifest_path: string }[] }} metadata
 * @returns {{ packages: object[], errors: string[] }}
 */
export function resolveRustPackages(lists, metadata) {
  const index = new Map();
  for (const one of metadata.packages) index.set(`${one.name}\u0000${one.version}`, one);
  const packages = [];
  const errors = [];
  for (const wanted of unionPackages(lists)) {
    const one = index.get(`${wanted.name}\u0000${wanted.version}`);
    if (one === undefined) {
      errors.push(`${wanted.name} ${wanted.version} : dans cargo tree, introuvable dans cargo metadata`);
      continue;
    }
    if (one.source === null) continue; // un paquet du dépôt lui-même
    packages.push({
      ecosystem: 'rust',
      name: one.name,
      version: one.version,
      license: one.license ?? null,
      repository: one.repository ?? null,
      dir: dirname(one.manifest_path),
    });
  }
  return { packages, errors };
}

// ── Les paquets de la page ─────────────────────────────────────────────────

/**
 * Les paquets de la page, d'après package-lock.json : tous ceux qui ne sont pas réservés au développement, plus
 * ceux de `extra` (par défaut les polices).
 *
 * @param {{ packages: Record<string, { version?: string, license?: string, dev?: boolean }> }} lock
 * @param {string} root  le dossier qui contient `node_modules` (les clés du verrou en sont relatives)
 * @returns {{ packages: object[], errors: string[] }}
 */
export function resolvePagePackages(lock, root, extra = PAGE_DEV_PACKAGES) {
  const prefix = 'node_modules/';
  const wanted = new Map();
  for (const [key, entry] of Object.entries(lock.packages)) {
    if (!key.startsWith(prefix) || entry.dev === true) continue;
    wanted.set(key.slice(key.lastIndexOf(prefix) + prefix.length), key);
  }
  const errors = [];
  for (const name of extra) {
    const key = prefix + name;
    if (lock.packages[key] === undefined) errors.push(`${name} : cité dans PAGE_DEV_PACKAGES, absent de package-lock.json`);
    else wanted.set(name, key);
  }
  const packages = [];
  for (const [name, key] of wanted) {
    const entry = lock.packages[key];
    if (entry === undefined) continue;
    packages.push({
      ecosystem: 'npm',
      name,
      version: entry.version ?? '',
      license: entry.license ?? null,
      repository: null,
      dir: join(root, key),
    });
  }
  packages.sort(comparePackages);
  return { packages, errors };
}

// ── Les textes ─────────────────────────────────────────────────────────────

/** Les fins de ligne en `\n`, sans marque d'ordre des octets, sans blanc à la fin : deux copies identiques se reconnaissent. */
export function normalizeText(text) {
  return text.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n').replace(/\s+$/, '');
}

/**
 * Les fichiers de licence directement dans un dossier : par leur nom, fichiers ordinaires (les liens ne sont pas
 * suivis), 256 Kio au plus. Triés par nom.
 *
 * @returns {{ name: string, content: string }[]}
 */
export function readLicenseFiles(dir) {
  if (!existsSync(dir)) return [];
  const files = [];
  for (const name of readdirSync(dir).sort(compareText)) {
    if (!LICENSE_FILE_NAME.test(name)) continue;
    const stat = lstatSync(join(dir, name));
    if (!stat.isFile() || stat.size > MAX_TEXT_BYTES) continue;
    files.push({ name, content: normalizeText(readFileSync(join(dir, name), 'utf8')) });
  }
  return files;
}

/**
 * Les compléments : un dossier par paquet, avec ses fichiers de licence et un SOURCE.txt qui dit d'où ils viennent.
 *
 * @returns {{ complements: Map<string, { files: { name: string, content: string }[], source: string }>, errors: string[] }}
 */
export function readComplements(dir) {
  const complements = new Map();
  const errors = [];
  if (!existsSync(dir)) return { complements, errors };
  for (const entry of readdirSync(dir, { withFileTypes: true }).sort((a, b) => compareText(a.name, b.name))) {
    if (!entry.isDirectory()) continue;
    const folder = join(dir, entry.name);
    const sourcePath = join(folder, 'SOURCE.txt');
    const files = readLicenseFiles(folder);
    if (!existsSync(sourcePath)) {
      errors.push(`complément ${entry.name} : SOURCE.txt manque (il dit d'où vient le texte)`);
      continue;
    }
    if (files.length === 0) {
      errors.push(`complément ${entry.name} : aucun fichier de licence`);
      continue;
    }
    complements.set(entry.name, { files, source: normalizeText(readFileSync(sourcePath, 'utf8')) });
  }
  return { complements, errors };
}

/**
 * Donne à chaque paquet ses textes : ceux de son dossier, puis ceux de son complément. Un complément qui ne va à
 * aucun paquet est une erreur : le dossier ne vieillit pas en silence.
 *
 * @returns {{ packages: object[], errors: string[] }}
 */
export function attachTexts(packages, complements, readFiles = readLicenseFiles) {
  const errors = [];
  const names = new Set(packages.map((one) => one.name));
  for (const name of complements.keys()) {
    if (!names.has(name)) errors.push(`complément ${name} : aucun paquet de ce nom dans les dépendances (à supprimer)`);
  }
  const done = packages.map((one) => {
    const own = readFiles(one.dir);
    const extra = complements.get(one.name);
    return {
      ...one,
      texts: [...own, ...(extra === undefined ? [] : extra.files)],
      complement: extra === undefined ? null : extra.source,
    };
  });
  return { packages: done, errors };
}

/** La garde, paquet par paquet : licence permise, un dépôt pour la MPL, au moins un texte. Les erreurs nomment le paquet. */
export function checkPackages(packages, allowed = ALLOWED_LICENSES) {
  const errors = [];
  for (const one of packages) {
    const label = `${one.name} ${one.version}`;
    const problem = checkExpression(one.license, allowed);
    if (problem !== null) errors.push(`${label} : ${problem}`);
    if (problem === null && needsSourceAddress(one) && (one.repository === null || one.repository === '')) {
      errors.push(`${label} : sous MPL-2.0 et sans adresse de dépôt : impossible de dire où obtenir le code source`);
    }
    if (one.texts.length === 0) {
      errors.push(`${label} : aucun texte de licence (ni dans son dossier ni dans les compléments)`);
    }
  }
  return errors;
}

/** Un paquet dont l'expression nomme MPL-2.0 doit dire où obtenir le code source de sa version. */
export function needsSourceAddress(one) {
  if (one.license === null || one.license === undefined) return false;
  try {
    return licenseNames(parseExpression(one.license)).includes('MPL-2.0');
  } catch {
    return false;
  }
}

// ── Le fichier ─────────────────────────────────────────────────────────────

const RULE = '-'.repeat(78);

/**
 * Numérote les textes : un numéro par texte distinct (après normalisation), dans l'ordre où les paquets (déjà
 * triés) les amènent. Renvoie les textes dans l'ordre des numéros, et pour chaque paquet les numéros des siens.
 */
export function numberTexts(packages) {
  const numbers = new Map();
  const texts = [];
  const perPackage = packages.map((one) =>
    [
      ...new Set(
        one.texts.map((text) => {
          const content = normalizeText(text.content);
          let number = numbers.get(content);
          if (number === undefined) {
            number = texts.length + 1;
            numbers.set(content, number);
            texts.push(content);
          }
          return number;
        }),
      ),
    ].sort((a, b) => a - b),
  );
  return { texts, perPackage };
}

/** Ce que le fichier dit de chaque paquet. */
function describePackage(one, numbers) {
  const lines = [`${one.name} ${one.version}`, `  Licence : ${one.license}`];
  if (one.repository !== null) lines.push(`  Dépôt : ${one.repository}`);
  if (needsSourceAddress(one)) {
    lines.push(`  Code source de cette version (MPL-2.0) : ${one.repository}, version ${one.version}`);
  }
  if (one.complement !== null) {
    lines.push('  Texte ajouté à la main. Sa provenance :');
    for (const line of one.complement.split('\n')) lines.push(`    ${line}`.replace(/\s+$/, ''));
  }
  lines.push(`  Textes : ${numbers.map((n) => `[${n}]`).join(' ')}`);
  return lines.join('\n');
}

/**
 * Compose le fichier : un en-tête, la section fixe, les paquets Rust, les paquets de la page, puis les textes
 * numérotés. Aucune date, aucun chemin de la machine : deux passages donnent le même fichier.
 *
 * @param {{ fixed: string, packages: object[] }} input  `packages` : Rust et page, avec leurs `texts`
 * @returns {string}
 */
export function composeFile({ fixed, packages }) {
  const sorted = [...packages].sort((a, b) => compareText(a.ecosystem, b.ecosystem) || comparePackages(a, b));
  const { texts, perPackage } = numberTexts(sorted);
  const part = (ecosystem) =>
    sorted
      .map((one, at) => ({ one, numbers: perPackage[at] }))
      .filter(({ one }) => one.ecosystem === ecosystem)
      .map(({ one, numbers }) => describePackage(one, numbers));

  const rust = part('rust');
  const page = part('npm');
  const out = [
    'LICENCES DES COMPOSANTS TIERS DE SHINKU',
    '',
    'Shinku embarque des composants écrits par d\'autres personnes. Ce fichier donne, pour chacun,',
    'son nom, sa version, sa licence, l\'adresse de son dépôt et le texte de sa licence, comme ces',
    'licences le demandent. Il est fabriqué par apps/shinku/scripts/licences-tiers.mjs, à partir des',
    'versions exactes utilisées (Cargo.lock et package-lock.json) ; ne le modifie pas à la main.',
    '',
    `Il compte ${rust.length} paquets Rust, ${page.length} paquets de l'interface et ${texts.length} textes de licence distincts.`,
    'Les textes sont à la fin, numérotés : chaque paquet renvoie aux siens par leur numéro entre crochets.',
    '',
    RULE,
    '1. COMPOSANTS FOURNIS AVEC LES PROGRAMMES',
    RULE,
    '',
    normalizeText(fixed),
    '',
    RULE,
    `2. PAQUETS RUST (${rust.length})`,
    RULE,
    '',
    rust.join('\n\n'),
    '',
    RULE,
    `3. PAQUETS DE L'INTERFACE (${page.length})`,
    RULE,
    '',
    page.join('\n\n'),
    '',
    RULE,
    `4. TEXTES DES LICENCES (${texts.length})`,
    RULE,
  ];
  texts.forEach((text, at) => {
    out.push('', `[${at + 1}]`, '', text);
    out.push('', RULE);
  });
  return `${out.join('\n')}\n`;
}

// ── Le tout, sur le vrai dépôt ─────────────────────────────────────────────

const appDir = join(dirname(fileURLToPath(import.meta.url)), '..');
const repoDir = join(appDir, '..', '..');
export const DEFAULT_OUTPUT = join(appDir, 'src-tauri', 'licences', 'LICENCES-TIERS.txt');

function runCargo(args) {
  return execFileSync('cargo', args, { cwd: repoDir, encoding: 'utf8', maxBuffer: 1 << 30, stdio: ['ignore', 'pipe', 'inherit'] });
}

/**
 * Tout ce que le script fait, sans écrire : lit le dépôt, lance cargo, applique la garde.
 *
 * @returns {{ content: string, errors: string[], stats: { rust: number, page: number, texts: number } }}
 */
export function build({ offline = false, run = runCargo } = {}) {
  const errors = [];
  const calls = cargoCalls({ offline });
  const lists = calls.trees.map((args) => parseCargoTree(run(args)));
  const metadata = JSON.parse(run(calls.metadata));
  const rust = resolveRustPackages(lists, metadata);
  errors.push(...rust.errors);

  const lock = JSON.parse(readFileSync(join(appDir, 'package-lock.json'), 'utf8'));
  const nodeModules = join(appDir, 'node_modules');
  const page = resolvePagePackages(lock, appDir);
  errors.push(...page.errors);
  if (!existsSync(nodeModules)) errors.push('node_modules manque : lance `npm ci` avant');

  const licenceDir = join(appDir, 'src-tauri', 'licences');
  const read = readComplements(join(licenceDir, 'complements'));
  errors.push(...read.errors);

  const attached = attachTexts([...rust.packages, ...page.packages], read.complements);
  errors.push(...attached.errors);
  errors.push(...checkPackages(attached.packages));

  const fixedPath = join(licenceDir, 'section-fixe.txt');
  let fixed = '';
  if (!existsSync(fixedPath) || normalizeText(readFileSync(fixedPath, 'utf8')) === '') {
    errors.push('src-tauri/licences/section-fixe.txt manque ou est vide');
  } else {
    fixed = readFileSync(fixedPath, 'utf8');
  }

  const content = composeFile({ fixed, packages: attached.packages });
  const texts = numberTexts(attached.packages).texts.length;
  return { content, errors, stats: { rust: rust.packages.length, page: page.packages.length, texts } };
}

/** Les arguments de la ligne de commande. */
export function parseArguments(argv) {
  const options = { verify: false, output: DEFAULT_OUTPUT, offline: process.env.SHINKU_CARGO_HORS_LIGNE === '1' };
  for (let at = 0; at < argv.length; at += 1) {
    const one = argv[at];
    if (one === '--verifier') options.verify = true;
    else if (one === '--hors-ligne') options.offline = true;
    else if (one === '--sortie' && argv[at + 1] !== undefined) options.output = argv[(at += 1)];
    else throw new Error(`argument inconnu : ${one}`);
  }
  return options;
}

function main() {
  let options;
  try {
    options = parseArguments(process.argv.slice(2));
  } catch (error) {
    console.error(`${error.message}\nusage : node scripts/licences-tiers.mjs [--verifier] [--sortie <fichier>] [--hors-ligne]`);
    process.exit(2);
  }
  let result;
  try {
    result = build({ offline: options.offline });
  } catch (error) {
    console.error(`licences-tiers : ${error.message}`);
    process.exit(1);
  }
  if (result.errors.length > 0) {
    // Rien n'est écrit : un fichier fabriqué malgré la garde passerait pour bon.
    console.error(`licences-tiers : ${result.errors.length} problème(s)`);
    for (const line of result.errors) console.error(`  - ${line}`);
    process.exit(1);
  }
  const { rust, page, texts } = result.stats;
  if (options.verify) {
    console.error(`licences-tiers : la garde passe (${rust} paquets Rust, ${page} paquets de l'interface, ${texts} textes distincts).`);
    return;
  }
  mkdirSync(dirname(options.output), { recursive: true });
  writeFileSync(options.output, result.content, 'utf8');
  console.error(`licences-tiers : fichier écrit (${rust} paquets Rust, ${page} paquets de l'interface, ${texts} textes distincts).`);
}

if (process.argv[1] !== undefined && import.meta.url === pathToFileURL(process.argv[1]).href) main();
