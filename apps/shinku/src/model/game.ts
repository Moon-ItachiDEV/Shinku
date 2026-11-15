// Ce que la page Mode jeu tire d'une vue du cœur : l'état de l'en-tête,
// l'avertissement à montrer, les pastilles, les listes d'apps et de jeux.
//
// Ces fonctions sont pures : elles ne font que lire ce que rend le cœur. Elles
// sont testées sans navigateur (tests/game.test.ts).

import type {
  AgentState,
  GameOption,
  GameSession,
  GameView,
  KnownGame,
  PowerOutcome,
  QuietApp,
} from '../backend/types.ts';

/** Délai de grâce : un agent arrêté n'est signalé qu'après ce temps, en millisecondes. */
export const STOPPED_GRACE = 15_000;

/** Délai entre deux demandes de la vue pendant que la page est affichée, en millisecondes. */
export const GAME_POLL_DELAY = 2000;

/**
 * Après « Tout remettre maintenant », l'agent n'a pas encore réécrit son
 * journal : la page garde la partie « remise en place » ce temps-là, même si
 * une vue dit le contraire, en millisecondes.
 */
export const RELEASE_GRACE = 6000;

/** Un geste resté sans réponse ce temps-là est abandonné : son bouton se libère et le sondage reprend, en millisecondes. */
export const GESTURE_TIMEOUT = 10_000;

/** Les échecs de suite de `game_view` après lesquels la page le dit, au lieu de « Chargement… ». */
export const VIEW_FAILURES = 3;

// ── L'en-tête ──────────────────────────────────────────────────────────────

/**
 * Ce que l'en-tête raconte.
 *
 * - `off` : le mode jeu automatique est éteint ;
 * - `idle` : il veille, aucune partie ;
 * - `playing` : une partie est en cours, les réglages sont appliqués ;
 * - `released` : la personne a tout remis en place, la partie continue ;
 * - `pending` : la remise en place a échoué, le cœur réessaie.
 *
 * Une remise en place en attente passe devant « remis en place » : l'avertissement
 * ne doit jamais être caché par une phrase rassurante.
 */
export type HeaderState = 'off' | 'idle' | 'playing' | 'released' | 'pending';

export function headerState(view: GameView): HeaderState {
  const session = view.session;
  if (session === null) {
    return view.auto ? 'idle' : 'off';
  }
  if (session.restorePending) {
    return 'pending';
  }
  return session.released ? 'released' : 'playing';
}

/**
 * La partie est en cours : l'en-tête porte son nom. Une remise en place en
 * attente est une partie finie, dont Shinku n'a pas pu tout remettre : son
 * en-tête a un titre à part.
 */
export function inGame(state: HeaderState): boolean {
  return state === 'playing' || state === 'released';
}

/** Les boutons de l'en-tête : seulement pendant une partie, pas pendant une remise en place en attente. */
export function showsButtons(state: HeaderState): boolean {
  return inGame(state);
}

/** « Tout remettre maintenant » n'a plus rien à faire une fois la partie remise en place. */
export function canRelease(session: GameSession): boolean {
  return !session.released;
}

/** Les pastilles de l'en-tête n'ont de sens que tant que les réglages sont appliqués. */
export function showsChips(state: HeaderState): boolean {
  return state === 'playing';
}

/** Ce que dit la pastille de l'alimentation ; `null` : rien à dire. */
export type PowerChip = 'applied' | 'alreadyFast' | 'onBattery' | 'custom' | 'refused';

export function powerChip(outcome: PowerOutcome): PowerChip | null {
  switch (outcome) {
    case 'applied':
    case 'alreadyFast':
    case 'onBattery':
    case 'custom':
    case 'refused':
      return outcome;
    // Éteinte, absente ou déconseillée : la page le dit plus bas, pas dans l'en-tête.
    case 'off':
    case 'unavailable':
    case 'unsuited':
      return null;
  }
}

// ── L'avertissement de la carte principale ─────────────────────────────────

export type GameWarning = 'agentMissing' | 'agentStopped' | 'startupDisabled';

/**
 * Quand l'agent est-il arrêté pour la première fois ? `previous` : la date déjà
 * notée, ou `null`. Un agent qui tourne ou manque efface la date.
 */
export function stoppedSince(previous: number | null, agent: AgentState, now: number): number | null {
  return agent === 'stopped' ? (previous ?? now) : null;
}

/**
 * La date d'arrêt de l'agent, à noter à chaque vue reçue. Un agent arrêté alors
 * que le mode automatique est éteint est son état normal : on ne le note pas,
 * pour que le délai de grâce ne commence qu'une fois le mode allumé.
 */
export function noteStop(previous: number | null, view: GameView, now: number): number | null {
  return stoppedSince(previous, view.auto ? view.agent : 'running', now);
}

/**
 * Au plus un avertissement, par ordre de priorité : l'agent absent, l'agent
 * arrêté depuis plus de 15 secondes (le temps qu'il démarre), puis le
 * démarrage désactivé dans Windows. Les deux derniers ne comptent que si le
 * mode automatique est allumé.
 */
export function gameWarning(view: GameView, stopped: number | null, now: number): GameWarning | null {
  if (view.agent === 'missing') {
    return 'agentMissing';
  }
  if (view.auto && view.agent === 'stopped' && stopped !== null && now - stopped > STOPPED_GRACE) {
    return 'agentStopped';
  }
  if (view.auto && view.startupDisabled) {
    return 'startupDisabled';
  }
  return null;
}

// ── L'alimentation ─────────────────────────────────────────────────────────

/** Ce que la ligne « Alimentation » montre : un interrupteur, ou la raison de son absence. */
export function powerRow(view: GameView): 'switch' | 'unavailable' | 'unsuited' {
  return view.powerSupport === 'available' ? 'switch' : view.powerSupport;
}

// ── Les apps en veilleuse ──────────────────────────────────────────────────

/** Les apps qui tournent, ou que la personne a ajoutées : ce sont les cases à montrer. */
export function shownApps(apps: readonly QuietApp[]): QuietApp[] {
  return apps.filter((app) => app.running || !app.builtIn);
}

/** Les noms des apps intégrées cochées qui ne tournent pas : elles seront en veilleuse si elles s'ouvrent. */
export function waitingApps(apps: readonly QuietApp[]): string[] {
  return apps.filter((app) => app.builtIn && app.on && !app.running).map((app) => app.label);
}

// ── Les gestes optimistes ──────────────────────────────────────────────────

/** La vue telle qu'elle sera quand le cœur aura accepté un interrupteur. */
export function withOption(view: GameView, option: GameOption, on: boolean): GameView {
  switch (option) {
    case 'auto':
      return { ...view, auto: on };
    case 'power':
      return { ...view, power: on };
    case 'quietApps':
      return { ...view, quietApps: on };
  }
}

/** Ce que vaut un interrupteur dans une vue. */
export function optionOf(view: GameView, option: GameOption): boolean {
  switch (option) {
    case 'auto':
      return view.auto;
    case 'power':
      return view.power;
    case 'quietApps':
      return view.quietApps;
  }
}

/** La vue telle qu'elle sera quand le cœur aura accepté la case d'une app. */
export function withQuietApp(view: GameView, exe: string, on: boolean): GameView {
  return { ...view, apps: view.apps.map((app) => (app.exe === exe ? { ...app, on } : app)) };
}

/** La vue telle qu'elle sera quand l'agent aura tout remis en place pour la partie en cours. */
export function withReleased(view: GameView, released: boolean): GameView {
  return view.session === null ? view : { ...view, session: { ...view.session, released } };
}

/**
 * « Ce n'est pas un jeu » est parti pour la partie en cours : l'en-tête attend
 * que l'agent la termine, tant que la vue montre encore cette partie.
 */
export function isLeaving(view: GameView, leaving: string | null): boolean {
  return leaving !== null && view.session?.path === leaving;
}

/** La partie dont « Tout remettre maintenant » est parti, et jusqu'à quand la page la tient pour remise en place. */
export interface Releasing {
  path: string;
  until: number;
}

/** La vue dément-elle encore, dans le délai de grâce, une partie que la personne a fait remettre en place ? */
export function releaseExpected(view: GameView, expected: Releasing | null, now: number): boolean {
  return (
    expected !== null &&
    now < expected.until &&
    view.session !== null &&
    !view.session.released &&
    view.session.path === expected.path
  );
}

/** La vue, avec la partie « remise en place » tant que l'agent n'a pas rattrapé la demande. */
export function withExpectedRelease(view: GameView, expected: Releasing | null, now: number): GameView {
  return releaseExpected(view, expected, now) ? withReleased(view, true) : view;
}

// ── Les jeux ───────────────────────────────────────────────────────────────

/** Un jeu ajouté par la personne se retire ; un jeu reconnu seul se refuse. */
export function isRemovable(game: KnownGame): boolean {
  return game.source === 'added';
}

/** Une app ajoutée par la personne se retire (sa ligne n'a pas de case) ; une app intégrée se coche. */
export function isRemovableApp(app: QuietApp): boolean {
  return !app.builtIn;
}

// ── Les gestes en vol et le focus ──────────────────────────────────────────

/** Ce qui identifie un geste en vol : son bouton est désactivé tant qu'il n'a pas de réponse. */
export const busyKey = {
  release: 'release',
  pick: 'pick',
  status: (path: string): string => `status:${path}`,
  quiet: (exe: string): string => `quiet:${exe}`,
};

/** Ce qui identifie un bouton où le focus peut être rendu (`data-fk` dans la page). */
export const focusKey = {
  headerNotGame: 'header-notgame',
  title: 'title',
  addGame: 'add-game',
  addApp: 'add-app',
  game: (path: string): string => `game:${path}`,
  never: (path: string): string => `never:${path}`,
  quiet: (exe: string): string => `quiet:${exe}`,
};

/**
 * Où va le focus quand une ligne disparaît : la ligne qui la suivait, sinon
 * celle qui la précédait, sinon `null`. `keys` : les lignes avant la
 * disparition, dans l'ordre affiché.
 */
export function neighbour(keys: readonly string[], removed: string): string | null {
  const index = keys.indexOf(removed);
  if (index < 0) {
    return null;
  }
  return keys[index + 1] ?? keys[index - 1] ?? null;
}
