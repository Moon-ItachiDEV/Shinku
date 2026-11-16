// Les boîtes de dialogue modales (`dialog.ask`) : s'ouvrir, et rendre le focus
// à ce qui les a ouvertes quand elles se referment.
//
// Une boîte est retirée de la page par lit-html sans passer par `close()` :
// le navigateur ne rend alors pas le focus, et une personne au clavier
// repartirait du haut de la page. Ce module retient l'élément qui l'avait
// quand la boîte s'ouvre, et le lui rend quand la boîte disparaît.

/** Ce qui avait le focus à l'ouverture de la boîte en cours. */
let opener: HTMLElement | null = null;

/** Rend le focus à ce qui avait ouvert la boîte, si c'est encore dans la page et que personne d'autre ne l'a pris. */
function giveFocusBack(): void {
  const target = opener;
  opener = null;
  if (target === null) {
    return;
  }
  // Après le dessin qui retire la boîte : c'est alors seulement que le focus est tombé sur la page.
  queueMicrotask(() => {
    const active = document.activeElement;
    if (target.isConnected && (active === null || active === document.body)) {
      target.focus();
    }
  });
}

/**
 * La fonction de `ref(...)` d'une boîte `dialog.ask` : l'ouvre en mode modal (le
 * reste de la fenêtre ne répond plus, Échap la referme) et lui rend le focus
 * à sa disparition. À son premier dessin elle n'est pas encore dans la page :
 * l'ouverture attend qu'elle y soit. lit-html rappelle la fonction avec
 * `undefined` quand la boîte est retirée.
 */
export function openModal(element: Element | undefined): void {
  if (element === undefined) {
    giveFocusBack();
    return;
  }
  if (!(element instanceof HTMLDialogElement)) {
    return;
  }
  // Ce qui a le focus maintenant, avant que la boîte ne le prenne.
  const active = document.activeElement;
  queueMicrotask(() => {
    if (element.isConnected && !element.open) {
      opener = active instanceof HTMLElement && active !== document.body ? active : null;
      element.showModal();
    }
  });
}
