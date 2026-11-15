// Les raccourcis du moteur web qui n'ont pas leur place dans une application :
// actualiser la page, chercher dans la page, imprimer, revenir en arrière…
//
// WebView2 les exécute si la page ne s'y oppose pas. La fenêtre de Shinku s'y
// oppose donc, sans toucher aux raccourcis d'édition (copier, coller, tout
// sélectionner, annuler).

/** Ce qu'on regarde d'une frappe au clavier. */
export interface Keystroke {
  key: string;
  ctrlKey: boolean;
  altKey: boolean;
}

/** Touches de fonction du navigateur, avec ou sans Ctrl et Maj. */
const BROWSER_KEYS: ReadonlySet<string> = new Set([
  'F3', // chercher le suivant
  'F5', // actualiser
  'F7', // navigation au curseur
  'F12', // outils de développement
  'BrowserBack',
  'BrowserForward',
  'BrowserRefresh',
  'BrowserStop',
  'BrowserSearch',
  'BrowserFavorites',
  'BrowserHome',
]);

/** Lettres qui, avec Ctrl, commandent le navigateur. */
const BROWSER_LETTERS: ReadonlySet<string> = new Set([
  'f', // chercher dans la page
  'g', // chercher le suivant
  'p', // imprimer
  'r', // actualiser
  's', // enregistrer la page
  'o', // ouvrir un fichier
  'u', // code source de la page
  'j', // téléchargements
  'h', // historique
  'n', // nouvelle fenêtre
  'e', // recherche
  'l', // barre d'adresse
  'd', // favori
]);

/** Vrai si cette frappe commande le navigateur plutôt que l'application. */
export function isBrowserShortcut(stroke: Keystroke): boolean {
  if (BROWSER_KEYS.has(stroke.key)) {
    return true;
  }
  if (stroke.altKey) {
    // Page précédente, page suivante, page d'accueil.
    return stroke.key === 'ArrowLeft' || stroke.key === 'ArrowRight' || stroke.key === 'Home';
  }
  return stroke.ctrlKey && BROWSER_LETTERS.has(stroke.key.toLowerCase());
}

/** Neutralise les raccourcis et le menu du navigateur dans cette page. */
export function silenceBrowser(target: Window): void {
  target.addEventListener(
    'keydown',
    (event) => {
      if (isBrowserShortcut(event)) {
        event.preventDefault();
      }
    },
    { capture: true },
  );
  // Le menu du moteur web (Actualiser, Enregistrer sous…). Copier un chemin
  // sélectionné reste possible au clavier.
  target.addEventListener('contextmenu', (event) => event.preventDefault());
}
