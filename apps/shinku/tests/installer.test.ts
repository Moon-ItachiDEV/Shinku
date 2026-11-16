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

test("habillage.nsh, agent.nsh, helper.nsh et oublier.nsh sont en ASCII : les textes n'y dépendent pas de l'option -INPUTCHARSET que Tauri passe à makensis", () => {
  for (const name of ['habillage.nsh', 'agent.nsh', 'helper.nsh', 'oublier.nsh']) {
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

/** Le corps d'un hook sans la ligne qui ferme la fenêtre de Shinku (la macro CheckIfAppIsRunning du modèle de Tauri). */
const withoutWindowClose = (body: string): string => body.replace(/^\s*!insertmacro CheckIfAppIsRunning .*\n/m, '');

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
    // L'agent passe en premier : il lit encore ses fichiers, que la suite peut supprimer. Seule
    // exception : à la désinstallation, la fenêtre de Shinku est fermée avant lui (voir plus bas).
    assert.match(withoutWindowClose(macro(hook)).trimStart(), new RegExp(`^!insertmacro ${agent}\\n`), `${hook} doit commencer par ${agent}`);
  }
  // Tauri n'accepte qu'un fichier de hooks : aucun autre ne doit en définir.
  for (const name of ['agent.nsh', 'helper.nsh', 'oublier.nsh', 'textes.nsh', 'francais.nsh']) {
    assert.doesNotMatch(read(name), /^!macro NSIS_HOOK_/m, `${name} définit un hook de Tauri`);
  }
});

test("les quatre hooks appellent aussi ceux de l'assistant élevé, juste après ceux de l'agent", () => {
  assert.match(read('habillage.nsh'), /^!include "\$\{SHINKU_INSTALLEUR\}\\helper\.nsh"$/m);
  for (const [hook, agent, helper] of [
    ['NSIS_HOOK_PREINSTALL', 'ShinkuAgentPreInstall', 'ShinkuHelperPreInstall'],
    ['NSIS_HOOK_POSTINSTALL', 'ShinkuAgentPostInstall', 'ShinkuHelperPostInstall'],
    ['NSIS_HOOK_PREUNINSTALL', 'ShinkuAgentPreUninstall', 'ShinkuHelperPreUninstall'],
    ['NSIS_HOOK_POSTUNINSTALL', 'ShinkuAgentPostUninstall', 'ShinkuHelperPostUninstall'],
  ] as const) {
    assert.match(read('helper.nsh'), new RegExp(`^!macro ${helper}$`, 'm'), `helper.nsh ne définit pas ${helper}`);
    assert.match(
      withoutWindowClose(macro(hook)).trimStart(),
      new RegExp(`^!insertmacro ${agent}\\n\\s*!insertmacro ${helper}\\n`),
      `${hook} doit appeler ${helper} juste après ${agent}`,
    );
  }
  // L'assistant est copié depuis target/release et retiré à la désinstallation, sans jamais de /REBOOTOK.
  const helper = read('helper.nsh').replace(/^\s*;.*$/gm, '');
  assert.match(helper, /^\s*File "\$\{SHINKU_HELPER_SOURCE\}"$/m);
  assert.equal(helper.match(/^\s*Delete "\$INSTDIR\\shinku-helper\.exe"$/gm)?.length, 2);
  assert.doesNotMatch(helper, /\/REBOOTOK/i);
});

/** Les lignes de code (sans commentaire ni blanc) d'une macro de helper.nsh. */
function helperLines(name: string): string[] {
  const body = new RegExp(`^!macro ${name}\\n([\\s\\S]*?)^!macroend`, 'm').exec(read('helper.nsh'))?.[1];
  assert.ok(body !== undefined, `helper.nsh ne définit pas ${name}`);
  return body
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '' && !line.startsWith(';'));
}

test("helper.nsh inclut LogicLib lui-même : il utilise \${If} sans dépendre de l'ordre d'inclusion", () => {
  assert.match(read('helper.nsh'), /^!include LogicLib\.nsh$/m);
});

test("l'assistant élevé est arrêté avant d'être copié ou supprimé, et le dossier ne part pas en mise à jour", () => {
  const stop = '!insertmacro ShinkuHelperStop';
  const remove = 'Delete "$INSTDIR\\shinku-helper.exe"';
  assert.deepEqual(helperLines('ShinkuHelperPreInstall'), [stop]);
  // Le fichier de l'assistant en marche est verrouillé : l'arrêt vient avant la copie et avant chaque suppression.
  assert.deepEqual(helperLines('ShinkuHelperPostInstall'), [stop, 'SetOutPath "$INSTDIR"', 'File "${SHINKU_HELPER_SOURCE}"']);
  assert.deepEqual(helperLines('ShinkuHelperPreUninstall'), [stop, remove]);
  // Le dossier n'est retiré (hors mise à jour) qu'une fois l'assistant arrêté et son fichier supprimé.
  assert.deepEqual(helperLines('ShinkuHelperPostUninstall'), [stop, remove, '${If} $UpdateMode <> 1', 'RMDir "$INSTDIR"', '${EndIf}']);
});

/** Un fichier .nsh sans ses commentaires (lignes qui commencent par « ; »). */
const code = (name: string): string => read(name).replace(/^\s*;.*$/gm, '');

/** Tous les fichiers .nsh de l'installeur. */
const NSH_FILES = ['habillage.nsh', 'agent.nsh', 'helper.nsh', 'oublier.nsh', 'textes.nsh', 'francais.nsh'];

/** Les lignes de code (sans commentaire ni blanc, indentation retirée) d'une macro d'un fichier .nsh. */
function macroLines(file: string, name: string): string[] {
  const body = new RegExp(`^!macro ${name}\\b[^\\n]*\\n([\\s\\S]*?)^!macroend`, 'm').exec(read(file))?.[1];
  assert.ok(body !== undefined, `${file} ne définit pas ${name}`);
  return body
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '' && !line.startsWith(';'));
}

/** Les lignes de code d'un texte, sans indentation. */
const linesOf = (text: string): string[] =>
  text
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line !== '' && !line.startsWith(';'));

const RUN_AS_USER = 'nsis_tauri_utils::RunAsUser "$INSTDIR\\${MAINBINARYNAME}.exe" "--oublier"';

test("NSIS_HOOK_PREUNINSTALL : la fenêtre de Shinku, l'agent, l'assistant, puis --oublier, dans cet ordre", () => {
  assert.deepEqual(macroLines('habillage.nsh', 'NSIS_HOOK_PREUNINSTALL'), [
    '!insertmacro CheckIfAppIsRunning "$INSTDIR\\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"',
    '!insertmacro ShinkuAgentPreUninstall',
    '!insertmacro ShinkuHelperPreUninstall',
    '!insertmacro ShinkuOublierPreUninstall',
  ]);
  // La macro du modèle n'est insérée qu'une fois par nos fichiers : le modèle la rejoue juste après le crochet.
  const inserted = NSH_FILES.flatMap((name) => code(name).match(/!insertmacro\s+CheckIfAppIsRunning\b/g) ?? []);
  assert.equal(inserted.length, 1);
  assert.deepEqual(macroLines('habillage.nsh', 'NSIS_HOOK_POSTUNINSTALL'), [
    '!insertmacro ShinkuAgentPostUninstall',
    '!insertmacro ShinkuHelperPostUninstall',
    '!insertmacro ShinkuOublierPostUninstall',
  ]);
  assert.match(read('habillage.nsh'), /^!include "\$\{SHINKU_INSTALLEUR\}\\oublier\.nsh"$/m);
});

test("--oublier n'est demandé que si la case est cochée et hors mise à jour ($DeleteAppDataCheckboxState = 1 et $UpdateMode <> 1)", () => {
  const lines = macroLines('oublier.nsh', 'ShinkuOublierPreUninstall');
  assert.deepEqual(lines.slice(0, 4), [
    'StrCpy $ShinkuOublier 0',
    '${If} $DeleteAppDataCheckboxState = 1',
    '${AndIf} $UpdateMode <> 1',
    'StrCpy $ShinkuOublier 1',
  ]);
  // Rien de la demande n'est hors de ce bloc : il s'ouvre à la ligne 2 et se ferme à la dernière ligne.
  assert.equal(lines.at(-1), '${EndIf}');
  assert.equal(lines.indexOf(RUN_AS_USER) > 3, true);
});

test('la réponse de Tauri est remise à 0 avant le lancement : son bloc ne fait plus ses deux RmDir /r', () => {
  const lines = macroLines('oublier.nsh', 'ShinkuOublierPreUninstall');
  const reset = lines.indexOf('StrCpy $DeleteAppDataCheckboxState 0');
  assert.ok(reset > 3, '$DeleteAppDataCheckboxState doit être remis à 0 dans le bloc de la case');
  assert.ok(reset < lines.indexOf(RUN_AS_USER), 'la remise à 0 vient avant le lancement de --oublier');
  // Ce n'est qu'à cet endroit que notre variable garde la réponse, et qu'on touche à celle de Tauri.
  assert.equal(NSH_FILES.flatMap((name) => code(name).match(/StrCpy \$DeleteAppDataCheckboxState\b/g) ?? []).length, 1);
});

test('--oublier est lancé par RunAsUser (droits ordinaires), avec ce seul mot, une seule fois', () => {
  const all = NSH_FILES.flatMap((name) => code(name).split('\n').map((line) => line.trim()));
  assert.deepEqual(
    all.filter((line) => line.includes('RunAsUser')),
    [RUN_AS_USER],
  );
  // Aucun autre moyen de lancer shinku.exe : pas d'Exec, ExecWait ni ExecShell sur lui (l'agent, lui, a le sien).
  for (const name of NSH_FILES) {
    assert.doesNotMatch(code(name), /^\s*(?:Exec|ExecWait|ExecShell|ExecShellWait)\b.*(?:shinku\.exe|MAINBINARYNAME)/im, `${name} : Exec sur shinku.exe`);
  }
});

test("aucun .nsh ne supprime quoi que ce soit dans un profil : ni Delete, ni RMDir, ni RmDir vers $APPDATA ou $LOCALAPPDATA", () => {
  // Plus de ShinkuDeleteSettings : ni la macro, ni un appel.
  for (const name of NSH_FILES) {
    assert.doesNotMatch(code(name), /ShinkuDeleteSettings/, `${name} : ShinkuDeleteSettings`);
    assert.doesNotMatch(code(name), /^\s*RMDir\b.*\s\/r\b/im, `${name} : RMDir /r`);
    const profile = /^\s*(?:Delete|RMDir)\b.*(?:\$APPDATA|\$LOCALAPPDATA|\$PROFILE|\$\{?APPDATA|\$\{?LOCALAPPDATA|%APPDATA%|%LOCALAPPDATA%|\$DOCUMENTS|\$TEMP)/gim;
    assert.deepEqual(code(name).match(profile) ?? [], [], `${name} efface dans un profil`);
  }
  // Et rien d'autre dans oublier.nsh : il ne supprime et n'écrit aucun fichier.
  assert.doesNotMatch(code('oublier.nsh'), /^\s*(?:Delete|RMDir|FileOpen|FileWrite|Rename|CopyFiles)\b/im);
});

test("l'attente : dix secondes au plus pour voir shinku.exe occupé, puis trente pour le voir libre, par pas de 100 ms", () => {
  const text = code('oublier.nsh');
  const value = (name: string): number => Number(new RegExp(`^!define ${name} (\\d+)$`, 'm').exec(text)?.[1]);
  const step = value('SHINKU_OUBLIER_PAS_MS');
  assert.equal(step, 100);
  assert.equal((value('SHINKU_OUBLIER_PAS_POUR_VOIR') * step) / 1000, 10);
  assert.equal((value('SHINKU_OUBLIER_PAS_POUR_FINIR') * step) / 1000, 30);
  const lines = macroLines('oublier.nsh', 'ShinkuOublierPreUninstall');
  // Chaque boucle dort un pas et sort quand son nombre de pas est atteint.
  assert.equal(lines.filter((line) => line === 'Sleep ${SHINKU_OUBLIER_PAS_MS}').length, 2);
  assert.equal(lines.filter((line) => line === '${If} $3 >= ${SHINKU_OUBLIER_PAS_POUR_VOIR}').length, 1);
  assert.equal(lines.filter((line) => line === '${If} $3 >= ${SHINKU_OUBLIER_PAS_POUR_FINIR}').length, 1);
  // D'abord le voir occupé, ensuite seulement le voir libre : la seconde boucle n'a lieu que s'il l'a été.
  const seen = lines.indexOf('${If} $3 >= ${SHINKU_OUBLIER_PAS_POUR_VOIR}');
  const done = lines.indexOf('${If} $3 >= ${SHINKU_OUBLIER_PAS_POUR_FINIR}');
  assert.ok(lines.indexOf(RUN_AS_USER) < seen && seen < lines.indexOf('${If} $4 = 1') && lines.indexOf('${If} $4 = 1') < done);
});

test("le verrou de shinku.exe se lit en ouvrant le fichier existant en écriture, sans partage, sans rien écrire ni tronquer", () => {
  const lines = macroLines('oublier.nsh', 'ShinkuOublierVerrou');
  // GENERIC_WRITE (0x40000000), partage nul, OPEN_EXISTING (3) : ni création ni troncature ; poignée fermée aussitôt.
  assert.equal(
    lines[0],
    `System::Call 'kernel32::CreateFileW(w "$INSTDIR\\\${MAINBINARYNAME}.exe", i 0x40000000, i 0, p 0, i 3, i 0, p 0) p .r1 ?e'`,
  );
  assert.ok(lines.includes("System::Call 'kernel32::CloseHandle(p r1)'"));
  // Un fichier absent (erreurs 2 et 3) n'est pas verrouillé : il ne faut pas attendre pour rien.
  assert.ok(lines.includes('${ElseIf} $2 = 2') && lines.includes('${OrIf} $2 = 3'));
});

test("si --oublier n'est jamais vu ou n'a pas fini : le message, avec /SD IDOK, jamais de repli élevé ; encore occupé, il est arrêté", () => {
  const nbsp = ' ';
  const text = /^LangString shinkuOublierEchec \$\{LANG_FRENCH\} "(.*)"$/m.exec(read('francais.nsh'))?.[1];
  assert.equal(
    text,
    `Shinku n'a pas pu supprimer tes réglages. Ils sont dans %APPDATA%\\Shinku et %LOCALAPPDATA%\\Shinku${nbsp}: tu peux les supprimer à la main.`,
  );
  const lines = macroLines('oublier.nsh', 'ShinkuOublierPreUninstall');
  const box = lines.filter((line) => line.startsWith('MessageBox'));
  assert.deepEqual(box, ['MessageBox MB_OK|MB_ICONEXCLAMATION "$(shinkuOublierEchec)" /SD IDOK']);
  // Le message n'est montré que si jamais vu occupé ($4 = 0) ou encore occupé ($0 = 1).
  const message = lines.indexOf(box[0] ?? '');
  assert.deepEqual(lines.slice(message - 7, message), [
    '${If} $4 = 0',
    '${OrIf} $0 = 1',
    '${If} $0 = 1',
    "nsExec::Exec 'taskkill /F /IM ${MAINBINARYNAME}.exe'",
    'Pop $0',
    'Sleep 500',
    '${EndIf}',
  ]);
  // Rien après le message : ni effacement, ni autre lancement.
  assert.deepEqual(lines.slice(message + 1), ['${EndIf}', '${EndIf}']);
});

test("le registre : le ménage du bloc « Delete app data » du modèle est refait par oublier.nsh, quand la case était cochée", () => {
  assert.deepEqual(macroLines('oublier.nsh', 'ShinkuOublierPostUninstall'), [
    '${If} $ShinkuOublier = 1',
    '${AndIf} $UpdateMode <> 1',
    'DeleteRegKey SHCTX "${MANUPRODUCTKEY}"',
    'DeleteRegKey /ifempty SHCTX "${MANUKEY}"',
    'DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"',
    'DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"',
    'DeleteRegKey /ifempty HKCU "${MANUKEY}"',
    '${EndIf}',
  ]);
});

// ── Le modèle NSIS embarqué par la version de Tauri installée ────────────────
//
// Le modèle n'est pas dans le dépôt : le CLI de Tauri l'embarque dans son binaire natif
// (@tauri-apps/cli-<plateforme>/*.node). Les essais ci-dessous le lisent, pour que ce qui tient
// notre désinstalleur (le bloc des deux RmDir /r, gardé par $DeleteAppDataCheckboxState, placé
// après NSIS_HOOK_PREUNINSTALL) casse ICI si une version de Tauri le change, au lieu de laisser
// revenir l'effacement élevé. Si le binaire manque (npm ci pas fait), l'essai ÉCHOUE, il ne passe
// pas en silence : sans lui rien ne garantit plus que l'ancien bloc est neutralisé.

interface TauriTemplate {
  /** Le fichier lu, pour les messages d'échec. */
  from: string;
  /** Le modèle installer.nsi, tel qu'embarqué. */
  installer: string;
  /** La macro CheckIfAppIsRunning (utils.nsh, embarqué à côté). */
  appIsRunning: string;
}

let embedded: TauriTemplate | undefined;

function tauriTemplate(): TauriTemplate {
  if (embedded !== undefined) {
    return embedded;
  }
  const scope = new URL('node_modules/@tauri-apps/', root);
  assert.ok(existsSync(scope), `@tauri-apps/ est absent de node_modules (${scope.pathname}) : lance « npm ci » pour pouvoir lire le modèle NSIS de Tauri`);
  const prefix = `cli-${process.platform}-${process.arch}`;
  const packages = readdirSync(scope).filter((name) => name.startsWith(prefix));
  const binaries = packages.flatMap((name) =>
    readdirSync(new URL(`${name}/`, scope))
      .filter((file) => file.endsWith('.node'))
      .map((file) => new URL(`${name}/${file}`, scope)),
  );
  const binary = binaries[0];
  assert.ok(
    binary !== undefined,
    `aucun binaire @tauri-apps/${prefix}*/*.node dans node_modules : lance « npm ci » sur cette plateforme (${process.platform}-${process.arch}) pour pouvoir lire le modèle NSIS de Tauri`,
  );
  // Les textes du modèle sont de l'ASCII ou de l'UTF-8 : latin1 garde chaque octet à sa place.
  const text = readFileSync(binary).toString('latin1').replaceAll('\r\n', '\n');
  const cut = (start: string, end: string): string => {
    const from = text.indexOf(start);
    assert.notEqual(from, -1, `le modèle NSIS de Tauri (${binary.pathname}) ne contient plus « ${start.trim()} »`);
    const to = text.indexOf(end, from);
    assert.notEqual(to, -1, `le modèle NSIS de Tauri (${binary.pathname}) : « ${end.trim()} » introuvable après « ${start.trim()} »`);
    return text.slice(from, to);
  };
  embedded = {
    from: binary.pathname,
    installer: cut('Unicode true\nManifestDPIAware true', 'Failed to setup handlebar template'),
    appIsRunning: cut('!macro CheckIfAppIsRunning executablePath productName', '\n!macroend'),
  };
  return embedded;
}

/** La section Uninstall du modèle, de son premier mot à son SectionEnd. */
function uninstallSection(template: TauriTemplate): string {
  const from = template.installer.indexOf('Section Uninstall\n');
  assert.notEqual(from, -1, `${template.from} : plus de « Section Uninstall »`);
  const to = template.installer.indexOf('\nSectionEnd', from);
  assert.notEqual(to, -1, `${template.from} : « Section Uninstall » ne finit plus par SectionEnd`);
  return template.installer.slice(from, to);
}

test("le modèle de Tauri : ses deux RmDir /r sont gardés par $DeleteAppDataCheckboxState, après NSIS_HOOK_PREUNINSTALL", () => {
  const template = tauriTemplate();
  const section = uninstallSection(template);
  const appdata = 'RmDir /r "$APPDATA\\${BUNDLEID}"';
  const local = 'RmDir /r "$LOCALAPPDATA\\${BUNDLEID}"';
  assert.equal(section.split(appdata).length - 1, 1, `${template.from} : ${appdata} attendu une fois`);
  assert.equal(section.split(local).length - 1, 1, `${template.from} : ${local} attendu une fois`);
  assert.equal(section.match(/\bRmDir\s+\/r\b/gi)?.length, 2, `${template.from} : un autre RmDir /r est apparu dans la section Uninstall`);
  // Le bloc : la case cochée, hors mise à jour, rien d'autre entre sa condition et ses deux RmDir /r.
  const guarded = new RegExp(
    '\\$\\{If\\} \\$DeleteAppDataCheckboxState = 1\\n\\s*\\$\\{AndIf\\} \\$UpdateMode <> 1\\n(?:(?!\\$\\{If\\})(?!\\$\\{EndIf\\})[\\s\\S])*?' +
      appdata.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') +
      '\\n\\s*' +
      local.replace(/[.*+?^${}()|[\]\\]/g, '\\$&') +
      '\\n\\s*\\$\\{EndIf\\}',
  );
  assert.match(section, guarded, `${template.from} : les deux RmDir /r ne sont plus dans le bloc gardé par $DeleteAppDataCheckboxState`);
  // Placé après le crochet de pré-désinstallation, et avant celui de post-désinstallation.
  const pre = section.indexOf('!insertmacro NSIS_HOOK_PREUNINSTALL');
  const post = section.indexOf('!insertmacro NSIS_HOOK_POSTUNINSTALL');
  assert.ok(pre !== -1 && post !== -1, `${template.from} : un crochet de désinstallation a disparu de la section`);
  assert.ok(pre < section.indexOf(appdata) && section.indexOf(local) < post, `${template.from} : l'ordre crochet, RmDir /r, crochet a changé`);
});

test("le modèle de Tauri : RunAsUser existe avec deux arguments (le programme et sa ligne de commande)", () => {
  const template = tauriTemplate();
  const calls = template.installer.match(/^\s*nsis_tauri_utils::RunAsUser\s+(?:"[^"]*"\s+){1}"[^"]*"\s*$/gm) ?? [];
  assert.ok(calls.length >= 2, `${template.from} : RunAsUser avec deux arguments attendu (« Lancer Shinku » et /R), vu ${calls.length}`);
  const all = template.installer.match(/^\s*nsis_tauri_utils::RunAsUser\b.*$/gm) ?? [];
  assert.equal(all.length, calls.length, `${template.from} : un appel de RunAsUser n'a pas deux arguments : ${all.join(' | ')}`);
  // Le modèle lui-même le lance avec la ligne de commande de /ARGS : les arguments passent.
  assert.match(template.installer, /GetOptions\} \$CMDLINE "\/ARGS" \$R0\n\s*nsis_tauri_utils::RunAsUser "\$INSTDIR\\\$\{MAINBINARYNAME\}\.exe" "\$R0"/);
});

test("le modèle de Tauri : le ménage de registre du bloc est exactement celui que refait notre crochet", () => {
  const template = tauriTemplate();
  const section = uninstallSection(template);
  const block = /\$\{If\} \$DeleteAppDataCheckboxState = 1\n\s*\$\{AndIf\} \$UpdateMode <> 1\n([\s\S]*?)\n\s*\$\{EndIf\}/.exec(section)?.[1];
  assert.ok(block !== undefined, `${template.from} : bloc « Delete app data » introuvable`);
  // Tout le bloc, sauf SetShellVarContext et les deux RmDir /r, que --oublier remplace.
  const registry = linesOf(block).filter((line) => !/^(?:SetShellVarContext|RmDir)\b/.test(line));
  assert.deepEqual(registry.filter((line) => /^Delete/.test(line)), registry, `${template.from} : le bloc fait autre chose que du registre`);
  const ours = macroLines('oublier.nsh', 'ShinkuOublierPostUninstall').filter((line) => /^Delete/.test(line));
  assert.deepEqual(ours, registry);
});

test("le modèle de Tauri : la case vaut 0 en mode silencieux et passif (page sautée), donc --oublier n'est jamais appelé", () => {
  const template = tauriTemplate().installer;
  // La seule écriture de la variable : en quittant la page de la case.
  assert.equal(template.match(/StrCpy \$DeleteAppDataCheckboxState\b/g)?.length ?? 0, 0, "le modèle écrit lui-même la case");
  assert.match(template, /Function un\.ConfirmLeave\n\s*SendMessage \$DeleteAppDataCheckbox \$\{BM_GETCHECK\} 0 0 \$DeleteAppDataCheckboxState\n\s*FunctionEnd/);
  // La page est sautée en mode passif (Abort avant son affichage) ; en silencieux, aucune page n'est montrée.
  assert.match(template, /Function un\.SkipIfPassive\n\s*\$\{IfThen\} \$PassiveMode = 1\s+\$\{\|\} Abort \$\{\|\}\n\s*FunctionEnd/);
  assert.match(template, /!define MUI_PAGE_CUSTOMFUNCTION_PRE un\.SkipIfPassive\n!insertmacro MUI_UNPAGE_CONFIRM/);
});

test("le modèle de Tauri : CheckIfAppIsRunning se réinsère à un autre endroit (étiquettes et identifiant venus de __LINE__) et est rejoué par le modèle", () => {
  const template = tauriTemplate();
  const macroText = template.appIsRunning;
  assert.match(macroText, /^\s*!define UniqueID \$\{__LINE__\}$/m, `${template.from} : UniqueID ne vient plus de __LINE__`);
  assert.match(macroText, /^\s*!undef UniqueID$/m);
  // Chaque étiquette de la macro porte l'identifiant : insérée à deux lignes différentes, aucune n'est en double.
  const labels = Array.from(macroText.matchAll(/^\s*([a-z_]+)(?:_\$\{UniqueID\}):/gm), (match) => match[1]);
  assert.ok(labels.length >= 4, `${template.from} : étiquettes de CheckIfAppIsRunning : ${labels.join(', ')}`);
  assert.equal(macroText.match(/^\s*[a-z_]+:\s*$/gm)?.length ?? 0, 0, 'une étiquette sans identifiant');
  // Le modèle la rejoue après notre crochet.
  const section = uninstallSection(template);
  assert.ok(
    section.indexOf('!insertmacro NSIS_HOOK_PREUNINSTALL') < section.indexOf('!insertmacro CheckIfAppIsRunning "$INSTDIR\\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"'),
    `${template.from} : le modèle ne rejoue plus CheckIfAppIsRunning après le crochet`,
  );
});

// ── L'essai sur le vrai Windows ────────────────────────────────────────────────

const script = readFileSync(new URL('scripts/essai-installeur.ps1', root));
const ci = readFileSync(new URL('../../.github/workflows/ci.yml', root), 'utf8');

test("essai-installeur.ps1 : une seule marque BOM, aucun retour chariot (Windows PowerShell 5.1 lit sinon les accents de travers)", () => {
  assert.deepEqual([...script.subarray(0, 3)], [0xef, 0xbb, 0xbf]);
  assert.deepEqual([...script.subarray(3, 6)].map((byte) => byte === 0xef), [false, false, false], 'deux BOM de suite');
  assert.equal(script.includes(0x0d), false, 'retour chariot dans le script');
});

test("l'intégration continue joue les trois essais de --oublier sur le vrai désinstalleur, et l'étape silencieuse ne supprime jamais les réglages", () => {
  assert.match(ci, /foreach \(\$etape in 'oublier', 'desinstaller-jonction', 'desinstaller'\)/);
  const steps = /ValidateSet\(([^)]*)\)\]\[string\]\$Etape/.exec(script.toString('utf8'))?.[1] ?? '';
  for (const etape of ['oublier', 'desinstaller-jonction', 'desinstaller']) {
    assert.ok(steps.includes(`'${etape}'`), `essai-installeur.ps1 ne connaît pas l'étape ${etape}`);
  }
  // La désinstallation silencieuse (/S) : la case vaut 0, --oublier n'est jamais appelé, les réglages restent.
  assert.match(ci, /# Une désinstallation silencieuse ne doit jamais supprimer les réglages\./);
  assert.match(ci, /throw "La désinstallation silencieuse a supprimé les réglages\."/);
  assert.match(ci, /Start-Process -FilePath "\$dossier\\uninstall\.exe" -ArgumentList '\/S' -Wait/);
});

test("essai-installeur.ps1 : chaque preuve est une ligne « FAIT : » qui dit ce qui a été mesuré, et la boîte d'échec de --oublier fait échouer l'essai", () => {
  const text = script.toString('utf8');
  assert.equal(text.match(/^\s*Fait \(?"/gm)?.length, 4, 'une ligne FAIT par preuve : à la main (droits ordinaires), refus élevé, jonction, fenêtre ouverte');
  assert.match(text, /function Fait\(\[string\]\$ligne\) \{ Write-Host "FAIT : \$ligne" \}/);
  assert.match(text, /Shinku n'a pas pu supprimer tes réglages/);
  // La boîte est fermée (l'essai ne reste pas bloqué), puis l'essai échoue.
  assert.match(text, /if \(\$script:Echecs\.Count -gt 0\) \{\s*Echouer/);
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

test('francais.nsh définit exactement les chaînes du français de Tauri, plus celle du désinstalleur', () => {
  const defined = Array.from(read('francais.nsh').matchAll(/^LangString (\w+) \$\{LANG_FRENCH\} "/gm), (match) => match[1] ?? '');
  assert.equal(new Set(defined).size, defined.length, 'une chaîne est définie deux fois');
  // shinkuOublierEchec n'est pas une chaîne de Tauri : c'est le message d'oublier.nsh.
  assert.deepEqual([...defined].sort(), [...TAURI_STRINGS, 'shinkuOublierEchec'].sort());
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
  assert.deepEqual(files.sort(), ['agent.nsh', 'francais.nsh', 'habillage.nsh', 'helper.nsh', 'oublier.nsh', 'textes.nsh']);
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
