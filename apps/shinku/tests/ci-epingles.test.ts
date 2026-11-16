// Le flux de l'intégration continue (.github/workflows/ci.yml) : les actions sont désignées par l'empreinte de leur
// commit, et l'artefact de l'installeur porte son empreinte et la liste des licences.
//
// Limite, dite dans docs/conception.md : rien ici ne vérifie que le commentaire « # vX.Y.Z » dit la bonne version de
// l'empreinte (il faudrait le réseau), ni que l'empreinte ne vieillit pas.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const ci = readFileSync(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8');

/** La forme acceptée : `propriétaire/dépôt[/chemin]@<40 hexadécimaux minuscules> # vX.Y.Z`, ou un chemin local. */
const PINNED = /^(?:\.\/\S+|[A-Za-z0-9][\w.-]*\/[\w.-]+(?:\/[\w./-]+)?@[0-9a-f]{40} # v\d+\.\d+\.\d+)$/;

/**
 * Toute ligne du flux où `uses` paraît comme clé, sous n'importe quelle forme (`uses: x`, `- uses: x`,
 * `- { uses: x }`, `"uses": x`, `'uses': x`), avec ce qui suit les deux-points. Les lignes de commentaire sont
 * ignorées. Une forme que cette lecture ne sait pas juger (valeur entre guillemets, accolade fermante) garde son
 * reste de ligne tel quel : elle ne ressemble pas à une action épinglée, et l'essai échoue au lieu de passer.
 */
function usesOf(text: string): string[] {
  const found: string[] = [];
  for (const line of text.split('\n')) {
    if (/^\s*#/.test(line)) continue;
    const match = /(?:^|[\s{,])(["']?)uses\1\s*:\s*(.*?)\s*$/.exec(line);
    if (match !== null) found.push(match[2] ?? '');
  }
  return found;
}

test("le balayage des `uses` voit toutes les formes de la clé, et chacune est refusée si elle n'est pas écrite à la forme épinglée", () => {
  const sha = '11d5960a326750d5838078e36cf38b85af677262';
  const samples = [
    '      - uses: actions/cache@v4',
    '        uses: actions/cache@v4',
    '      - { uses: actions/cache@v4 }',
    '      - {uses: actions/cache@v4, with: {a: b}}',
    '      - "uses": actions/cache@v4',
    "      - 'uses': actions/cache@v4",
    '        "uses" : actions/cache@v4',
    '      - uses: "actions/cache@v4"',
    '      - uses: docker://alpine:3.20',
    `      - { uses: actions/checkout@${sha} # v4.4.0 }`,
  ];
  for (const sample of samples) {
    const seen = usesOf(sample);
    assert.equal(seen.length, 1, `non vu : ${sample}`);
    assert.doesNotMatch(seen[0] ?? '', PINNED, `devrait être refusé : ${sample}`);
  }
  // Les formes lues et justes passent ; un commentaire ou un mot voisin n'est pas une clé.
  assert.deepEqual(usesOf(`      - uses: actions/checkout@${sha} # v4.4.0`), [`actions/checkout@${sha} # v4.4.0`]);
  assert.deepEqual(usesOf('      # uses: actions/cache@v4\n      - name: reuses: autre'), []);
});

test("la forme d'une action épinglée : l'empreinte entière du commit et la version en commentaire, rien d'autre", () => {
  const sha = '11d5960a326750d5838078e36cf38b85af677262';
  for (const ok of [`actions/checkout@${sha} # v4.4.0`, `Swatinem/rust-cache@${sha} # v2.9.3`, `actions/cache/restore@${sha} # v4.2.1`, './.github/actions/local']) {
    assert.match(ok, PINNED, `${ok} devrait être accepté`);
  }
  for (const bad of [
    'actions/checkout@v4',
    'actions/checkout@main',
    `actions/checkout@${sha}`, // pas de version en commentaire
    `actions/checkout@${sha} # v4`, // version incomplète
    `actions/checkout@${sha} # main`,
    `actions/checkout@${sha.slice(0, 7)} # v4.4.0`, // empreinte abrégée
    `actions/checkout@${sha.toUpperCase()} # v4.4.0`, // majuscules
    `actions/checkout@${sha}0 # v4.4.0`, // trop longue
    `docker://alpine:3.20`,
    `actions/checkout # v4.4.0`,
  ]) {
    assert.doesNotMatch(bad, PINNED, `${bad} devrait être refusé`);
  }
});

test('chaque `uses:` du flux est une action épinglée par l\'empreinte de son commit (ou un chemin local)', () => {
  const uses = usesOf(ci);
  assert.ok(uses.length >= 13, `le balayage ne voit plus les actions : ${uses.length} trouvées`);
  for (const one of uses) assert.match(one, PINNED, `ci.yml : « uses: ${one} » n'est pas épinglée (propriétaire/dépôt@<40 hexadécimaux> # vX.Y.Z)`);
});

test('les quatre actions utilisées gardent leur version majeure, avec la même empreinte partout', () => {
  const majors: Record<string, string> = { 'actions/checkout': 'v4', 'actions/setup-node': 'v4', 'actions/upload-artifact': 'v4', 'Swatinem/rust-cache': 'v2' };
  const seen = new Map<string, Set<string>>();
  for (const one of usesOf(ci)) {
    const match = /^([^@]+)@([0-9a-f]{40}) # (v\d+)\.\d+\.\d+$/.exec(one);
    assert.ok(match !== null, one);
    const [, name = '', sha = '', major = ''] = match;
    assert.equal(major, majors[name], `${name} : la version majeure a changé (${major})`);
    seen.set(name, (seen.get(name) ?? new Set()).add(sha));
  }
  assert.deepEqual([...seen.keys()].sort(), Object.keys(majors).sort());
  for (const [name, shas] of seen) assert.equal(shas.size, 1, `${name} : plusieurs empreintes dans le même flux`);
});

/** Une étape du travail Windows : son texte, de « - name: » à la suivante. */
function step(name: string): string {
  const found = ci.split(/^ {6}- /m).find((one) => one.startsWith(`name: ${name}\n`));
  assert.ok(found !== undefined, `ci.yml : l'étape « ${name} » manque`);
  return found;
}

/** Les noms des étapes, dans l'ordre. */
const stepNames = (): string[] => [...ci.matchAll(/^ {6}- name: (.*)$/gm)].map((match) => match[1] ?? '');

test("l'artefact de l'installeur : l'installeur, SHA256SUMS.txt et LICENCES-TIERS.txt, sous le même nom", () => {
  const publish = step("Publier l'installeur");
  assert.match(publish, /name: shinku-installeur-x64\n/, "le nom de l'artefact ne change pas");
  const paths = /path: \|\n((?: {12}\S.*\n)+)/.exec(publish)?.[1]?.split('\n').map((line) => line.trim()).filter(Boolean);
  assert.deepEqual(paths, ['target/release/bundle/nsis/*-setup.exe', 'target/release/bundle/nsis/SHA256SUMS.txt', 'target/release/bundle/nsis/LICENCES-TIERS.txt']);
  assert.match(publish, /if-no-files-found: error\n/);
  assert.match(publish, /retention-days: 14\n/);
});

test("SHA256SUMS.txt : Get-FileHash SHA256, empreinte en minuscules, deux espaces, nom du fichier, \\n, sans marque d'ordre des octets", () => {
  const sums = step("Empreinte de l'installeur");
  assert.match(sums, /Get-FileHash -Algorithm SHA256 -LiteralPath \$setup\.FullName/);
  assert.match(sums, /\.Hash\.ToLowerInvariant\(\)/);
  assert.match(sums, /\$ligne = "\$empreinte {2}\$\(\$setup\.Name\)`n"/);
  assert.match(sums, /WriteAllText\(\(Join-Path \$setup\.DirectoryName 'SHA256SUMS\.txt'\), \$ligne, \(New-Object System\.Text\.UTF8Encoding \$false\)\)/);
  assert.match(sums, /if \(\$setups\.Count -ne 1\) \{ throw /);
  // La liste des licences voyage dans le même dossier que l'installeur : l'artefact n'a pas de sous-dossiers.
  assert.match(sums, /Copy-Item -LiteralPath apps\/shinku\/src-tauri\/licences\/LICENCES-TIERS\.txt -Destination \(Join-Path \$setup\.DirectoryName 'LICENCES-TIERS\.txt'\)/);
  // Aucune autre étape ne prend « le premier *-setup.exe » dans un dossier qui contiendrait autre chose : ces fichiers n'en sont pas.
  assert.ok(!'SHA256SUMS.txt'.endsWith('-setup.exe') && !'LICENCES-TIERS.txt'.endsWith('-setup.exe'));
});

test("l'étape de l'empreinte est juste avant la publication, et l'ordre des autres étapes ne change pas", () => {
  const names = stepNames();
  const at = names.indexOf("Empreinte de l'installeur");
  assert.ok(at > 0);
  assert.equal(names[at + 1], "Publier l'installeur");
  assert.equal(names[at - 1], "Publier les captures de l'installeur");
  assert.deepEqual(
    names.slice(names.indexOf('Installeur'), at).filter((name) => name === 'Aller-retour silencieux' || name === 'Installation' || name === 'Installeur'),
    ['Installeur', 'Installation', 'Aller-retour silencieux'],
  );
  assert.equal(names.indexOf("Publier l'installeur"), names.length - 1, "la publication de l'installeur reste la dernière étape");
});

test("l'aller-retour silencieux vérifie que LICENCES-TIERS.txt est posé, puis qu'il est retiré avec le dossier", () => {
  const trip = step('Aller-retour silencieux');
  assert.match(trip, /foreach \(\$fichier in [^)]*'LICENCES-TIERS\.txt'[^)]*\) \{\n\s+if \(-not \(Test-Path "\$dossier\\\$fichier"\)\) \{ throw "L'installation silencieuse n'a pas posé \$fichier\." \}/);
  assert.match(trip, /if \(Test-Path "\$dossier\\LICENCES-TIERS\.txt"\) \{ throw /);
  // Le fichier est contrôlé avant le dossier : le test du fichier peut échouer à lui seul (un dossier qui reste
  // contiendrait encore le fichier), au lieu de ne jamais être atteint.
  assert.ok(trip.indexOf('Test-Path "$dossier\\LICENCES-TIERS.txt"') < trip.indexOf('if (Test-Path $dossier) { throw'), 'le contrôle du fichier doit précéder celui du dossier');
  // Les autres vérifications restent : les trois programmes et le désinstalleur.
  for (const file of ['shinku.exe', 'shinku-agent.exe', 'shinku-helper.exe', 'uninstall.exe']) assert.ok(trip.includes(`'${file}'`), file);
});

test("l'étape « Licences des dépendances » : dans « Cœur et interface », après les dépendances de l'interface, lance la garde dans apps/shinku", () => {
  const job = ci.slice(ci.indexOf('  coeur-et-interface:'), ci.indexOf('  lint-windows:'));
  const names = [...job.matchAll(/^ {6}- (?:name: (.*)|uses: .*)$/gm)].map((match) => match[1] ?? '(uses)');
  const at = names.indexOf('Licences des dépendances');
  assert.ok(at > 0, "l'étape manque");
  assert.equal(names[at - 1], "Dépendances de l'interface", "elle suit npm ci : node_modules est nécessaire");
  assert.equal(names[at + 1], "Types de l'interface");
  const text = job.split(/^ {6}- /m).find((one) => one.startsWith('name: Licences des dépendances\n')) ?? '';
  assert.match(text, /working-directory: apps\/shinku\n\s+run: node scripts\/licences-tiers\.mjs --verifier\n/);
  // Les autres étapes du travail gardent leur nom et leur ordre.
  assert.deepEqual(
    names.filter((name) => name !== 'Licences des dépendances'),
    ['(uses)', '(uses)', 'Rust', '(uses)', 'Format', 'Lint', 'Tests du cœur', "Dépendances de l'interface", "Types de l'interface", "Tests de l'interface, du contrat et du câblage", "Compilation de l'interface"],
  );
});
