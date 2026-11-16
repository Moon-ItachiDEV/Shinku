// La page Désinstallation : les apps installées, le désinstalleur de l'éditeur
// qui travaille à l'écran, puis ce que l'app a laissé dans le compte, que la
// personne met à la corbeille si elle le veut.
//
// Elle n'envoie jamais un chemin ni une commande : seulement l'identifiant que
// le cœur a donné à l'app, et les clés qu'il a données aux restes.

import { html, nothing, type TemplateResult } from 'lit-html';
import { keyed } from 'lit-html/directives/keyed.js';
import { ref } from 'lit-html/directives/ref.js';
import { repeat } from 'lit-html/directives/repeat.js';

import type { AppRow, AppsView, FollowView, LeftoverRow } from '../backend/types.ts';
import { splitPath } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import {
  APPS_SORTS,
  type AppsSort,
  checkable,
  checkedRows,
  chipOf,
  confirmExtras,
  detailParts,
  dockTexts,
  failureText,
  initialOf,
  leftoverSize,
  leftoverText,
  noneBecause,
  proposed,
  rowEnd,
  screenOf,
  sceneTexts,
  listedApps,
  sizeOf,
  tallyOf,
  uninstallFocus,
} from '../model/uninstall.ts';
import type { Actions, AppState } from '../state.ts';
import { checkIcon, crossIcon, doneArt, moduleArt, ring } from './art.ts';
import { closesOnVeil, openModal } from './modal.ts';
import { stage } from './parts.ts';

type Content = TemplateResult | typeof nothing;

/** Un geste en vol : les boutons attendent la réponse du cœur, un seul geste à la fois. */
const isBusy = (state: AppState): boolean => state.appsBusy !== null;

// ── En-tête ────────────────────────────────────────────────────────────────

function openWindows(actions: Actions): TemplateResult {
  return html`<button type="button" class="ghost small" @click=${() => actions.openWindowsSettings('apps')}>
    ${fr.uninstall.openWindows}
  </button>`;
}

function header(view: AppsView, actions: Actions): TemplateResult {
  return html`<header class="phead">
    <div class="part">${moduleArt('uninstall')}</div>
    <div class="ptx">
      <p class="eb">${fr.uninstall.eyebrow}</p>
      <h1 tabindex="-1" data-fk=${uninstallFocus.title}>${fr.uninstall.title(view.apps.length)}</h1>
      <p class="ptext">${fr.uninstall.intro}</p>
      <div class="pline">
        <p class="d">${fr.uninstall.store}</p>
        ${openWindows(actions)}
      </div>
    </div>
  </header>`;
}

// ── Barre d'outils : la recherche et le tri ────────────────────────────────

const SORT_LABELS: Record<AppsSort, string> = {
  name: fr.uninstall.sortName,
  size: fr.uninstall.sortSize,
  date: fr.uninstall.sortDate,
};

function search(state: AppState, actions: Actions): TemplateResult {
  const clear = (): void => {
    actions.setAppsQuery('');
    // Le bouton disparaît avec le texte : le focus revient au champ.
    document.querySelector<HTMLElement>(`[data-fk="${uninstallFocus.search}"]`)?.focus();
  };
  return html`<div class="usearch">
    <input
      type="search"
      class="uinput"
      data-fk=${uninstallFocus.search}
      aria-label=${fr.uninstall.searchLabel}
      placeholder=${fr.uninstall.searchLabel}
      autocomplete="off"
      spellcheck="false"
      .value=${state.appsQuery}
      @input=${(event: Event) => actions.setAppsQuery((event.target as HTMLInputElement).value)}
      @keydown=${(event: KeyboardEvent) => {
        if (event.key === 'Escape' && state.appsQuery !== '') {
          // Échap vide le champ ; sans texte, il laisse la fenêtre faire ce qu'elle fait.
          event.preventDefault();
          actions.setAppsQuery('');
        }
      }}
    />
    ${state.appsQuery === ''
      ? nothing
      : html`<button type="button" class="icb uclear" aria-label=${fr.uninstall.clearSearch} @click=${clear}>
          ${crossIcon()}
        </button>`}
  </div>`;
}

/** Les flèches déplacent le choix, comme dans tout groupe de boutons radio : le focus suit. */
function onSortKey(event: KeyboardEvent, current: AppsSort, actions: Actions): void {
  const step = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
  const home = event.key === 'Home';
  const end = event.key === 'End';
  if (step === 0 && !home && !end) {
    return;
  }
  event.preventDefault();
  const index = APPS_SORTS.indexOf(current);
  const next = home ? 0 : end ? APPS_SORTS.length - 1 : (index + step + APPS_SORTS.length) % APPS_SORTS.length;
  const sort = APPS_SORTS[next];
  if (sort === undefined) {
    return;
  }
  actions.setAppsSort(sort);
  // Le bouton existe déjà : le focus passe tout de suite, sans attendre le dessin.
  const group = (event.currentTarget as HTMLElement).parentElement;
  group?.querySelector<HTMLElement>(`[data-fk="${uninstallFocus.sort(sort)}"]`)?.focus();
}

function sortGroup(state: AppState, actions: Actions): TemplateResult {
  return html`<div class="usort">
    <span class="usortl" id="uninstall-sort-label">${fr.uninstall.sortLabel}</span>
    <div class="segc" role="radiogroup" aria-labelledby="uninstall-sort-label">
      ${APPS_SORTS.map(
        (sort) => html`<button
          type="button"
          role="radio"
          data-fk=${uninstallFocus.sort(sort)}
          aria-checked=${state.appsSort === sort ? 'true' : 'false'}
          tabindex=${state.appsSort === sort ? '0' : '-1'}
          @click=${() => actions.setAppsSort(sort)}
          @keydown=${(event: KeyboardEvent) => onSortKey(event, state.appsSort, actions)}
        >
          ${SORT_LABELS[sort]}
        </button>`,
      )}
    </div>
  </div>`;
}

// ── Les lignes ─────────────────────────────────────────────────────────────

/** La fin d'une ligne : le bouton « Désinstaller », ou la raison de son absence. */
function rowControl(state: AppState, app: AppRow, actions: Actions): TemplateResult {
  switch (rowEnd(app)) {
    case 'button':
      return html`<span class="uend">
        <button
          type="button"
          class="ghost small"
          data-fk=${uninstallFocus.row(app.id)}
          aria-label=${fr.uninstall.uninstallLabel(app.name)}
          aria-disabled=${isBusy(state) ? 'true' : nothing}
          @click=${() => (isBusy(state) ? undefined : actions.askUninstall(app.id))}
        >
          ${fr.uninstall.uninstall}
        </button>
      </span>`;
    case 'refused':
      return html`<span class="d uend uno" title=${fr.uninstall.refusedTitle}>${fr.uninstall.refused}</span>`;
    case 'own':
      return html`<span class="d uend uno">${fr.uninstall.own}</span>`;
    case 'needed':
      return html`<span class="d uend uno" title=${fr.uninstall.neededTitle}>${fr.uninstall.needed}</span>`;
  }
}

function appRow(state: AppState, app: AppRow, actions: Actions): TemplateResult {
  const details = detailParts(app, state.appsSort, state.now).join(fr.uninstall.separator);
  const chip = chipOf(app);
  const size = sizeOf(app);
  return html`<li class="row static">
    <span class="av" aria-hidden="true">${initialOf(app.name)}</span>
    <span class="tx">
      <span class="t1 path" title=${app.name}>${app.name}</span>
      <span class="t2 path" title=${details}>${details}</span>
    </span>
    ${chip === null ? nothing : html`<span class="tag" title=${chip}>${chip}</span>`}
    ${size === null ? nothing : html`<span class="val usize">${size}</span>`} ${rowControl(state, app, actions)}
  </li>`;
}

function appsCard(state: AppState, view: AppsView, actions: Actions): Content {
  const shown = listedApps(view, state.appsQuery, state.appsSort);
  if (view.apps.length === 0 && view.complete) {
    return nothing;
  }
  return html`<div class="card ucard">
    ${view.complete
      ? nothing
      : html`<div class="pwarn">
          <i aria-hidden="true"></i><span>${fr.uninstall.incomplete}</span>
          <button type="button" class="ghost small" data-fk=${uninstallFocus.retry} @click=${actions.retryApps}>
            ${fr.uninstall.retry}
          </button>
        </div>`}
    ${shown.length === 0
      ? view.apps.length === 0
        ? nothing
        : html`<p class="d unone" role="status">${fr.uninstall.noMatch(state.appsQuery.trim())}</p>`
      : html`<ul class="plist">
          ${repeat(
            shown,
            (app) => app.id,
            (app) => appRow(state, app, actions),
          )}
        </ul>`}
  </div>`;
}

// ── L'écran de la liste ────────────────────────────────────────────────────

function listScreen(state: AppState, actions: Actions): TemplateResult {
  const view = state.apps;
  return html`<div class="scroll">
    <div class="page startup uninstall">
      ${view === null
        ? html`<p class="note" role="status">${state.appsFailed ? fr.toast.unexpected : fr.common.loading}</p>
            ${state.appsFailed
              ? html`<button type="button" class="ghost small pretry" data-fk=${uninstallFocus.retry} @click=${actions.retryApps}>
                  ${fr.uninstall.retry}
                </button>`
              : nothing}`
        : html`${header(view, actions)}
            ${view.apps.length === 0
              ? nothing
              : html`<div class="utool">${search(state, actions)} ${sortGroup(state, actions)}</div>`}
            ${appsCard(state, view, actions)}
            <p class="note pnote">${fr.uninstall.note}</p>`}
    </div>
  </div>`;
}

// ── L'attente ──────────────────────────────────────────────────────────────

/** Le cœur du cercle : « Revenir à la liste » ; pendant `launching`, rien (il n'y a pas de bouton). */
function sceneCore(state: AppState, follow: FollowView, actions: Actions): TemplateResult {
  if (follow.phase === 'launching') {
    return html`<span class="core inert" aria-hidden="true"></span>`;
  }
  return html`<button
    type="button"
    class="core"
    data-fk=${uninstallFocus.back}
    aria-disabled=${isBusy(state) ? 'true' : nothing}
    @click=${() => (isBusy(state) ? undefined : actions.stopUninstall())}
  >
    <span>${fr.uninstall.scene.back}</span>
  </button>`;
}

function sceneScreen(follow: FollowView): TemplateResult {
  const name = follow.phase === 'idle' ? '' : follow.name;
  return html`<div class="hero uscene enter">
    ${moduleArt('uninstall', true)}
    <h1 tabindex="-1" data-fk=${uninstallFocus.title}>${fr.uninstall.scene.title(name)}</h1>
    ${sceneTexts(follow).map((text) => html`<p>${text}</p>`)}
  </div>`;
}

function sceneDock(state: AppState, follow: FollowView, actions: Actions): TemplateResult {
  return html`<div class="dock">${ring('uninstall-r', null, sceneCore(state, follow, actions))}</div>`;
}

// ── Les restes ─────────────────────────────────────────────────────────────

/** Un chemin coupé au milieu : le début s'abrège, le dernier nom reste lisible ; le chemin entier est dans `title`. */
function middlePath(path: string): TemplateResult {
  const { name } = splitPath(path);
  const head = name !== '' && path.endsWith(name) ? path.slice(0, path.length - name.length) : '';
  return html`<span class="umid" title=${path}
    >${head === '' ? nothing : html`<span class="umidh">${head}</span>`}<span class="umidt">${head === '' ? path : name}</span></span
  >`;
}

function leftoverRow(state: AppState, row: LeftoverRow, actions: Actions): TemplateResult {
  const checked = state.appsChecked.includes(row.path);
  const failure = state.appsFailures[row.path];
  const size = leftoverSize(row);
  const base = `uninstall-left-${row.key}`;
  if (failure === 'destroyed') {
    // Windows l'a supprimé pour de bon : il n'y a plus rien à cocher ni à envoyer.
    return html`<li>
      <div class="row uleft static">
        <span class="box ugone" aria-hidden="true"></span>
        <span class="tx">
          <span class="t1">${middlePath(row.path)}</span>
          <span class="t2">${leftoverText(row)}</span>
          <span class="t2 ufail">${failureText(failure)}</span>
        </span>
        ${size === null ? nothing : html`<span class="val usize">${size}</span>`}
      </div>
    </li>`;
  }
  return html`<li>
    <label class="row uleft">
      <input
        type="checkbox"
        data-fk=${uninstallFocus.leftover(row.key)}
        .checked=${checked}
        aria-label=${row.path}
        aria-describedby="${base}-d${failure === undefined ? '' : ` ${base}-f`}"
        aria-disabled=${isBusy(state) ? 'true' : nothing}
        @click=${(event: Event) => {
          // Pendant un geste, la case ne bouge pas : l'écran et l'état disent la même chose.
          if (isBusy(state)) {
            event.preventDefault();
          }
        }}
        @change=${() => actions.toggleLeftover(row.path)}
      />
      <span class="box">${checkIcon()}</span>
      <span class="tx">
        <span class="t1" id="${base}-p">${middlePath(row.path)}</span>
        <span class="t2" id="${base}-d">${leftoverText(row)}</span>
        ${failure === undefined ? nothing : html`<span class="t2 ufail" id="${base}-f">${failureText(failure)}</span>`}
      </span>
      ${size === null ? nothing : html`<span class="val usize">${size}</span>`}
    </label>
  </li>`;
}

/** Une carte, une ligne par reste, rien de coché d'avance. */
function leftoversCard(state: AppState, rows: readonly LeftoverRow[], actions: Actions): TemplateResult {
  return html`<div class="card ucard">
    <ul class="plist">
      ${repeat(
        rows,
        (row) => row.path,
        (row) => leftoverRow(state, row, actions),
      )}
    </ul>
  </div>`;
}

function leftoversHeader(name: string, text: string): TemplateResult {
  return html`<header class="phead">
    <div class="part">${doneArt('uninstall-done')}</div>
    <div class="ptx">
      <p class="eb">${fr.uninstall.eyebrow}</p>
      <h1 tabindex="-1" data-fk=${uninstallFocus.title}>${fr.uninstall.leftovers.title(name)}</h1>
      <p class="ptext">${text}</p>
    </div>
  </header>`;
}

function leftoversScreen(state: AppState, follow: FollowView, actions: Actions): TemplateResult {
  if (follow.phase !== 'removed') {
    return html`${nothing}`;
  }
  const because = noneBecause(follow);
  const rows = proposed(follow);
  // Ceux que Windows a supprimés pour de bon restent à l'écran, après ce qui est encore là.
  const shown = [...rows, ...state.appsDestroyed.filter((gone) => !rows.some((row) => row.path === gone.path))];
  const text =
    because === 'family'
      ? fr.uninstall.leftovers.family
      : because === 'unsure'
        ? fr.uninstall.leftovers.unsure
        : because === 'none'
          ? state.appsEmptied
            ? fr.uninstall.leftovers.allGone
            : fr.uninstall.leftovers.none
          : fr.uninstall.leftovers.found(rows.length);
  return html`<div class="scroll enter">
    <div class="page startup uninstall">
      ${leftoversHeader(follow.name, text)}
      ${shown.length > 0 ? leftoversCard(state, shown, actions) : nothing}
    </div>
  </div>`;
}

/**
 * Le socle des restes : l'orbe, sa légende, et les deux boutons discrets. Sans reste à proposer, l'orbe devient
 * « Terminé » (actif, sans légende), avec « Chercher à nouveau » à sa gauche sauf si une app du même nom reste.
 */
function leftoversDock(state: AppState, follow: FollowView, actions: Actions): Content {
  if (follow.phase !== 'removed') {
    return nothing;
  }
  const rows = checkable(proposed(follow), state.appsFailures);
  const busy = isBusy(state);
  if (rows.length === 0) {
    return html`<div class="dock udock">
      <div class="row2">
        ${noneBecause(follow) === 'family'
          ? nothing
          : html`<button
              type="button"
              class="ghost small uleftb"
              data-fk=${uninstallFocus.rescan}
              aria-disabled=${busy ? 'true' : nothing}
              @click=${() => (busy ? undefined : actions.rescanLeftovers())}
            >
              ${fr.uninstall.leftovers.rescan}
            </button>`}
        <button
          type="button"
          class="orb uorb"
          data-fk=${uninstallFocus.done}
          aria-disabled=${busy ? 'true' : nothing}
          @click=${() => (busy ? undefined : actions.stopUninstall())}
        >
          <span>${fr.uninstall.leftovers.done}</span>
        </button>
      </div>
    </div>`;
  }
  const tally = tallyOf(checkedRows(rows, state.appsChecked));
  const inactive = busy || tally.count === 0;
  return html`<div class="dock udock">
    <div class="row2">
      <button
        type="button"
        class="ghost small uleftb"
        data-fk=${uninstallFocus.rescan}
        aria-disabled=${busy ? 'true' : nothing}
        @click=${() => (busy ? undefined : actions.rescanLeftovers())}
      >
        ${fr.uninstall.leftovers.rescan}
      </button>
      <button
        type="button"
        class="orb uorb"
        data-fk=${uninstallFocus.orb}
        aria-describedby="uninstall-cap"
        aria-disabled=${inactive ? 'true' : nothing}
        @click=${() => (inactive ? undefined : actions.removeLeftovers())}
      >
        <span>${fr.uninstall.leftovers.trash}</span>
      </button>
      <button
        type="button"
        class="ghost small urightb"
        data-fk=${uninstallFocus.done}
        aria-disabled=${busy ? 'true' : nothing}
        @click=${() => (busy ? undefined : actions.stopUninstall())}
      >
        ${fr.uninstall.leftovers.done}
      </button>
    </div>
    <p class="cap" id="uninstall-cap" role="status">
      ${dockTexts(tally).map((text) => html`<span class="ucap">${text}</span>`)}
    </p>
  </div>`;
}

// ── La confirmation ────────────────────────────────────────────────────────

export function uninstallDialog(state: AppState, actions: Actions): Content {
  const ask = state.appsConfirm;
  if (ask === null) {
    return nothing;
  }
  // Échap : c'est l'application qui referme, pour que l'écran et son état disent la même chose.
  const onCancel = (event: Event): void => {
    event.preventDefault();
    actions.dismissUninstall();
  };
  // Un clic à côté de la boîte la referme : il tombe sur le voile, donc sur le dialogue lui-même.
  const onClick = (event: Event): void => {
    if (closesOnVeil(event)) {
      actions.dismissUninstall();
    }
  };
  return html`<dialog
    class="ask"
    aria-labelledby="uninstall-ask-title"
    aria-describedby="uninstall-ask-text"
    ${ref(openModal)}
    @cancel=${onCancel}
    @close=${actions.dismissUninstall}
    @click=${onClick}
  >
    <div class="askbox">
      <h2 id="uninstall-ask-title">${fr.uninstall.confirm.title(ask.name)}</h2>
      <div id="uninstall-ask-text">
        <p class="askintro">${fr.uninstall.confirm.text(ask.name)}</p>
        ${confirmExtras(ask).map((text) => html`<p class="askintro uextra">${text}</p>`)}
      </div>
      <div class="askbtns">
        <button type="button" class="ghost" autofocus @click=${actions.dismissUninstall}>
          ${fr.uninstall.confirm.cancel}
        </button>
        <button type="button" class="pill" @click=${actions.confirmUninstall}>${fr.uninstall.confirm.confirm}</button>
      </div>
    </div>
  </dialog>`;
}

// ── La page ────────────────────────────────────────────────────────────────

export function uninstallView(state: AppState, actions: Actions): TemplateResult {
  const follow = state.apps?.follow;
  const screen = screenOf(follow);
  // La région vivante ne change jamais de place : seule sa phrase change, et les lecteurs d'écran l'annoncent.
  const live = html`<div class="sr" aria-live="polite">${state.appsAnnounce}</div>`;
  // Chaque écran est reconstruit quand la phase en change : il entre en fondu, le titre reçoit le focus.
  const body =
    follow === undefined || screen === 'list'
      ? keyed('list', listScreen(state, actions))
      : screen === 'scene'
        ? keyed('scene', sceneScreen(follow))
        : keyed('leftovers', leftoversScreen(state, follow, actions));
  const dock =
    follow === undefined || screen === 'list'
      ? nothing
      : screen === 'scene'
        ? sceneDock(state, follow, actions)
        : leftoversDock(state, follow, actions);
  return stage({ body: html`${live} ${body}`, dock });
}
