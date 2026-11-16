// Ce que l'essai de la fenêtre (`essai-fenetre.mjs`) attend de la page Journal et du bloc « À propos » des
// Paramètres : les sélecteurs qu'il cherche, les textes qu'il lit, et les fonctions qui lisent la page. Ils sont ici,
// et non dans le script, pour que `tests/journal.test.ts` puisse rendre la page et vérifier que chacun y trouve
// son élément : renommer une classe ou un texte de la page casse alors `npm test`, pas seulement la machine Windows.
//
// Les sélecteurs n'emploient que : une balise, des classes, des attributs, les combinateurs « descendant » et « > ».

export const DOM = {
  title: '.main h1',
  /** La page chargée, et sa note d'attente. */
  page: '.page.journal',
  loading: '.page.journal .jwait',
  /** Une ligne du Journal, sa phrase, sa date, son lien de retour. */
  row: '.page.journal li.jrow',
  sentence: '.jsentence',
  date: '.jdate',
  link: 'button.jlink',
  /** Le bouton « Oublier les désinstallations » : l'essai le cherche, il n'y clique jamais. */
  forget: '.page.journal .jforget button',
  /** Le bouton « Voir leurs licences » des Paramètres : cherché seulement, jamais cliqué (il lance un autre programme). */
  licences: 'button[data-fk="settings-licences"]',
  /** La barre latérale, et ses entrées. */
  side: '.side',
  nav: '.side button.nav',
};

/** Les textes que l'essai cherche : ils doivent être dans `fr.ts`, tels quels (blancs insécables mis à part). */
export const EXPECTED = {
  title: 'Journal',
  forget: 'Oublier les désinstallations',
  licences: 'Voir leurs licences',
  uninstalled: 'Désinstallé :',
  cleaned: 'Grand vide :',
  leftovers: '3 restes mis à la corbeille',
};

/** La page Journal : son titre, son attente, ses lignes (phrase, date, lien), la présence du bouton « Oublier ». S'exécute dans la page. */
export function readJournal(dom) {
  const text = (element) => (element?.innerText ?? '').replace(/[   ]+/g, ' ').trim();
  return {
    title: text(document.querySelector(dom.title)),
    ready: document.querySelector(dom.page) !== null,
    loading: document.querySelector(dom.loading) !== null,
    rows: [...document.querySelectorAll(dom.row)].map((row) => ({
      sentence: text(row.querySelector(dom.sentence)),
      date: text(row.querySelector(dom.date)),
      link: text(row.querySelector(dom.link)),
    })),
    forget: text(document.querySelector(dom.forget)),
  };
}

/** Le bloc « À propos » : le bouton des licences est-il là, visible, et que dit-il ? Rien n'est cliqué. S'exécute dans la page. */
export function readAbout(dom) {
  const text = (element) => (element?.innerText ?? '').replace(/[   ]+/g, ' ').trim();
  const button = document.querySelector(dom.licences);
  const box = button?.getBoundingClientRect();
  return {
    licences: button === null ? null : text(button),
    visible: box !== undefined && box.width > 0 && box.height > 0,
  };
}

/** Le menu tient-il dans la fenêtre sans défiler ? La dernière entrée finit-elle avant le bas de la fenêtre ? S'exécute dans la page. */
export function readMenu(dom) {
  const side = document.querySelector(dom.side);
  const entries = [...document.querySelectorAll(dom.nav)];
  const last = entries.at(-1)?.getBoundingClientRect();
  return {
    entries: entries.length,
    scroll: side === null ? null : side.scrollHeight,
    client: side === null ? null : side.clientHeight,
    lastBottom: last === undefined ? null : Math.round(last.bottom),
    height: window.innerHeight,
  };
}
