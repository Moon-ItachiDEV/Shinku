// Ce que l'essai de la fenêtre (`essai-fenetre.mjs`) attend de la page Désinstallation : les sélecteurs
// qu'il cherche et les textes qu'il lit. Ils sont ici, et non dans le script, pour que
// `tests/essai-desinstallation.test.ts` puisse rendre la page et vérifier que chacun y trouve son élément :
// renommer une classe ou un texte de la page casse alors `npm test`, pas l'essai sur le vrai Windows.
//
// Les sélecteurs n'emploient que : une balise, des classes, des attributs (`[a]`, `[a="v"]`, `[a^="v"]`),
// les combinateurs « descendant » et « > », et `:has(...)` : c'est ce que le test sait lire.

export const DOM = {
  title: '.main h1',
  /** La phrase sous le titre : l'introduction de la liste, ou le bilan de la désinstallation. */
  lead: '.page.uninstall .phead .ptext',
  loading: '.page.uninstall .note[role="status"]',
  warning: '.page.uninstall .pwarn',
  search: 'input[data-fk="uninstall-search"]',
  row: '.page.uninstall .plist > li.row',
  rowName: '.t1',
  rowDetail: '.t2',
  rowEnd: '.uend',
  rowButton: 'button[data-fk^="uninstall:"]',
  /** Tous les boutons des lignes, pour trouver celui d'une app par son nom accessible. */
  rowButtons: '.page.uninstall .row button',
  leftover: '.page.uninstall li:has(input[type="checkbox"])',
  /** Sa case : son `aria-label` est le chemin entier du reste. */
  leftoverBox: 'input[type="checkbox"]',
  leftoverText: '.t2',
  orb: '.orb[data-fk="uninstall-orb"]',
  /** La boîte de confirmation ; elle porte `[open]` une fois ouverte par `showModal`. */
  dialog: 'dialog.ask',
  dialogButton: 'button',
  dock: '.dock button',
};

/** Les textes de la page que l'essai cherche : ils doivent être dans `fr.ts`, tels quels (blancs insécables mis à part). */
export const EXPECTED = {
  store: 'Les apps du Microsoft Store ne sont pas ici',
  noApps: 'Aucune app classique installée',
  uninstallButton: 'Désinstaller',
  uninstalled: 'est désinstallée',
  nothingChecked: 'Rien n’est coché',
  folder: 'Dossier de l’app',
  data: 'Réglages et données de l’app',
  shortcut: 'Raccourci qui ne mène plus nulle part',
  trash: 'Mettre à la corbeille',
  done: 'Terminé',
  family: 'Une app du même nom est encore installée',
  incomplete: 'Shinku n’a pas pu lire toute la liste',
};
