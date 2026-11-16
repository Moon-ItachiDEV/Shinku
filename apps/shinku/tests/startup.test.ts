// Ce que la page Démarrage tire d'une vue du cœur : les comptes de l'en-tête,
// les morceaux et les pastilles d'une ligne, les gestes optimistes, les textes
// de retour, et ce que les gabarits écrivent.

import assert from 'node:assert/strict';
import { test } from 'node:test';

import type { CategorySummary, SessionView, StartupEntry, StartupView } from '../src/backend/types.ts';
import { NBSP } from '../src/format.ts';
import { fr } from '../src/i18n/fr.ts';
import {
  detailId,
  detailParts,
  entriesOf,
  entryById,
  controlKey,
  entryKeys,
  focusAfter,
  focusTarget,
  initialOf,
  labelRanks,
  machineChip,
  MAX_TAGS,
  rowEnd,
  rowTags,
  sayDone,
  sayRefused,
  sayStale,
  showsScope,
  startupCounts,
  startupFocus,
  startupSummary,
  TITLE_KEY,
  withEntry,
  withEntryEnabled,
} from '../src/model/startup.ts';
import { readFileSync } from 'node:fs';

import { fakeDocument, htmlOf, textOf } from './template.ts';

fakeDocument();
const { startupView } = await import('../src/views/startup.ts');
const { homeView } = await import('../src/views/home.ts');
const { initialState } = await import('../src/state.ts');

const at = (year: number, month: number, day: number): number => new Date(year, month - 1, day, 12).getTime();
const NOW = at(2026, 11, 15);
const DAY = 24 * 3600 * 1000;

function entry(parts: Partial<StartupEntry> = {}): StartupEntry {
  return {
    id: 'ur-0123456789abcdef',
    name: 'Discord',
    alias: null,
    publisher: 'Discord Inc.',
    scope: 'user',
    origin: 'registry',
    state: 'enabled',
    disabledAt: null,
    mark: null,
    target: 'found',
    host: null,
    hint: null,
    own: false,
    adjustable: true,
    ...parts,
  };
}

function view(entries: StartupEntry[], unreadable: StartupView['unreadable'] = []): StartupView {
  return { entries, unreadable };
}

// ── L'en-tête ──────────────────────────────────────────────────────────────

test('le titre compte toutes les entrées listées, coupées comprises, et s’accorde', () => {
  assert.equal(fr.startup.title(0), 'Aucune app inscrite ici');
  assert.equal(fr.startup.title(1), `1${NBSP}app inscrite au démarrage`);
  assert.equal(fr.startup.title(12), `12${NBSP}apps inscrites au démarrage`);
  const counts = startupCounts(
    view([
      entry({ id: 'ur-1' }),
      entry({ id: 'ur-2', state: 'disabled' }),
      entry({ id: 'mr-3', scope: 'machine', adjustable: false }),
      entry({ id: 'ur-4', state: 'unknown', adjustable: false }),
    ]),
  );
  // Une entrée que Shinku ne sait pas lire est listée, mais n'est ni active ni coupée.
  assert.deepEqual(counts, { total: 4, running: 2, cut: 1 });
});

test('la phrase dit « 1 se lance », « 1 est coupée », « aucune n’est coupée »', () => {
  const sentence = (running: number, cut: number): string | null => startupSummary({ total: running + cut, running, cut });
  assert.equal(sentence(1, 0), `1${NBSP}se lance à l’ouverture de ta session, aucune n’est coupée.`);
  assert.equal(sentence(1, 1), `1${NBSP}se lance à l’ouverture de ta session, 1${NBSP}est coupée.`);
  assert.equal(sentence(9, 2), `9${NBSP}se lancent à l’ouverture de ta session, 2${NBSP}sont coupées.`);
  assert.equal(sentence(0, 3), `Aucune ne se lance à l’ouverture de ta session, 3${NBSP}sont coupées.`);
  assert.equal(sentence(1200, 0), `1 200${NBSP}se lancent à l’ouverture de ta session, aucune n’est coupée.`);
  // Rien à dire : aucune entrée, ou seulement des entrées illisibles.
  assert.equal(sentence(0, 0), null);
});

// ── Les cartes ─────────────────────────────────────────────────────────────

test('une portée vide et lisible n’a pas de carte ; illisible, elle en a une', () => {
  const user = entry({ id: 'ur-1' });
  assert.equal(showsScope(view([user]), 'user'), true);
  assert.equal(showsScope(view([user]), 'machine'), false);
  assert.equal(showsScope(view([user], ['machine']), 'machine'), true);
  assert.equal(showsScope(view([], ['user']), 'user'), true);
  assert.equal(showsScope(view([]), 'user'), false);
  assert.deepEqual(
    entriesOf(view([user, entry({ id: 'mr-2', scope: 'machine' })]), 'machine').map((found) => found.id),
    ['mr-2'],
  );
});

// ── Une ligne ──────────────────────────────────────────────────────────────

test('l’initiale est le premier caractère alphanumérique en majuscule ; « # » s’il n’y en a aucun', () => {
  assert.equal(initialOf('Discord'), 'D');
  assert.equal(initialOf('discord'), 'D');
  assert.equal(initialOf('  épique'), 'É');
  assert.equal(initialOf('çà'), 'Ç');
  assert.equal(initialOf('Ɛlan'), 'Ɛ');
  assert.equal(initialOf('7-Zip'), '7');
  assert.equal(initialOf('1Password'), '1');
  assert.equal(initialOf('_outil'), 'O');
  assert.equal(initialOf('[x]'), 'X');
  assert.equal(initialOf('\u{1F600} smile'), 'S');
  assert.equal(initialOf('\u{1F600}'), '#');
  assert.equal(initialOf('Notepad++'), 'N');
  assert.equal(initialOf(''), '#');
  assert.equal(initialOf('   '), '#');
  // Une lettre hors du plan de base reste entière, sans demi-paire.
  assert.equal(initialOf('\u{1D49C}rt'), '\u{1D49C}');
});

test('la seconde ligne dit l’éditeur, le porteur, le nom d’inscription et le raccourci, dans cet ordre', () => {
  assert.deepEqual(detailParts(entry()), ['Discord Inc.']);
  assert.deepEqual(
    detailParts(entry({ alias: 'OneDrive', publisher: 'Microsoft Corporation' })),
    ['Microsoft Corporation', `inscrite sous «${NBSP}OneDrive${NBSP}»`],
  );
  assert.deepEqual(
    detailParts(entry({ origin: 'folder', alias: 'Adobe', publisher: 'Adobe Inc.' })),
    ['Adobe Inc.', `inscrite sous «${NBSP}Adobe${NBSP}»`, 'raccourci du dossier Démarrage'],
  );
  // Le fichier est là sans société : on le dit, sans rien inventer.
  assert.deepEqual(detailParts(entry({ publisher: null })), ['Éditeur inconnu']);
  // Un programme porteur ne donne ni éditeur ni « Éditeur inconnu » : on dit qui lance.
  assert.deepEqual(detailParts(entry({ publisher: null, target: 'hosted', host: 'rundll32.exe' })), [
    'Lancée par rundll32.exe',
  ]);
  // Une cible introuvable ou illisible n'a rien à dire de son éditeur.
  assert.deepEqual(detailParts(entry({ publisher: null, target: 'missing', origin: 'folder' })), [
    'raccourci du dossier Démarrage',
  ]);
  assert.deepEqual(detailParts(entry({ publisher: null, target: 'unknown' })), []);
});

test('les pastilles suivent la priorité : désinstallée, remise, coupée, synchronise ; deux au plus', () => {
  const cut = NOW - 5 * DAY;
  assert.deepEqual(rowTags(entry(), NOW), []);
  assert.deepEqual(rowTags(entry({ target: 'missing' }), NOW), ['Semble désinstallée']);
  assert.deepEqual(rowTags(entry({ hint: 'sync' }), NOW), ['Synchronise tes fichiers']);
  assert.deepEqual(rowTags(entry({ mark: { kind: 'cameBack', cut } }), NOW), [
    `S’est remise toute seule depuis le 10${NBSP}nov.`,
  ]);
  assert.deepEqual(rowTags(entry({ state: 'disabled', mark: { kind: 'cutByShinku', at: NOW - 3 * DAY } }), NOW), [
    `Coupée par Shinku le 12${NBSP}nov.`,
  ]);
  // Coupée sans marque : la date de Windows si elle est lue, sinon rien.
  assert.deepEqual(rowTags(entry({ state: 'disabled', disabledAt: at(2025, 11, 2) }), NOW), [
    `Coupée le 2${NBSP}nov. 2025`,
  ]);
  assert.deepEqual(rowTags(entry({ state: 'disabled' }), NOW), ['Coupée']);
  // « Synchronise tes fichiers » ne se dit que d'une app active.
  assert.deepEqual(rowTags(entry({ state: 'disabled', hint: 'sync' }), NOW), ['Coupée']);
  // Trois pastilles possibles : les deux premières seulement, la synchronisation tombe.
  const crowded = entry({ target: 'missing', hint: 'sync', mark: { kind: 'cameBack', cut } });
  assert.equal(MAX_TAGS, 2);
  assert.deepEqual(rowTags(crowded, NOW), ['Semble désinstallée', `S’est remise toute seule depuis le 10${NBSP}nov.`]);
  // Désinstallée et active avec synchronisation : les deux tiennent.
  assert.deepEqual(rowTags(entry({ target: 'missing', hint: 'sync' }), NOW), [
    'Semble désinstallée',
    'Synchronise tes fichiers',
  ]);
  // Une entrée coupée et désinstallée : désinstallée d'abord.
  assert.deepEqual(rowTags(entry({ target: 'missing', state: 'disabled' }), NOW), ['Semble désinstallée', 'Coupée']);
});

test('la fin de la ligne : un interrupteur, ou la raison de son absence', () => {
  assert.equal(rowEnd(entry()), 'switch');
  assert.equal(rowEnd(entry({ state: 'disabled' })), 'switch');
  // Une entrée « missing » se règle comme les autres.
  assert.equal(rowEnd(entry({ target: 'missing' })), 'switch');
  assert.equal(rowEnd(entry({ own: true, adjustable: false })), 'own');
  // Un état que Shinku ne sait pas lire, et une entrée lue qu'il ne peut pas régler, ne se disent pas pareil.
  assert.equal(rowEnd(entry({ state: 'unknown', adjustable: false })), 'unknown');
  assert.equal(rowEnd(entry({ adjustable: false })), 'unadjustable');
  assert.equal(rowEnd(entry({ state: 'disabled', adjustable: false })), 'unadjustable');
  // Le PC : jamais d'interrupteur, même pour une entrée que le cœur dirait réglable.
  assert.equal(rowEnd(entry({ scope: 'machine', adjustable: false })), 'machine');
  assert.equal(rowEnd(entry({ scope: 'machine', adjustable: true })), 'machine');
  // Une seule pastille d'état sur le PC : « Active », « Coupée le … » ou « Coupée ».
  assert.equal(machineChip(entry({ scope: 'machine' }), NOW), 'Active');
  assert.equal(machineChip(entry({ scope: 'machine', state: 'disabled' }), NOW), 'Coupée');
  assert.equal(
    machineChip(entry({ scope: 'machine', state: 'disabled', disabledAt: at(2025, 11, 2) }), NOW),
    `Coupée le 2${NBSP}nov. 2025`,
  );
  assert.equal(machineChip(entry({ scope: 'machine', state: 'unknown' }), NOW), null);
  // Elle ne se répète pas parmi les pastilles de gauche ; les autres y restent.
  const cutMachine = entry({ scope: 'machine', state: 'disabled', disabledAt: at(2025, 11, 2), target: 'missing' });
  assert.deepEqual(rowTags(cutMachine, NOW), ['Semble désinstallée']);
  assert.deepEqual(rowTags(entry({ scope: 'machine', hint: 'sync' }), NOW), ['Synchronise tes fichiers']);
});

test('au plus deux pastilles par ligne, pastille de la fin de ligne comprise', () => {
  const cut = NOW - 5 * DAY;
  // Le PC : la pastille de fin compte pour une, il en reste une à gauche.
  const crowdedMachine = entry({ scope: 'machine', adjustable: false, target: 'missing', hint: 'sync' });
  assert.deepEqual(rowTags(crowdedMachine, NOW), ['Semble désinstallée']);
  assert.equal(machineChip(crowdedMachine, NOW), 'Active');
  // Une entrée du PC dont l'état n'est pas lu n'a pas de pastille de fin : deux à gauche.
  const unreadMachine = entry({ scope: 'machine', adjustable: false, state: 'unknown', target: 'missing', hint: 'sync' });
  assert.deepEqual(rowTags(unreadMachine, NOW), ['Semble désinstallée']);
  // L'entrée de Shinku : aucune pastille d'état à gauche ; coupée, « Coupée dans Windows » est à la fin et compte pour une.
  const own = entry({ own: true, adjustable: false, mark: { kind: 'cameBack', cut } });
  assert.deepEqual(rowTags(own, NOW), []);
  const ownCut = entry({ own: true, adjustable: false, state: 'disabled', target: 'missing', hint: 'sync' });
  assert.deepEqual(rowTags(ownCut, NOW), ['Semble désinstallée']);
  assert.deepEqual(rowTags(entry({ own: true, adjustable: false, state: 'disabled' }), NOW), []);
  // Une entrée dont l'état n'est pas lu : pas de pastille d'état, ni « Coupée » ni « Active ».
  const unknown = entry({ state: 'unknown', adjustable: false, mark: { kind: 'cutByShinku', at: cut }, hint: 'sync' });
  assert.deepEqual(rowTags(unknown, NOW), []);
  assert.deepEqual(rowTags(entry({ state: 'unknown', adjustable: false, target: 'missing' }), NOW), ['Semble désinstallée']);
  // Sur toutes les combinaisons : jamais plus de deux pastilles, fin de ligne comprise.
  for (const scope of ['user', 'machine'] as const) {
    for (const state of ['enabled', 'disabled', 'unknown'] as const) {
      for (const own of [false, true]) {
        for (const target of ['found', 'missing'] as const) {
          for (const hint of [null, 'sync'] as const) {
            for (const mark of [null, { kind: 'cameBack', cut }, { kind: 'cutByShinku', at: cut }] as const) {
              const found = entry({ scope, state, own, target, hint, mark, adjustable: false });
              const ends = (scope === 'machine' && machineChip(found, NOW) !== null) || (scope === 'user' && own && state === 'disabled') ? 1 : 0;
              assert.ok(rowTags(found, NOW).length + ends <= MAX_TAGS, JSON.stringify(found));
            }
          }
        }
      }
    }
  }
});

test('deux lignes de même nom et de même alias dans une carte se distinguent par leur rang', () => {
  const list = [
    entry({ id: 'a', name: 'Java', alias: 'java.exe' }),
    entry({ id: 'b', name: 'Java', alias: 'java.exe' }),
    entry({ id: 'c', name: 'Java', alias: null }),
    entry({ id: 'd', name: 'Java', alias: 'java.exe' }),
    entry({ id: 'e', name: 'Autre' }),
  ];
  assert.deepEqual([...labelRanks(list)], [['a', 1], ['b', 2], ['c', 1], ['d', 3], ['e', 1]]);
  assert.equal(detailId('ur-1'), 'startup-ur-1-d');
});

test('le focus d’une ligne disparue va à la suivante, sinon à la précédente, sinon au titre', () => {
  const before = ['k1', 'k2', 'k3'];
  assert.equal(focusAfter(before, ['k1', 'k2', 'k3'], 'k2'), 'k2', 'la ligne est encore là');
  assert.equal(focusAfter(before, ['k1', 'k3'], 'k2'), 'k3', 'la suivante');
  assert.equal(focusAfter(before, ['k1'], 'k2'), 'k1', 'plus de suivante : la précédente');
  assert.equal(focusAfter(before, ['k1'], 'k3'), 'k1');
  assert.equal(focusAfter(before, [], 'k2'), TITLE_KEY, 'plus aucune ligne : le titre');
  assert.equal(focusAfter(before, ['k1'], 'inconnue'), TITLE_KEY);
  const listed = view([
    entry({ id: 'ur-1' }),
    entry({ id: 'ur-own', own: true, adjustable: false }),
    entry({ id: 'mr-1', scope: 'machine', adjustable: false }),
    entry({ id: 'ur-2' }),
  ]);
  // Seules les lignes à interrupteur ont une clé de focus.
  assert.deepEqual(entryKeys(listed), [startupFocus.entry('ur-1'), startupFocus.entry('ur-2')]);
});

test('une phrase que la vue démentirait est périmée', () => {
  const cutSteam = sayDone(entry({ id: 'ur-steam', name: 'Steam' }), false);
  const stillCut = view([entry({ id: 'ur-steam', state: 'disabled' })]);
  const backOn = view([entry({ id: 'ur-steam' })]);
  assert.equal(sayStale(cutSteam, stillCut), false);
  assert.equal(sayStale(cutSteam, backOn), true, 'la coupure annoncée n’est plus vraie');
  assert.equal(sayStale(cutSteam, view([])), true, 'l’entrée n’est plus listée');
  const restored = sayDone(entry({ id: 'ur-steam', name: 'Steam', state: 'disabled' }), true);
  assert.equal(sayStale(restored, backOn), false);
  assert.equal(sayStale(restored, stillCut), true);
  // Une phrase sans « Annuler » n'annonce aucun état.
  assert.equal(sayStale({ text: 'x', undo: null }, view([])), false);
});

// ── Les gestes optimistes ──────────────────────────────────────────────────

test('couper une entrée la montre coupée par Shinku tout de suite ; la remettre efface sa marque', () => {
  const steam = entry({ id: 'ur-steam', name: 'Steam' });
  const other = entry({ id: 'ur-other', name: 'Autre' });
  const cutView = withEntryEnabled(view([steam, other]), 'ur-steam', false, NOW);
  assert.deepEqual(entryById(cutView, 'ur-steam'), {
    ...steam,
    state: 'disabled',
    disabledAt: NOW,
    mark: { kind: 'cutByShinku', at: NOW },
  });
  assert.equal(entryById(cutView, 'ur-other'), other, 'les autres lignes ne bougent pas');

  const back = withEntryEnabled(cutView, 'ur-steam', true, NOW + 1000);
  assert.deepEqual(entryById(back, 'ur-steam'), steam);
  // Une entrée remise après s'être remise seule perd aussi sa marque.
  const cameBack = entry({ id: 'ur-s', mark: { kind: 'cameBack', cut: NOW - DAY } });
  assert.equal(entryById(withEntryEnabled(view([cameBack]), 'ur-s', true, NOW), 'ur-s')?.mark, null);
  // Un identifiant inconnu ne change rien.
  assert.deepEqual(withEntryEnabled(view([steam]), 'ur-absent', false, NOW), view([steam]));
});

test('un geste refusé remet la ligne comme elle était, avec sa marque d’avant', () => {
  const spotify = entry({ id: 'ur-spotify', mark: { kind: 'cameBack', cut: NOW - 5 * DAY } });
  const optimistic = withEntryEnabled(view([spotify]), 'ur-spotify', false, NOW);
  assert.notDeepEqual(optimistic, view([spotify]));
  assert.deepEqual(withEntry(optimistic, spotify), view([spotify]));
});

// ── La zone de retour ──────────────────────────────────────────────────────

test('la phrase de retour dit ce qui s’est passé, et « Annuler » fait le geste inverse', () => {
  const cut = sayDone(entry({ name: 'Steam', id: 'ur-steam' }), false);
  assert.equal(
    cut.text,
    `Steam est coupée. Si elle est ouverte, elle le reste${NBSP}: elle ne se lancera plus toute seule à ta prochaine ouverture de session.`,
  );
  assert.deepEqual(cut.undo, { id: 'ur-steam', enabled: true });

  const restored = sayDone(entry({ name: 'Steam', id: 'ur-steam', state: 'disabled' }), true);
  assert.equal(restored.text, 'Steam se lancera de nouveau à l’ouverture de ta session.');
  assert.deepEqual(restored.undo, { id: 'ur-steam', enabled: false });

  // Une app de synchronisation coupée : la phrase le dit aussi.
  const sync = sayDone(entry({ name: 'OneDrive', hint: 'sync' }), false);
  assert.ok(sync.text.endsWith(' Tes fichiers ne se synchroniseront plus tant que tu ne l’ouvres pas.'), sync.text);
  assert.ok(sync.text.startsWith(cut.text.replace('Steam', 'OneDrive')));
  // La remise ne parle pas de synchronisation.
  assert.doesNotMatch(sayDone(entry({ hint: 'sync', state: 'disabled' }), true).text, /synchronis/);
});

test('chaque refus a sa phrase ; « busy » n’en a pas', () => {
  const refused = (reason: string) => sayRefused({ kind: 'startupRefused', reason });
  assert.deepEqual(refused('failed'), { text: 'Le changement n’a pas pu être fait.', undo: null });
  assert.deepEqual(refused('unknownEntry'), { text: 'Cette entrée n’existe plus.', undo: null });
  assert.deepEqual(refused('notAdjustable'), { text: 'Cette entrée ne se règle pas ici.', undo: null });
  assert.equal(refused('busy'), null);
  // Toute autre panne est un changement qui n'a pas pu être fait.
  assert.equal(sayRefused(new Error('ipc'))?.text, fr.startup.say.failed);
  assert.equal(sayRefused({ kind: 'settingsNotSaved', reason: 'x' })?.text, fr.startup.say.failed);
  assert.equal(sayRefused(null)?.text, fr.startup.say.failed);
});

test('les clés de focus sont celles des lignes', () => {
  assert.equal(startupFocus.entry('ur-1'), 'startup:ur-1');
  assert.notEqual(startupFocus.entry('ur-1'), startupFocus.entry('ur-2'));
});

// ── Ce que les gabarits écrivent ───────────────────────────────────────────

const noActions = new Proxy({}, { get: () => () => {} }) as import('../src/state.ts').Actions;

function page(startup: StartupView | null, parts: Partial<import('../src/state.ts').AppState> = {}): string {
  const state = { ...initialState(NOW), route: 'startup' as const, startup, ...parts };
  return htmlOf(startupView(state, noActions));
}

const FEW = view(
  [
    entry({ id: 'ur-discord', name: 'Discord' }),
    entry({ id: 'ur-onedrive', name: 'Microsoft OneDrive', alias: 'OneDrive', hint: 'sync' }),
    entry({ id: 'ur-onedrive2', name: 'Microsoft OneDrive', alias: 'OneDrive Entreprise', hint: 'sync' }),
    entry({ id: 'ur-shinku', name: 'Shinku', own: true, adjustable: false }),
    entry({ id: 'ur-old', name: 'Ancien outil', state: 'unknown', adjustable: false }),
    entry({ id: 'mr-sec', name: 'Sécurité Windows', scope: 'machine', adjustable: false }),
    entry({ id: 'mr-cut', name: 'Logitech', scope: 'machine', adjustable: false, state: 'disabled' }),
  ],
  [],
);

test('la page n’écrit aucun attribut style, et sa zone de retour est une région vivante polie', () => {
  const written = page(FEW);
  assert.doesNotMatch(written, /\sstyle\s*=/);
  assert.match(written, /class="pzone"/);
  // La région vivante est permanente et ne porte que la phrase.
  assert.match(written, /class="sr" aria-live="polite"><\/div>/);
  // Vide au repos : aucune ligne de retour, aucun bouton « Annuler ».
  assert.doesNotMatch(written, /class="psay"/);
  assert.ok(!written.includes('Annuler'));
});

test('seules les lignes réglables du compte ont un interrupteur, nommé par ce qu’il règle', () => {
  const written = page(FEW);
  // Discord et les deux OneDrive : trois interrupteurs. Shinku, l'entrée illisible et le PC n'en ont pas.
  assert.equal(written.match(/role="switch"/g)?.length, 3);
  assert.ok(written.includes(`aria-label=Lancer Discord à l’ouverture de session`));
  // Deux lignes de même nom se distinguent par leur nom d'inscription.
  assert.ok(written.includes('aria-label=Lancer Microsoft OneDrive à l’ouverture de session (OneDrive)'));
  assert.ok(written.includes('aria-label=Lancer Microsoft OneDrive à l’ouverture de session (OneDrive Entreprise)'));
  // Les clés de focus sont celles du modèle.
  assert.ok(written.includes(`data-fk=${startupFocus.entry('ur-discord')}`));
  // L'entrée de Shinku renvoie au Mode jeu, l'entrée illisible à Windows.
  assert.ok(written.includes('Réglé par le Mode jeu'));
  assert.ok(written.includes('Voir le Mode jeu'));
  assert.ok(written.includes('Shinku ne sait pas lire son réglage'));
  assert.ok(written.includes('Ouvrir dans Windows'));
  // Deux OneDrive : la seconde ligne de chaque interrupteur est sa description.
  assert.ok(written.includes(`aria-describedby=${detailId('ur-onedrive')}`));
  assert.ok(written.includes(`id=${detailId('ur-onedrive')}`));
  // Sans seconde ligne, pas de description à citer.
  const bare = page(view([entry({ id: 'ur-bare', publisher: null, target: 'unknown' })]));
  assert.ok(!bare.includes(`aria-describedby=${detailId('ur-bare')}`));
  // Même nom, même alias : le rang les distingue.
  const twins = page(view([entry({ id: 'ur-j1', name: 'Java', alias: 'j' }), entry({ id: 'ur-j2', name: 'Java', alias: 'j' })]));
  assert.ok(twins.includes('aria-label=Lancer Java à l’ouverture de session (j)'));
  assert.ok(twins.includes('aria-label=Lancer Java à l’ouverture de session (j) (2)'));
});

test('une ligne a une clé ; le titre peut recevoir le focus sans être un contrôle', () => {
  const written = page(FEW);
  assert.ok(written.includes(`<h1 tabindex="-1" data-fk=${TITLE_KEY}>`));
  // La liste est clée par l'identifiant : un interrupteur ne change pas de ligne quand la liste change.
  const entries = FEW.entries.filter((found) => found.scope === 'user');
  const keyed = startupView({ ...initialState(NOW), route: 'startup', startup: view(entries) }, noActions);
  assert.ok(JSON.stringify(keyed, (_key, value: unknown) => (typeof value === 'function' ? value.toString() : value)).includes('entry.id'));
});

test('l’entrée de Shinku et une entrée illisible n’ont pas de pastille d’état à gauche', () => {
  const own = page(view([entry({ id: 'ur-own', name: 'Shinku', own: true, adjustable: false, state: 'disabled', disabledAt: NOW, mark: { kind: 'cutByShinku', at: NOW } })]));
  // « Coupée dans Windows » à la fin, une seule fois, aucune « Coupée par Shinku ».
  assert.equal(own.match(/>Coupée dans Windows</g)?.length, 1);
  assert.ok(!own.includes('Coupée par Shinku'));
  const ownOn = page(view([entry({ id: 'ur-own', name: 'Shinku', own: true, adjustable: false })]));
  assert.ok(!ownOn.includes('Coupée dans Windows'));
  const unknown = page(view([entry({ id: 'ur-old', state: 'unknown', adjustable: false, mark: { kind: 'cameBack', cut: NOW - DAY } })]));
  assert.ok(!unknown.includes('S’est remise toute seule'));
  assert.ok(unknown.includes('Shinku ne sait pas lire son réglage'));
  // Lue mais non réglable : une autre raison.
  const fixed = page(view([entry({ id: 'ur-fixed', adjustable: false })]));
  assert.ok(fixed.includes('Shinku ne peut pas la régler ici'));
  assert.ok(!fixed.includes('Shinku ne sait pas lire son réglage'));
  assert.ok(fixed.includes('Ouvrir dans Windows'));
});

test('la phrase de décompte est omise quand aucune entrée n’est lue', () => {
  const unreadable = textOf(
    startupView({ ...initialState(NOW), startup: view([entry({ id: 'ur-1', state: 'unknown', adjustable: false }), entry({ id: 'ur-2', state: 'unknown', adjustable: false })]) }, noActions),
  );
  assert.ok(unreadable.includes(`2${NBSP}apps inscrites au démarrage`));
  assert.ok(!unreadable.includes('à l’ouverture de ta session'), unreadable);
  assert.ok(!unreadable.includes('aucune n’est coupée'), unreadable);
});

test('la zone de retour réserve sa place au défilement ; « Réessayer » n’est là qu’après une première vue ratée', () => {
  const spy = noActions;
  const say = { text: 'Discord est coupée.', undo: { id: 'ur-discord', enabled: true } };
  const state = { ...initialState(NOW), route: 'startup' as const, startup: FEW, startupSay: say };
  const written = htmlOf(startupView(state, spy));
  assert.ok(written.includes('class="scroll sayon"'));
  assert.ok(!htmlOf(startupView({ ...state, startupSay: null }, spy)).includes('sayon'));
  const failed = htmlOf(startupView({ ...initialState(NOW), startup: null, startupFailed: true }, spy));
  assert.ok(failed.includes('pretry') && failed.includes('Réessayer'));
  assert.ok(!htmlOf(startupView({ ...initialState(NOW), startup: null }, spy)).includes('Réessayer'));
  assert.ok(written.includes('Annuler'));
});

test('la carte du PC n’a pas d’interrupteur : une pastille « Active » ou « Coupée »', () => {
  const machineOnly = page(view(FEW.entries.filter((found) => found.scope === 'machine')));
  assert.ok(!machineOnly.includes('role="switch"'));
  assert.ok(machineOnly.includes('<span class="tag" title=Active>Active</span>'));
  assert.match(machineOnly, /Pour tous les comptes de ce PC/);
  assert.ok(machineOnly.includes('Ces apps se règlent dans Windows pour l’instant'));
  // Seul le PC : pas de carte « Pour ton compte ».
  assert.ok(!machineOnly.includes('Pour ton compte'));
  assert.ok(!machineOnly.includes('class="pwarn"'));
});

test('une portée illisible garde sa carte, avec l’avertissement ; vide et lisible, elle disparaît', () => {
  const written = page(view([entry({ id: 'ur-1' })], ['machine']));
  assert.ok(written.includes('Pour tous les comptes de ce PC'));
  assert.ok(written.includes('Shinku n’a pas pu lire toute cette liste.'));
  const empty = page(view([]));
  assert.ok(!empty.includes('Pour ton compte') && !empty.includes('Pour tous les comptes'));
  assert.ok(empty.includes('Aucune app inscrite ici'));
  assert.ok(!empty.includes('class="ptext"'), 'aucune entrée : pas de phrase de décompte');
  // Le texte qui borne ce que Shinku voit est toujours là.
  assert.ok(empty.includes('sauf les apps du Microsoft Store'));
  assert.ok(empty.includes('Couper une app ne la désinstalle pas'));
});

test('pendant un geste en vol, les interrupteurs et « Annuler » sont désactivés', () => {
  const say = { text: 'Discord est coupée.', undo: { id: 'ur-discord', enabled: true } };
  const idle = page(FEW, { startupSay: say });
  assert.ok(idle.includes('Annuler'));
  assert.ok(!idle.includes('aria-disabled=true'));
  const busy = page(FEW, { startupSay: say, startupBusy: 'ur-discord' });
  // Trois interrupteurs et « Annuler ».
  assert.equal(busy.match(/aria-disabled=true/g)?.length, 4);
  // Jamais l'attribut `disabled` : le contrôle reste focalisable, le focus ne tombe pas sur le corps de la page.
  assert.doesNotMatch(busy, /\?disabled=|\sdisabled[\s>=]/);
});

test('la page attend sa première vue, ou dit qu’elle n’a pas pu la lire', () => {
  assert.ok(textOf(startupView({ ...initialState(NOW), startup: null }, noActions)).includes('Chargement…'));
  const failed = textOf(startupView({ ...initialState(NOW), startup: null, startupFailed: true }, noActions));
  assert.ok(failed.includes(fr.toast.unexpected));
  assert.ok(!failed.includes('Chargement'));
});

test('une app coupée par Shinku : sa pastille, le titre et la phrase de décompte', () => {
  const written = textOf(
    startupView(
      {
        ...initialState(NOW),
        startup: view([entry({ id: 'ur-steam', name: 'Steam', state: 'disabled', mark: { kind: 'cutByShinku', at: NOW - 3 * DAY } })]),
      },
      noActions,
    ),
  );
  assert.ok(written.includes(`Coupée par Shinku le 12${NBSP}nov.`), written);
  assert.ok(written.includes(`1${NBSP}app inscrite au démarrage`), written);
  assert.ok(written.includes(`Aucune ne se lance à l’ouverture de ta session, 1${NBSP}est coupée.`), written);
});

// ── Le résumé d'analyse ────────────────────────────────────────────────────


function resultsState(startup: CategorySummary | null, pane: 'scan' | 'results') {
  const base = (module: CategorySummary['module'], lot: CategorySummary['lot']): CategorySummary => ({
    module,
    lot,
    risk: 'A',
    bytes: 0,
    items: 0,
    state: 'ready',
  });
  const categories = [
    base('recycleBin', 'auto'),
    base('junkFiles', 'auto'),
    base('duplicates', 'optional'),
    base('largeFiles', 'review'),
    ...(startup === null ? [] : [startup]),
  ];
  const session: SessionView = {
    ...initialState(NOW).session,
    state: 'results',
    summary: { complete: true, started: NOW - 60_000, finished: NOW, autoBytes: 0, categories },
  };
  return { ...initialState(NOW), route: 'home' as const, pane, session };
}

const startupCategory = (items: number, state: CategorySummary['state'] = 'ready'): CategorySummary => ({
  module: 'startup',
  lot: 'review',
  risk: 'C',
  bytes: 0,
  items,
  state,
});

test('les résultats disent combien d’apps sont actives, et plus « Bientôt »', () => {
  const lines = (items: CategorySummary): string => textOf(homeView(resultsState(items, 'results'), noActions));
  assert.ok(lines(startupCategory(12)).includes(`12${NBSP}apps actives au démarrage de Windows`));
  assert.ok(lines(startupCategory(1)).includes(`1${NBSP}app active au démarrage de Windows`));
  assert.ok(lines(startupCategory(0)).includes('Aucune app active au démarrage'));
  assert.ok(lines(startupCategory(0, 'unavailable')).includes('Démarrage Non mesuré'));
  assert.ok(!lines(startupCategory(3)).includes('Bientôt'));
  // Une catégorie absente du rapport n'invente rien non plus.
  assert.ok(textOf(homeView(resultsState(null, 'results'), noActions)).includes('Démarrage Non mesuré'));
});

test('l’écran d’analyse écrit « n apps actives » ou « Non mesuré » sur la ligne Démarrage', () => {
  const scan = (category: CategorySummary): string => textOf(homeView(resultsState(category, 'scan'), noActions));
  assert.ok(scan(startupCategory(12)).includes(`Démarrage 12${NBSP}apps actives`));
  assert.ok(scan(startupCategory(1)).includes(`Démarrage 1${NBSP}app active`));
  assert.ok(scan(startupCategory(2, 'unavailable')).includes('Démarrage Non mesuré'));
  assert.ok(!scan(startupCategory(2)).includes('Bientôt'));
  assert.ok(scan(startupCategory(0)).includes('Démarrage Aucune app active'));
});

// ── Deuxième relecture ─────────────────────────────────────────────────────

/** Les directives `repeat` d'un gabarit : leurs valeurs `[éléments, clé, gabarit]`. */
function repeats(value: unknown, found: unknown[][] = []): unknown[][] {
  if (Array.isArray(value)) {
    value.forEach((one) => repeats(one, found));
  } else if (typeof value === 'object' && value !== null && 'values' in value) {
    const { values } = value as { values: unknown[] };
    if ('_$litDirective$' in value && values.length === 3 && typeof values[1] === 'function') {
      found.push(values);
    }
    values.forEach((one) => repeats(one, found));
  }
  return found;
}

test('la liste des lignes est clée par l’identifiant (`repeat`), pas écrite par un simple `map`', () => {
  const steam = entry({ id: 'ur-steam', name: 'Steam' });
  const discord = entry({ id: 'ur-discord', name: 'Discord' });
  const state = { ...initialState(NOW), route: 'startup' as const, startup: view([steam, discord]) };
  const lists = repeats(startupView(state, noActions));
  assert.equal(lists.length, 1, 'une carte, une liste clée');
  const [items, key] = lists[0] as [StartupEntry[], (item: StartupEntry) => unknown];
  assert.deepEqual([...items].map((item) => item.id), ['ur-steam', 'ur-discord']);
  // La clé est l'identifiant de l'entrée : un contrôle ne change pas de ligne quand la liste change.
  assert.equal(key(steam), 'ur-steam');
  assert.equal(key(discord), 'ur-discord');
});

test('rang, clés de contrôle et cible du focus ne comptent que les contrôles qui existent', () => {
  const list = [
    entry({ id: 'a', name: 'Steam', adjustable: false }),
    entry({ id: 'b', name: 'Steam' }),
  ];
  // La première n'a pas d'interrupteur : l'unique interrupteur n'est pas « (2) ».
  assert.deepEqual([...labelRanks(list)], [['b', 1]]);
  assert.equal(controlKey(entry({ id: 'x' })), startupFocus.entry('x'));
  assert.equal(controlKey(entry({ id: 'x', own: true, adjustable: false })), startupFocus.end('x'));
  assert.equal(controlKey(entry({ id: 'x', state: 'unknown', adjustable: false })), startupFocus.end('x'));
  assert.equal(controlKey(entry({ id: 'x', scope: 'machine', adjustable: false })), null);

  const unreadable = view([entry({ id: 'a', state: 'unknown', adjustable: false }), entry({ id: 'b' })]);
  const readable = view([entry({ id: 'a' }), entry({ id: 'b' })]);
  assert.equal(focusTarget(unreadable, readable, startupFocus.end('a')), startupFocus.entry('a'));
  assert.equal(focusTarget(readable, unreadable, startupFocus.entry('a')), startupFocus.end('a'));
  assert.equal(focusTarget(readable, view([entry({ id: 'b' })]), startupFocus.entry('a')), startupFocus.entry('b'));
  assert.equal(focusTarget(readable, view([]), startupFocus.entry('a')), TITLE_KEY);
  // Pas un contrôle de ligne, ou pas dans la vue d'avant : rien à déplacer.
  assert.equal(focusTarget(readable, readable, TITLE_KEY), null);
  assert.equal(focusTarget(readable, readable, startupFocus.entry('inconnue')), null);
  assert.equal(focusTarget(null, readable, startupFocus.entry('a')), null);
});

test('les pastilles portent leur texte entier en infobulle ; les boutons de ligne citent l’app ; « Annuler » est hors de la région vivante', () => {
  const written = page(
    view([
      entry({ id: 'ur-old', name: 'Ancien outil', state: 'unknown', adjustable: false }),
      entry({ id: 'ur-s', name: 'Steam', mark: { kind: 'cameBack', cut: NOW - 5 * DAY } }),
      entry({ id: 'mr-1', name: 'Java', scope: 'machine', adjustable: false }),
    ]),
  );
  assert.ok(written.includes(`<span class="tag" title=S’est remise toute seule depuis le 10${NBSP}nov.>S’est remise toute seule depuis le 10${NBSP}nov.</span>`));
  assert.ok(written.includes('<span class="tag" title=Active>Active</span>'));
  assert.ok(written.includes('aria-label=Ouvrir dans Windows, pour Ancien outil'));
  assert.ok(written.includes(`data-fk=${startupFocus.end('ur-old')}`));

  const say = { text: 'Steam est coupée.', undo: { id: 'ur-s', enabled: true } };
  const zone = page(view([entry({ id: 'ur-s', name: 'Steam' })]), { startupSay: say });
  // La région vivante ne contient que la phrase.
  assert.match(zone, /<div class="sr" aria-live="polite">Steam est coupée\.<\/div>/);
  assert.ok(!/aria-live="polite">[^<]*<button/.test(zone));
  assert.ok(zone.includes('Annuler'));
});

test('les règles de défilement qui gardent la ligne hors de la barre sont dans la feuille de style', () => {
  const css = readFileSync(new URL('../src/styles/startup.css', import.meta.url), 'utf8');
  assert.match(css, /\.scroll\.sayon\s*\{[^}]*scroll-padding-bottom/);
  assert.match(css, /\.startup \.plist > li\s*\{[^}]*scroll-margin-bottom/);
  // La seconde ligne a le contraste du deuxième gris, pas celui du troisième.
  assert.match(css, /\.startup \.row \.t2\s*\{[^}]*color:\s*var\(--text-2\)/);
  // Aucune couleur écrite en dur dans cette feuille.
  assert.doesNotMatch(css, /rgba?\(\s*\d/);
});
