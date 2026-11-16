// L'habillage de l'installeur Windows, vérifié sans Windows.
//
// L'installeur lui-même n'est essayé que sur la machine Windows de
// l'intégration continue (scripts/essai-installeur.ps1). Ce qui peut l'être à
// la lecture des fichiers l'est ici : une image manquante, un fichier qui n'est
// plus en ASCII, une chaîne de Tauri oubliée, un vouvoiement ou un fond qui ne
// colle plus ne se verraient sinon qu'une fois l'installeur construit.

import assert from 'node:assert/strict';
import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { test } from 'node:test';

const root = new URL('../', import.meta.url);
const tauri = new URL('src-tauri/', root);
const folder = new URL('installeur/', tauri);
const read = (name: string): string => readFileSync(new URL(name, folder), 'utf8');

const config = JSON.parse(readFileSync(new URL('tauri.conf.json', tauri), 'utf8')) as {
  bundle: { windows: { nsis: Record<string, unknown> } };
};
const nsis = config.bundle.windows.nsis;

/** Les échelles de Windows prises en charge, et la taille des images à chacune (en pixels). */
const SCALES = [100, 125, 150, 175, 200];
const SIZES = {
  bandeau: { width: 191, height: 362 },
  signe: { width: 76, height: 58 },
};
const scaled = (size: number, scale: number): number => Math.round((size * scale) / 100);

test("la configuration de l'installeur pointe sur des fichiers qui existent", () => {
  for (const key of ['installerIcon', 'uninstallerIcon', 'sidebarImage', 'headerImage', 'installerHooks']) {
    const path = nsis[key];
    assert.equal(typeof path, 'string', `bundle.windows.nsis.${key} manque`);
    // Tauri résout ces chemins depuis src-tauri, comme `bundle.icon`.
    assert.ok(existsSync(new URL(path as string, tauri)), `${key} : ${String(path)} n'existe pas`);
  }
  const languages = nsis.customLanguageFiles as Record<string, string>;
  assert.deepEqual(Object.keys(languages), ['French']);
  assert.ok(existsSync(new URL(languages.French ?? '', tauri)), 'customLanguageFiles.French');
  assert.equal(nsis.template, undefined, 'le modèle de Tauri ne doit pas être remplacé');
  // Le bandeau et le signe à 100 % servent de repli à toutes les tailles.
  assert.equal(nsis.sidebarImage, 'installeur/images/bandeau-100.bmp');
  assert.equal(nsis.headerImage, 'installeur/images/signe-100.bmp');
});

test("habillage.nsh et agent.nsh sont en ASCII : les textes n'y dépendent pas de l'option -INPUTCHARSET que Tauri passe à makensis", () => {
  for (const name of ['habillage.nsh', 'agent.nsh']) {
    const bytes = readFileSync(new URL(name, folder));
    const outside = bytes.findIndex((byte) => byte > 0x7f);
    assert.equal(outside, -1, `${name} : octet non ASCII à la position ${outside} (les textes vont dans textes.nsh)`);
  }
  assert.match(read('habillage.nsh'), /!include \/CHARSET=UTF8 "\$\{SHINKU_INSTALLEUR\}\\textes\.nsh"/);
});

/** Le corps d'une macro NSIS de habillage.nsh, sans ses commentaires. */
function macro(name: string): string {
  const body = new RegExp(`^!macro ${name}\\n([\\s\\S]*?)^!macroend`, 'm').exec(read('habillage.nsh'))?.[1];
  assert.ok(body !== undefined, `habillage.nsh ne définit pas ${name}`);
  return body.replace(/^\s*;.*$/gm, '');
}

test("les quatre hooks de Tauri sont dans habillage.nsh et appellent ceux de l'agent du mode jeu", () => {
  const text = read('habillage.nsh');
  assert.match(text, /^!include "\$\{SHINKU_INSTALLEUR\}\\agent\.nsh"$/m);
  for (const [hook, agent] of [
    ['NSIS_HOOK_PREINSTALL', 'ShinkuAgentPreInstall'],
    ['NSIS_HOOK_POSTINSTALL', 'ShinkuAgentPostInstall'],
    ['NSIS_HOOK_PREUNINSTALL', 'ShinkuAgentPreUninstall'],
    ['NSIS_HOOK_POSTUNINSTALL', 'ShinkuAgentPostUninstall'],
  ] as const) {
    assert.match(read('agent.nsh'), new RegExp(`^!macro ${agent}$`, 'm'), `agent.nsh ne définit pas ${agent}`);
    // L'agent passe en premier : il lit encore ses fichiers, que la suite peut supprimer.
    assert.match(macro(hook).trimStart(), new RegExp(`^!insertmacro ${agent}\\n`), `${hook} doit commencer par ${agent}`);
  }
  // Tauri n'accepte qu'un fichier de hooks : aucun autre ne doit en définir.
  for (const name of ['agent.nsh', 'textes.nsh', 'francais.nsh']) {
    assert.doesNotMatch(read(name), /^!macro NSIS_HOOK_/m, `${name} définit un hook de Tauri`);
  }
});

/** Un fichier .nsh sans ses commentaires (lignes qui commencent par « ; »). */
const code = (name: string): string => read(name).replace(/^\s*;.*$/gm, '');

/** Les fichiers que « Supprimer aussi mes réglages » retire, par dossier. */
const SETTINGS_FILES: Record<string, string[]> = {
  $APPDATA: ['settings.json', 'settings.illisible.json', 'journal.json', 'settings.json.tmp', 'journal.json.tmp'],
  $LOCALAPPDATA: ['game.json', 'game.json.tmp', 'startup.json', 'startup.json.tmp'],
};

test('« Supprimer aussi mes réglages » supprime fichier par fichier, jamais un dossier entier', () => {
  const body = macro('ShinkuDeleteSettings');
  // Aucun « RMDir /r », nulle part : il suit les jonctions, avec les droits d'administrateur.
  for (const name of ['habillage.nsh', 'agent.nsh']) {
    assert.doesNotMatch(code(name), /^\s*RMDir\b.*\s\/r\b/im, `${name} : RMDir /r`);
  }
  // Toute suppression de fichier de habillage.nsh est dans la macro, sans option ni joker.
  const everywhere = Array.from(code('habillage.nsh').matchAll(/^\s*Delete\b(.*)$/gim), (match) => (match[1] ?? '').trim());
  const expected = Object.entries(SETTINGS_FILES).flatMap(([root, files]) => files.map((file) => `"${root}\\Shinku\\${file}"`));
  assert.deepEqual([...everywhere].sort(), [...expected].sort());
  assert.equal(body.match(/^\s*Delete\b/gim)?.length, expected.length, 'un Delete est hors de ShinkuDeleteSettings');
  // Chaque dossier : pas un point d'analyse (jonction, lien), ses fichiers un par un, puis RMDir sans /r.
  const literal = (text: string): string => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  for (const [root, files] of Object.entries(SETTINGS_FILES)) {
    const folder = `${root}\\Shinku`;
    const lines = [
      `\${If} ${root} != ""`,
      `System::Call 'kernel32::GetFileAttributesW(w "${folder}") i .r0'`,
      '${If} $0 <> -1',
      'IntOp $0 $0 & 0x400',
      '${If} $0 = 0',
      ...files.map((file) => `Delete "${folder}\\${file}"`),
      `RMDir "${folder}"`,
      '${EndIf}',
      '${EndIf}',
      '${EndIf}',
    ];
    const guarded = new RegExp(lines.map((line) => `^\\s*${literal(line)}\\n`).join('(?:\\s*\\n)*'), 'm');
    assert.match(body, guarded, `${folder} : garde, fichiers ou RMDir inattendus`);
  }
  // La suppression n'a lieu qu'à un endroit : case cochée et hors mise à jour.
  assert.equal(code('habillage.nsh').match(/!insertmacro\s+ShinkuDeleteSettings\b/g)?.length, 1);
  assert.doesNotMatch(code('agent.nsh'), /ShinkuDeleteSettings/);
  assert.match(
    macro('NSIS_HOOK_POSTUNINSTALL'),
    /^\s*!insertmacro ShinkuAgentPostUninstall\n\s*\$\{If\} \$DeleteAppDataCheckboxState = 1\n\s*\$\{AndIf\} \$UpdateMode <> 1\n\s*!insertmacro ShinkuDeleteSettings\n\s*\$\{EndIf\}\s*$/,
  );
});

/** Un BMP 24 bits : ses dimensions et la couleur de ses quatre coins. */
function bitmap(name: string): { width: number; height: number; corners: string[] } {
  const bytes = readFileSync(new URL(`images/${name}`, folder));
  assert.equal(bytes.toString('latin1', 0, 2), 'BM', `${name} n'est pas un BMP`);
  assert.equal(bytes.readUInt16LE(28), 24, `${name} n'est pas en 24 bits`);
  assert.equal(bytes.readUInt32LE(30), 0, `${name} est compressé`);
  const width = bytes.readInt32LE(18);
  const height = bytes.readInt32LE(22);
  assert.ok(height > 0, `${name} est écrit de haut en bas : NSIS attend l'inverse`);
  const stride = Math.ceil((width * 3) / 4) * 4;
  const offset = bytes.readUInt32LE(10);
  assert.equal(bytes.length, offset + stride * height, `${name} : taille du fichier`);
  const pixel = (x: number, y: number): string => {
    // Les lignes sont de bas en haut, les pixels en BGR.
    const at = offset + (height - 1 - y) * stride + x * 3;
    return [bytes[at + 2], bytes[at + 1], bytes[at]].map((value) => (value ?? 0).toString(16).padStart(2, '0')).join('');
  };
  return {
    width,
    height,
    corners: [pixel(0, 0), pixel(width - 1, 0), pixel(0, height - 1), pixel(width - 1, height - 1)],
  };
}

test("chaque image existe aux cinq tailles, au bon format, et son coin a la couleur du fond", () => {
  const background = /!define MUI_BGCOLOR "([0-9A-Fa-f]{6})"/.exec(read('habillage.nsh'))?.[1];
  assert.equal(background?.toLowerCase(), 'f4f1ee', 'le fond Brume');
  for (const [name, size] of Object.entries(SIZES)) {
    for (const scale of SCALES) {
      const file = `${name}-${scale}.bmp`;
      const image = bitmap(file);
      assert.deepEqual(
        { width: image.width, height: image.height },
        { width: scaled(size.width, scale), height: scaled(size.height, scale) },
        `${file} : dimensions`,
      );
      // Sans cette couleur exacte, une couture se verrait entre l'image et la page.
      assert.deepEqual(image.corners, Array(4).fill(background?.toLowerCase()), `${file} : couleur des coins`);
    }
  }
  const images = readdirSync(new URL('images/', folder)).sort();
  assert.equal(images.length, SCALES.length * Object.keys(SIZES).length, `images en trop ou en moins : ${images.join(', ')}`);
});

/** Les 27 chaînes du fichier de langue français de Tauri (tauri-bundler 2.10.4, languages/French.nsh). */
const TAURI_STRINGS = [
  'addOrReinstall',
  'alreadyInstalled',
  'alreadyInstalledLong',
  'appRunning',
  'appRunningOkKill',
  'chooseMaintenanceOption',
  'chooseHowToInstall',
  'createDesktop',
  'dontUninstall',
  'dontUninstallDowngrade',
  'failedToKillApp',
  'installingWebview2',
  'newerVersionInstalled',
  'older',
  'olderOrUnknownVersionInstalled',
  'silentDowngrades',
  'unableToUninstall',
  'uninstallApp',
  'uninstallBeforeInstalling',
  'unknown',
  'webview2AbortError',
  'webview2DownloadError',
  'webview2DownloadSuccess',
  'webview2Downloading',
  'webview2InstallError',
  'webview2InstallSuccess',
  'deleteAppData',
];

test('francais.nsh définit exactement les chaînes du français de Tauri', () => {
  const defined = Array.from(read('francais.nsh').matchAll(/^LangString (\w+) \$\{LANG_FRENCH\} "/gm), (match) => match[1] ?? '');
  assert.equal(new Set(defined).size, defined.length, 'une chaîne est définie deux fois');
  assert.deepEqual([...defined].sort(), [...TAURI_STRINGS].sort());
});

test("francais.nsh ne dépend d'aucun chemin relatif : Tauri en fait une copie ailleurs", () => {
  const text = read('francais.nsh');
  const paths = Array.from(text.matchAll(/^\s*(?:!include|File)\s.*$/gm), (match) => match[0]);
  for (const line of paths) {
    assert.match(line, /\$\{SHINKU_INSTALLEUR\}/, `chemin relatif : ${line.trim()}`);
  }
});

test("aucun fichier de l'installeur ne vouvoie", () => {
  const formal = /(?<![\p{L}\p{N}_])(vous|votre|vos|veuillez|cliquez|choisissez|sélectionnez)(?![\p{L}\p{N}_])/giu;
  const files = readdirSync(folder).filter((name) => name.endsWith('.nsh'));
  assert.deepEqual(files.sort(), ['agent.nsh', 'francais.nsh', 'habillage.nsh', 'textes.nsh']);
  for (const name of files) {
    for (const [number, line] of read(name).split('\n').entries()) {
      const found = line.match(formal);
      assert.equal(found, null, `${name}, ligne ${number + 1} : « ${found?.[0] ?? ''} »`);
    }
  }
});

test("une espace insécable précède « : », « ? » et « ! » dans les textes affichés", () => {
  for (const name of ['textes.nsh', 'francais.nsh']) {
    for (const [number, line] of read(name).split('\n').entries()) {
      if (line.startsWith(';')) {
        continue;
      }
      // Seulement le texte entre guillemets, hors séquences $\r$\n et chemins comme $0.
      for (const text of line.matchAll(/"([^"]*)"/g)) {
        const bad = /[^ \s](?: ?)[?!]|\S :|[^ \s]:(?=\s|$)/u.exec((text[1] ?? '').replaceAll(/\$\\[rn]/g, '\n'));
        assert.equal(bad, null, `${name}, ligne ${number + 1} : espace insécable attendue avant « ${bad?.[0] ?? ''} »`);
      }
    }
  }
});
