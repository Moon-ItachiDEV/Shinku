// La page Démarrage : les apps inscrites au démarrage de Windows. Elle règle les
// entrées du compte au même endroit que le Gestionnaire des tâches (l'app reste
// installée, rien n'est supprimé) et montre celles du PC en lecture seule.
//
// Elle n'envoie jamais un nom ni un chemin : seulement l'identifiant que le
// cœur a donné à la ligne.

import { html, nothing, type TemplateResult } from 'lit-html';
import { repeat } from 'lit-html/directives/repeat.js';

import type { StartupEntry, StartupScope, StartupView } from '../backend/types.ts';
import { fr } from '../i18n/fr.ts';
import {
  detailId,
  detailParts,
  entriesOf,
  initialOf,
  labelRanks,
  machineChip,
  rowEnd,
  rowTags,
  showsScope,
  startupCounts,
  startupFocus,
  startupSummary,
  TITLE_KEY,
} from '../model/startup.ts';
import type { Actions, AppState } from '../state.ts';
import { moduleArt } from './art.ts';
import { stage } from './parts.ts';

type Content = TemplateResult | typeof nothing;

/** Un geste en vol : les interrupteurs et « Annuler » attendent la réponse du cœur, un seul changement à la fois. */
const isBusy = (state: AppState): boolean => state.startupBusy !== null;

// ── En-tête ────────────────────────────────────────────────────────────────

function openWindows(actions: Actions): TemplateResult {
  return html`<button type="button" class="ghost small" @click=${() => actions.openWindowsSettings('startupApps')}>
    ${fr.startup.openWindows}
  </button>`;
}

function header(view: StartupView, actions: Actions): TemplateResult {
  const counts = startupCounts(view);
  const summary = startupSummary(counts);
  return html`<header class="phead">
    <div class="part">${moduleArt('startup')}</div>
    <div class="ptx">
      <p class="eb">${fr.startup.eyebrow}</p>
      <h1 tabindex="-1" data-fk=${TITLE_KEY}>${fr.startup.title(counts.total)}</h1>
      ${summary === null ? nothing : html`<p class="ptext">${summary}</p>`}
      <div class="pline">
        <p class="d">${fr.startup.scope}</p>
        ${openWindows(actions)}
      </div>
    </div>
  </header>`;
}

// ── Zone de retour ─────────────────────────────────────────────────────────

/**
 * La barre de retour : dernier enfant de la page, collée en bas de la zone qui défile. La région vivante est
 * toujours là, vide et sans hauteur : seule sa phrase change, et les lecteurs d'écran l'annoncent.
 */
function sayZone(state: AppState, actions: Actions): TemplateResult {
  const say = state.startupSay;
  // Tant que la personne a le focus dans la barre ou la survole, elle ne s'efface pas.
  // La phrase seule est annoncée (région vivante permanente, hors écran) ; « Annuler » reste dans la barre.
  return html`<div class="pzone">
    <div class="sr" aria-live="polite">${say?.text ?? ''}</div>
    ${say === null
      ? nothing
      : html`<p
          class="psay"
          @focusin=${() => actions.holdStartupSay('focus', true)}
          @focusout=${() => actions.holdStartupSay('focus', false)}
          @mouseenter=${() => actions.holdStartupSay('hover', true)}
          @mouseleave=${() => actions.holdStartupSay('hover', false)}
        >
          <span aria-hidden="true">${say.text}</span>
          ${say.undo === null
            ? nothing
            : html`<button
                type="button"
                class="ghost small"
                aria-disabled=${isBusy(state) ? 'true' : nothing}
                @click=${() => (isBusy(state) ? undefined : actions.undoStartup())}
              >
                ${fr.startup.undo}
              </button>`}
        </p>`}
  </div>`;
}

// ── Lignes ─────────────────────────────────────────────────────────────────

function switchButton(
  state: AppState,
  entry: StartupEntry,
  rank: number,
  hasDetails: boolean,
  actions: Actions,
): TemplateResult {
  const on = entry.state === 'enabled';
  return html`<button
    type="button"
    role="switch"
    class="sw"
    data-fk=${startupFocus.entry(entry.id)}
    aria-checked=${on ? 'true' : 'false'}
    aria-label=${fr.startup.switchLabel(entry.name, entry.alias, rank)}
    aria-describedby=${hasDetails ? detailId(entry.id) : nothing}
    aria-disabled=${isBusy(state) ? 'true' : nothing}
    @click=${() => (isBusy(state) ? undefined : actions.setStartupEntry(entry.id, !on))}
  ></button>`;
}

/** Ce que la fin de la ligne montre : un interrupteur, ou la raison de son absence. */
function rowControl(
  state: AppState,
  entry: StartupEntry,
  rank: number,
  hasDetails: boolean,
  actions: Actions,
): Content {
  switch (rowEnd(entry)) {
    case 'switch':
      return switchButton(state, entry, rank, hasDetails, actions);
    case 'own':
      return html`<span class="pend">
        <span class="d">${fr.startup.own}</span>
        ${entry.state === 'disabled' ? html`<span class="tag" title=${fr.startup.ownCut}>${fr.startup.ownCut}</span>` : nothing}
        <button type="button" class="ghost small" data-fk=${startupFocus.end(entry.id)} @click=${() => actions.navigate('gameMode')}>
          ${fr.startup.seeGameMode}
        </button>
      </span>`;
    case 'unknown':
    case 'unadjustable':
      return html`<span class="pend">
        <span class="d">${rowEnd(entry) === 'unknown' ? fr.startup.unreadableEntry : fr.startup.notAdjustableHere}</span>
        <button
          type="button"
          class="ghost small"
          data-fk=${startupFocus.end(entry.id)}
          aria-label=${fr.startup.openInWindowsFor(entry.name)}
          @click=${() => actions.openWindowsSettings('startupApps')}
        >
          ${fr.startup.openInWindows}
        </button>
      </span>`;
    case 'machine': {
      const chip = machineChip(entry, state.now);
      return chip === null ? html`<span class="d">${fr.startup.unreadableEntry}</span>` : html`<span class="tag" title=${chip}>${chip}</span>`;
    }
  }
}

function row(state: AppState, entry: StartupEntry, rank: number, actions: Actions): TemplateResult {
  const details = detailParts(entry).join(fr.startup.separator);
  return html`<li class="row static">
    <span class="av" aria-hidden="true">${initialOf(entry.name)}</span>
    <span class="tx">
      <span class="t1 path" title=${entry.name}>${entry.name}</span>
      ${details === ''
        ? nothing
        : html`<span class="t2 path" id=${detailId(entry.id)} title=${details}>${details}</span>`}
    </span>
    ${rowTags(entry, state.now).map((tag) => html`<span class="tag" title=${tag}>${tag}</span>`)}
    ${rowControl(state, entry, rank, details !== '', actions)}
  </li>`;
}

// ── Cartes ─────────────────────────────────────────────────────────────────

function scopeCard(state: AppState, view: StartupView, scope: StartupScope, actions: Actions): TemplateResult {
  const entries = entriesOf(view, scope);
  const ranks = labelRanks(entries);
  return html`<div class="card">
    ${view.unreadable.includes(scope)
      ? html`<p class="pwarn"><i aria-hidden="true"></i><span>${fr.startup.unreadable}</span></p>`
      : nothing}
    ${entries.length === 0
      ? nothing
      : html`<ul class="plist">
          ${repeat(
            entries,
            (entry) => entry.id,
            (entry) => row(state, entry, ranks.get(entry.id) ?? 1, actions),
          )}
        </ul>`}
  </div>`;
}

function userSection(state: AppState, view: StartupView, actions: Actions): Content {
  if (!showsScope(view, 'user')) {
    return nothing;
  }
  return html`<section class="psec">
    <h2 class="sub flat">${fr.startup.user}</h2>
    ${scopeCard(state, view, 'user', actions)}
  </section>`;
}

function machineSection(state: AppState, view: StartupView, actions: Actions): Content {
  if (!showsScope(view, 'machine')) {
    return nothing;
  }
  return html`<section class="psec">
    <h2 class="sub flat">${fr.startup.machine}</h2>
    <div class="pline pmach">
      <p class="d">${fr.startup.machineHint}</p>
      ${openWindows(actions)}
    </div>
    ${scopeCard(state, view, 'machine', actions)}
  </section>`;
}

// ── La page ────────────────────────────────────────────────────────────────

export function startupView(state: AppState, actions: Actions): TemplateResult {
  const view = state.startup;
  return stage({
    // Tant que la barre de retour est là, la zone qui défile lui laisse sa place (voir startup.css).
    body: html`<div class="scroll ${state.startupSay === null ? '' : 'sayon'}">
      <div class="page startup">
        ${view === null
          ? html`<p class="note" role="status">${state.startupFailed ? fr.toast.unexpected : fr.common.loading}</p>
            ${state.startupFailed
              ? html`<button type="button" class="ghost small pretry" data-fk=${startupFocus.retry} @click=${actions.retryStartup}>
                  ${fr.startup.retry}
                </button>`
              : nothing}`
          : html`${header(view, actions)} ${userSection(state, view, actions)} ${machineSection(state, view, actions)}
            <p class="note pnote">${fr.startup.note}</p>
            ${sayZone(state, actions)}`}
      </div>
    </div>`,
  });
}
