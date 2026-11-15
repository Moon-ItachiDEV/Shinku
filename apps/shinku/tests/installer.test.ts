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

test("habillage.nsh est en ASCII : les textes n'y dépendent pas de l'option -INPUTCHARSET que Tauri passe à makensis", () => {
  const bytes = readFileSync(new URL('habillage.nsh', folder));
  const outside = bytes.findIndex((byte) => byte > 0x7f);
  assert.equal(outside, -1, `octet non ASCII à la position ${outside} : les textes vont dans textes.nsh`);
  assert.match(bytes.toString('latin1'), /!include \/CHARSET=UTF8 "\$\{SHINKU_INSTALLEUR\}\\textes\.nsh"/);
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
  assert.deepEqual(files.sort(), ['francais.nsh', 'habillage.nsh', 'textes.nsh']);
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
