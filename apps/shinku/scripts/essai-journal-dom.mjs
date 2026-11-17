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
  /** Les mots des totaux d'un grand vide fini (`fr.journal.clean.totals`) : l'un des deux au moins. */
  freed: 'libérés',
  binned: 'mis à la corbeille',
  leftovers: '3 restes mis à la corbeille',
};

/** La page Journal : son titre, son attente, ses lignes (phrase, date, `datetime`, lien), la présence du bouton « Oublier ». S'exécute dans la page. */
export function readJournal(dom) {
  const text = (element) => (element?.innerText ?? '').replace(/[   ]+/g, ' ').trim();
  return {
    title: text(document.querySelector(dom.title)),
    ready: document.querySelector(dom.page) !== null,
    loading: document.querySelector(dom.loading) !== null,
    rows: [...document.querySelectorAll(dom.row)].map((row) => ({
      sentence: text(row.querySelector(dom.sentence)),
      date: text(row.querySelector(dom.date)),
      /** L'attribut `datetime` du `<time>` de la ligne ; vide s'il n'y en a pas. */
      iso: row.querySelector(`${dom.date}[datetime]`)?.getAttribute('datetime') ?? '',
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

/**
 * La date d'une ligne : l'attribut `datetime` de son `<time>`, lisible, pas avant `from`, pas après `now` plus une
 * minute (millisecondes). Rend `null` si tout va bien, sinon la phrase d'échec. `what` complète la phrase (« du grand vide »).
 */
export function judgeDated(row, from, now, what = '') {
  const label = what === '' ? '' : ` ${what}`;
  const at = Date.parse(row.iso);
  if (row.iso === '' || Number.isNaN(at)) {
    return `la ligne${label} n’a pas de date lisible (datetime « ${row.iso} ») : « ${row.sentence} »`;
  }
  if (at < from) {
    return `la ligne${label} est datée ${row.iso}, avant le début de l’essai (${new Date(from).toISOString()}) : « ${row.sentence} »`;
  }
  if (at > now + 60_000) {
    return `la ligne${label} est datée ${row.iso}, dans le futur : « ${row.sentence} »`;
  }
  return null;
}

/**
 * Le grand vide de l'essai : la première ligne « Grand vide » (la plus récente, la liste est triée) doit être finie
 * (« Grand vide : »), avoir ses totaux, et une date entre `from` et `now`. Rend `null` ou la phrase d'échec.
 */
export function judgeClean(rows, expected, from, now) {
  const cleaned = rows.find((row) => row.sentence.toLocaleLowerCase('fr').includes('grand vide'));
  if (cleaned === undefined) {
    return 'le Journal n’a aucune ligne « Grand vide » après le grand vide';
  }
  if (!cleaned.sentence.startsWith(expected.cleaned)) {
    return `la ligne la plus récente du grand vide n’est pas finie (attendu « ${expected.cleaned} ») : « ${cleaned.sentence} »`;
  }
  const has = (word) => cleaned.sentence.toLocaleLowerCase('fr').includes(word.toLocaleLowerCase('fr'));
  if (!has(expected.freed) && !has(expected.binned)) {
    return `la ligne du grand vide n’a pas ses totaux (« ${expected.freed} » ou « ${expected.binned} ») : « ${cleaned.sentence} »`;
  }
  return judgeDated(cleaned, from, now, 'du grand vide');
}

/** Début du grand vide planté dans `journal.json` par l'essai du second lancement : le 1er janvier 2020 (UTC), en millisecondes. */
export const PLANTED_STARTED_MS = Date.UTC(2020, 0, 1);

/** Nombre de grands vides que le cœur garde et relit (`RUNS_KEPT` de `clean/journal.rs`). */
export const JOURNAL_RUNS_KEPT = 100;

/**
 * Le document `journal.json` de l'essai du second lancement : celui qu'on lui donne (le texte du fichier, une marque
 * d'ordre des octets en tête est permise), plus **un grand vide** « en cours » daté de 2020, en tête de liste (le plus
 * ancien), totaux à 0. Un second lancement qui écrirait dans le journal le marquerait « coupé ». Le texte donné n'est pas
 * changé. Si le journal garde déjà `JOURNAL_RUNS_KEPT` grands vides, les plus anciens sont laissés de côté : le cœur ne
 * relit que les derniers, et un grand vide planté au-delà ne serait pas vu. Rend le texte du nouveau document, ou `null`
 * si le texte donné n'est pas un journal lisible (ce n'est alors pas le nôtre : l'essai ne le touche pas).
 */
export function plantInterruptedRun(original) {
  let document;
  try {
    document = JSON.parse(original.replace(/^\ufeff/, ''));
  } catch {
    return null;
  }
  if (document === null || typeof document !== 'object' || Array.isArray(document) || !Array.isArray(document.runs)) {
    return null;
  }
  const planted = {
    startedMs: PLANTED_STARTED_MS,
    finishedMs: null,
    state: 'running',
    freedBytes: 0,
    recoverableBytes: 0,
    skippedFiles: 0,
  };
  return JSON.stringify({ ...document, runs: [planted, ...document.runs.slice(-(JOURNAL_RUNS_KEPT - 1))] }, null, 2);
}
