// Mise en forme des tailles, des nombres et des dates.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  NBSP,
  NNBSP,
  countOf,
  driveName,
  formatAgo,
  formatBytes,
  formatCount,
  formatDay,
  formatDuration,
  formatLimit,
  formatSince,
  frenchSpacing,
  listOf,
  percent,
  plural,
  splitPath,
} from '../src/format.ts';

const KB = 1024;
const MB = 1024 ** 2;
const GB = 1024 ** 3;

test('les tailles se lisent comme dans l’Explorateur, avec la virgule française', () => {
  assert.equal(formatBytes(0), `0${NBSP}octet`);
  assert.equal(formatBytes(1), `1${NBSP}octet`);
  assert.equal(formatBytes(1023), `1023${NBSP}octets`);
  assert.equal(formatBytes(KB), `1,0${NBSP}Ko`);
  assert.equal(formatBytes(3_972_844_749), `3,7${NBSP}Go`);
  assert.equal(formatBytes(150 * MB), `150${NBSP}Mo`);
  assert.equal(formatBytes(6.8 * GB), `6,8${NBSP}Go`);
  assert.equal(formatBytes(2048 * GB), `2,0${NBSP}To`);
});

test('une taille négative ou fractionnaire ne casse pas l’affichage', () => {
  assert.equal(formatBytes(-5), `0${NBSP}octet`);
  assert.equal(formatBytes(1.9), `1${NBSP}octet`);
});

test('un seuil rond s’écrit sans décimale', () => {
  assert.equal(formatLimit(3 * GB), `3${NBSP}Go`);
  assert.equal(formatLimit(2.5 * GB), `2,5${NBSP}Go`);
  assert.equal(formatLimit(500 * MB), `500${NBSP}Mo`);
});

test('les milliers sont séparés par une espace fine insécable', () => {
  assert.equal(formatCount(0), '0');
  assert.equal(formatCount(999), '999');
  assert.equal(formatCount(1284), `1${NNBSP}284`);
  assert.equal(formatCount(196_295), `196${NNBSP}295`);
  assert.equal(formatCount(1_000_000), `1${NNBSP}000${NNBSP}000`);
});

test('zéro et un prennent le singulier', () => {
  assert.equal(plural(0, 'fichier', 'fichiers'), 'fichier');
  assert.equal(plural(1, 'fichier', 'fichiers'), 'fichier');
  assert.equal(plural(2, 'fichier', 'fichiers'), 'fichiers');
  assert.equal(countOf(1284, 'élément', 'éléments'), `1${NNBSP}284${NBSP}éléments`);
  assert.equal(countOf(1, 'élément', 'éléments'), `1${NBSP}élément`);
});

test('un pourcentage reste entre 0 et 100', () => {
  assert.equal(percent(0, 0), 0);
  assert.equal(percent(1, 3), 33);
  assert.equal(percent(5, 4), 100);
  assert.equal(percent(-1, 4), 0);
});

test('le temps écoulé se dit simplement', () => {
  const now = Date.UTC(2026, 10, 14, 12, 0, 0);
  const minute = 60_000;
  const day = 24 * 60 * minute;
  assert.equal(formatAgo(now - 10_000, now), 'à l’instant');
  assert.equal(formatAgo(now - 5 * minute, now), `il y a 5${NBSP}min`);
  assert.equal(formatAgo(now - 3 * 60 * minute, now), `il y a 3${NBSP}h`);
  assert.equal(formatAgo(now - day, now), `il y a 1${NBSP}jour`);
  assert.equal(formatAgo(now - 12 * day, now), `il y a 12${NBSP}jours`);
  assert.equal(formatAgo(now - 243 * day, now), `il y a 7${NBSP}mois`);
  assert.equal(formatAgo(now - 400 * day, now), `il y a 1${NBSP}an`);
  assert.equal(formatAgo(now - 760 * day, now), `il y a 2${NBSP}ans`);
  // Une date dans le futur (horloge déréglée) ne donne pas de durée négative.
  assert.equal(formatAgo(now + day, now), 'à l’instant');
});

test('un chemin se sépare en nom et en dossier parent', () => {
  assert.deepEqual(splitPath('C:\\Users\\Léa\\Downloads\\Windows11.iso'), {
    name: 'Windows11.iso',
    parent: 'C:\\Users\\Léa\\Downloads',
  });
  assert.deepEqual(splitPath('C:\\pagefile.sys'), { name: 'pagefile.sys', parent: 'C:\\' });
  assert.deepEqual(splitPath('D:\\Sauvegardes\\'), { name: 'Sauvegardes', parent: 'D:\\' });
  assert.deepEqual(splitPath('/tmp/essai.bin'), { name: 'essai.bin', parent: '/tmp' });
  assert.deepEqual(splitPath('/essai.bin'), { name: 'essai.bin', parent: '/' });
  assert.deepEqual(splitPath('seul.txt'), { name: 'seul.txt', parent: '' });
});

test('la lettre d’un disque se lit sans sa barre', () => {
  assert.equal(driveName('C:\\'), 'C:');
  assert.equal(driveName('\\\\serveur\\partage'), '\\\\serveur\\partage');
});

test('la ponctuation double ne se retrouve jamais seule en début de ligne', () => {
  assert.equal(frenchSpacing('Trouvé : 3 fichiers'), `Trouvé${NBSP}: 3 fichiers`);
  assert.equal(frenchSpacing('Vraiment ? Oui !'), `Vraiment${NNBSP}? Oui${NNBSP}!`);
  // Un chemin Windows n'a pas d'espace avant ses deux-points : il reste intact.
  assert.equal(frenchSpacing('Retirer C:\\Jeux'), 'Retirer C:\\Jeux');
});

test('une énumération se termine par « et »', () => {
  assert.equal(listOf([]), '');
  assert.equal(listOf(['Discord']), 'Discord');
  assert.equal(listOf(['Google Chrome', 'Discord']), 'Google Chrome et Discord');
  assert.equal(listOf(['Google Chrome', 'Discord', 'Steam']), 'Google Chrome, Discord et Steam');
});

test('une durée de jeu se lit « 45 min » ou « 2 h 10 »', () => {
  const minute = 60_000;
  assert.equal(formatDuration(45 * minute), `45${NBSP}min`);
  assert.equal(formatDuration(59 * minute + 59_000), `59${NBSP}min`);
  assert.equal(formatDuration(60 * minute), `1${NBSP}h`);
  assert.equal(formatDuration(130 * minute), `2${NBSP}h${NBSP}10`);
  assert.equal(formatDuration(125 * minute), `2${NBSP}h${NBSP}05`);
  assert.equal(formatDuration(26 * 60 * minute + 7 * minute), `26${NBSP}h${NBSP}07`);
});

test('une durée de jeu ne descend jamais sous une minute, ni sous zéro', () => {
  assert.equal(formatDuration(0), `1${NBSP}min`);
  assert.equal(formatDuration(20_000), `1${NBSP}min`);
  assert.equal(formatDuration(-5 * 60_000), `1${NBSP}min`);
});

test('le début d’une partie dit le jour quand ce n’est pas aujourd’hui', () => {
  // Heures et jours sont ceux de la machine : les dates se construisent en heure locale.
  const at = (year: number, month: number, day: number, hour: number, minute: number) =>
    new Date(year, month - 1, day, hour, minute).getTime();
  const now = at(2026, 11, 15, 9, 5);
  assert.equal(formatSince(at(2026, 11, 15, 0, 1), now), '00:01');
  assert.equal(formatSince(at(2026, 11, 15, 8, 30), now), '08:30');
  // Hier soir, même à moins de 24 h : la veille s'annonce.
  assert.equal(formatSince(at(2026, 11, 14, 23, 40), now), 'hier, 23:40');
  // Une horloge en avance (jusqu'à 24 h) : le lendemain montre son jour, pas une heure d'aujourd'hui.
  assert.equal(formatSince(at(2026, 11, 16, 1, 5), now), `le 16${NBSP}nov., 01:05`);
  assert.equal(formatSince(at(2026, 11, 14, 0, 0), now), 'hier, 00:00');
  assert.equal(formatSince(at(2026, 11, 14, 23, 40), at(2026, 11, 14, 23, 50)), '23:40');
  // Plus loin : le jour et le mois, sans l'année de cette année.
  assert.equal(formatSince(at(2026, 11, 13, 23, 40), now), `le 13${NBSP}nov., 23:40`);
  assert.equal(formatSince(at(2026, 10, 1, 7, 0), now), `le 1er${NBSP}oct., 07:00`);
  assert.equal(formatSince(at(2026, 7, 4, 12, 0), now), `le 4${NBSP}juil., 12:00`);
  assert.equal(formatSince(at(2026, 5, 4, 12, 0), now), `le 4${NBSP}mai, 12:00`);
  // Une autre année la porte.
  assert.equal(formatSince(at(2025, 12, 31, 22, 15), now), `le 31${NBSP}déc. 2025, 22:15`);
  // Une date dans le futur (horloge qui recule) ne fait pas dire n'importe quoi.
  assert.equal(formatSince(now + 3_600_000, now), '10:05');
});

test('un jour se lit « le 15 nov. », avec l’année si ce n’est pas l’année en cours', () => {
  const at = (year: number, month: number, day: number, hour = 12) => new Date(year, month - 1, day, hour).getTime();
  const now = at(2026, 11, 15, 9);
  assert.equal(formatDay(at(2026, 11, 15), now), `le 15${NBSP}nov.`);
  assert.equal(formatDay(at(2026, 11, 2), now), `le 2${NBSP}nov.`);
  assert.equal(formatDay(at(2026, 1, 1), now), `le 1er${NBSP}janv.`);
  assert.equal(formatDay(at(2026, 5, 4), now), `le 4${NBSP}mai`);
  assert.equal(formatDay(at(2026, 8, 31), now), `le 31${NBSP}août`);
  // Une autre année la porte, l'an dernier comme l'an prochain.
  assert.equal(formatDay(at(2025, 11, 2), now), `le 2${NBSP}nov. 2025`);
  assert.equal(formatDay(at(2024, 12, 31, 23), now), `le 31${NBSP}déc. 2024`);
  assert.equal(formatDay(at(2027, 1, 3), now), `le 3${NBSP}janv. 2027`);
  // Le 1er janvier de l'année en cours n'a pas d'année, la veille de l'an dernier en a une.
  assert.equal(formatDay(at(2025, 12, 31, 23), at(2026, 1, 1, 0)), `le 31${NBSP}déc. 2025`);
});

test('la phrase de la partie en cours porte ce début', async () => {
  const { fr } = await import('../src/i18n/fr.ts');
  assert.equal(
    fr.game.textPlaying('hier, 23:40'),
    'Mode jeu actif depuis hier, 23:40. Tout revient en place quand tu quittes le jeu.',
  );
});
