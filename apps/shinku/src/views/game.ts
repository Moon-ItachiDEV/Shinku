// La page Mode jeu : des réglages directs. Shinku ne lance rien ; son agent
// reconnaît le jeu quand il passe au premier plan, applique ces réglages, puis
// remet tout en place. La page les change, montre l'état et rien d'autre.
//
// Elle n'envoie jamais un chemin qu'elle aurait écrit : seulement les
// identifiants et les noms que le cœur lui a donnés.

import { html, nothing, type TemplateResult } from 'lit-html';
import { ref } from 'lit-html/directives/ref.js';
import { repeat } from 'lit-html/directives/repeat.js';

import type { GameOption, GameSession, GameView, KnownGame, OpenApp, QuietApp } from '../backend/types.ts';
import { formatAgo, formatDuration, formatSince, listOf } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import {
  canRelease,
  type GameWarning,
  gameWarning,
  busyKey,
  focusKey,
  type HeaderState,
  headerState,
  inGame,
  isLeaving,
  isRemovable,
  isRemovableApp,
  type PowerChip,
  powerChip,
  powerRow,
  shownApps,
  showsButtons,
  showsChips,
  waitingApps,
} from '../model/game.ts';
import type { Actions, AppState } from '../state.ts';
import { checkIcon, crossIcon, moduleArt } from './art.ts';
import { openModal } from './modal.ts';
import { stage } from './parts.ts';

type Content = TemplateResult | typeof nothing;

// ── En-tête ────────────────────────────────────────────────────────────────

/** Ce que dit un geste en vol : son bouton est désactivé jusqu'à la réponse du cœur. */
const isBusy = (state: AppState, key: string): boolean => state.gameBusy.includes(key);

function headerTitle(state: HeaderState, session: GameSession | null): string {
  if (session !== null && inGame(state)) {
    return fr.game.titlePlaying(session.name);
  }
  switch (state) {
    case 'pending':
      return fr.game.titlePending;
    case 'off':
      return fr.game.titleOff;
    default:
      return fr.game.titleIdle;
  }
}

function headerText(state: HeaderState, session: GameSession | null, now: number, leaving: boolean): string {
  if (leaving) {
    return fr.game.textLeaving;
  }
  switch (state) {
    case 'off':
      return fr.game.textOff;
    case 'idle':
      return fr.game.textIdle;
    case 'playing':
      return fr.game.textPlaying(formatSince(session?.since ?? now, now));
    case 'released':
      return fr.game.textReleased;
    case 'pending':
      return fr.game.textPending;
  }
}

const POWER_CHIP_TEXT: Record<PowerChip, string> = {
  applied: fr.game.powerApplied,
  alreadyFast: fr.game.powerAlreadyFast,
  onBattery: fr.game.powerOnBattery,
  custom: fr.game.powerCustom,
  refused: fr.game.powerRefused,
};

/** Les pastilles de la partie en cours : l'alimentation, puis les apps en veilleuse. */
function chips(session: GameSession): Content {
  const power = powerChip(session.power);
  if (power === null && session.quietedApps === 0) {
    return nothing;
  }
  return html`<p class="gchips">
    ${power === null ? nothing : html`<span class="tag">${POWER_CHIP_TEXT[power]}</span>`}
    ${session.quietedApps > 0 ? html`<span class="tag">${fr.game.quietCount(session.quietedApps)}</span>` : nothing}
  </p>`;
}

function lastGame(view: GameView): Content {
  const last = view.last;
  if (last === null || view.session !== null) {
    return nothing;
  }
  const duration = formatDuration(last.ended - last.started);
  return html`<p class="d glast">
    ${last.complete ? fr.game.lastOk(last.name, duration) : fr.game.lastIncomplete(last.name, duration)}
  </p>`;
}

function header(state: AppState, view: GameView, actions: Actions): TemplateResult {
  const head = headerState(view);
  const session = view.session;
  // « Ce n'est pas un jeu » est parti : l'agent ne l'a pas encore terminé, l'en-tête attend (un délai, et jamais devant l'avertissement).
  const leaving = isLeaving(
    view,
    state.gameLeaving,
    state.now,
    !isBusy(state, busyKey.status(state.gameLeaving?.path ?? '')),
  );
  return html`<header class="ghead">
    <div class="gart">${moduleArt('gameMode')}</div>
    <div class="gtx">
      <p class="eb">${fr.game.eyebrow}</p>
      <h1 tabindex="-1" data-fk=${focusKey.title}>${headerTitle(head, session)}</h1>
      <p class="gtext" role="status">${headerText(head, session, state.now, leaving)}</p>
      ${session !== null && showsChips(head) ? chips(session) : nothing}
      ${session !== null && showsButtons(head)
        ? html`<div class="gbtns">
            ${canRelease(session)
              ? html`<button
                  type="button"
                  class="ghost small"
                  ?disabled=${leaving || isBusy(state, busyKey.release)}
                  @click=${actions.releaseGame}
                >
                  ${fr.game.release}
                </button>`
              : nothing}
            <button
              type="button"
              class="ghost small"
              data-fk=${focusKey.headerNotGame}
              aria-label=${fr.game.notAGameNamed(session.name)}
              ?disabled=${leaving || isBusy(state, busyKey.status(session.path))}
              @click=${() => actions.setGameStatus(session.path, 'notGame')}
            >
              ${fr.game.notAGame}
            </button>
          </div>`
        : session === null
          ? lastGame(view)
          : nothing}
    </div>
  </header>`;
}

// ── Interrupteurs ──────────────────────────────────────────────────────────

/**
 * Un interrupteur : un vrai bouton `role="switch"`, lié à son libellé et à sa
 * description. Il se dessine avec `.sw` (src/styles/forms.css).
 */
function switchButton(
  id: string,
  checked: boolean,
  big: boolean,
  onToggle: (on: boolean) => void,
): TemplateResult {
  return html`<button
    type="button"
    role="switch"
    id=${id}
    class="sw ${big ? 'big' : ''}"
    aria-checked=${checked ? 'true' : 'false'}
    aria-describedby=${`${id}-d`}
    @click=${() => onToggle(!checked)}
  ></button>`;
}

function switchRow(
  option: GameOption,
  label: string,
  hint: string,
  checked: boolean,
  actions: Actions,
  extra: Content = nothing,
  big = false,
): TemplateResult {
  const id = `game-${option}`;
  return html`<div class="srow ${big ? 'tall' : ''}">
    <div class="lab">
      <label class="l" id=${`${id}-l`} for=${id}>${label}</label>
      <span class="d" id=${`${id}-d`}>${hint}</span>
    </div>
    ${extra} ${switchButton(id, checked, big, (on) => actions.setGameOption(option, on))}
  </div>`;
}

// ── Carte principale ───────────────────────────────────────────────────────

const WARNING_TEXT: Record<GameWarning, string> = {
  agentMissing: fr.game.agentMissing,
  agentStopped: fr.game.agentStopped,
  startupDisabled: fr.game.startupDisabled,
};

function mainCard(state: AppState, view: GameView, actions: Actions): TemplateResult {
  const warning = gameWarning(view, state.gameStoppedSince, state.now);
  // La région vivante est toujours là : seule la phrase qu'elle contient change, et les lecteurs d'écran l'annoncent.
  return html`<div class="card gmain">
    ${switchRow('auto', fr.game.auto, fr.game.autoHint, view.auto, actions, nothing, true)}
    <div role="status">
      ${warning === null
        ? nothing
        : html`<p class="gwarn"><i aria-hidden="true"></i><span>${WARNING_TEXT[warning]}</span></p>`}
    </div>
  </div>`;
}

// ── Pendant que tu joues ───────────────────────────────────────────────────

function powerSection(view: GameView, actions: Actions): TemplateResult {
  switch (powerRow(view)) {
    case 'switch':
      return switchRow(
        'power',
        fr.game.power,
        fr.game.powerHint,
        view.power,
        actions,
        view.onBattery ? html`<span class="tag">${fr.game.onBattery}</span>` : nothing,
      );
    case 'unavailable':
    case 'unsuited': {
      const unavailable = view.powerSupport === 'unavailable';
      return html`<div class="srow">
        <div class="lab">
          <span class="l">${fr.game.power}</span>
          <span class="d">${unavailable ? fr.game.powerUnavailable : fr.game.powerUnsuited}</span>
        </div>
        ${unavailable
          ? html`<button type="button" class="ghost small" @click=${() => actions.openWindowsSettings('power')}>
              ${fr.game.powerOpen}
            </button>`
          : nothing}
      </div>`;
    }
  }
}

/** Une app intégrée : une case. Une app ajoutée par la personne : pas de case, une croix pour la retirer. */
function quietRow(state: AppState, app: QuietApp, actions: Actions): TemplateResult {
  const open = app.running ? html`<span class="tag">${fr.game.open}</span>` : nothing;
  if (isRemovableApp(app)) {
    const busy = isBusy(state, busyKey.quiet(app.exe));
    return html`<li class="row short static">
      <span class="tx"><span class="t1">${app.label}</span></span>
      ${open}
      <span class="tag">${fr.game.addedApp}</span>
      <button
        type="button"
        class="icb"
        data-fk=${focusKey.quiet(app.exe)}
        aria-label=${fr.game.remove(app.label)}
        title=${fr.game.remove(app.label)}
        ?disabled=${busy}
        @click=${() => actions.setQuietApp(app.exe, false)}
      >
        ${crossIcon()}
      </button>
    </li>`;
  }
  // La pastille « Ouverte » est hors de l'étiquette : le nom de la case n'est que le nom de l'app.
  return html`<li class="gcheck">
    <label class="row short">
      <input
        type="checkbox"
        data-fk=${focusKey.quiet(app.exe)}
        .checked=${app.on}
        @change=${(event: Event) => {
          actions.setQuietApp(app.exe, (event.target as HTMLInputElement).checked);
        }}
      />
      <span class="box">${checkIcon()}</span>
      <span class="tx"><span class="t1">${app.label}</span></span>
    </label>
    ${open}
  </li>`;
}

function quietBlock(state: AppState, view: GameView, actions: Actions): Content {
  if (!view.quietApps) {
    return nothing;
  }
  const shown = shownApps(view.apps);
  const waiting = waitingApps(view.apps);
  return html`<div class="sblock gquiet">
    ${shown.length > 0
      ? html`<ul class="list glist" aria-labelledby="game-quietApps-l">
          ${repeat(
            shown,
            (app) => app.exe,
            (app) => quietRow(state, app, actions),
          )}
        </ul>`
      : nothing}
    ${waiting.length > 0 ? html`<p class="d">${fr.game.quietAlso(listOf(waiting))}</p>` : nothing}
    <p class="d">${fr.game.quietAdvice}</p>
    <button type="button" class="ghost small add" data-fk=${focusKey.addApp} @click=${() => actions.openPicker('app')}>
      ${fr.game.quietAdd}
    </button>
  </div>`;
}

function duringSection(state: AppState, view: GameView, actions: Actions): TemplateResult {
  return html`<section class="gsec">
    <h2 class="sub flat">${fr.game.during}</h2>
    <div class="card ${view.auto ? '' : 'dim'}">
      ${powerSection(view, actions)}
      ${switchRow('quietApps', fr.game.quiet, fr.game.quietHint, view.quietApps, actions)}
      ${quietBlock(state, view, actions)}
    </div>
  </section>`;
}

// ── Tes jeux ───────────────────────────────────────────────────────────────

function gameRow(state: AppState, game: KnownGame, actions: Actions): TemplateResult {
  const busy = isBusy(state, busyKey.status(game.path));
  return html`<li class="row static">
    <span class="tx">
      <span class="t1">${game.name}</span>
      <span class="t2 path" title=${game.path}>${game.path}</span>
    </span>
    <span class="tag">${game.source === 'added' ? fr.game.added : fr.game.recognised}</span>
    ${game.lastPlayed === null ? nothing : html`<span class="st">${fr.game.lastPlayed(formatAgo(game.lastPlayed, state.now))}</span>`}
    ${isRemovable(game)
      ? html`<button
          type="button"
          class="icb"
          data-fk=${focusKey.game(game.path)}
          aria-label=${fr.game.remove(game.name)}
          title=${fr.game.remove(game.name)}
          ?disabled=${busy}
          @click=${() => actions.setGameStatus(game.path, 'forgotten')}
        >
          ${crossIcon()}
        </button>`
      : html`<button
          type="button"
          class="ghost small"
          data-fk=${focusKey.game(game.path)}
          aria-label=${fr.game.notAGameNamed(game.name)}
          ?disabled=${busy}
          @click=${() => actions.setGameStatus(game.path, 'notGame')}
        >
          ${fr.game.notAGame}
        </button>`}
  </li>`;
}

function notGameRow(state: AppState, entry: { path: string; name: string }, actions: Actions): TemplateResult {
  return html`<li class="row static">
    <span class="tx">
      <span class="t1">${entry.name}</span>
      <span class="t2 path" title=${entry.path}>${entry.path}</span>
    </span>
    <button
      type="button"
      class="ghost small"
      data-fk=${focusKey.never(entry.path)}
      aria-label=${fr.game.restoreNamed(entry.name)}
      ?disabled=${isBusy(state, busyKey.status(entry.path))}
      @click=${() => actions.setGameStatus(entry.path, 'forgotten')}
    >
      ${fr.game.restore}
    </button>
  </li>`;
}

function gamesSection(state: AppState, view: GameView, actions: Actions): TemplateResult {
  return html`<section class="gsec">
    <h2 class="sub flat">${fr.game.games}</h2>
    <div class="card">
      <div class="sblock">
        <p class="d gintro">${fr.game.gamesHint}</p>
        ${view.games.length === 0
          ? html`<p class="d gempty">${fr.game.gamesEmpty}</p>`
          : html`<ul class="list">
              ${repeat(
                view.games,
                (game) => game.path,
                (game) => gameRow(state, game, actions),
              )}
            </ul>`}
        <button type="button" class="ghost small add" data-fk=${focusKey.addGame} @click=${() => actions.openPicker('game')}>
          ${fr.game.addGame}
        </button>
        <p class="d">${fr.game.addGameHint}</p>
        ${view.notGames.length === 0
          ? nothing
          : html`<h3 class="sub gnever">${fr.game.never}</h3>
              <ul class="list">
                ${repeat(
                  view.notGames,
                  (entry) => entry.path,
                  (entry) => notGameRow(state, entry, actions),
                )}
              </ul>`}
      </div>
    </div>
  </section>`;
}

// ── Windows s'en occupe déjà ───────────────────────────────────────────────

function windowsSection(actions: Actions): TemplateResult {
  return html`<section class="gsec">
    <h2 class="sub flat">${fr.game.windows}</h2>
    <div class="card">
      <div class="srow">
        <div class="lab">
          <span class="l">${fr.game.winMode}</span>
          <span class="d">${fr.game.winModeHint}</span>
        </div>
        <button type="button" class="ghost small" @click=${() => actions.openWindowsSettings('gameMode')}>
          ${fr.game.winOpen}
        </button>
      </div>
      <div class="srow">
        <div class="lab">
          <span class="l">${fr.game.winNotif}</span>
          <span class="d">${fr.game.winNotifHint}</span>
        </div>
      </div>
    </div>
  </section>`;
}

// ── La page ────────────────────────────────────────────────────────────────

export function gameView(state: AppState, actions: Actions): TemplateResult {
  const view = state.game;
  return stage({
    body: html`<div class="scroll">
      <div class="page game">
        ${view === null
          ? html`<p class="note" role="status">${state.gameFailed ? fr.toast.unexpected : fr.common.loading}</p>`
          : html`${header(state, view, actions)} ${mainCard(state, view, actions)} ${duringSection(state, view, actions)}
            ${gamesSection(state, view, actions)} ${windowsSection(actions)}
            <p class="note gpromise">${fr.game.promise}</p>`}
      </div>
    </div>`,
  });
}

// ── Sélecteur ──────────────────────────────────────────────────────────────

function pickRow(app: OpenApp, busy: boolean, actions: Actions): TemplateResult {
  return html`<li>
    <button type="button" class="row" ?disabled=${busy} @click=${() => actions.pickOpenApp(app.id)}>
      <span class="tx">
        <span class="t1">${app.name}</span>
        <span class="t2 path" title=${app.path}>${app.path}</span>
      </span>
    </button>
  </li>`;
}

/**
 * Échap et clic sur le voile : c'est l'application qui referme, pour que
 * l'écran et son état disent la même chose.
 */
export function pickerHandlers(actions: Pick<Actions, 'closePicker'>): {
  onCancel: (event: Event) => void;
  onClick: (event: Event) => void;
} {
  return {
    onCancel: (event) => {
      event.preventDefault();
      actions.closePicker();
    },
    // Un clic à côté de la boîte tombe sur le voile, donc sur le dialogue lui-même.
    onClick: (event) => {
      if (event.target === event.currentTarget) {
        actions.closePicker();
      }
    },
  };
}

/** Le sélecteur d'un jeu ou d'une app, parmi ce qui est ouvert en ce moment. */
export function pickerDialog(state: AppState, actions: Actions): Content {
  const picker = state.picker;
  if (picker === null) {
    return nothing;
  }
  const { onCancel, onClick } = pickerHandlers(actions);
  const apps = picker.apps;
  return html`<dialog
    class="ask"
    aria-labelledby="pick-title"
    aria-describedby="pick-hint"
    ${ref(openModal)}
    @cancel=${onCancel}
    @close=${actions.closePicker}
    @click=${onClick}
  >
    <div class="askbox">
      <h2 id="pick-title">${picker.kind === 'game' ? fr.game.pickGame : fr.game.pickApp}</h2>
      <p id="pick-hint" class="askintro">${fr.game.pickHint}</p>
      ${apps === null
        ? html`<p class="note" role="status">${fr.common.loading}</p>`
        : apps.length === 0
          ? html`<p class="note">${fr.game.pickEmpty}</p>`
          : html`<ul class="list pickl">
              ${repeat(
                apps,
                (app) => app.id,
                (app) => pickRow(app, isBusy(state, busyKey.pick), actions),
              )}
            </ul>`}
      <div class="askbtns">
        <button type="button" class="ghost" autofocus @click=${actions.closePicker}>${fr.game.cancel}</button>
      </div>
    </div>
  </dialog>`;
}
