// Les boîtes modales du mode jeu : ouverture, Échap, clic hors de la fenêtre,
// et focus rendu au bouton qui les avait ouvertes.
//
// Sans navigateur : de faux éléments qui notent ce qu'on leur demande.

import assert from 'node:assert/strict';
import { beforeEach, test } from 'node:test';

class FakeElement {
  isConnected = true;
  focused = 0;
  focus(): void {
    this.focused += 1;
    environment.active = this;
  }
}
class FakeButton extends FakeElement {}
class FakeDialog extends FakeElement {
  open = false;
  opened = 0;
  showModal(): void {
    this.open = true;
    this.opened += 1;
    // Une boîte modale prend le focus.
    environment.active = this;
  }
}

const body = new FakeElement();
const environment: { active: FakeElement | null } = { active: body };

Object.assign(globalThis, {
  HTMLElement: FakeElement,
  HTMLDialogElement: FakeDialog,
  document: {
    body,
    // lit-html (importé par la page du sélecteur) en a besoin à son import.
    createTreeWalker: () => ({}),
    get activeElement() {
      return environment.active;
    },
  },
});

const { openModal } = await import('../src/views/modal.ts');
const { pickerHandlers } = await import('../src/views/game.ts');

const flush = (): Promise<void> => new Promise((resolve) => setImmediate(resolve));
const as = (element: FakeElement): Element => element as unknown as Element;

beforeEach(() => {
  environment.active = body;
  // Le focus de la boîte précédente ne doit pas fuir d'un test à l'autre.
  openModal(undefined);
});

test('la boîte s’ouvre en modal, une seule fois, quand elle est dans la page', async () => {
  const dialog = new FakeDialog();
  openModal(as(dialog));
  assert.equal(dialog.open, false, 'pas avant qu’elle soit dans la page : l’ouverture attend');
  await flush();
  assert.equal(dialog.open, true);
  // Un nouveau dessin avec la même boîte ne la rouvre pas.
  openModal(as(dialog));
  await flush();
  assert.equal(dialog.opened, 1);
  // Une boîte qui n'est plus dans la page ne s'ouvre pas.
  const gone = new FakeDialog();
  gone.isConnected = false;
  openModal(as(gone));
  await flush();
  assert.equal(gone.open, false);
});

test('à la disparition de la boîte, le focus revient au bouton qui l’avait ouverte', async () => {
  const opener = new FakeButton();
  environment.active = opener;
  const dialog = new FakeDialog();
  openModal(as(dialog));
  await flush();
  assert.equal(environment.active, dialog);

  // lit-html retire la boîte sans la fermer : le navigateur ne rend pas le focus, il tombe sur la page.
  environment.active = body;
  openModal(undefined);
  await flush();
  assert.equal(opener.focused, 1);
  assert.equal(environment.active, opener);
  // Et une seule fois.
  openModal(undefined);
  await flush();
  assert.equal(opener.focused, 1);
});

test('si le bouton d’origine a disparu, rien n’est focalisé et rien ne casse', async () => {
  const opener = new FakeButton();
  environment.active = opener;
  const dialog = new FakeDialog();
  openModal(as(dialog));
  await flush();
  opener.isConnected = false;
  environment.active = body;
  openModal(undefined);
  await flush();
  assert.equal(opener.focused, 0);
  assert.equal(environment.active, body);
});

test('si la personne a déjà mis le focus ailleurs, il n’est pas repris', async () => {
  const opener = new FakeButton();
  environment.active = opener;
  const dialog = new FakeDialog();
  openModal(as(dialog));
  await flush();
  const elsewhere = new FakeButton();
  environment.active = elsewhere;
  openModal(undefined);
  await flush();
  assert.equal(opener.focused, 0);
  assert.equal(environment.active, elsewhere);
});

test('Échap et le clic hors de la fenêtre ferment le sélecteur ; un clic dedans non', () => {
  let closed = 0;
  const { onCancel, onClick } = pickerHandlers({ closePicker: () => (closed += 1) });

  // Échap : l'application referme (le navigateur ne doit pas le faire lui-même).
  let prevented = 0;
  onCancel({ preventDefault: () => (prevented += 1) } as unknown as Event);
  assert.equal(prevented, 1);
  assert.equal(closed, 1);

  // Un clic sur le voile tombe sur le dialogue lui-même.
  const dialog = {};
  onClick({ target: dialog, currentTarget: dialog } as unknown as Event);
  assert.equal(closed, 2);
  // Un clic sur un bouton de la boîte ne la ferme pas.
  onClick({ target: {}, currentTarget: dialog } as unknown as Event);
  assert.equal(closed, 2);
});
