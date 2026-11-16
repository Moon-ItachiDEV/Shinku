// Ce que l'essai de la fenêtre (`essai-fenetre.mjs`) attend de la page Démarrage : les sélecteurs qu'il
// cherche et les textes qu'il lit. Ils sont ici, et non dans le script, pour que `tests/startup.test.ts`
// puisse rendre la page et vérifier que chacun y trouve son élément : renommer une classe ou un texte de
// la page casse alors `npm test`, pas seulement l'essai sur le vrai Windows.
//
// Les sélecteurs n'emploient que : une balise, des classes, des attributs (`[a]`, `[a="v"]`, `[a^="v"]`),
// les combinateurs « descendant » et « > », et `:has(...)` : c'est ce que le test sait lire.

export const DOM = {
  title: '.main h1',
  /** La note de chargement (ou d'échec) seule : la note du bas de page porte aussi la classe `note`. */
  loading: '.page.startup .note[role="status"]',
  section: '.page.startup section.psec',
  sectionTitle: 'h2',
  /** Les phrases de la carte du PC : celle de l'accès (ou du lot 1), sous le titre de la carte. */
  access: '.pmach .d',
  /** Le bouton « Apps de démarrage de Windows » de la carte du PC (lot 1 : sans accès). */
  windowsButton: '.pmach button',
  /** La phrase de prudence, dès qu'il y a un accès. */
  caution: '.pcaut',
  row: '.page.startup .plist > li.row',
  rowName: '.t1',
  /** La première seconde ligne d'une ligne du compte : l'éditeur, le nom d'inscription, le raccourci. */
  rowDetail: '.t2',
  /** La ligne « Inscrite sous « … » » d'une ligne du PC. */
  registered: '.t2.reg',
  /** La phrase d'attente de la ligne dont le changement attend la personne. */
  waiting: '.t2.wait',
  rowSwitch: 'button[role="switch"]',
  tag: '.tag',
  /** La zone de retour : sa phrase (dans `span`) et le bouton « Annuler ». */
  say: '.pzone .psay',
  sayText: 'span',
  sayButton: 'button',
};

/** Les textes de la page que l'essai cherche : ils doivent être dans `fr.ts`, tels quels (blancs insécables mis à part). */
export const EXPECTED = {
  userCard: 'Pour ton compte',
  machineCard: 'Pour tous les comptes',
  registered: 'Inscrite sous',
  /** Les trois phrases d'accès, reconnues par leur début. */
  consent: 'Windows te demande ton accord',
  password: 'Windows demandera le mot de passe',
  direct: 'Shinku te demandera de confirmer',
  /** La phrase du lot 1, sans accès. */
  unavailable: 'Ces apps se règlent dans Windows',
  caution: 'Certaines servent au son',
  openWindows: 'Apps de démarrage de Windows',
  cut: 'est coupée',
  restored: 'se lancera de nouveau',
  active: 'Active',
};
