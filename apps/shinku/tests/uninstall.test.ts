// Ce que la page Désinstallation tire d'une vue du cœur : la liste filtrée et
// triée, les textes d'une ligne, les phases, les restes cochés, le focus, et ce
// que les gabarits écrivent.

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

import type { AppRow, AppsView, FollowView, LeftoverRow } from '../src/backend/types.ts';
import { frenchSpacing as t, NBSP } from '../src/format.ts';
import { fr } from '../src/i18n/fr.ts';
import {
  announcement,
  appById,
  askFor,
  BIN_NOTE_FROM,
  checkedRows,
  checkable,
  chipOf,
  confirmExtras,
  detailParts,
  dockTexts,
  failureText,
  filterApps,
  focusAfterRemoval,
  focusTarget,
  fold,
  initialOf,
  installedTime,
  isPolling,
  LATE_AFTER,
  leftoverSize,
  leftoverText,
  listedApps,
  mergedFailures,
  noneBecause,
  proposed,
  refusalOf,
  removalOf,
  removalToast,
  rowEnd,
  sceneTexts,
  screenOf,
  sizeOf,
  sortApps,
  tallyOf,
  uninstallFocus,
} from '../src/model/uninstall.ts';
import { DOM, EXPECTED } from '../scripts/essai-desinstallation-dom.mjs';
import { type Node, parse, query, queryAll, textOfNode } from './html-tree.ts';
import { fakeDocument, htmlOf, markupOf, textOf } from './template.ts';

fakeDocument();
const { uninstallDialog, uninstallView } = await import('../src/views/uninstall.ts');
const { initialState } = await import('../src/state.ts');

const NOW = new Date(2026, 10, 15, 12).getTime();

function app(parts: Partial<AppRow> & { name: string }): AppRow {
  return { id: `ac:${parts.name}`, scope: 'account', kind: 'plain', removal: 'offered', ...parts };
}

const DISCORD = app({ name: 'Discord', publisher: 'Discord Inc.', version: '1.0.9163', bytes: 388 * 1024 ** 2, installed: '2024-11-02' });
const GIT = app({ id: 'm6:{git}', name: 'Git', scope: 'machine', publisher: 'The Git Development Community', version: '2.42.0', bytes: 360 * 1024 ** 2, installed: '2024-01-20' });
const VLC = app({ id: 'm6:{vlc}', name: 'VLC media player', scope: 'machine', bytes: 2.1 * 1024 ** 3 });
const SPOTIFY = app({ name: 'Spotify', publisher: 'Spotify AB', installed: '2025-01-11' });
const OLD = app({ name: 'Ancien outil' });
const VCPP = app({ id: 'm6:{vc}', name: 'Microsoft Visual C++ 2015-2022 Redistributable (x64)', scope: 'machine', kind: 'runtime', bytes: 20 * 1024 ** 2 });
const REALTEK = app({ id: 'm6:{rt}', name: 'Realtek Audio Driver', scope: 'machine', kind: 'driver' });
const EDGE = app({ id: 'm6:{edge}', name: 'Microsoft Edge', scope: 'machine', removal: 'refused' });
const SHINKU = app({ id: 'm6:{shinku}', name: 'Shinku', scope: 'machine', removal: 'own' });
const WEBVIEW = app({ id: 'm6:{wv}', name: 'Microsoft Edge WebView2 Runtime', scope: 'machine', removal: 'needed' });

const ALL = [DISCORD, GIT, OLD, SHINKU, SPOTIFY, VCPP, VLC, WEBVIEW, EDGE, REALTEK];

const IDLE: FollowView = { phase: 'idle' };

function view(apps: AppRow[] = ALL, parts: Partial<AppsView> = {}): AppsView {
  return { apps, complete: true, follow: IDLE, ...parts };
}

const FOLDER: LeftoverRow = { key: 0, kind: 'folder', path: 'C:\\Users\\Léa\\AppData\\Local\\Discord', bytes: 1.1 * 1024 ** 3, files: 8912 };
const DATA: LeftoverRow = { key: 1, kind: 'data', path: 'C:\\Users\\Léa\\AppData\\Roaming\\discord', bytes: 240 * 1024 ** 2, files: 1204 };
const LINK: LeftoverRow = { key: 2, kind: 'shortcut', path: 'C:\\Users\\Léa\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Discord.lnk', bytes: 1229, files: 1 };

function removed(parts: Partial<Extract<FollowView, { phase: 'removed' }>> = {}): FollowView {
  return { phase: 'removed', id: 'ac:Discord', name: 'Discord', leftovers: [FOLDER, DATA, LINK], family: false, complete: true, ...parts };
}

const noActions = new Proxy({}, { get: () => () => {} }) as never;

function page(parts: Record<string, unknown> = {}): string {
  return htmlOf(uninstallView({ ...initialState(NOW), route: 'uninstall', apps: view(), ...parts } as never, noActions));
}

// ── La liste ───────────────────────────────────────────────────────────────

test('le texte cherché se plie : sans casse ni accents, sur le nom et sur l’éditeur', () => {
  assert.equal(fold('Éditeur Œuvre'), 'editeur œuvre');
  assert.deepEqual(filterApps(ALL, 'git').map((one) => one.name), ['Git']);
  assert.deepEqual(filterApps(ALL, 'DISCORD INC').map((one) => one.name), ['Discord'], 'l’éditeur compte');
  assert.deepEqual(filterApps(ALL, 'spotify ab').map((one) => one.name), ['Spotify']);
  assert.deepEqual(filterApps([app({ name: 'Éditeur' })], 'editeur').length, 1);
  assert.equal(filterApps(ALL, '   ').length, ALL.length, 'un texte vide les garde toutes');
  assert.equal(filterApps(ALL, 'zzz').length, 0);
});

test('le tri par taille met les plus grosses d’abord et les tailles inconnues à la fin', () => {
  const sorted = sortApps(ALL, 'size').map((one) => one.name);
  assert.deepEqual(sorted.slice(0, 4), ['VLC media player', 'Discord', 'Git', 'Microsoft Visual C++ 2015-2022 Redistributable (x64)']);
  // Les tailles inconnues gardent l'ordre du cœur.
  assert.deepEqual(sorted.slice(4), ['Ancien outil', 'Shinku', 'Spotify', 'Microsoft Edge WebView2 Runtime', 'Microsoft Edge', 'Realtek Audio Driver']);
});

test('le tri par date met les plus récentes d’abord et les dates inconnues à la fin', () => {
  const sorted = sortApps(ALL, 'date').map((one) => one.name);
  assert.deepEqual(sorted.slice(0, 3), ['Spotify', 'Discord', 'Git']);
  assert.deepEqual(sorted.slice(3), ['Ancien outil', 'Shinku', 'Microsoft Visual C++ 2015-2022 Redistributable (x64)', 'VLC media player', 'Microsoft Edge WebView2 Runtime', 'Microsoft Edge', 'Realtek Audio Driver']);
});

test('le tri par nom ne change pas l’ordre du cœur, et aucun tri ne touche à la liste d’origine', () => {
  const before = ALL.map((one) => one.id);
  assert.deepEqual(sortApps(ALL, 'name').map((one) => one.id), before);
  sortApps(ALL, 'size');
  sortApps(ALL, 'date');
  assert.deepEqual(ALL.map((one) => one.id), before);
});

test('la liste montrée est filtrée, puis triée', () => {
  assert.deepEqual(listedApps(view(), 'm', 'size').map((one) => one.name).slice(0, 2), ['VLC media player', 'Git']);
  assert.deepEqual(listedApps(view(), 'discord', 'date').map((one) => one.name), ['Discord']);
});

test('l’initiale est le premier caractère alphanumérique en majuscule ; « # » s’il n’y en a aucun', () => {
  assert.equal(initialOf('discord'), 'D');
  assert.equal(initialOf('  énergie'), 'É');
  assert.equal(initialOf('7-Zip'), '7');
  assert.equal(initialOf('.NET'), 'N');
  assert.equal(initialOf('Notepad++'), 'N');
  assert.equal(initialOf('+-'), '#');
  assert.equal(initialOf(''), '#');
});

test('la seconde ligne dit l’éditeur et la version ; « Éditeur inconnu » s’il manque', () => {
  assert.deepEqual(detailParts(DISCORD, 'name', NOW), ['Discord Inc.', 'version 1.0.9163']);
  assert.deepEqual(detailParts(OLD, 'name', NOW), ['Éditeur inconnu']);
  assert.deepEqual(detailParts(app({ name: 'X', version: '2' }), 'name', NOW), ['Éditeur inconnu', 'version 2']);
  assert.deepEqual(detailParts(SPOTIFY, 'name', NOW), ['Spotify AB']);
});

test('triée par date, la ligne dit quand l’app a été installée ; une date invalide ne dit rien', () => {
  assert.deepEqual(detailParts(DISCORD, 'date', NOW), ['Discord Inc.', 'version 1.0.9163', `installée le 2${NBSP}nov. 2024`]);
  assert.deepEqual(detailParts(SPOTIFY, 'date', NOW), ['Spotify AB', `installée le 11${NBSP}janv. 2025`]);
  assert.deepEqual(detailParts(OLD, 'date', NOW), ['Éditeur inconnu']);
  assert.deepEqual(detailParts(app({ name: 'Z', installed: '2024-13-40' }), 'date', NOW), ['Éditeur inconnu']);
  assert.equal(installedTime('2024-02-30'), null);
  assert.equal(installedTime(undefined), null);
  assert.equal(installedTime('2024-02-29'), new Date(2024, 1, 29).getTime());
});

test('la pastille est une ressemblance ; la taille se tait quand elle est inconnue', () => {
  assert.equal(chipOf(VCPP), 'Souvent utile à d’autres apps');
  assert.equal(chipOf(REALTEK), 'Ressemble à un pilote');
  assert.equal(chipOf(DISCORD), null);
  assert.equal(chipOf({ ...VCPP, removal: 'needed' }), null, 'pas de pastille sans bouton');
  assert.equal(chipOf({ ...REALTEK, removal: 'refused' }), null);
  assert.equal(sizeOf(VLC), `2,1${NBSP}Go`);
  assert.equal(sizeOf(OLD), null);
  assert.equal(sizeOf(app({ name: 'Zéro', bytes: 0 })), null, '0 compte comme inconnu');
});

test('la fin de la ligne : le bouton, ou la raison de son absence', () => {
  assert.equal(rowEnd(DISCORD), 'button');
  assert.equal(rowEnd(EDGE), 'refused');
  assert.equal(rowEnd(SHINKU), 'own');
  assert.equal(rowEnd(WEBVIEW), 'needed');
});

test('le titre compte toutes les apps, et s’accorde', () => {
  assert.equal(fr.uninstall.title(38), `38${NBSP}apps installées`);
  assert.equal(fr.uninstall.title(1), `1${NBSP}app installée`);
  assert.equal(fr.uninstall.title(0), 'Aucune app classique installée');
  assert.ok(page().includes(`10${NBSP}apps installées`));
  // Filtrée, la liste garde son titre : il compte ce que Windows connaît.
  assert.ok(page({ appsQuery: 'git' }).includes(`10${NBSP}apps installées`));
});

// ── Les phases ─────────────────────────────────────────────────────────────

test('trois écrans : la liste, l’attente, les restes ; l’interrogation ne vaut que pour l’attente', () => {
  assert.equal(screenOf(null), 'list');
  assert.equal(screenOf(undefined), 'list');
  assert.equal(screenOf(IDLE), 'list');
  assert.equal(screenOf({ phase: 'launching', id: 'a', name: 'A' }), 'scene');
  assert.equal(screenOf({ phase: 'waiting', id: 'a', name: 'A', seconds: 0 }), 'scene');
  assert.equal(screenOf({ phase: 'settling', id: 'a', name: 'A' }), 'scene');
  assert.equal(screenOf(removed()), 'leftovers');
  assert.equal(isPolling(IDLE), false);
  assert.equal(isPolling(null), false);
  assert.equal(isPolling(removed()), false);
  assert.equal(isPolling({ phase: 'launching', id: 'a', name: 'A' }), true);
  assert.equal(isPolling({ phase: 'waiting', id: 'a', name: 'A', seconds: 4 }), true);
  assert.equal(isPolling({ phase: 'settling', id: 'a', name: 'A' }), true);
});

test('l’attente dit sa phrase, et la seconde après vingt secondes', () => {
  assert.deepEqual(sceneTexts({ phase: 'launching', id: 'a', name: 'A' }), ['Windows te demande peut-être ton accord.']);
  assert.deepEqual(sceneTexts({ phase: 'waiting', id: 'a', name: 'A', seconds: LATE_AFTER - 1 }), [fr.uninstall.scene.waiting]);
  assert.deepEqual(sceneTexts({ phase: 'waiting', id: 'a', name: 'A', seconds: LATE_AFTER }), [fr.uninstall.scene.waiting, fr.uninstall.scene.waitingLate]);
  assert.deepEqual(sceneTexts({ phase: 'settling', id: 'a', name: 'A' }), ['Presque fini…']);
  assert.deepEqual(sceneTexts(IDLE), []);
});

test('chaque phase a sa phrase vivante', () => {
  assert.match(announcement({ phase: 'launching', id: 'a', name: 'Discord' }), /Discord/);
  assert.match(announcement({ phase: 'waiting', id: 'a', name: 'Discord', seconds: 1 }), /attend/);
  assert.match(announcement({ phase: 'settling', id: 'a', name: 'Discord' }), /Presque fini/);
  assert.match(announcement(removed()), /Discord est désinstallée\. 3.restes à regarder/u);
  assert.match(announcement(removed({ family: true })), /n’a rien à te proposer/, t('une app de la même famille : rien n’est proposé'));
  assert.equal(announcement(IDLE), fr.uninstall.announce.list);
});

test('les restes ne sont proposés que si rien ne les rend douteux', () => {
  assert.equal(proposed(removed()).length, 3);
  assert.equal(proposed(removed({ family: true })).length, 0, t('une app de la même famille reste : rien'));
  assert.equal(proposed(removed({ complete: false })).length, 0, t('vérification incomplète : rien'));
  assert.equal(proposed(IDLE).length, 0);
  assert.equal(noneBecause(removed()), null);
  assert.equal(noneBecause(removed({ leftovers: [] })), 'none');
  assert.equal(noneBecause(removed({ family: true })), 'family');
  assert.equal(noneBecause(removed({ complete: false })), 'unsure');
  assert.equal(noneBecause(removed({ family: true, complete: false })), 'family');
  assert.equal(noneBecause(IDLE), null);
});

// ── Les restes ─────────────────────────────────────────────────────────────

test('chaque reste dit ce qu’il est, et sa taille avec ses fichiers', () => {
  assert.equal(leftoverText(FOLDER), 'Dossier de l’app');
  assert.match(leftoverText(DATA), /^Réglages et données de l’app\. Peut contenir ce que tu as fait avec elle/);
  assert.equal(leftoverText(LINK), 'Raccourci qui ne mène plus nulle part');
  assert.equal(leftoverSize(DATA), `240${NBSP}Mo · 1${String.fromCharCode(0x202f)}204${NBSP}fichiers`);
  assert.equal(leftoverSize({ key: 9, kind: 'folder', path: 'C:\\x' }), null);
  assert.equal(leftoverSize({ key: 9, kind: 'folder', path: 'C:\\x', bytes: 2048 }), `2,0${NBSP}Ko`);
  assert.equal(leftoverSize({ key: 9, kind: 'folder', path: 'C:\\x', files: 1 }), `1${NBSP}fichier`);
});

test('la légende compte ce qui est coché, et prévient au-delà d’un Go', () => {
  const none = tallyOf([]);
  assert.deepEqual(dockTexts(none), ['Rien n’est coché']);
  const two = tallyOf([DATA, LINK]);
  assert.equal(two.count, 2);
  assert.equal(two.atLeast, false);
  assert.deepEqual(dockTexts(two), [`2${NBSP}éléments, 240${NBSP}Mo.`]);
  assert.ok(DATA.bytes! + LINK.bytes! < BIN_NOTE_FROM);
  const big = tallyOf([FOLDER, DATA]);
  assert.deepEqual(dockTexts(big), [`2${NBSP}éléments, 1,3${NBSP}Go.`, 'Ta corbeille peut abandonner d’anciens éléments pour faire de la place.']);
  // Une taille inconnue : le total est un minimum, jamais une valeur inventée.
  const unknown = tallyOf([LINK, { key: 3, kind: 'data', path: 'C:\\x' }]);
  assert.equal(unknown.atLeast, true);
  assert.match(dockTexts(unknown)[0] ?? '', /^2.éléments, au moins 1,2.Ko\.$/u);
  assert.equal(fr.uninstall.leftovers.caption(1, 0, true), `1${NBSP}élément.`);
});

test('les cases cochées sont retrouvées par chemin, dans l’ordre de la liste', () => {
  assert.deepEqual(checkedRows([FOLDER, DATA, LINK], [LINK.path, FOLDER.path]).map((row) => row.key), [0, 2]);
  assert.deepEqual(checkedRows([FOLDER], ['C:\\ailleurs']), []);
});

test('un reste qui n’a pas pu partir garde sa raison ; une raison inconnue se dit « n’a pas pu partir »', () => {
  assert.equal(failureText('inUse'), t('Encore utilisé : ferme ce qui l’utilise, puis réessaie.'));
  assert.equal(failureText('refused'), 'La corbeille ne peut pas le recevoir.');
  assert.equal(failureText('partial'), 'Une partie est restée.');
  assert.equal(failureText('other'), 'N’a pas pu partir.');
});

test('la réponse du cœur : combien sont partis, ce qui reste avec sa raison, et la phrase du toast', () => {
  const answer = {
    view: view(ALL, { follow: removed({ leftovers: [{ ...LINK, key: 0 }] }) }),
    results: [
      { key: 0, outcome: 'removed' as const },
      { key: 1, outcome: 'failed' as const, reason: 'inUse' as const },
      { key: 2, outcome: 'failed' as const },
    ],
    bytes: 1.1 * 1024 ** 3,
  };
  const removal = removalOf([FOLDER, DATA, LINK], answer);
  assert.equal(removal.moved, 1);
  assert.deepEqual(removal.failures, { [DATA.path]: 'inUse', [LINK.path]: 'other' });
  assert.deepEqual(removal.settled, [FOLDER.path, DATA.path, LINK.path]);
  assert.equal(removalToast(removal), `1${NBSP}élément mis à la corbeille, 1,1${NBSP}Go.`);
  assert.equal(fr.uninstall.toast.removed(2, 240 * 1024 ** 2), `2${NBSP}éléments mis à la corbeille, 240${NBSP}Mo.`);

  // « refused » et « alreadyGone » : le premier est un échec sans raison, le second n'est ni un échec ni une réussite.
  const other = removalOf([FOLDER, DATA], {
    view: view(),
    results: [
      { key: 0, outcome: 'refused' },
      { key: 1, outcome: 'alreadyGone' },
    ],
    bytes: 0,
  });
  assert.deepEqual(other.failures, { [FOLDER.path]: 'other' });
  assert.equal(other.moved, 0);
  assert.equal(removalToast(other), 'Rien n’a pu être mis à la corbeille.');
  assert.equal(removalToast({ moved: 0, bytes: 0, failures: {}, destroyed: [], settled: [] }), null);
  // Une clé que la page ne connaît pas est ignorée.
  assert.equal(removalOf([FOLDER], { view: view(), results: [{ key: 7, outcome: 'removed' }], bytes: 0 }).moved, 0);
});

test('« destroyed » : Windows a supprimé pour de bon, la ligne reste sans case, ne compte pas, et ne se renvoie pas', () => {
  assert.equal(fr.uninstall.leftovers.destroyed, t('Windows l’a supprimé pour de bon : il n’est pas dans la Corbeille.'));
  assert.equal(failureText('destroyed'), fr.uninstall.leftovers.destroyed);
  const removal = removalOf([FOLDER, DATA], {
    view: view(),
    results: [
      { key: 0, outcome: 'failed', reason: 'destroyed' },
      { key: 1, outcome: 'removed' },
    ],
    bytes: 240 * 1024 ** 2,
  });
  assert.equal(removal.moved, 1, 'un élément détruit ne compte pas comme « envoyé à la corbeille »');
  assert.deepEqual(removal.destroyed.map((row) => row.path), [FOLDER.path]);
  assert.equal(removal.failures[FOLDER.path], 'destroyed');
  assert.deepEqual(checkable([FOLDER, DATA], removal.failures), [DATA]);
  // Seul « destroyed » : rien n'est parti à la corbeille.
  const only = removalOf([FOLDER], { view: view(), results: [{ key: 0, outcome: 'failed', reason: 'destroyed' }], bytes: 0 });
  assert.equal(removalToast(only), 'Rien n’a pu être mis à la corbeille.');

  // La page : pas de case pour la ligne détruite, sa raison dite, et l'orbe devient « Terminé ».
  const gone = page({
    apps: view(ALL, { follow: removed({ leftovers: [] }) }),
    appsDestroyed: [FOLDER],
    appsFailures: { [FOLDER.path]: 'destroyed' },
    appsEmptied: true,
  });
  assert.ok(gone.includes(fr.uninstall.leftovers.destroyed));
  assert.ok(gone.includes(`title=${FOLDER.path}`));
  assert.ok(!gone.includes('type="checkbox"'), 'plus de case');
  assert.ok(gone.includes('Terminé') && !gone.includes('Mettre à la corbeille'));
  assert.ok(gone.includes('Il ne reste plus rien à te proposer.'));

  // Comme le vrai cœur : la ligne détruite est encore dans la vue. Pas de case, et le pied est « Terminé », pas l'orbe de la corbeille.
  const kept = page({
    apps: view(ALL, { follow: removed({ leftovers: [FOLDER] }) }),
    appsFailures: { [FOLDER.path]: 'destroyed' },
  });
  assert.ok(kept.includes(fr.uninstall.leftovers.destroyed));
  assert.ok(!kept.includes('type="checkbox"'), 'plus de case');
  assert.ok(kept.includes('Terminé') && !kept.includes('Mettre à la corbeille'), 'le pied est « Terminé »');
  assert.ok(!/class="orb uorb"[^>]*uninstall-orb/.test(kept));
});

test('les échecs qui restent : les nouveaux, et ceux d’avant pour les restes qui n’ont pas été retentés', () => {
  const removal = { moved: 0, bytes: 0, failures: { [DATA.path]: 'partial' as const }, destroyed: [], settled: [DATA.path, FOLDER.path] };
  const merged = mergedFailures({ [DATA.path]: 'inUse', [FOLDER.path]: 'refused', [LINK.path]: 'other', 'C:\\parti': 'other' }, removal, [DATA, LINK]);
  // DATA : le nouvel échec remplace lt('ancien ; FOLDER est parti ; LINK n')a pas été retenté ; « parti » n'est plus dans la liste.
  assert.deepEqual(merged, { [DATA.path]: 'partial', [LINK.path]: 'other' });
  // Retenté et réussi : plus d'échec.
  assert.deepEqual(mergedFailures({ [DATA.path]: 'inUse' }, { moved: 1, bytes: 1, failures: {}, destroyed: [], settled: [DATA.path] }, [DATA]), {});
});

test('les refus de « Désinstaller » : rien pour « cancelled », une phrase pour les autres, et la liste est relue', () => {
  const refused = (reason: string) => ({ kind: 'uninstallRefused', reason });
  assert.deepEqual(refusalOf(refused('cancelled'), 'Discord'), { toast: null, reload: true });
  assert.deepEqual(refusalOf(refused('broken'), 'Discord'), { toast: 'Le désinstalleur de Discord est introuvable. Essaie depuis Windows.', reload: true });
  assert.deepEqual(refusalOf(refused('vanished'), 'Discord'), { toast: 'Discord n’est plus installée.', reload: true });
  for (const reason of ['busy', 'failed', 'unreadable', 'notOffered']) {
    assert.deepEqual(refusalOf(refused(reason), 'Discord'), { toast: fr.uninstall.toast.failed, reload: true }, reason);
  }
  assert.deepEqual(refusalOf(new Error('ipc'), 'Discord'), { toast: fr.uninstall.toast.failed, reload: true });
  assert.deepEqual(refusalOf(null, 'Discord'), { toast: fr.uninstall.toast.failed, reload: true });
});

// ── La confirmation ────────────────────────────────────────────────────────

test('la confirmation ajoute ce que la ressemblance du nom et la portée de l’app doivent faire savoir', () => {
  assert.deepEqual(confirmExtras(askFor(DISCORD)), []);
  assert.deepEqual(confirmExtras(askFor(VCPP)), ['D’autres apps peuvent en avoir besoin pour fonctionner.', 'Windows va te demander ton accord.']);
  assert.deepEqual(confirmExtras(askFor(REALTEK)), [t('Ça ressemble à un pilote : un appareil de ton PC peut cesser de marcher.'), 'Windows va te demander ton accord.']);
  assert.deepEqual(confirmExtras(askFor(GIT)), ['Windows va te demander ton accord.']);
  assert.deepEqual(confirmExtras({ ...askFor(DISCORD), kind: 'runtime' }), ['D’autres apps peuvent en avoir besoin pour fonctionner.']);
  assert.equal(appById(view(), 'ac:Discord'), DISCORD);
  assert.equal(appById(view(), 'ac:inconnue'), undefined);
  assert.equal(appById(null, 'ac:Discord'), undefined);
});

test('la boîte de confirmation dit ce qui va se passer, et « Annuler » a le focus', () => {
  const written = textOf(uninstallDialog({ ...initialState(NOW), appsConfirm: askFor(GIT) } as never, noActions));
  assert.ok(written.includes(t('Désinstaller Git ?')));
  assert.ok(written.includes('Shinku ouvre le désinstalleur de Git. C’est lui qui retire l’app, et il peut te poser des questions. Ensuite, Shinku te montre ce qui reste.'));
  assert.ok(written.includes('Windows va te demander ton accord.'));
  assert.ok(written.endsWith('Annuler Désinstaller'));
  const html = htmlOf(uninstallDialog({ ...initialState(NOW), appsConfirm: askFor(GIT) } as never, noActions));
  assert.match(html, /<button type="button" class="ghost" autofocus/);
  assert.match(html, /aria-labelledby="uninstall-ask-title"/);
  assert.match(html, /aria-describedby="uninstall-ask-text"/);
  // Une app du compte, sans ressemblance : rien en plus.
  const plain = textOf(uninstallDialog({ ...initialState(NOW), appsConfirm: askFor(DISCORD) } as never, noActions));
  assert.ok(!plain.includes('Windows va te demander'));
  assert.equal(htmlOf(uninstallDialog(initialState(NOW), noActions)), '');
});

// ── Le focus ───────────────────────────────────────────────────────────────

test('le focus d’un bouton disparu va au suivant, sinon au précédent, sinon au titre', () => {
  const rows = [DISCORD, GIT, SPOTIFY];
  const key = (one: AppRow): string => uninstallFocus.row(one.id);
  assert.equal(focusTarget(rows, [DISCORD, SPOTIFY], key(GIT)), key(SPOTIFY));
  assert.equal(focusTarget(rows, [DISCORD, GIT], key(SPOTIFY)), key(GIT));
  assert.equal(focusTarget(rows, [], key(GIT)), uninstallFocus.title);
  assert.equal(focusTarget(rows, rows, key(GIT)), key(GIT), 'si la ligne est là, c’est elle');
  // Les lignes sans bouton n'en reçoivent pas.
  assert.equal(focusTarget([DISCORD, EDGE, GIT], [DISCORD, EDGE], key(DISCORD)), key(DISCORD));
  assert.equal(focusTarget([DISCORD, EDGE, GIT], [EDGE, GIT], key(DISCORD)), key(GIT));
  // Ce qui n'est pas un bouton de ligne ne bouge pas.
  assert.equal(focusTarget(rows, [], uninstallFocus.search), null);
  assert.equal(focusTarget(rows, [], 'uninstall:ac:inconnue'), null);
});

test('après la corbeille, le focus va au premier reste encore là, sinon à « Terminé »', () => {
  assert.equal(focusAfterRemoval([DATA, LINK]), uninstallFocus.leftover(1));
  assert.equal(focusAfterRemoval([]), uninstallFocus.done);
});

// ── Ce que les gabarits écrivent ───────────────────────────────────────────

test('la liste : une ligne par app, clée par l’identifiant, avec la fin qui lui convient', () => {
  const written = page();
  assert.equal((written.match(/<li class="row static">/g) ?? []).length, ALL.length);
  assert.ok(written.includes('aria-label=Désinstaller Discord'));
  assert.ok(written.includes('Windows ne permet pas de la retirer'));
  assert.ok(written.includes('C’est Shinku'));
  assert.ok(written.includes('Shinku en a besoin pour s’afficher'));
  assert.ok(written.includes('Souvent utile à d’autres apps'));
  assert.ok(written.includes('Ressemble à un pilote'));
  assert.ok(written.includes('Discord Inc. · version 1.0.9163'));
  assert.ok(written.includes('Éditeur inconnu'));
  // Seules les apps proposées ont un bouton.
  assert.equal((written.match(/aria-label=Désinstaller /g) ?? []).length, 7);
  assert.ok(!written.includes('aria-label=Désinstaller Microsoft Edge'));
  assert.ok(!written.includes('aria-label=Désinstaller Shinku'));
  const source = readFileSync(new URL('../src/views/uninstall.ts', import.meta.url), 'utf8');
  assert.match(source, /repeat\(\s*shown,\s*\(app\) => app\.id,/);
  assert.match(source, /repeat\(\s*rows,\s*\(row\) => row\.path,/);
});

test('l’en-tête : surtitre, titre qui peut recevoir le focus, phrase, mention du Store, bouton vers Windows', () => {
  const written = page();
  assert.ok(written.includes('Désinstallation'));
  assert.match(written, /<h1 tabindex="-1" data-fk=uninstall-title>/);
  assert.ok(written.includes('Shinku ouvre le désinstalleur de l’app, puis te montre ce qu’elle a laissé derrière elle.'));
  assert.ok(written.includes(t('Les apps du Microsoft Store ne sont pas ici : elles se retirent dans Windows.')));
  assert.ok(written.includes('Apps installées de Windows'));
  assert.ok(written.includes('Shinku ne supprime jamais une app lui-même et ne touche pas au registre'));
  assert.ok(written.includes('La taille est celle que l’app déclare.'));
});

test('la recherche est un champ « search » nommé ; le tri est un groupe de boutons radio', () => {
  const written = page({ appsSort: 'size' });
  assert.match(written, /type="search"/);
  assert.match(written, /aria-label=Chercher une app/);
  assert.match(written, /role="radiogroup"/);
  assert.equal((written.match(/role="radio"/g) ?? []).length, 3);
  assert.match(written, /aria-checked=true\s+tabindex=0[^>]*>\s*Taille/);
  // Un seul bouton du groupe est dans l'ordre de tabulation.
  assert.equal((written.match(/role="radio"[^>]*tabindex=0/g) ?? []).length, 1);
  for (const label of ['Nom', 'Taille', 'Date']) {
    assert.ok(written.includes(label), label);
  }
  // Pas de bouton « vider » sans texte ; un bouton nommé avec.
  assert.ok(!written.includes('Effacer la recherche'));
  assert.ok(page({ appsQuery: 'git' }).includes('aria-label=Effacer la recherche'));
});

test('une recherche sans résultat le dit, avec ce qui a été cherché', () => {
  const written = textOf(uninstallView({ ...initialState(NOW), apps: view(), appsQuery: 'zzz' } as never, noActions));
  assert.ok(written.includes(`Aucune app ne correspond à «${NBSP}zzz${NBSP}».`));
  assert.equal((page({ appsQuery: 'zzz' }).match(/<li class="row static">/g) ?? []).length, 0);
  assert.ok(!page({ appsQuery: 'git' }).includes('Aucune app ne correspond'));
});

test('une liste vide n’a ni barre d’outils ni carte ; incomplète, elle garde son avertissement et « Réessayer »', () => {
  const empty = page({ apps: view([]) });
  assert.ok(empty.includes('Aucune app classique installée'));
  assert.ok(!empty.includes('type="search"'));
  assert.ok(!empty.includes('class="card'));
  const partial = page({ apps: view(ALL, { complete: false }) });
  assert.ok(partial.includes('Shinku n’a pas pu lire toute la liste des apps.'));
  assert.ok(partial.includes('Réessayer'));
  assert.ok(!page().includes('Shinku n’a pas pu lire toute la liste'));
  // Incomplète et vide : l'avertissement seul.
  assert.ok(page({ apps: view([], { complete: false }) }).includes('Shinku n’a pas pu lire toute la liste des apps.'));
});

test('la page attend sa première vue, ou dit qu’elle n’a pas pu la lire, et propose « Réessayer »', () => {
  assert.ok(textOf(uninstallView({ ...initialState(NOW), apps: null } as never, noActions)).includes('Chargement…'));
  const failed = textOf(uninstallView({ ...initialState(NOW), apps: null, appsFailed: true } as never, noActions));
  assert.ok(failed.includes(fr.toast.unexpected));
  assert.ok(failed.includes('Réessayer'));
  assert.ok(!textOf(uninstallView({ ...initialState(NOW), apps: null } as never, noActions)).includes('Réessayer'));
});

test('pendant un geste, les boutons restent focalisables : « aria-disabled », jamais « disabled »', () => {
  const idle = page({ appsBusy: 'start' });
  assert.match(idle, /aria-disabled=true/);
  assert.ok(!/\sdisabled[\s=>]/.test(idle), 'aucun attribut disabled');
  assert.ok(!/\?disabled/.test(readFileSync(new URL('../src/views/uninstall.ts', import.meta.url), 'utf8')));
  assert.ok(!page().includes('aria-disabled=true'));
});

test('l’attente : une scène, un titre, une phrase, et « Revenir à la liste » sauf pendant `launching`', () => {
  const launching = page({ apps: view(ALL, { follow: { phase: 'launching', id: 'ac:Discord', name: 'Discord' } }) });
  assert.ok(launching.includes('Désinstallation de Discord'));
  assert.ok(launching.includes('Windows te demande peut-être ton accord.'));
  assert.ok(!launching.includes('Revenir à la liste'), 'pas de bouton pendant le lancement');
  assert.ok(!launching.includes('type="search"'), 'la liste laisse la place à la scène');

  const waiting = page({ apps: view(ALL, { follow: { phase: 'waiting', id: 'ac:Discord', name: 'Discord', seconds: 3 } }) });
  assert.ok(waiting.includes('Si une fenêtre s’ouvre, suis ses étapes. Certaines apps se retirent sans rien afficher. Shinku attend.'));
  assert.ok(!waiting.includes(t('Rien ne se passe ?')));
  assert.ok(waiting.includes('Revenir à la liste'));
  const late = page({ apps: view(ALL, { follow: { phase: 'waiting', id: 'ac:Discord', name: 'Discord', seconds: 20 } }) });
  assert.ok(late.includes(t('Rien ne se passe ? Regarde dans la barre des tâches')));
  const settling = page({ apps: view(ALL, { follow: { phase: 'settling', id: 'ac:Discord', name: 'Discord' } }) });
  assert.ok(settling.includes('Presque fini…'));
  assert.ok(settling.includes('Revenir à la liste'));
  assert.match(settling, /data-fk=uninstall-title/);
});

test('la région vivante est dans toutes les phases, hors écran, et porte la phrase de la phase', () => {
  for (const follow of [IDLE, { phase: 'waiting', id: 'a', name: 'A', seconds: 1 } as const, removed()]) {
    const written = page({ apps: view(ALL, { follow }), appsAnnounce: 'Une phrase.' });
    assert.match(written, /<div class="sr" aria-live="polite">Une phrase\.<\/div>/);
  }
  assert.match(htmlOf(uninstallView({ ...initialState(NOW), apps: null } as never, noActions)), /aria-live="polite"/);
});

test('les restes : une case par reste, jamais cochée d’avance, le chemin entier en infobulle', () => {
  const written = page({ apps: view(ALL, { follow: removed() }) });
  assert.ok(written.includes('Discord est désinstallée'));
  assert.ok(written.includes(fr.uninstall.leftovers.found(3)));
  assert.equal(fr.uninstall.leftovers.found(3), t(`Shinku a trouvé 3${NBSP}éléments qui semblent venir d’elle. Rien n’est coché : regarde avant de choisir.`));
  assert.equal((written.match(/type="checkbox"/g) ?? []).length, 3);
  assert.ok(!written.includes('.checked=true'));
  assert.ok(written.includes(`title=${FOLDER.path}`));
  assert.ok(written.includes('Dossier de l’app'));
  assert.ok(written.includes(`aria-label=${FOLDER.path}`), 'le nom de la case est le chemin entier, sans espace de plus');
  assert.ok(written.includes('umid') && !/class="mid"/.test(written), 'la classe du chemin est celle de la page');
  assert.ok(written.includes('Raccourci qui ne mène plus nulle part'));
  assert.ok(written.includes('Rien n’est coché'));
  assert.match(written, /class="orb uorb"[^>]*aria-disabled=true/, 'l’orbe est inactif tant que rien n’est coché');
  for (const label of ['Mettre à la corbeille', 'Chercher à nouveau', 'Terminé']) {
    assert.ok(written.includes(label), label);
  }
  // Coché, l'orbe s'allume et la légende compte.
  const checked = page({ apps: view(ALL, { follow: removed() }), appsChecked: [DATA.path, LINK.path] });
  assert.ok(!/class="orb uorb"[^>]*aria-disabled=true/.test(checked));
  assert.ok(checked.includes(`2${NBSP}éléments, 240${NBSP}Mo`));
  assert.equal((checked.match(/\.checked=true/g) ?? []).length, 2);
  assert.ok(!checked.includes('Ta corbeille peut abandonner'));
  assert.ok(page({ apps: view(ALL, { follow: removed() }), appsChecked: [FOLDER.path] }).includes('Ta corbeille peut abandonner d’anciens éléments pour faire de la place.'));
  const big = page({ apps: view(ALL, { follow: removed() }), appsChecked: [FOLDER.path] });
  assert.equal((big.match(/class="ucap"/g) ?? []).length, 2, 'deux phrases, chacune dans son bloc');
});

test('un reste qui n’a pas pu partir dit pourquoi, et la case le cite', () => {
  const written = page({ apps: view(ALL, { follow: removed() }), appsFailures: { [DATA.path]: 'inUse' } });
  assert.ok(written.includes(t('Encore utilisé : ferme ce qui l’utilise, puis réessaie.')));
  assert.match(written, /aria-describedby="uninstall-left-1-d uninstall-left-1-f"/);
  assert.match(written, /aria-describedby="uninstall-left-0-d"/);
});

test('rien à proposer : la raison, et les boutons qui conviennent', () => {
  const family = page({ apps: view(ALL, { follow: removed({ family: true }) }) });
  assert.ok(family.includes('Une app du même nom est encore installée. Shinku ne propose rien, pour ne pas toucher à ce qui lui sert.'));
  assert.ok(family.includes('Terminé'));
  assert.ok(!family.includes('Chercher à nouveau'));
  assert.ok(/class="orb uorb"/.test(family), 'le pied de scène porte l’orbe « Terminé »');
  assert.ok(!family.includes('uleftb'), 'pas de « Chercher à nouveau » pour une famille');
  assert.ok(!family.includes('type="checkbox"'));

  const unsure = page({ apps: view(ALL, { follow: removed({ complete: false }) }) });
  assert.ok(unsure.includes(t('Shinku n’a pas pu vérifier à qui sont les dossiers : il ne propose rien.')));
  assert.ok(unsure.includes('Chercher à nouveau'));
  assert.ok(unsure.includes('Terminé'));

  const none = page({ apps: view(ALL, { follow: removed({ leftovers: [] }) }) });
  assert.ok(none.includes('Shinku n’a rien trouvé à te proposer. Dans ton compte, il ne cherche que les dossiers qui portent exactement le nom de l’app, et les raccourcis qui menaient vers elle.'));
  assert.ok(none.includes('Chercher à nouveau'));
  assert.ok(!none.includes('Mettre à la corbeille'), 'pas d’orbe sans rien à mettre');

  // Tout est parti : le titre reste, et la phrase ne dit plus « rien trouvé ».
  const emptied = page({ apps: view(ALL, { follow: removed({ leftovers: [] }) }), appsEmptied: true });
  assert.ok(emptied.includes('Discord est désinstallée'));
  assert.ok(emptied.includes('Il ne reste plus rien à te proposer.'));
  assert.ok(!emptied.includes('rien trouvé'));
});

test('pendant la corbeille, la case ne bouge pas et les boutons attendent', () => {
  const written = page({ apps: view(ALL, { follow: removed() }), appsBusy: 'remove', appsChecked: [DATA.path] });
  assert.match(written, /class="orb uorb"[^>]*aria-disabled=true/);
  assert.equal((written.match(/type="checkbox"[^>]*aria-disabled=true/g) ?? []).length, 3);
});

test('la page n’écrit aucun attribut style ni couleur, et le texte cherché ne vit pas dans les réglages', () => {
  for (const follow of [IDLE, removed(), { phase: 'waiting', id: 'a', name: 'A', seconds: 1 } as const]) {
    assert.ok(!/\sstyle\s*=/.test(page({ apps: view(ALL, { follow }) })));
  }
  const css = readFileSync(new URL('../src/styles/uninstall.css', import.meta.url), 'utf8');
  assert.ok(!/#[0-9a-fA-F]{3,8}\b/.test(css), 'aucune couleur hors de tokens.css');
  assert.ok(!/\b(rgb|rgba|hsl)\((?!var)/.test(css.replace(/rgb\(var\(--ov\)[^)]*\)\)?/g, '')), 'aucune couleur écrite à la main');
  const index = readFileSync(new URL('../src/styles/index.css', import.meta.url), 'utf8');
  assert.ok(index.includes("@import './uninstall.css';"));
  const state = initialState(NOW);
  assert.equal(state.appsQuery, '');
  assert.equal(state.appsSort, 'name');
  assert.deepEqual(state.appsChecked, []);
});

test('la page n’envoie jamais un chemin ni une commande : seulement des identifiants et des clés', () => {
  const backend = readFileSync(new URL('../src/backend/tauri.ts', import.meta.url), 'utf8');
  assert.match(backend, /invoke<AppsView>\('start_uninstall', \{ id \}\)/);
  assert.match(backend, /invoke<RemoveLeftovers>\('remove_leftovers', \{ keys \}\)/);
  const types = readFileSync(new URL('../src/backend/types.ts', import.meta.url), 'utf8');
  assert.match(types, /startUninstall\(id: string\): Promise<AppsView>/);
  assert.match(types, /removeLeftovers\(keys: number\[\]\): Promise<RemoveLeftovers>/);
});

test('« Désinstallation » n’est plus un module à venir, et son texte ne parle plus du registre', () => {
  const modules = readFileSync(new URL('../src/views/modules.ts', import.meta.url), 'utf8');
  // Plus aucun module n'est « à venir » : la page « bientôt » a disparu avec Confidentialité.
  assert.doesNotMatch(modules, /SoonRoute|soonView/);
  assert.ok(!/registre/i.test(fr.soon.uninstall));
  assert.ok(!/registre/i.test(fr.uninstall.intro));
});

// ── L'essai de la fenêtre : ce qu'il cherche existe dans la page ───────────

/** Les mêmes blancs et la même casse que `shows` dans `essai-fenetre.mjs` : blancs insécables lus comme des espaces. */
const flat = (text: string): string => text.replace(/[\u00a0\u202f\s]+/g, ' ').trim().toLocaleLowerCase('fr');

function tree(parts: Record<string, unknown> = {}): Node {
  // La page est dans `<main class="main">` de la coque : l'essai y cherche aussi le titre.
  return parse(`<main class="main">${markupOf(uninstallView({ ...initialState(NOW), route: 'uninstall', apps: view(), ...parts } as never, noActions))}</main>`);
}

function found(scope: Node, selector: string, what: string): Node[] {
  const nodes = queryAll(scope, selector);
  assert.ok(nodes.length > 0, `« ${what} » : le sélecteur ${selector} ne trouve rien dans la page`);
  return nodes;
}

test('essai de la fenêtre : les sélecteurs de la liste trouvent la page (titre, phrase, recherche, lignes, boutons, avertissement)', () => {
  const list = tree({ apps: view(ALL, { complete: false }) });
  found(list, DOM.title, 'titre');
  assert.ok(flat(textOfNode(found(list, DOM.lead, 'phrase')[0] as Node)).length > 0);
  found(list, DOM.search, 'recherche');
  const rows = found(list, DOM.row, 'lignes');
  assert.equal(rows.length, ALL.length, 'une ligne par app');
  const first = rows[0] as Node;
  found(first, DOM.rowName, 'nom de la ligne');
  found(first, DOM.rowDetail, 'seconde ligne');
  const withButton = rows.find((row) => query(row, DOM.rowButton) !== null);
  assert.ok(withButton !== undefined, 'une ligne a un bouton « Désinstaller »');
  assert.ok(flat(query(withButton, DOM.rowButton)?.attrs['aria-label'] ?? '').startsWith(flat(EXPECTED.uninstallButton)));
  assert.ok(rows.some((row) => query(row, DOM.rowEnd) !== null), 'une ligne dit pourquoi elle n’a pas de bouton');
  assert.ok(found(list, DOM.rowButtons, 'boutons des lignes').some((button) => (button.attrs['aria-label'] ?? '').includes('Discord')));
  assert.ok(flat(textOfNode(found(list, DOM.warning, 'avertissement « liste incomplète »')[0] as Node)).includes(flat(EXPECTED.incomplete)));
  assert.ok(flat(textOfNode(list)).includes(flat(EXPECTED.store)));

  // Pas d'app : le titre le dit. Pas encore de vue : la note de chargement.
  assert.ok(flat(textOfNode(found(tree({ apps: view([]) }), DOM.title, 'titre sans app')[0] as Node)).includes(flat(EXPECTED.noApps)));
  found(tree({ apps: null }), DOM.loading, 'note de chargement');
});

test('essai de la fenêtre : les sélecteurs des restes trouvent le bilan, les cases (chemin entier en aria-label), l’orbe et le pied', () => {
  const page = tree({ apps: view(ALL, { follow: removed() }) });
  assert.ok(flat(textOfNode(found(page, DOM.title, 'titre')[0] as Node)).includes(flat(EXPECTED.uninstalled)));
  assert.ok(flat(textOfNode(found(page, DOM.lead, 'phrase du bilan')[0] as Node)).includes(flat(EXPECTED.nothingChecked)));
  const items = found(page, DOM.leftover, 'restes');
  assert.equal(items.length, 3);
  assert.deepEqual(
    items.map((item) => query(item, DOM.leftoverBox)?.attrs['aria-label']),
    [FOLDER.path, DATA.path, LINK.path],
    'l’aria-label de la case est le chemin entier : c’est là que l’essai le lit',
  );
  const kinds = items.map((item) => flat(textOfNode(query(item, DOM.leftoverText) as Node)));
  assert.ok(kinds[0]?.startsWith(flat(EXPECTED.folder)) && kinds[1]?.startsWith(flat(EXPECTED.data)) && kinds[2]?.startsWith(flat(EXPECTED.shortcut)));
  const orb = found(page, DOM.orb, 'orbe');
  assert.ok(flat(textOfNode(orb[0] as Node)).startsWith(flat(EXPECTED.trash)));
  const dock = found(page, DOM.dock, 'boutons du pied').map((button) => flat(textOfNode(button)));
  assert.ok(dock.some((text) => text.startsWith(flat(EXPECTED.done))), 'un bouton « Terminé » dans le pied');

  // Rien à proposer : l'orbe devient « Terminé » (dans le pied), et la famille a sa phrase.
  const none = tree({ apps: view(ALL, { follow: removed({ leftovers: [] }) }) });
  assert.ok(found(none, DOM.dock, 'pied').some((button) => flat(textOfNode(button)).startsWith(flat(EXPECTED.done))));
  assert.equal(queryAll(none, DOM.orb).length, 0, 'pas d’orbe « corbeille » sans reste');
  const family = tree({ apps: view(ALL, { follow: removed({ family: true }) }) });
  assert.ok(flat(textOfNode(found(family, DOM.lead, 'phrase de la famille')[0] as Node)).includes(flat(EXPECTED.family)));
  assert.equal(queryAll(family, DOM.leftover).length, 0);
});

test('essai de la fenêtre : la boîte de confirmation porte « Désinstaller », et ses textes sont ceux de fr.ts', () => {
  const dialog = parse(markupOf(uninstallDialog({ ...initialState(NOW), route: 'uninstall', apps: view(), appsConfirm: askFor(DISCORD) } as never, noActions)));
  const box = found(dialog, DOM.dialog, 'boîte de confirmation');
  const buttons = found(box[0] as Node, DOM.dialogButton, 'boutons de la boîte').map((button) => flat(textOfNode(button)));
  assert.ok(buttons.some((text) => text.startsWith(flat(EXPECTED.uninstallButton))));
  // Chaque texte que l'essai attend est dans fr.ts, tel quel.
  const words = [fr.uninstall.leftovers.folder, fr.uninstall.leftovers.data, fr.uninstall.leftovers.shortcut, fr.uninstall.leftovers.family, fr.uninstall.leftovers.found(3), fr.uninstall.store, fr.uninstall.title(0), fr.uninstall.incomplete, fr.uninstall.leftovers.title('X'), fr.uninstall.leftovers.trash, fr.uninstall.leftovers.done, fr.uninstall.uninstall].map(flat);
  for (const [name, expected] of Object.entries(EXPECTED)) {
    assert.ok(words.some((text) => text.includes(flat(expected))), `EXPECTED.${name} (« ${expected} ») n’est dans aucun texte de fr.ts`);
  }
  assert.ok(flat(fr.uninstall.toast.removed(3, 1000)).includes('à la corbeille') && flat(fr.uninstall.toast.removed(3, 1000)).includes('3 éléments'), 'le toast que l’essai attend');
});

test('essai de la fenêtre : les sélecteurs de la page savent lire un sélecteur cassé (le test n’est pas complaisant)', () => {
  const page = tree({ apps: view(ALL, { follow: removed() }) });
  assert.equal(queryAll(page, '.mid').length, 0, 'l’ancienne classe des chemins n’existe plus');
  assert.equal(queryAll(page, '.page.uninstall li:has(input[type="radio"])').length, 0);
  assert.throws(() => queryAll(page, 'li + li'), /non pris en charge/);
  assert.throws(() => queryAll(page, 'li:has(.page input)'), /non pris en charge/, ':has avec un combinateur est refusé');
  assert.throws(() => queryAll(page, 'li:has(input, a)'), /non pris en charge|sélecteur/);
});
