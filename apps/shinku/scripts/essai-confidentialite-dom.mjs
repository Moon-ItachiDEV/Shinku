// Ce que les essais de la fenêtre (`essai-fenetre.mjs` et `essai-confidentialite.mjs`) attendent de la page
// Confidentialité : les sélecteurs qu'ils cherchent, les textes qu'ils lisent, et la fonction qui lit la page.
// Ils sont ici, et non dans les scripts, pour que `tests/confidentialite-essai.test.ts` puisse rendre la page et
// vérifier que chacun y trouve son élément : renommer une classe ou un texte de la page casse alors `npm test`,
// pas seulement l'essai sur le vrai Windows.
//
// Les sélecteurs n'emploient que : une balise, des classes, des attributs (`[a]`, `[a="v"]`, `[a^="v"]`),
// les combinateurs « descendant » et « > », et `:has(...)` : c'est ce que le test sait lire.

export const DOM = {
  title: '.main h1',
  /** La page elle-même (chargée), et sa note de chargement. */
  page: '.page.privacy',
  loading: '.page.privacy .note[role="status"]',
  /** La phrase sous le titre : « Des traces de ton activité… Rien n’est coché d’avance. » */
  intro: '.phead .ptext',
  /** Le bouton « Effacer ce qui est coché » de l'en-tête (la zone du bas en a un second, que l'essai ne touche pas). */
  erase: '.phead .pline button',
  /** La carte d'un navigateur, son nom, et sa phrase « Ferme… » quand il est ouvert. */
  browserCard: '.page.privacy .pbrowser',
  browserName: '.pbname',
  running: '.pwarn',
  /** Les lignes d'un navigateur ; les lignes d'un bloc Windows sont dans `.pblock` (voir `readPrivacy`). */
  browserRow: '.pbrowser .plist > li',
  /** Les mêmes lignes, cherchées à partir de leur carte. */
  cardRow: '.plist > li',
  windowsBlock: '.page.privacy section.pblock',
  blockTitle: 'h2',
  windowsRow: '.card .plist > li',
  rowTitle: '.t1',
  rowSub: '.t2',
  /** La phrase d'issue sous une ligne, après un effacement. */
  rowOutcome: '.t2.pout',
  rowValue: '.val.usize',
  check: 'input[type="checkbox"]',
  /** Une ligne sans case : « Rien à effacer » (`pempty`) ou inerte (`pinert` : navigateur ouvert, trace illisible). */
  rowInert: '.row.pinert',
  rowEmpty: '.row.pempty',
  /** La carte « Dernier effacement, à 14 h 32 » : son titre (daté), puis une ligne par trace, son bloc, sa trace, son issue en deux mots et son aide. */
  lastTitle: 'h2[id="privacy-last-title"]',
  last: '.plast',
  lastBlock: '.t1',
  lastTrace: '.t2',
  lastHelp: '.phelp',
  lastValue: '.val.pres',
  /** La confirmation. */
  dialog: 'dialog.ask',
  askBlock: '.askblock',
  askName: '.askname',
  askLine: '.asklist li',
  askConfirm: 'button.pill',
  askCancel: 'button.ghost',
};

/** Les textes de la page que l'essai cherche : ils doivent être dans `fr.ts`, tels quels (blancs insécables mis à part). */
export const EXPECTED = {
  title: 'Confidentialité',
  /** Rien n'est coché d'avance : la phrase de l'en-tête. */
  intro: 'Rien n’est coché d’avance',
  erase: 'Effacer ce qui est coché',
  confirm: 'Effacer',
  cancel: 'Annuler',
  direct: 'Elles sont supprimées directement',
  /** Une seule trace cochée (le cas de l'essai) : le titre de la confirmation et sa phrase au singulier. */
  askTitleOne: 'Effacer cette trace ?',
  directOne: 'Elle est supprimée directement',
  chrome: 'Chrome',
  windows: 'Windows',
  /** Le nom d'une ligne de navigateur est celui du profil ; sa seconde ligne, la trace. */
  history: 'Historique de navigation',
  recent: 'Fichiers et dossiers récents',
  typed: 'Ce que tu as tapé dans Windows',
  clipboard: 'Presse-papiers',
  /** Le début de la phrase d'un navigateur ouvert (« Ferme Chrome pour effacer son historique… »). */
  running: 'pour effacer son historique',
  /** Le début du titre de la carte (il porte ensuite l'heure : « Dernier effacement, à 14 h 32 »). */
  lastTitle: 'Dernier effacement',
  /** La phrase d'une confirmation qui se referme parce que la liste a changé. */
  listChanged: 'La liste a changé',
  /** Les issues, en deux mots, telles que la carte « Dernier effacement » les écrit. */
  outcomes: {
    erased: 'Effacé',
    nothing: 'Rien à effacer',
    inUse: 'En cours d’utilisation',
    changed: 'Profil changé',
    partial: 'Effacé en partie',
    failed: 'Pas pu être effacé',
  },
};

/**
 * Lit la page Confidentialité telle qu'elle s'affiche. S'exécute dans la page : elle ne voit rien de ce
 * fichier, tout ce qu'il lui faut arrive par `dom`.
 */
export function readPrivacy(dom) {
  const text = (element) => (element?.innerText ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  // Les titres de bloc sont écrits en capitales par la feuille de style (`innerText` les rend ainsi) : on lit leur texte.
  const plain = (element) => (element?.textContent ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim();
  const rowOf = (item, block) => {
    const input = item.querySelector(dom.check);
    const row = item.querySelector('.row');
    return {
      block,
      title: text(item.querySelector(dom.rowTitle)),
      sub: text(item.querySelector(dom.rowSub)),
      outcome: text(item.querySelector(dom.rowOutcome)),
      value: text(item.querySelector(dom.rowValue)),
      kind: input !== null ? 'check' : row?.classList.contains('pinert') ? 'inert' : row?.classList.contains('pempty') ? 'empty' : 'autre',
      checked: input?.checked === true,
      label: (input?.getAttribute('aria-label') ?? '').replace(/[ \u00a0\u202f]+/g, ' ').trim(),
      text: text(item),
    };
  };
  const rows = [];
  const running = [];
  for (const card of document.querySelectorAll(dom.browserCard)) {
    const name = plain(card.querySelector(dom.browserName));
    const warning = text(card.querySelector(dom.running));
    if (warning !== '') {
      running.push({ block: name, text: warning });
    }
    for (const item of card.querySelectorAll(dom.cardRow)) {
      rows.push(rowOf(item, name));
    }
  }
  for (const section of document.querySelectorAll(dom.windowsBlock)) {
    // La section des navigateurs a aussi la classe `pblock` : ses lignes sont déjà lues, avec leur carte.
    if (section.querySelector(dom.browserCard) !== null) {
      continue;
    }
    const name = plain(section.querySelector(dom.blockTitle));
    for (const item of section.querySelectorAll(dom.windowsRow)) {
      rows.push(rowOf(item, name));
    }
  }
  const erase = document.querySelector(dom.erase);
  const dialog = document.querySelector(dom.dialog);
  return {
    title: text(document.querySelector(dom.title)),
    ready: document.querySelector(dom.page) !== null && document.querySelector(dom.loading) === null,
    busy: document.querySelector(dom.page)?.getAttribute('aria-busy') === 'true',
    intro: text(document.querySelector(dom.intro)),
    rows,
    running,
    // Toutes les phrases d'avertissement de la page (celles des navigateurs ouverts, et un refus comme « La liste a changé »).
    warnings: [...document.querySelectorAll(dom.running)].map((warning) => text(warning)),
    lastTitle: plain(document.querySelector(dom.lastTitle)),
    erase: erase === null ? null : { label: text(erase), inert: erase.getAttribute('aria-disabled') === 'true' },
    last: [...document.querySelectorAll(dom.last)].map((line) => ({
      block: text(line.querySelector(dom.lastBlock)),
      trace: text(line.querySelector(dom.lastTrace)),
      help: text(line.querySelector(dom.lastHelp)),
      outcome: text(line.querySelector(dom.lastValue)),
    })),
    dialog:
      dialog === null
        ? null
        : {
            open: dialog.open === true,
            title: plain(dialog.querySelector('h2')),
            blocks: [...dialog.querySelectorAll(dom.askBlock)].map((block) => ({
              name: plain(block.querySelector(dom.askName)),
              lines: [...block.querySelectorAll(dom.askLine)].map((line) => text(line)),
            })),
            text: text(dialog),
            focus: text(document.activeElement),
          },
  };
}
