// La page Mode jeu : des réglages directs. Shinku ne lance rien ; son agent
// reconnaît le jeu quand il passe au premier plan, applique ces réglages, puis
// remet tout en place. La page les change, montre l'état et rien d'autre.
//
// Elle n'envoie jamais un chemin qu'elle aurait écrit : seulement les
// identifiants et les noms que le cœur lui a donnés.

import { html, nothing, type TemplateResult } from 'lit-html';
import { ref } from 'lit-html/directives/ref.js';

import type { GameOption, GameSession, GameView, KnownGame, OpenApp, QuietApp } from '../backend/types.ts';
import { formatAgo, formatClock, formatDuration, listOf } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import {
  canRelease,
  type GameWarning,
  gameWarning,
  type HeaderState,
  headerState,
  inGame,
  isRemovable,
  type PowerChip,
  powerChip,
  powerRow,
  shownApps,
  showsChips,
  waitingApps,
} from '../model/game.ts';
import type { Actions, AppState } from '../state.ts';
import { checkIcon, crossIcon, moduleArt } from './art.ts';
import { stage } from './parts.ts';

type Content = TemplateResult | typeof nothing;

// ── En-tête ────────────────────────────────────────────────────────────────

function headerTitle(state: HeaderState, session: GameSession | null): string {
  if (session !== null && inGame(state)) {
    return fr.game.titlePlaying(session.name);
  }
  return state === 'off' ? fr.game.titleOff : fr.game.titleIdle;
}

function headerText(state: HeaderState, session: GameSession | null): string {
  switch (state) {
    case 'off':
      return fr.game.textOff;
    case 'idle':
      return fr.game.textIdle;
    case 'playing':
      return fr.game.textPlaying(formatClock(session?.since ?? 0));
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

function header(view: GameView, actions: Actions): TemplateResult {
  const state = headerState(view);
  const session = view.session;
  const playing = session !== null && inGame(state);
  return html`<header class="ghead">
    <div class="gart">${moduleArt('gameMode', playing)}</div>
    <div class="gtx">
      <p class="eb">${fr.game.eyebrow}</p>
      <h1>${headerTitle(state, session)}</h1>
      <p class="gtext" role="status">${headerText(state, session)}</p>
      ${session !== null && showsChips(state) ? chips(session) : nothing}
      ${session !== null
        ? html`<div class="gbtns">
            ${canRelease(session)
              ? html`<button type="button" class="ghost small" @click=${actions.releaseGame}>${fr.game.release}</button>`
              : nothing}
            <button
              type="button"
              class="ghost small"
              @click=${() => actions.setGameStatus(session.path, 'notGame')}
            >
              ${fr.game.notAGame}
            </button>
          </div>`
        : lastGame(view)}
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
): TemplateResult {
  const id = `game-${option}`;
  return html`<div class="srow">
    <div class="lab">
      <label class="l" for=${id}>${label}</label>
      <span class="d" id=${`${id}-d`}>${hint}</span>
    </div>
    ${extra} ${switchButton(id, checked, false, (on) => actions.setGameOption(option, on))}
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
  return html`<div class="card gmain">
    <div class="srow tall">
      <div class="lab">
        <label class="l" for="game-auto">${fr.game.auto}</label>
        <span class="d" id="game-auto-d">${fr.game.autoHint}</span>
      </div>
      ${switchButton('game-auto', view.auto, true, (on) => actions.setGameOption('auto', on))}
    </div>
    ${warning === null
      ? nothing
      : html`<p class="gwarn" role="status"><i aria-hidden="true"></i><span>${WARNING_TEXT[warning]}</span></p>`}
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

function quietRow(app: QuietApp, actions: Actions): TemplateResult {
  return html`<li>
    <label class="row short">
      <input type="checkbox" .checked=${app.on} @change=${(event: Event) => {
        actions.setQuietApp(app.exe, (event.target as HTMLInputElement).checked);
      }} />
      <span class="box">${checkIcon()}</span>
      <span class="tx"><span class="t1">${app.label}</span></span>
      ${app.running ? html`<span class="tag">${fr.game.open}</span>` : nothing}
    </label>
  </li>`;
}

function quietBlock(view: GameView, actions: Actions): Content {
  if (!view.quietApps) {
    return nothing;
  }
  const shown = shownApps(view.apps);
  const waiting = waitingApps(view.apps);
  return html`<div class="sblock gquiet">
    ${shown.length > 0
      ? html`<ul class="list plain glist">
          ${shown.map((app) => quietRow(app, actions))}
        </ul>`
      : nothing}
    ${waiting.length > 0 ? html`<p class="d">${fr.game.quietAlso(listOf(waiting))}</p>` : nothing}
    <p class="d">${fr.game.quietAdvice}</p>
    <button type="button" class="ghost small add" @click=${() => actions.openPicker('app')}>
      ${fr.game.quietAdd}
    </button>
  </div>`;
}

function duringSection(view: GameView, actions: Actions): TemplateResult {
  return html`<section class="gsec">
    <h2 class="sub flat">${fr.game.during}</h2>
    <div class="card ${view.auto ? '' : 'dim'}">
      ${powerSection(view, actions)}
      ${switchRow('quietApps', fr.game.quiet, fr.game.quietHint, view.quietApps, actions)}
      ${quietBlock(view, actions)}
    </div>
  </section>`;
}

// ── Tes jeux ───────────────────────────────────────────────────────────────

function gameRow(game: KnownGame, now: number, actions: Actions): TemplateResult {
  return html`<li class="row static">
    <span class="tx">
      <span class="t1">${game.name}</span>
      <span class="t2 path" title=${game.path}>${game.path}</span>
    </span>
    ${game.lastPlayed === null ? nothing : html`<span class="st">${fr.game.lastPlayed(formatAgo(game.lastPlayed, now))}</span>`}
    <span class="tag">${game.source === 'added' ? fr.game.added : fr.game.recognised}</span>
    ${isRemovable(game)
      ? html`<button
          type="button"
          class="icb"
          aria-label=${fr.game.remove(game.name)}
          title=${fr.game.remove(game.name)}
          @click=${() => actions.setGameStatus(game.path, 'forgotten')}
        >
          ${crossIcon()}
        </button>`
      : html`<button type="button" class="ghost small" @click=${() => actions.setGameStatus(game.path, 'notGame')}>
          ${fr.game.notAGame}
        </button>`}
  </li>`;
}

function notGameRow(entry: { path: string; name: string }, actions: Actions): TemplateResult {
  return html`<li class="row static">
    <span class="tx">
      <span class="t1">${entry.name}</span>
      <span class="t2 path" title=${entry.path}>${entry.path}</span>
    </span>
    <button type="button" class="ghost small" @click=${() => actions.setGameStatus(entry.path, 'forgotten')}>
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
          : html`<ul class="list plain">
              ${view.games.map((game) => gameRow(game, state.now, actions))}
            </ul>`}
        <button type="button" class="ghost small add" @click=${() => actions.openPicker('game')}>
          ${fr.game.addGame}
        </button>
        <p class="d">${fr.game.addGameHint}</p>
        ${view.notGames.length === 0
          ? nothing
          : html`<h3 class="sub gnever">${fr.game.never}</h3>
              <ul class="list plain">
                ${view.notGames.map((entry) => notGameRow(entry, actions))}
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
          ? html`<p class="note">${fr.common.loading}</p>`
          : html`${header(view, actions)} ${mainCard(state, view, actions)} ${duringSection(view, actions)}
            ${gamesSection(state, view, actions)} ${windowsSection(actions)}
            <p class="note gpromise">${fr.game.promise}</p>`}
      </div>
    </div>`,
  });
}

// ── Sélecteur ──────────────────────────────────────────────────────────────

/**
 * Ouvre la boîte en mode modal : le reste de la fenêtre ne répond plus, et
 * Échap la referme. À son premier dessin elle n'est pas encore dans la page :
 * l'ouverture attend qu'elle y soit.
 */
function openModal(element: Element | undefined): void {
  if (!(element instanceof HTMLDialogElement)) {
    return;
  }
  queueMicrotask(() => {
    if (element.isConnected && !element.open) {
      element.showModal();
    }
  });
}

function pickRow(app: OpenApp, actions: Actions): TemplateResult {
  return html`<li>
    <button type="button" class="row" @click=${() => actions.pickOpenApp(app.id)}>
      <span class="tx">
        <span class="t1">${app.name}</span>
        <span class="t2 path" title=${app.path}>${app.path}</span>
      </span>
    </button>
  </li>`;
}

/** Le sélecteur d'un jeu ou d'une app, parmi ce qui est ouvert en ce moment. */
export function pickerDialog(state: AppState, actions: Actions): Content {
  const picker = state.picker;
  if (picker === null) {
    return nothing;
  }
  // Échap et clic sur le voile : c'est l'application qui referme, pour que l'écran et son état disent la même chose.
  const onCancel = (event: Event): void => {
    event.preventDefault();
    actions.closePicker();
  };
  const onClick = (event: Event): void => {
    if (event.target === event.currentTarget) {
      actions.closePicker();
    }
  };
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
          : html`<ul class="list plain pickl">
              ${apps.map((app) => pickRow(app, actions))}
            </ul>`}
      <div class="askbtns">
        <button type="button" class="ghost" autofocus @click=${actions.closePicker}>${fr.game.cancel}</button>
      </div>
    </div>
  </dialog>`;
}
