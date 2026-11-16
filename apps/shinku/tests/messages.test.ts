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

test('les textes du démarrage disent ce qu’il faut, au tutoiement, au caractère près', () => {
  const s = fr.startup;
  assert.equal(s.eyebrow, 'Démarrage');
  assert.equal(
    s.scope,
    'Shinku liste ce que règle le Gestionnaire des tâches, sauf les apps du Microsoft Store. Les tâches planifiées et les services ne sont pas ici.',
  );
  assert.equal(s.openWindows, 'Apps de démarrage de Windows');
  assert.equal(s.user, 'Pour ton compte');
  assert.equal(s.machine, 'Pour tous les comptes de ce PC');
  assert.equal(s.machineHint, `Ces apps se règlent dans Windows pour l’instant${NBSP}: il faut l’accord d’un administrateur.`);
  assert.equal(s.unreadable, 'Shinku n’a pas pu lire toute cette liste.');
  assert.equal(
    s.note,
    'Couper une app ne la désinstalle pas et ne supprime rien : elle ne se lance plus toute seule, et tu peux toujours l’ouvrir toi-même. Certaines apps se réinscrivent d’elles-mêmes : règle-le aussi dans leurs options. L’éditeur affiché est celui que le fichier déclare.'.replaceAll(
      ' :',
      `${NBSP}:`,
    ),
  );
  assert.equal(s.unknownPublisher, 'Éditeur inconnu');
  assert.equal(s.hostedBy('rundll32.exe'), 'Lancée par rundll32.exe');
  assert.equal(s.alias('OneDrive'), `inscrite sous «${NBSP}OneDrive${NBSP}»`);
  assert.equal(s.folder, 'raccourci du dossier Démarrage');
  assert.equal(s.missing, 'Semble désinstallée');
  assert.equal(s.cameBack(`le 10${NBSP}nov.`), `S’est remise toute seule depuis le 10${NBSP}nov.`);
  assert.equal(s.cutByShinku(`le 12${NBSP}nov.`), `Coupée par Shinku le 12${NBSP}nov.`);
  assert.equal(s.cutOn(`le 2${NBSP}nov. 2025`), `Coupée le 2${NBSP}nov. 2025`);
  assert.equal(s.cut, 'Coupée');
  assert.equal(s.active, 'Active');
  assert.equal(s.sync, 'Synchronise tes fichiers');
  assert.equal(s.own, 'Réglé par le Mode jeu');
  assert.equal(s.ownCut, 'Coupée dans Windows');
  assert.equal(s.seeGameMode, 'Voir le Mode jeu');
  assert.equal(s.unreadableEntry, 'Shinku ne sait pas lire son réglage');
  assert.equal(s.notAdjustableHere, 'Shinku ne peut pas la régler ici');
  assert.equal(s.retry, 'Réessayer');
  assert.equal(s.separator, ' · ');
  assert.equal(s.initialFallback, '#');
  assert.equal(s.openInWindows, 'Ouvrir dans Windows');
  assert.equal(s.undo, 'Annuler');
  assert.equal(s.openInWindowsFor('Steam'), 'Ouvrir dans Windows, pour Steam');
  assert.equal(s.say.restored('Steam'), 'Steam se lancera de nouveau à l’ouverture de ta session.');
  assert.equal(s.say.failed, 'Le changement n’a pas pu être fait.');
  assert.equal(s.say.unknownEntry, 'Cette entrée n’existe plus.');
  assert.equal(s.say.notAdjustable, 'Cette entrée ne se règle pas ici.');
});

test('le nom d’un interrupteur du démarrage dit ce qu’il règle, et distingue deux lignes de même nom', () => {
  assert.equal(fr.startup.switchLabel('Discord', null), 'Lancer Discord à l’ouverture de session');
  assert.equal(
    fr.startup.switchLabel('Microsoft OneDrive', 'OneDrive'),
    'Lancer Microsoft OneDrive à l’ouverture de session (OneDrive)',
  );
  // Deux lignes de même nom et de même alias se distinguent par leur rang.
  assert.equal(fr.startup.switchLabel('Java', null, 2), 'Lancer Java à l’ouverture de session (2)');
  assert.equal(fr.startup.switchLabel('Java', 'a.exe', 3), 'Lancer Java à l’ouverture de session (a.exe) (3)');
  assert.notEqual(fr.startup.switchLabel('Java', 'a.exe'), fr.startup.switchLabel('Java', 'b.exe'));
});

test('les textes du démarrage s’accordent, au singulier comme au pluriel', () => {
  assert.equal(fr.startup.title(0), 'Aucune app inscrite ici');
  assert.equal(fr.startup.title(1), `1${NBSP}app inscrite au démarrage`);
  assert.equal(fr.startup.title(2), `2${NBSP}apps inscrites au démarrage`);
  assert.equal(fr.scan.startupCount(0), 'Aucune app active');
  assert.equal(fr.scan.startupCount(1), `1${NBSP}app active`);
  assert.equal(fr.scan.startupCount(8), `8${NBSP}apps actives`);
  assert.equal(fr.scan.startupNotMeasured, 'Non mesuré');
  assert.equal(fr.results.startupHint(0), 'Aucune app active au démarrage');
  assert.equal(fr.results.startupHint(1), `1${NBSP}app active au démarrage de Windows`);
  assert.equal(fr.results.startupHint(8), `8${NBSP}apps actives au démarrage de Windows`);
  assert.equal(fr.results.startupNotMeasured, 'Non mesuré');
  assert.equal(fr.startup.say.cut('Steam', false).includes('synchronis'), false);
  assert.equal(
    fr.startup.say.cut('OneDrive', true),
    `OneDrive est coupée. Si elle est ouverte, elle le reste${NBSP}: elle ne se lancera plus toute seule à ta prochaine ouverture de session. Tes fichiers ne se synchroniseront plus tant que tu ne l’ouvres pas.`,
  );
});

test('aucune phrase du démarrage ne promet plus que ce que Shinku lit', () => {
  const texts = [
    ...allTexts(fr.startup, 'fr.startup'),
    ['fr.scan.startupCount', fr.scan.startupCount(0)],
    ['fr.results.startupHint', fr.results.startupHint(0)],
    ['fr.results.startupHint', fr.results.startupHint(5)],
    ['fr.startup.summary', fr.startup.summary(0, 0)],
    ['fr.startup.summary', fr.startup.summary(0, 3)],
    ['fr.startup.say.cut', fr.startup.say.cut('X', true)],
  ];
  for (const [name, text] of texts) {
    // « Rien ne se lance » et « plus rien » promettent ce que Shinku ne voit pas (Store, tâches planifiées, services).
    assert.doesNotMatch(text ?? '', /rien ne se lance|plus rien/i, `${name} : « ${text} »`);
    // Aucun chiffre inventé : ni gain de temps, ni durée, ni impact.
    assert.doesNotMatch(text ?? '', /impact|secondes?\b|temps qu/i, `${name} : « ${text} »`);
  }
  // L'ancien texte de la tuile promettait « le temps qu'elles te coûtent » : il n'est plus nulle part.
  assert.ok(!('startup' in fr.soon));
  assert.ok(!('tile' in fr.startup));
});
