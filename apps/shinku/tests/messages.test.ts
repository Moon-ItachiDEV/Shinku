// Les textes de l'interface.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import { NBSP, NNBSP } from '../src/format.ts';
import { fr } from '../src/i18n/fr.ts';

/** Tous les textes, y compris ceux qui se calculent (avec des valeurs d'exemple). */
function allTexts(group: object, path = 'fr'): Array<[string, string]> {
  return Object.entries(group).flatMap(([key, value]): Array<[string, string]> => {
    const name = `${path}.${key}`;
    if (typeof value === 'string') {
      return [[name, value]];
    }
    if (typeof value === 'function') {
      const sample = (value as (...args: unknown[]) => string)(3 * 1024 ** 3, 5, 5);
      return [[name, sample]];
    }
    return allTexts(value as object, name);
  });
}

test('aucun signe de ponctuation double n’est précédé d’une espace ordinaire', () => {
  for (const [name, text] of allTexts(fr)) {
    assert.doesNotMatch(text, / [:;!?]/, `${name} : « ${text} »`);
  }
});

test('les apostrophes sont typographiques', () => {
  for (const [name, text] of allTexts(fr)) {
    assert.doesNotMatch(text, /'/, `${name} : « ${text} »`);
  }
});

test('les textes calculés accordent les nombres', () => {
  assert.equal(fr.junk.files(1), `1${NBSP}fichier`);
  assert.equal(fr.junk.files(4210), `4${NNBSP}210${NBSP}fichiers`);
  assert.equal(fr.duplicates.found(0), 'Aucun doublon');
  assert.equal(fr.duplicates.found(1), `1${NBSP}groupe de doublons`);
  assert.equal(fr.large.found(4, 3 * 1024 ** 3), `4${NBSP}fichiers de plus de 3${NBSP}Go`);
  assert.equal(fr.results.skipped(1), `1${NBSP}dossier n’a pas pu être lu${NBSP}: il n’est pas compté.`);
  assert.equal(fr.results.skipped(3), `3${NBSP}dossiers n’ont pas pu être lus${NBSP}: ils ne sont pas comptés.`);
  // La jauge du disque coupe la phrase en deux : le chiffre en grand, puis la suite.
  assert.equal(`${fr.disk.free(796 * 1024 ** 3)} ${fr.disk.freeRest(2 * 1024 ** 4)}`, `796${NBSP}Go libres sur 2,0${NBSP}To`);
});
