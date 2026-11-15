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
  formatLimit,
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
