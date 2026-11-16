// La page Confidentialité : des traces de l'activité de la personne sur ce PC, dans son
// compte, par catégories. Elle supprime celles qui sont cochées, après une confirmation,
// et dit pour chaque bloc ce qui reste malgré tout.
//
// Elle ne montre jamais le contenu d'une trace (ni adresse, ni nom de fichier) : seulement
// qu'elle existe, son poids ou son nombre d'éléments, et ce qu'on perd en l'effaçant. Elle
// n'envoie jamais un nom ni un chemin : seulement l'identifiant que le cœur a donné à la trace.

import { html, nothing, type TemplateResult } from 'lit-html';
import { ref } from 'lit-html/directives/ref.js';
import { repeat } from 'lit-html/directives/repeat.js';

import type { PrivacyGroup, PrivacyView, TraceItem } from '../backend/types.ts';
import { fr } from '../i18n/fr.ts';
import {
  browserSections,
  checkLabel,
  groupTitle,
  hasBrowser,
  type BrowserSection,
  nothingToErase,
  otherGroups,
  outcomeHelp,
  outcomeLine,
  outcomeShort,
  privacyFocus,
  profileLabel,
  remainsOfBrowsers,
  remainsOfWindows,
  rowKind,
  traceName,
  traceSize,
  WINDOWS_PAGES,
} from '../model/privacy.ts';
import type { Actions, AppState } from '../state.ts';
import { checkIcon, moduleArt } from './art.ts';
import { closesOnVeil, openModal } from './modal.ts';
import { stage } from './parts.ts';

type Content = TemplateResult | typeof nothing;

/** Un effacement est parti : tout attend sa réponse. */
const isBusy = (state: AppState): boolean => state.privacyBusy;

// ── En-tête ────────────────────────────────────────────────────────────────

function eraseButton(state: AppState, actions: Actions, key: string | null): TemplateResult {
  // Inerte aussi pendant la confirmation : la boîte tient la page, la barre ne répond pas derrière elle.
  const inert = state.privacyBusy || state.privacyAsk !== null || state.privacyChecked.length === 0;
  return html`<button
    type="button"
    class="pill small"
    data-fk=${key ?? nothing}
    aria-disabled=${inert ? 'true' : nothing}
    aria-describedby=${state.privacyChecked.length === 0 ? 'privacy-hint' : nothing}
    @click=${() => (inert ? undefined : actions.askErase())}
  >
    ${fr.privacy.erase}
  </button>`;
}

function header(state: AppState, view: PrivacyView, actions: Actions): TemplateResult {
  const none = state.privacyChecked.length === 0;
  return html`<header class="phead">
    <div class="part">${moduleArt('privacy')}</div>
    <div class="ptx">
      <p class="eb">${fr.privacy.eyebrow}</p>
      <h1 tabindex="-1" data-fk=${privacyFocus.title}>${fr.privacy.title}</h1>
      <p class="ptext">${fr.privacy.intro}</p>
      <div class="pline">
        ${eraseButton(state, actions, privacyFocus.erase)}
        ${none
          ? html`<p class="d" id="privacy-hint">${nothingToErase(view) ? fr.privacy.nothingNow : fr.privacy.eraseHint}</p>`
          : html`<p class="d">${fr.privacy.checked(state.privacyChecked.length)}</p>`}
      </div>
    </div>
  </header>`;
}

// ── Ce qui vient d'arriver ─────────────────────────────────────────────────

/** Un refus du cœur, ou l'issue de chaque trace du dernier effacement : lisible, jamais seulement une couleur. */
function lastCard(state: AppState): Content {
  const refused = state.privacyRefused;
  const lines = state.privacyLast;
  return html`${refused === null
    ? nothing
    : html`<div class="card pcard">
        <p class="pwarn"><i aria-hidden="true"></i><span>${fr.privacy.refused[refused]}</span></p>
      </div>`}
  ${lines.length === 0
    ? nothing
    : html`<section class="psec" aria-labelledby="privacy-last-title">
        <h2 class="sub flat" id="privacy-last-title">${fr.privacy.lastTitle}</h2>
        <div class="card pcard">
          <ul class="plist">
            ${repeat(
              lines,
              (line) => line.id,
              (line) => {
                const help = outcomeHelp(line);
                return html`<li class="row static uleft plast">
                  <span class="tx">
                    <span class="t1"
                      >${line.browser === null || line.profile === null
                        ? line.block
                        : html`${line.browser} · <bdi>${line.profile}</bdi>`}</span
                    >
                    <span class="t2">${line.trace}</span>
                    ${help === null ? nothing : html`<span class="t2 phelp">${help}</span>`}
                  </span>
                  <span class="val pres ${line.outcome === 'erased' || line.outcome === 'nothing' ? '' : 'pbad'}">${outcomeShort(line.outcome)}</span>
                </li>`;
              },
            )}
          </ul>
        </div>
      </section>`}`;
}

// ── Lignes ─────────────────────────────────────────────────────────────────

function outcomeOf(state: AppState, item: TraceItem): Content {
  const line = state.privacyLast.find((one) => one.id === item.id);
  return line === undefined ? nothing : html`<span class="t2 pout" id="${rowId(item)}-o">${outcomeLine(line)}</span>`;
}

const rowId = (item: TraceItem): string => `privacy-${item.id.replace(/[^\w-]/g, '_')}`;

/**
 * Une ligne. Navigateur : le nom du profil (tel quel) en premier, « Historique de navigation » en second.
 * Windows : le nom de la trace, puis ce qu'on perd.
 */
function row(state: AppState, group: PrivacyGroup, item: TraceItem, actions: Actions): TemplateResult {
  const base = rowId(item);
  const size = traceSize(item);
  const kind = rowKind(group, item);
  const browser = group.kind === 'browser';
  const view = state.privacy ?? undefined;
  const title = browser ? html`<bdi>${profileLabel(group, view)}</bdi>` : traceName(item.kind);
  const sub = kind === 'unreadable' ? fr.privacy.unreadable : browser ? traceName(item.kind) : fr.privacy.loses[item.kind as keyof typeof fr.privacy.loses];
  if (kind !== 'check') {
    // Ni case, ni geste : la ligne dit pourquoi. La place de la case reste, pour que les noms restent alignés.
    return html`<li>
      <div class="row uleft static ${kind === 'empty' ? 'pempty' : 'pinert'}">
        <span class="box ugone" aria-hidden="true"></span>
        <span class="tx">
          <span class="t1">${title}</span>
          ${kind === 'empty' && !browser ? nothing : html`<span class="t2">${sub}</span>`}
          ${outcomeOf(state, item)}
        </span>
        ${kind === 'empty'
          ? html`<span class="val usize pnone">${fr.privacy.nothing}</span>`
          : size === null
            ? nothing
            : html`<span class="val usize">${size}</span>`}
      </div>
    </li>`;
  }
  const checked = state.privacyChecked.includes(item.id);
  return html`<li>
    <label class="row uleft">
      <input
        type="checkbox"
        data-fk=${privacyFocus.check(item.id)}
        .checked=${checked}
        aria-label=${checkLabel(group, item, state.privacy ?? undefined)}
        aria-describedby="${base}-d${state.privacyLast.some((one) => one.id === item.id) ? ` ${base}-o` : ''}"
        aria-disabled=${isBusy(state) ? 'true' : nothing}
        @click=${(event: Event) => {
          // Pendant un effacement, la case ne bouge pas : l'écran et l'état disent la même chose.
          if (isBusy(state)) {
            event.preventDefault();
          }
        }}
        @change=${() => actions.toggleTrace(item.id)}
      />
      <span class="box">${checkIcon()}</span>
      <span class="tx">
        <span class="t1">${title}</span>
        <span class="t2" id="${base}-d">${sub}</span>
        ${outcomeOf(state, item)}
      </span>
      ${size === null ? nothing : html`<span class="val usize">${size}</span>`}
    </label>
  </li>`;
}

// ── Blocs ──────────────────────────────────────────────────────────────────

/** « Ce qui reste », une fois : le même texte vaut pour tous les profils. Les lignes d'aide suivent, sur le même bord gauche. */
function remains(texts: readonly string[], extras: readonly string[]): TemplateResult {
  return html`<div class="premain">
    ${texts.length === 0
      ? nothing
      : html`<p class="plabel">${fr.privacy.remainsLabel}</p>
          ${texts.map((text) => html`<p class="d">${text}</p>`)}`}
    ${extras.map((text) => html`<p class="d">${text}</p>`)}
  </div>`;
}

/** La carte d'un navigateur : son nom, la phrase « Ferme… » s'il est ouvert, une ligne par profil. */
function browserCard(state: AppState, section: BrowserSection, actions: Actions): TemplateResult {
  const title = `privacy-browser-${section.app}`;
  return html`<div class="card pcard pbrowser" role="group" aria-labelledby=${title}>
    <h3 class="pbname" id=${title}>${section.name}</h3>
    ${section.running
      ? html`<p class="pwarn"><i aria-hidden="true"></i><span>${fr.privacy.running(section.name)}</span></p>`
      : nothing}
    <ul class="plist">
      ${repeat(
        section.groups,
        (group) => group.key,
        (group) =>
          group.items.map((item) => row(state, group, item, actions)),
      )}
    </ul>
  </div>`;
}

function browsers(state: AppState, view: PrivacyView, actions: Actions): Content {
  if (!hasBrowser(view) && !view.firefox) {
    return nothing;
  }
  return html`<section class="psec pblock" aria-labelledby="privacy-browsers">
    <h2 class="sub flat" id="privacy-browsers">${fr.privacy.browsers.title}</h2>
    ${hasBrowser(view) ? html`<p class="d pintro">${fr.privacy.browsers.intro}</p>` : nothing}
    ${repeat(
      browserSections(view),
      (section) => section.app,
      (section) => browserCard(state, section, actions),
    )}
    ${remains(
      hasBrowser(view) ? remainsOfBrowsers(view) : [],
      [...(view.firefox ? [fr.privacy.firefox] : []), ...(hasBrowser(view) ? [fr.privacy.cache] : [])],
    )}
  </section>`;
}

function windowsBlock(state: AppState, group: PrivacyGroup, index: number, actions: Actions): TemplateResult {
  const title = `privacy-block-${index}`;
  return html`<section class="psec pblock" aria-labelledby=${title}>
    <h2 class="sub flat" id=${title}>${groupTitle(group)}</h2>
    <div class="card pcard">
      <ul class="plist">
        ${repeat(
          group.items,
          (item) => item.id,
          (item) => row(state, group, item, actions),
        )}
      </ul>
    </div>
    ${remains(remainsOfWindows(), [])}
  </section>`;
}

// ── Dans Windows ───────────────────────────────────────────────────────────

function inWindows(actions: Actions): TemplateResult {
  return html`<section class="psec" aria-labelledby="privacy-inwindows">
    <h2 class="sub flat" id="privacy-inwindows">${fr.privacy.inWindows.title}</h2>
    <p class="d pintro">${fr.privacy.inWindows.intro}</p>
    <div class="card pcard">
      <ul class="plist">
        ${WINDOWS_PAGES.map((page) => {
          const entry = fr.privacy.inWindows.pages[page];
          return html`<li class="row static uleft pwin">
            <span class="tx">
              <span class="t1" id="privacy-win-${page}-n">${entry.name}</span>
              <span class="t2">${entry.text}</span>
            </span>
            <button
              type="button"
              class="ghost small"
              id="privacy-win-${page}-b"
              aria-labelledby="privacy-win-${page}-b privacy-win-${page}-n"
              @click=${() => actions.openWindowsSettings(page)}
            >
              ${fr.privacy.inWindows.open}
            </button>
          </li>`;
        })}
      </ul>
    </div>
  </section>`;
}

// ── Ce que Shinku ne touche pas ────────────────────────────────────────────

function untouched(): TemplateResult {
  return html`<section class="psec">
    <details class="card pfold">
      <summary>${fr.privacy.untouched.title}</summary>
      <div class="pfoldbody">
        <p class="d">${fr.privacy.untouched.intro}</p>
        <ul class="pitems">
          ${Object.values(fr.privacy.untouched.items).map((text) => html`<li class="d">${text}</li>`)}
        </ul>
        <p class="d">${fr.privacy.untouched.outside}</p>
      </div>
    </details>
  </section>`;
}

// ── La barre de retour et la région vivante ────────────────────────────────

/** Ce que les lecteurs d'écran entendent : l'attente, puis les issues. La région est toujours là, hors écran. */
function live(state: AppState): TemplateResult {
  return html`<div class="sr" aria-live="polite">${state.privacyBusy ? fr.privacy.working : state.privacyAnnounce}</div>`;
}

/** En bas de la zone qui défile : le compte de cases cochées et le même bouton, pour ne pas remonter. */
function zone(state: AppState, actions: Actions): Content {
  if (state.privacyChecked.length === 0 && !state.privacyBusy) {
    return nothing;
  }
  return html`<div class="pzone">
    <p class="psay">
      <span>${state.privacyBusy ? fr.privacy.working : fr.privacy.checked(state.privacyChecked.length)}</span>
      ${eraseButton(state, actions, null)}
    </p>
  </div>`;
}

// ── La confirmation ────────────────────────────────────────────────────────

export function privacyDialog(state: AppState, actions: Actions): Content {
  const ask = state.privacyAsk;
  if (ask === null) {
    return nothing;
  }
  // Échap : c'est l'application qui referme, pour que l'écran et son état disent la même chose.
  const onCancel = (event: Event): void => {
    event.preventDefault();
    actions.dismissErase();
  };
  // Un clic à côté de la boîte la referme : il tombe sur le voile, donc sur le dialogue lui-même.
  const onClick = (event: Event): void => {
    if (closesOnVeil(event)) {
      actions.dismissErase();
    }
  };
  return html`<dialog
    class="ask"
    aria-labelledby="privacy-ask-title"
    aria-describedby="privacy-ask-text"
    ${ref(openModal)}
    @cancel=${onCancel}
    @close=${actions.dismissErase}
    @click=${onClick}
  >
    <div class="askbox">
      <h2 id="privacy-ask-title">${ask.ids.length === 1 ? fr.privacy.confirm.titleOne : fr.privacy.confirm.title}</h2>
      <div id="privacy-ask-text">
        <p class="askintro">${fr.privacy.confirm.intro}</p>
        ${ask.blocks.map(
          (one) => html`<div class="askblock">
            <p class="askname">${one.title}</p>
            <ul class="asklist">
              ${one.traces.map(
                (trace) =>
                  html`<li>${trace.profile === null ? trace.name : html`<bdi>${trace.profile}</bdi>${trace.tail}`}</li>`,
              )}
            </ul>
          </div>`,
        )}
        <p class="askintro askdirect">${ask.ids.length === 1 ? fr.privacy.confirm.directOne : fr.privacy.confirm.direct}</p>
      </div>
      <div class="askbtns">
        <button type="button" class="ghost" autofocus @click=${actions.dismissErase}>${fr.privacy.confirm.cancel}</button>
        <button type="button" class="pill" @click=${actions.confirmErase}>${fr.privacy.confirm.confirm}</button>
      </div>
    </div>
  </dialog>`;
}

// ── La page ────────────────────────────────────────────────────────────────

export function privacyView(state: AppState, actions: Actions): TemplateResult {
  const view = state.privacy;
  return stage({
    body: html`<div class="scroll ${state.privacyChecked.length === 0 && !state.privacyBusy ? '' : 'sayon'}">
      <div class="page startup privacy" aria-busy=${state.privacyBusy ? 'true' : nothing}>
        ${live(state)}
        ${view === null
          ? html`<p class="note" role="status">${state.privacyFailed ? fr.toast.unexpected : fr.common.loading}</p>
            ${state.privacyFailed
              ? html`<button type="button" class="ghost small pretry" data-fk=${privacyFocus.retry} @click=${actions.retryPrivacy}>
                  ${fr.privacy.retry}
                </button>`
              : nothing}`
          : html`${header(state, view, actions)}
            ${state.privacyBusy ? html`<p class="note pwait">${fr.privacy.working}</p>` : nothing}
            ${lastCard(state)}
            ${browsers(state, view, actions)}
            ${repeat(
              otherGroups(view),
              (group) => group.key,
              (group) => windowsBlock(state, group, view.groups.indexOf(group), actions),
            )}
            ${inWindows(actions)} ${untouched()} ${zone(state, actions)}`}
      </div>
    </div>`,
  });
}
