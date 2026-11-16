// La page Journal : ce que Shinku a noté de ce qu'il a fait, en une liste, la plus récente en haut.
//
// Elle ne fait rien : chaque ligne a, à droite, le chemin du retour quand il existe (la page où le geste est déjà).
// Aucun chemin ni nom de fichier n'y figure ; un nom d'app est du texte, montré tel quel (lit-html l'échappe),
// coupé par le style s'il est long, entier dans `title`.

import { html, nothing, type TemplateResult } from 'lit-html';
import { ref } from 'lit-html/directives/ref.js';

import { formatSince } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import { hasUninstalls, isoOf, journalFocus, type JournalRow, rowOf } from '../model/journal.ts';
import type { Actions, AppState } from '../state.ts';
import { navIcon } from './art.ts';
import { closesOnVeil, openModal } from './modal.ts';
import { stage } from './parts.ts';

type Content = TemplateResult | typeof nothing;

function when(row: JournalRow, now: number): TemplateResult {
  if (row.date === null) {
    return html`<span class="jdate">${fr.journal.unknownDate}</span>`;
  }
  return html`<time class="jdate" datetime=${isoOf(row.date) ?? nothing}>${formatSince(row.date, now)}</time>`;
}

function line(row: JournalRow, now: number, actions: Actions): TemplateResult {
  return html`<li class="row static jrow">
    <span class="ricon" aria-hidden="true">${navIcon(row.icon, 16)}</span>
    <span class="tx"
      ><span class="t1 jsentence"
        >${row.before}${row.name === null
          ? nothing
          : html`<bdi class="jname" title=${row.name.name}>${row.name.name}</bdi>`}${row.after}</span
      ><span class="t2">${when(row, now)}</span></span
    >
    ${row.link === null
      ? nothing
      : html`<button type="button" class="ghost small jlink" @click=${() => actions.navigate(row.link?.route ?? 'journal')}>
          ${row.link.label}
        </button>`}
  </li>`;
}

function body(state: AppState, actions: Actions): Content {
  const view = state.history;
  if (view === null) {
    return state.historyFailed
      ? html`<div class="card jcard">
          <p class="pwarn"><i aria-hidden="true"></i><span role="alert">${fr.journal.failed}</span></p>
          <p class="jretry">
            <button type="button" class="ghost small" data-fk=${journalFocus.retry} @click=${actions.retryJournal}>
              ${fr.journal.retry}
            </button>
          </p>
        </div>`
      : html`<p class="d jwait" role="status">${fr.journal.loading}</p>`;
  }
  return html`${view.lines.length === 0
      ? html`<div class="card jcard"><p class="jempty">${fr.journal.empty}</p></div>`
      : html`<div class="card jcard">
          <ul class="plist jlist" aria-label=${fr.journal.list}>
            ${view.lines.map((one) => line(rowOf(one, state.now), state.now, actions))}
          </ul>
        </div>`}
    ${view.more ? html`<p class="d jmore">${fr.journal.more}</p>` : nothing}
    <p class="d jnote">${fr.journal.note}</p>
    ${hasUninstalls(view)
      ? html`<p class="jforget">
          <button
            type="button"
            class="ghost small"
            data-fk=${journalFocus.forget}
            ?disabled=${state.historyBusy}
            @click=${actions.askForgetUninstalls}
          >
            ${fr.journal.forget}
          </button>
        </p>`
      : nothing}`;
}

export function journalView(state: AppState, actions: Actions): TemplateResult {
  return stage({
    body: html`<div class="scroll">
      <div class="page journal">
        <h1 tabindex="-1" data-fk=${journalFocus.title}>${fr.journal.title}</h1>
        <p class="pintro">${fr.journal.intro}</p>
        <div class="jbody">${body(state, actions)}</div>
      </div>
    </div>`,
  });
}

/** La confirmation de « Oublier les désinstallations » : ce qui part, et ce qui ne bouge pas. */
export function journalDialog(state: AppState, actions: Actions): Content {
  if (!state.historyAsk) {
    return nothing;
  }
  // Échap : c'est l'application qui referme, pour que l'écran et son état disent la même chose.
  const onCancel = (event: Event): void => {
    event.preventDefault();
    actions.dismissForgetUninstalls();
  };
  // Un clic à côté de la boîte la referme : il tombe sur le voile, donc sur le dialogue lui-même.
  const onClick = (event: Event): void => {
    if (closesOnVeil(event)) {
      actions.dismissForgetUninstalls();
    }
  };
  return html`<dialog
    class="ask"
    aria-labelledby="journal-ask-title"
    aria-describedby="journal-ask-text"
    ${ref(openModal)}
    @cancel=${onCancel}
    @close=${actions.dismissForgetUninstalls}
    @click=${onClick}
  >
    <div class="askbox">
      <h2 id="journal-ask-title">${fr.journal.confirm.title}</h2>
      <p class="askintro" id="journal-ask-text">${fr.journal.confirm.text}</p>
      <div class="askbtns">
        <button type="button" class="ghost" autofocus @click=${actions.dismissForgetUninstalls}>
          ${fr.journal.confirm.cancel}
        </button>
        <button type="button" class="pill" @click=${actions.confirmForgetUninstalls}>${fr.journal.confirm.confirm}</button>
      </div>
    </div>
  </dialog>`;
}
