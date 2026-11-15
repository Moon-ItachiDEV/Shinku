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
});

test('les guillemets français tiennent à leur texte par des espaces insécables', () => {
  for (const [name, text] of allTexts(fr)) {
    assert.doesNotMatch(text, /« | »/, `${name} : « ${text} »`);
  }
  assert.match(fr.game.powerHint, new RegExp(`«${NBSP}Performances élevées${NBSP}»`));
  assert.match(fr.game.powerUnavailable, new RegExp(`«${NBSP}Performances élevées${NBSP}»`));
});

test('les textes neufs du mode jeu disent ce qu’il faut, au tutoiement', () => {
  assert.equal(fr.game.titlePending, 'Remise en place en cours');
  assert.equal(fr.game.addedApp, 'Ajoutée par toi');
  assert.equal(fr.game.notAGameNamed('Hollow Peaks'), `Ce n’est pas un jeu${NBSP}: Hollow Peaks`);
  assert.equal(fr.game.restoreNamed('Hollow Peaks'), 'Rétablir Hollow Peaks');
  assert.equal(fr.game.toast.unknownApp, 'Cette app n’est plus dans la liste.');
});
