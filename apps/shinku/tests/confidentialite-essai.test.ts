// L'essai de la Confidentialité sur le vrai Windows (scripts/essai-confidentialite.ps1 et .mjs, l'étape de la
// CI), vérifié sans Windows : ce qu'il attend de la page existe dans la page, ses scripts sont écrits comme
// Windows PowerShell 5 veut les lire, le garde-fou est en tête de chaque fichier, l'essai permanent de la
// fenêtre ne clique jamais sur « Effacer », et la CI ne pose la variable que sur ses deux étapes.
//
// Il casse quand on renomme une classe ou un texte de la page : mieux ici, dans `npm test`, que sur la
// machine Windows de la CI, qui n'a pas toujours un compte pour tourner.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import type { PrivacyGroup, PrivacyView, TraceItem, TraceKind } from '../src/backend/types.ts';
import { fr } from '../src/i18n/fr.ts';
import { askFor, lastLines } from '../src/model/privacy.ts';
import { DOM, EXPECTED } from '../scripts/essai-confidentialite-dom.mjs';
import { type Node, parse, query, queryAll, textOfNode } from './html-tree.ts';
import { fakeDocument, markupOf } from './template.ts';

fakeDocument();
const { privacyDialog, privacyView } = await import('../src/views/privacy.ts');
const { initialState } = await import('../src/state.ts');

const NOW = new Date(2026, 10, 16, 12).getTime();
const scripts = new URL('../scripts/', import.meta.url);
const read = (name: string): string => readFileSync(new URL(name, scripts), 'utf8');
const workflow = readFileSync(new URL('../../../.github/workflows/ci.yml', import.meta.url), 'utf8');

/** Les mêmes blancs et la même casse que `shows` dans les essais : blancs insécables lus comme des espaces. */
const flat = (text: string): string => text.replace(/[  \s]+/g, ' ').trim().toLocaleLowerCase('fr');

// ── Une machine d'exemple : un faux profil, Windows, un navigateur ouvert ──

const trace = (id: string, kind: TraceKind, parts: Partial<TraceItem> = {}): TraceItem => ({ id, kind, state: 'present', bytes: null, count: null, ...parts });

function browser(app: NonNullable<PrivacyGroup['app']>, slot: number, profile: string | null, history: Partial<TraceItem>, running = false): PrivacyGroup {
  const key = `${app}:${slot}`;
  return { key, kind: 'browser', app, profile, slot, running, items: [trace(`${key}-h`, 'history', history)] };
}

const PROFILE_77 = browser('chrome', 77, null, { bytes: 90_000 });
const EMPTY_ONE = browser('chrome', 3, null, { state: 'empty' });
const OPEN_ONE = browser('edge', 0, null, { bytes: 3_000_000 }, true);
const WINDOWS: PrivacyGroup = {
  key: 'windows',
  kind: 'windows',
  app: null,
  profile: null,
  slot: null,
  running: false,
  items: [trace('w-recent', 'recent', { count: 4 }), trace('w-typed', 'typed', { count: 6 }), trace('w-clip', 'clipboard')],
};
const VIEW: PrivacyView = { groups: [PROFILE_77, EMPTY_ONE, OPEN_ONE, WINDOWS], firefox: false, edgeImportsChrome: false };

const noActions = {
  askErase: () => {},
  confirmErase: () => {},
  dismissErase: () => {},
  toggleTrace: () => {},
  openWindowsSettings: () => {},
  retryPrivacy: () => {},
} as never;

function page(parts: Record<string, unknown> = {}, shown: PrivacyView | null = VIEW): Node {
  const state = { ...initialState(NOW), route: 'privacy', privacy: shown, ...parts } as never;
  // La page est dans `<main class="main">` de la coque : l'essai y cherche aussi le titre.
  return parse(`<main class="main">${markupOf(privacyView(state, noActions))}</main>`);
}

function found(scope: Node, selector: string, what: string): Node[] {
  const nodes = queryAll(scope, selector);
  assert.ok(nodes.length > 0, `« ${what} » : le sélecteur ${selector} ne trouve rien dans la page`);
  return nodes;
}

const contains = (node: Node | null | undefined, expected: string): boolean => node !== null && node !== undefined && flat(textOfNode(node)).includes(flat(expected));

// ── Les sélecteurs trouvent leurs éléments ─────────────────────────────────

test('essai de la Confidentialité : les sélecteurs de la page chargée trouvent le titre, la phrase, le bouton, les cartes et les lignes', () => {
  const tree = page();
  assert.ok(contains(found(tree, DOM.title, 'titre')[0], EXPECTED.title));
  found(tree, DOM.page, 'page');
  assert.equal(queryAll(tree, DOM.loading).length, 0, 'chargée : plus de note de chargement');
  assert.ok(contains(found(tree, DOM.intro, 'phrase sous le titre')[0], EXPECTED.intro));
  const erase = found(tree, DOM.erase, 'bouton Effacer de l’en-tête');
  assert.equal(erase.length, 1, 'un seul bouton « Effacer ce qui est coché » dans l’en-tête');
  assert.ok(contains(erase[0], EXPECTED.erase));
  assert.equal(erase[0]?.attrs['aria-disabled'], 'true', 'rien n’est coché : le bouton est inerte');

  const cards = found(tree, DOM.browserCard, 'cartes de navigateur');
  assert.deepEqual(cards.map((card) => flat(textOfNode(query(card, DOM.browserName) as Node))), ['chrome', 'edge']);
  assert.ok(contains(query(cards[1] as Node, DOM.running), 'pour effacer son historique'));
  const rows = found(tree, DOM.browserRow, 'lignes de navigateur');
  assert.equal(rows.length, 3);
  const [checkRow, emptyRow, inertRow] = rows as [Node, Node, Node];
  assert.ok(query(checkRow, DOM.check) !== null, 'une ligne cochable a sa case');
  assert.equal(query(checkRow, DOM.check)?.attrs['aria-label'], 'Chrome, Profil 77, historique de navigation');
  assert.ok(contains(query(checkRow, DOM.rowTitle), 'Profil 77'));
  assert.ok(contains(query(checkRow, DOM.rowSub), EXPECTED.history));
  assert.ok(query(checkRow, DOM.rowValue) !== null, 'son poids');
  assert.ok(query(emptyRow, DOM.rowEmpty) !== null && query(emptyRow, DOM.check) === null, '« Rien à effacer » : sans case');
  assert.ok(query(inertRow, DOM.rowInert) !== null && query(inertRow, DOM.check) === null, 'un navigateur ouvert : ligne inerte, sans case');
  // La carte d'un navigateur se relit à partir d'elle-même (`cardRow`).
  assert.equal(queryAll(cards[0] as Node, DOM.cardRow).length, 2);

  const blocks = found(tree, DOM.windowsBlock, 'bloc Windows');
  const windows = blocks.find((block) => queryAll(block, DOM.browserCard).length === 0) as Node;
  assert.ok(windows !== undefined, 'le bloc Windows n’a pas de carte de navigateur');
  assert.ok(contains(found(windows, DOM.blockTitle, 'titre du bloc Windows')[0], EXPECTED.windows));
  const windowsRows = found(windows, DOM.windowsRow, 'lignes Windows');
  assert.deepEqual(windowsRows.map((row) => flat(textOfNode(query(row, DOM.rowTitle) as Node))), [EXPECTED.recent, EXPECTED.typed, EXPECTED.clipboard].map(flat));
  assert.deepEqual(windowsRows.map((row) => query(row, DOM.check)?.attrs['aria-label']), ['Windows, Fichiers et dossiers récents', 'Windows, Ce que tu as tapé dans Windows', 'Windows, Presse-papiers']);
  // Les sections de navigateurs ont aussi la classe `pblock` : l'essai les reconnaît à leurs cartes.
  assert.ok(blocks.length >= 1);
});

test('essai de la Confidentialité : la note de chargement, et rien d’autre, quand la vue n’est pas arrivée', () => {
  const tree = page({}, null);
  assert.ok(contains(found(tree, DOM.loading, 'note de chargement')[0], fr.common.loading));
  assert.equal(queryAll(tree, DOM.erase).length, 0);
});

test('essai de la Confidentialité : après un effacement, la carte « Dernier effacement » et l’issue sous la ligne', () => {
  const ask = askFor(VIEW, ['chrome:77-h', 'edge:0-h', 'w-typed']);
  assert.ok(ask !== null);
  const lines = lastLines(ask, [
    { id: 'chrome:77-h', outcome: 'inUse' },
    { id: 'w-typed', outcome: 'erased' },
  ] as never);
  assert.equal(lines.length, 2);
  const tree = page({ privacyLast: lines, privacyLineOutcomes: true, privacyLastAt: new Date(2026, 10, 16, 14, 32).getTime() });
  assert.ok(contains(found(tree, DOM.lastTitle, 'titre de la carte')[0], `${EXPECTED.lastTitle}, à 14 h 32`));
  const last = found(tree, DOM.last, 'lignes du dernier effacement');
  assert.equal(last.length, 2);
  const [first, second] = last as [Node, Node];
  assert.ok(contains(query(first, DOM.lastBlock), 'Chrome · Profil 77'));
  assert.ok(contains(query(first, DOM.lastTrace), EXPECTED.history));
  assert.ok(contains(query(first, DOM.lastValue), EXPECTED.outcomes.inUse));
  assert.ok(contains(query(first, DOM.lastHelp), 'Ferme Chrome et recommence'));
  assert.ok(contains(query(second, DOM.lastBlock), EXPECTED.windows));
  assert.ok(contains(query(second, DOM.lastTrace), EXPECTED.typed));
  assert.ok(contains(query(second, DOM.lastValue), EXPECTED.outcomes.erased));
  assert.equal(query(second, DOM.lastHelp), null, 'un effacement réussi n’a pas d’aide');
  // Sous la ligne elle-même : une phrase qui commence par l'issue de la carte.
  const outcomes = found(tree, DOM.rowOutcome, 'issue sous la ligne').map((node) => flat(textOfNode(node)));
  assert.ok(outcomes.some((text) => text.startsWith(flat(EXPECTED.outcomes.inUse))));
  assert.ok(outcomes.some((text) => text.startsWith(flat(EXPECTED.outcomes.erased))));
});

test('essai de la Confidentialité : la page occupée porte aria-busy, et la confirmation a ses blocs, ses lignes et ses deux boutons', () => {
  const busy = page({ privacyBusy: true });
  assert.equal(query(busy, DOM.page)?.attrs['aria-busy'], 'true');
  assert.equal(query(page(), DOM.page)?.attrs['aria-busy'], undefined);

  // Une seule trace : le cas de l'essai, qui ne coche jamais qu'une ligne à la fois.
  const one = askFor(VIEW, ['chrome:77-h']);
  const state = { ...initialState(NOW), route: 'privacy', privacy: VIEW, privacyAsk: one };
  const tree = parse(markupOf(privacyDialog(state as never, noActions)));
  const dialog = found(tree, DOM.dialog, 'confirmation')[0] as Node;
  assert.ok(contains(query(dialog, 'h2'), EXPECTED.askTitleOne));
  const blocks = found(dialog, DOM.askBlock, 'blocs de la confirmation');
  assert.equal(blocks.length, 1);
  assert.equal(flat(textOfNode(query(blocks[0] as Node, DOM.askName) as Node)), 'chrome');
  // Le nom du profil est dans un <bdi> : l'aide d'arbre met le texte propre avant les enfants, on vérifie les deux morceaux.
  const bullets = found(blocks[0] as Node, DOM.askLine, 'lignes de la confirmation');
  assert.equal(bullets.length, 1);
  assert.equal(flat(textOfNode(query(bullets[0] as Node, 'bdi') as Node)), 'profil 77');
  assert.ok(flat(textOfNode(bullets[0] as Node)).includes(': historique de navigation'));
  assert.ok(flat(textOfNode(dialog)).includes(flat(EXPECTED.directOne)));
  assert.ok(contains(found(dialog, DOM.askConfirm, 'bouton de confirmation')[0], EXPECTED.confirm));
  assert.ok(contains(found(dialog, DOM.askCancel, 'bouton Annuler')[0], EXPECTED.cancel));

  const several = askFor(VIEW, ['chrome:77-h', 'w-typed']);
  const text = flat(textOfNode(parse(markupOf(privacyDialog({ ...state, privacyAsk: several } as never, noActions)))));
  assert.ok(text.includes(flat(EXPECTED.direct)), 'plusieurs traces : la phrase au pluriel');
});

// ── Les textes attendus sont dans fr.ts ────────────────────────────────────

test('essai de la Confidentialité : chaque texte attendu est écrit dans fr.ts, tel quel', () => {
  const written = (text: string, where: string): void => assert.equal(flat(where), flat(text));
  written(EXPECTED.title, fr.privacy.title);
  assert.ok(flat(fr.privacy.intro).includes(flat(EXPECTED.intro)));
  written(EXPECTED.erase, fr.privacy.erase);
  written(EXPECTED.confirm, fr.privacy.confirm.confirm);
  written(EXPECTED.cancel, fr.privacy.confirm.cancel);
  written(EXPECTED.askTitleOne, fr.privacy.confirm.titleOne);
  written(EXPECTED.lastTitle, fr.privacy.lastTitle);
  assert.ok(flat(fr.privacy.lastTitleAt(14, 32)).startsWith(flat(EXPECTED.lastTitle)));
  assert.ok(flat(fr.privacy.refused.listChanged).startsWith(flat(EXPECTED.listChanged)));
  assert.ok(flat(fr.privacy.confirm.direct).startsWith(flat(EXPECTED.direct)));
  assert.ok(flat(fr.privacy.confirm.directOne).startsWith(flat(EXPECTED.directOne)));
  written(EXPECTED.chrome, fr.privacy.browser.chrome);
  written(EXPECTED.windows, fr.privacy.windows);
  written(EXPECTED.history, fr.privacy.trace.history);
  written(EXPECTED.recent, fr.privacy.trace.recent);
  written(EXPECTED.typed, fr.privacy.trace.typed);
  written(EXPECTED.clipboard, fr.privacy.trace.clipboard);
  assert.ok(flat(fr.privacy.running('Chrome')).includes(flat(EXPECTED.running)));
  for (const outcome of ['erased', 'nothing', 'inUse', 'changed', 'partial', 'failed'] as const) {
    written(EXPECTED.outcomes[outcome], fr.privacy.outcomeShort[outcome]);
  }
  // Toutes les issues du cœur sont couvertes ; « Résultat inconnu » est celle qu'une issue inattendue écrit : l'essai la refuse.
  assert.deepEqual(
    Object.keys(fr.privacy.outcomeShort).filter((key) => !(key in EXPECTED.outcomes)),
    ['unknown'],
  );
});

// ── Le script PowerShell ───────────────────────────────────────────────────

const ps1 = read('essai-confidentialite.ps1');
const driver = read('essai-confidentialite.mjs');
const windowTest = read('essai-fenetre.mjs');

/** Les étapes que le script accepte (`ValidateSet` de -Etape). */
const steps = (): string[] => {
  const set = /\[ValidateSet\(([^)]*)\)\]\s*\[string\]\$Etape/.exec(ps1);
  assert.ok(set !== null, 'le script déclare -Etape avec un ValidateSet');
  return [...(set[1] ?? '').matchAll(/'([^']+)'/g)].map((one) => one[1] as string);
};

test('essai de la Confidentialité : le script commence par une marque BOM, a des fins de ligne Unix, et reste dans la syntaxe de Windows PowerShell 5', () => {
  const bytes = readFileSync(new URL('essai-confidentialite.ps1', scripts));
  assert.deepEqual([...bytes.subarray(0, 3)], [0xef, 0xbb, 0xbf], 'BOM UTF-8 : sans elle, PowerShell 5 lit les accents de travers');
  assert.ok(!ps1.includes('\r'), 'pas de retour chariot');
  assert.ok(!ps1.slice(1).includes('\uFEFF'), 'une seule marque BOM, en tête (deux se liraient comme un caractère étranger dans le premier commentaire)');
  // Hors des textes C# et Python embarqués : pas de `??`, `?.`, `&&` ni `||` (PowerShell 7 seulement).
  const outside = ps1.replace(/@'[\s\S]*?\n'@/g, '');
  for (const word of ['??', '?.', '&&', '||']) {
    assert.ok(!outside.includes(word), `« ${word} » n’existe pas dans Windows PowerShell 5`);
  }
  // Une suppression qui suivrait un lien (jonction, lien symbolique) effacerait de l'autre côté : seule Remove-TreeSafe supprime des arbres.
  const code = ps1.split('\n').filter((line) => !/^\s*#/.test(line)).join('\n');
  assert.ok(!/Remove-Item[^\n]*-Recurse/.test(code), 'Remove-Item -Recurse suit les liens sous Windows PowerShell 5');
  assert.ok(!/\$[A-Za-z_]+:[A-Za-z_(]/.test(outside.replace(/\$env:|\$script:|\$global:/g, '')), '`$nom:` colle un deux-points à une variable : $($nom) ou ${nom}');
  // Les accolades et les parenthèses s'équilibrent (une faute de frappe ne se verrait sinon que sur Windows).
  for (const [open, close] of [['{', '}'], ['(', ')'], ['[', ']']] as const) {
    assert.equal(ps1.split(open).length, ps1.split(close).length, `${open} et ${close} ne s’équilibrent pas`);
  }
});

test('essai de la Confidentialité : le C#, le Python et le petit script « tenir » embarqués sont en ASCII, et le C# reste en C# 5', () => {
  const sources = [...ps1.matchAll(/@'\n([\s\S]*?)\n'@/g)].map((one) => one[1] as string);
  assert.equal(sources.length, 3, 'le C# (Add-Type), le Python et le script qui tient un fichier');
  for (const source of sources) {
    const outside = [...source].findIndex((letter) => (letter.codePointAt(0) ?? 0) > 127);
    assert.equal(outside, -1, `texte embarqué non ASCII à la position ${outside} : « ${source.slice(Math.max(0, outside - 20), outside + 20)} »`);
  }
  const csharp = sources.find((source) => source.includes('public static class')) as string;
  // Le compilateur de Windows PowerShell 5 est un C# 5 : ni `$"..."`, ni `?.`, ni `nameof`, ni `out var`, ni `=>` de membre.
  for (const feature of ['$"', '?.', 'nameof(', 'out var', ') =>', 'is not ']) {
    assert.ok(!csharp.includes(feature), `« ${feature} » n’existe pas en C# 5`);
  }
});

test('essai de la Confidentialité : le garde-fou est en tête du script et du pilote, avant tout geste', () => {
  for (const [name, text] of [['le script', ps1], ['le pilote', driver]] as const) {
    const head = text.split('\n').slice(0, 12).join('\n');
    assert.ok(head.includes('CET ESSAI EFFACE VRAIMENT'), `${name} le dit en tête`);
    assert.ok(head.includes('machine jetable'), `${name} dit que c’est pour une machine jetable`);
    assert.ok(head.includes('SHINKU_ESSAI_CONFIDENTIALITE'), `${name} nomme la variable en tête`);
  }
  const guard = ps1.indexOf("$env:SHINKU_ESSAI_CONFIDENTIALITE -ne '1'");
  assert.ok(guard > 0);
  assert.match(ps1.slice(guard, guard + 600), /exit 2/);
  for (const first of ['Add-Type', 'function ', 'Get-State', '[IO.Directory]', 'New-Item']) {
    assert.ok(ps1.indexOf(first, ps1.indexOf('param(')) > guard || ps1.indexOf(first, ps1.indexOf('param(')) === -1, `${first} vient avant le garde-fou`);
  }
  const check = driver.indexOf("process.env.SHINKU_ESSAI_CONFIDENTIALITE !== '1'");
  assert.ok(check > 0);
  assert.match(driver.slice(check, check + 700), /process\.exit\(2\)/);
  assert.ok(check < driver.indexOf('async function runStep'), 'le pilote vérifie avant de savoir lancer quoi que ce soit');
  assert.ok(check < driver.indexOf('process.exitCode = await main()'));
  // Sans le script, le pilote ne vaut que pour la démo (il ne peut alors rien effacer).
  assert.match(driver, /if \(WITHOUT_SCRIPT\) \{\n  const local = /);
});

test('essai de la Confidentialité : chaque étape que le pilote lance existe dans le script, est dispatchée, et imprime une ligne « FAIT : »', () => {
  const known = steps();
  const used = new Set([...driver.matchAll(/'((?:[a-z]+-)+(?:preparer|verifier|remplacer|creer|ouvrir|fermer|avant|apres)|nettoyer(?:-scenario)?)'/g)].map((one) => one[1] as string));
  assert.ok(used.size >= 20, `le pilote lance au moins vingt étapes (vu : ${used.size})`);
  for (const step of used) {
    assert.ok(known.includes(step), `le pilote lance « ${step} » : le ValidateSet du script ne la connaît pas`);
  }
  for (const step of known) {
    assert.ok(used.has(step), `le script a une étape « ${step} » que le pilote ne lance jamais`);
    const dispatched = new RegExp(`'${step}' \\{ (Step-[A-Za-z]+)(?: \\$[a-z]+)? \\}`).exec(ps1);
    assert.ok(dispatched !== null, `l’étape « ${step} » n’a pas de ligne dans le switch`);
    const name = dispatched[1] as string;
    const start = ps1.search(new RegExp(`^function ${name}\\b`, 'm'));
    assert.ok(start > 0, `${name} n’existe pas`);
    const body = ps1.slice(start, ps1.indexOf('\n}\n', start));
    if (!step.startsWith('nettoyer')) {
      assert.ok(/\bProof /.test(body), `${name} n’imprime aucune ligne « FAIT : »`);
    }
  }
  assert.match(ps1, /function Proof\(\[string\]\$Line\) \{\n    if \(-not \$Constat\) \{ Write-Host "FAIT : \$Line"; return \}/);
  // Les deux paramètres du script que le pilote passe.
  assert.match(ps1, /\[ValidateSet\('Chromium', 'Aucun'\)\]\s*\[string\]\$Partage/);
  assert.ok(driver.includes("'-Partage', 'Chromium'") && driver.includes("'-Partage', 'Aucun'"));
  // Un écart sort avec le code 1 ; Chrome absent, avec 10.
  assert.match(ps1, /\$exitCode = 1/);
  assert.match(ps1, /exit 10/);
  assert.match(driver, /tolerate: \[10\]/);
});

// ── Le pilote ──────────────────────────────────────────────────────────────

test('essai de la Confidentialité : le pilote ne coche qu’une ligne, vérifie la confirmation avant de confirmer, et ne confirme qu’après', () => {
  const erase = driver.slice(driver.indexOf('async function erase('), driver.indexOf('/** Compare l'));
  const verify = erase.indexOf('JSON.stringify(seen) !== JSON.stringify(wanted)');
  const confirm = erase.indexOf('window.click(EXPECTED.confirm');
  assert.ok(verify > 0 && confirm > verify, 'la liste de la confirmation est vérifiée avant le clic sur « Effacer »');
  assert.ok(erase.indexOf('others.length > 0') > 0, 'seule la ligne visée est cochée');
  assert.equal((driver.match(/window\.click\(EXPECTED\.confirm/g) ?? []).length, 1, 'un seul endroit confirme');
  // Le pilote ne vise que les lignes de ses scénarios.
  for (const target of ['chrome', 'chromeReal', 'recent', 'typed', 'clipboard']) {
    assert.ok(driver.includes(`  ${target}:`), `cible ${target}`);
  }
});

test('essai de la Confidentialité : les options du pilote sont celles que la CI lui passe', () => {
  const declared = [...(/parseArgs\(\{\s*options: \{([\s\S]*?)\n  \},\n\}\);/.exec(driver)?.[1] ?? '').matchAll(/^\s{4}(?:'([a-z-]+)'|([a-z]+)): \{/gm)].map((one) => one[1] ?? one[2]);
  assert.deepEqual(declared.sort(), ['adresse', 'captures', 'profil', 'programme', 'sans-script', 'seulement']);
  const step = workflow.split(/^ {6}- /m).find((one) => one.includes('essai-confidentialite.mjs')) as string;
  assert.ok(step !== undefined);
  for (const flag of step.match(/--[a-z-]+/g) ?? []) {
    assert.ok(declared.includes(flag.slice(2)), `la CI passe ${flag}, que le pilote ne connaît pas`);
  }
  assert.ok(!step.includes('--sans-script'), 'la CI ne saute jamais le script');
});

// ── L'essai permanent de la fenêtre ────────────────────────────────────────

test('essai de la fenêtre : il visite « Confidentialité » pour lire, et ne clique jamais sur « Effacer » dans cette page', () => {
  assert.ok(!windowTest.includes('Ce module arrive dans une prochaine version'), 'l’ancienne attente du module n’y est plus');
  assert.ok(windowTest.includes("from './essai-confidentialite-dom.mjs'"));
  const start = windowTest.indexOf('async function checkPrivacy(');
  assert.ok(start > 0, 'checkPrivacy existe');
  const body = windowTest.slice(start, windowTest.indexOf('\n}\n', start));
  assert.ok(body.includes('window.open(PRIVACY_EXPECTED.title)'));
  assert.ok(body.includes('PRIVACY_EXPECTED.intro') && body.includes('screen.erase.inert') && body.includes('row.checked'));
  for (const gesture of ['.click(', '.press(', 'dispatchMouseEvent', 'clickInPage', 'PRIVACY_DOM.askConfirm', 'PRIVACY_DOM.erase', 'PRIVACY_EXPECTED.confirm', '.clickSwitch(']) {
    assert.ok(!body.includes(gesture), `la visite en lecture seule ne doit pas faire « ${gesture} »`);
  }
  assert.ok(/await checkPrivacy\(window\);/.test(windowTest), 'main la visite');
  // Nulle part ailleurs dans l'essai permanent le bouton ni la confirmation de cette page ne sont visés.
  for (const selector of Object.values(DOM).filter((one) => /\.phead|dialog\.ask|button\.pill|\.pline|\.askblock/.test(one))) {
    assert.ok(!windowTest.includes(`'${selector}'`), `l’essai permanent ne vise pas « ${selector} »`);
  }
  assert.ok(!windowTest.includes('SHINKU_ESSAI_CONFIDENTIALITE'), 'l’essai permanent ne connaît pas la variable : il ne peut pas effacer');
});

// ── La CI ──────────────────────────────────────────────────────────────────

test('la CI pose SHINKU_ESSAI_CONFIDENTIALITE sur les deux étapes de l’essai, et nulle part ailleurs', () => {
  const stepsOf = workflow.split(/^ {6}- /m);
  const withVariable = stepsOf.filter((one) => one.includes('SHINKU_ESSAI_CONFIDENTIALITE'));
  assert.equal(withVariable.length, 2, 'l’essai et son nettoyage');
  for (const one of withVariable) {
    assert.ok(one.includes('essai-confidentialite'), 'la variable n’est posée que sur un pas qui lance cet essai');
    assert.match(one, /\n {8}env:\n {10}SHINKU_ESSAI_CONFIDENTIALITE: '1'\n/);
  }
  // Ni au niveau du workflow, ni à celui du travail (leur `env:` est avant les `steps:`).
  // (Le premier pas du travail Windows est l'action checkout, désignée par l'empreinte de son commit.)
  const windowsJob = workflow.indexOf('  windows:');
  const firstStep = windowsJob + workflow.slice(windowsJob).search(/^ {6}- uses: actions\/checkout@[0-9a-f]{40} # v4\.\d+\.\d+$/m);
  assert.ok(firstStep > windowsJob, 'le premier pas du travail Windows n’est plus l’action checkout épinglée');
  const beforeSteps = workflow.slice(0, firstStep);
  assert.ok(!beforeSteps.includes('SHINKU_ESSAI_CONFIDENTIALITE'));
  assert.equal((workflow.match(/SHINKU_ESSAI_CONFIDENTIALITE: /g) ?? []).length, 2);
});

test('la CI lance l’essai après l’essai de la fenêtre et avant la publication de l’installeur, et remet la machine en place quoi qu’il arrive', () => {
  const at = (needle: string): number => {
    const index = workflow.indexOf(needle);
    assert.ok(index > 0, `« ${needle} » manque dans ci.yml`);
    return index;
  };
  const windowStep = at('- name: Essai de la fenêtre');
  const essai = at('- name: Essai de la confidentialité');
  const cleaning = at("- name: Remettre la machine d'essai en place (confidentialité)");
  const publish = at("- name: Publier l'installeur");
  const reinstall = at("- name: Installeur sur une installation existante");
  assert.ok(windowStep < essai && essai < cleaning && cleaning < reinstall && reinstall < publish);
  const essaiText = workflow.slice(essai, cleaning);
  assert.match(essaiText, /if: \$\{\{ !cancelled\(\) && steps\.install\.outcome == 'success' \}\}/);
  assert.ok(essaiText.includes('node scripts/essai-confidentialite.mjs --programme "$env:ProgramFiles\\Shinku\\shinku.exe"'));
  const cleaningText = workflow.slice(cleaning, reinstall);
  assert.match(cleaningText, /if: always\(\)/);
  assert.ok(cleaningText.includes('-File scripts/essai-confidentialite.ps1 -Etape nettoyer'));
  // Les captures de la page sont publiées, même après un échec.
  assert.match(cleaningText, /name: captures-de-la-confidentialite/);
  // L'essai permanent, lui, ne change pas : même ligne qu'avant, sans la variable.
  assert.ok(workflow.includes('node scripts/essai-fenetre.mjs --programme "$env:ProgramFiles\\Shinku\\shinku.exe" --captures "$env:RUNNER_TEMP\\captures" --grand-vide'));
});

// ── Les corrections de la relecture ────────────────────────────────────────

test('essai de la Confidentialité : chaque scénario est nettoyé après lui, la table des scénarios est figée, et le piège du registre est défait', () => {
  const table = driver.slice(driver.indexOf('const SCENARIOS = ['), driver.indexOf('const unproven'));
  const line = (name: string): string => table.split('\n').find((one) => one.includes(`name: '${name}'`)) ?? '';
  // L'issue attendue de chaque scénario : une issue affaiblie (inUse -> measure) ferait passer un tout-ou-rien cassé.
  const wanted: Record<string, string> = {
    profil: "outcome: 'erased'",
    'verrou-fichier': "outcome: 'inUse'",
    'profil-remplace': "outcome: 'changed'",
    registre: "outcome: 'erased'",
    'registre-explorateur': "outcome: 'erased'",
    recents: "outcome: 'erased'",
    jonction: "outcome: 'refused'",
    'lien-physique': "outcome: 'refused'",
  };
  for (const [name, outcome] of Object.entries(wanted)) {
    assert.ok(line(name).includes(outcome), `${name} : ${outcome} attendu`);
  }
  for (const name of ['verrou-lockfile-chromium', 'verrou-lockfile-aucun']) {
    assert.ok(line(name).includes("row: 'inert'"), `${name} : la ligne doit être inerte`);
  }
  // Le nettoyage de chaque scénario, réussi ou non : après le `try`/`catch` de `attempt`, pas dans le `catch`.
  const attempt = driver.slice(driver.indexOf('const attempt = async'), driver.indexOf('try {\n    for (const scenario of SCENARIOS)'));
  const cleaning = attempt.indexOf("runStep('nettoyer-scenario'");
  assert.ok(cleaning > attempt.lastIndexOf('} catch (error) {'), 'le nettoyage n’est pas réservé aux échecs');
  assert.ok(cleaning > attempt.indexOf('done(`${name} : fait`)'));
  assert.ok(driver.includes("await runStep('nettoyer', [], { timeout: 120_000 })"), 'et un nettoyage final remet le User Data d’origine');
  // Le lien de registre est défait par sa vérification, et la pose des valeurs refuse un lien resté.
  const verifier = ps1.slice(ps1.indexOf('function Step-LienRegistreVerifier'), ps1.indexOf('# ── 4. Un profil remplacé'));
  assert.ok(/\} catch \{\n        \$original = \$_\n    \}[\s\S]*Remove-RegistryTrap[\s\S]*throw \$original/.test(verifier), 'la vérification défait le piège même après un écart, sans masquer l’écart d’origine');
  assert.ok(ps1.slice(ps1.indexOf('function Remove-RegistryTrap'), ps1.indexOf('function Step-LienRegistreVerifier')).includes('DeleteLink'));
  const typed = ps1.slice(ps1.indexOf('function Set-TypedValues'), ps1.indexOf('function Show-TypedCounts'));
  assert.ok(typed.indexOf('IsLink') > 0 && typed.indexOf('IsLink') < typed.indexOf('CreateSubKey'), 'refuse de poser des valeurs à travers un lien');
});

test('essai de la Confidentialité : les dossiers d’essai quittent TEMP, l’épinglé se compare en chemins longs, et Chrome reçoit son --user-data-dir', () => {
  const code = ps1.split('\n').filter((line) => !/^\s*#/.test(line)).join('\n');
  assert.ok(!/\$env:TEMP/.test(code.replace(/\$Work = Join-Path[^\n]*\n/, '')), 'RUNNER~1 : la forme courte de TEMP n’est pas celle du Shell');
  assert.ok(code.includes('$env:USERPROFILE') && code.includes('GetLongPathNameW'));
  assert.ok(code.includes('.Self.Path') && code.includes('-ieq'), 'chemins longs, sans tenir compte de la casse');
  // « impossible » seulement quand le verbe a échoué ; sinon « non vu » : une comparaison ratée ne l’affirme jamais.
  assert.ok(/if \(\$invoked\) \{[\s\S]*?'epingle'/.test(code));
  assert.ok(code.includes('ÉPINGLÉ NON VU') && code.includes('ÉPINGLAGE IMPOSSIBLE'));
  assert.ok(code.includes('--user-data-dir={1}'), 'Chrome écrit dans le User Data de l’essai');
  // Si Chrome est installé, il crée son User Data avant tout ; le faux Local State n'est écrit qu'en repli.
  const install = ps1.slice(ps1.indexOf('function Install-BrowserState'), ps1.indexOf('function Initialize-FakeBrowser'));
  assert.ok(install.indexOf("Start-Chrome 'chrome-modele'") > 0 && install.indexOf("Start-Chrome 'chrome-modele'") < install.lastIndexOf('WriteAllText'));
  // L'Explorateur : EnumWindows et WM_CLOSE, pas Shell.Application.Windows().
  assert.ok(code.includes('CabinetWClass') && code.includes('0x0010') && !code.includes('.Windows()'));
  // Le repli des récents, le constat, et un nettoyage incomplet qui garde l'état.
  assert.ok(code.includes('WScript.Shell') && code.includes('SHAddToRecentDocs n'));
  assert.ok(/switch\]\$Constat/.test(code) && driver.includes("['-Constat']"));
  const cleaning = ps1.slice(ps1.indexOf('function Step-Nettoyer'), ps1.indexOf('# ── Programme'));
  assert.ok(cleaning.indexOf('nettoyage incomplet (l\'état est gardé') < cleaning.indexOf("& $run 'dossier de travail'"), 'l’état n’est supprimé qu’après avoir vérifié qu’il ne reste rien à remettre');
  // Aucun verdict « synchrone » : il ne mesurait rien.
  assert.ok(!/est synchrone/.test(code));
});

test('essai de la Confidentialité : --sans-script exige une adresse locale et refuse le vrai shinku.exe', () => {
  const guard = driver.slice(driver.indexOf('if (WITHOUT_SCRIPT) {'), driver.indexOf('if (!WITHOUT_SCRIPT && process.env'));
  assert.ok(guard.includes('(127\\.0\\.0\\.1|localhost):\\d+') && guard.includes("'shinku.exe'") && guard.includes('process.exit(2)'));
  // La même expression, rejouée : seules les adresses de la machine locale passent.
  const local = /^http:\/\/(127\.0\.0\.1|localhost):\d+(\/|$)/;
  for (const good of ['http://127.0.0.1:8765/', 'http://localhost:5173', 'http://127.0.0.1:1/?privacy=few']) assert.ok(local.test(good), good);
  for (const bad of ['http://tauri.localhost/', 'https://tauri.localhost/', 'http://127.0.0.1/', 'http://example.org:80/', 'http://localhost.evil.org:80/']) assert.ok(!local.test(bad), bad);
});

test('la CI : la variable ne passe jamais par GITHUB_ENV, et l’essai a un délai', () => {
  assert.ok(!/GITHUB_ENV/.test(workflow.split('\n').filter((one) => one.includes('SHINKU_ESSAI_CONFIDENTIALITE')).join('\n')));
  for (const step of workflow.split(/^ {6}- /m).filter((one) => one.includes('essai-confidentialite'))) {
    assert.ok(!step.includes('GITHUB_ENV'), 'la variable ne doit pas fuir vers les étapes suivantes');
    assert.ok(!step.includes('GITHUB_OUTPUT'));
  }
  assert.match(workflow, /- name: Essai de la confidentialité\n {8}if: [^\n]*\n {8}timeout-minutes: 20\n/);
  assert.match(workflow, /- name: Remettre la machine d'essai en place \(confidentialité\)\n {8}if: always\(\)\n {8}timeout-minutes: 5\n/);
  // Le commentaire dit la vérité : les trois clés et les récents sont vidés, pas remis.
  assert.ok(workflow.includes('VIDÉS, pas'));
});

test('essai de la Confidentialité : le mode -Constat ne dit jamais « FAIT », la ligne inerte absente échoue avant tout retour, et le résumé nomme le non prouvé', () => {
  const proof = ps1.slice(ps1.indexOf('function Proof('), ps1.indexOf('function Fail('));
  const constat = proof.slice(proof.indexOf('if ($script:Gaps -eq 0)'));
  assert.ok(constat.includes('CONSTAT') && !constat.includes('FAIT'), 'la branche -Constat n’écrit pas « FAIT »');
  const inert = driver.slice(driver.indexOf("if (scenario.row === 'inert' && STRICT) {"), driver.indexOf("if (row === null || row.kind !== 'check') {\n        done("));
  const absent = inert.indexOf('if (row === null) {');
  assert.ok(absent > 0 && inert.slice(absent, inert.indexOf('}', absent)).includes('fail('), 'une ligne absente échoue');
  assert.ok(!inert.slice(0, absent).includes('return'), 'aucun return avant l’échec de la ligne absente');
  assert.ok(driver.includes('ÉPINGLÉ NON VU') && driver.includes('ÉPINGLAGE IMPOSSIBLE') && driver.includes('voie : repli'));
  assert.ok(driver.includes("taskkill', ['/im'"), 'le nettoyage final arrête shinku.exe');
  assert.match(workflow, /timeout-minutes: 60\n/);
});
