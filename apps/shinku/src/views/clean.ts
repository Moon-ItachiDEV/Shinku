// Le grand vide : la confirmation, l'avancement, puis le bilan.
//
// La confirmation est la seule porte : rien ne part tant que la personne n'a
// pas lu ce qui va partir, étape par étape, et choisi « Faire le vide ». Ce
// que la confirmation liste est exactement la demande envoyée au cœur.

import { html, nothing, type TemplateResult } from 'lit-html';
import { ref } from 'lit-html/directives/ref.js';

import type { CleanReport, CleanStop, CleanView, LeftOpen } from '../backend/types.ts';
import { driveName, formatBytes, listOf, splitPath } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import {
  binWasFull,
  cleanFraction,
  type CleanStepId,
  type ConfirmStep,
  confirmSteps,
  countedBeyondGain,
  failedSteps,
  headline,
  reportParts,
  type SkipReason,
  skippedParts,
  skippedTotal,
  stepPercent,
} from '../model/clean.ts';
import { mainVolume } from '../model/scan.ts';
import type { Actions, AppState } from '../state.ts';
import { doneArt, hole, ring } from './art.ts';
import { closesOnVeil, openModal } from './modal.ts';
import { CLEAN_NAMES, SEGMENT_CLASS, stage, stopCore } from './parts.ts';
import { styles } from './style.ts';

type Content = TemplateResult | typeof nothing;

/** Les apps restées ouvertes, en une phrase : leurs noms, leur nombre, leur place. */
function openApps(apps: readonly LeftOpen[]): [names: string, count: number, bytes: number] {
  return [listOf(apps.map((app) => app.label)), apps.length, apps.reduce((total, app) => total + app.bytes, 0)];
}

// ── Confirmation ───────────────────────────────────────────────────────────

function confirmHint(step: ConfirmStep): string {
  switch (step.id) {
    case 'recycleBin':
      return fr.confirm.bin(step.count);
    case 'junkFiles':
      return fr.confirm.junk(step.count);
    case 'duplicates':
      return fr.confirm.duplicates(step.count);
  }
}

export function confirmDialog(state: AppState, actions: Actions): Content {
  const confirmation = state.confirm;
  if (confirmation === null) {
    return nothing;
  }
  const { preview, request } = confirmation;
  const rows = confirmSteps(preview, request).map(
    (step) =>
      html`<li class="row static">
        <span class="dot ${SEGMENT_CLASS[step.id]}" aria-hidden="true"></span>
        <span class="tx">
          <span class="t1">${CLEAN_NAMES[step.id]}</span>
          <span class="t2">${confirmHint(step)}</span>
        </span>
        <span class="val"><span>${formatBytes(step.bytes)}</span></span>
      </li>`,
  );
  const notes = [
    request.junkFiles && preview.junk.openApps.length > 0 ? fr.confirm.openApps(...openApps(preview.junk.openApps)) : null,
    request.duplicates && preview.duplicates.syncedCopies > 0
      ? fr.confirm.syncedCopies(preview.duplicates.syncedCopies)
      : null,
    request.duplicates && preview.duplicates.otherDiskCopies > 0
      ? fr.confirm.otherDiskCopies(preview.duplicates.otherDiskCopies)
      : null,
  ].filter((note): note is string => note !== null);

  // Échap : c'est l'application qui referme, pour que l'écran et son état disent la même chose.
  const onCancel = (event: Event): void => {
    event.preventDefault();
    actions.dismissClean();
  };
  // Un clic à côté de la boîte la referme : il tombe sur le voile, donc sur le dialogue lui-même.
  const onClick = (event: Event): void => {
    if (closesOnVeil(event)) {
      actions.dismissClean();
    }
  };

  return html`<dialog
    class="ask"
    aria-labelledby="ask-title"
    aria-describedby="ask-intro"
    ${ref(openModal)}
    @cancel=${onCancel}
    @close=${actions.dismissClean}
    @click=${onClick}
  >
    <div class="askbox">
      <h2 id="ask-title">${fr.confirm.title}</h2>
      <p id="ask-intro" class="askintro">${fr.confirm.intro}</p>
      <ul class="list plain">
        ${rows}
      </ul>
      ${notes.map((note) => html`<p class="note">${note}</p>`)}
      <div class="askbtns">
        <button type="button" class="ghost" autofocus ?disabled=${state.cleanBusy} @click=${actions.dismissClean}>
          ${fr.confirm.cancel}
        </button>
        <button type="button" class="pill" ?disabled=${state.cleanBusy} @click=${actions.confirmClean}>
          ${fr.confirm.confirm}
        </button>
      </div>
    </div>
  </dialog>`;
}

// ── Grand vide en cours ────────────────────────────────────────────────────

function stepLabel(clean: CleanView): string {
  switch (clean.progress.phase) {
    case 'preparing':
      return fr.clean.preparing;
    case 'recycleBin':
      return fr.clean.recycleBin;
    case 'junkFiles':
      return fr.clean.junkFiles;
    case 'duplicates':
      return fr.clean.duplicates;
    case 'done':
      return fr.clean.finishing;
  }
}

function runningStage(state: AppState, actions: Actions, clean: CleanView): TemplateResult {
  const done = stepPercent(clean);
  return stage({
    key: 'running',
    body: html`<div class="hero">
      ${hole('cl', 220, true)}
      <h1>${fr.clean.title}</h1>
      <p>
        <span role="status">${stepLabel(clean)}</span>
        ${done === null ? nothing : html`<span class="num">${fr.clean.at(done)}</span>`}
      </p>
      <p class="fine sofar">${fr.clean.soFar(clean.progress.freedBytes, clean.progress.recycledBytes)}</p>
    </div>`,
    dock: html`<div class="dock">
      ${ring(
        'clr',
        cleanFraction(clean),
        stopCore(actions.stopClean, state.stopping),
      )}
      ${state.stopping ? html`<p class="cap" role="status">${fr.clean.stopping}</p>` : nothing}
    </div>`,
  });
}

// ── Bilan ──────────────────────────────────────────────────────────────────

const SKIP_TEXT: Record<SkipReason, (files: number) => string> = {
  inUse: fr.done.skippedInUse,
  changed: fr.done.skippedChanged,
  denied: fr.done.skippedDenied,
  unrecyclable: fr.done.skippedUnrecyclable,
  other: fr.done.skippedOther,
};

const FAILED_TEXT: Record<CleanStepId, string> = {
  recycleBin: fr.done.binFailed,
  junkFiles: fr.done.junkFailed,
  duplicates: fr.done.duplicatesFailed,
};

function stopText(stop: CleanStop): string {
  const { name } = splitPath(stop.file);
  switch (stop.reason) {
    case 'notInRecycleBin':
      return fr.done.stopNotInBin(name);
    case 'keptCopyChanged':
      return fr.done.stopKeptChanged(name);
  }
}

/** Ce qui ne s'est pas passé comme prévu, et qu'il faut avoir lu. */
function warnings(report: CleanReport): string[] {
  const stop = report.duplicates.stop;
  return [
    ...(stop === null ? [] : [stopText(stop)]),
    ...failedSteps(report).map((id) => FAILED_TEXT[id]),
    ...(binWasFull(report) ? [fr.done.binWasFull] : []),
  ];
}

/** Ce qui est resté en place, et pourquoi : rien d'inquiétant, mais rien de caché. */
function remarks(report: CleanReport, counted: number | null): string[] {
  const skipped = skippedParts(report);
  return [
    ...(report.outcome === 'cancelled' ? [fr.done.cancelled] : []),
    ...(report.junk.leftOpen.length > 0 ? [fr.done.leftOpen(...openApps(report.junk.leftOpen))] : []),
    ...(skipped.length > 0
      ? [fr.done.skipped(skippedTotal(report), listOf(skipped.map((part) => SKIP_TEXT[part.reason](part.files))))]
      : []),
    ...(counted === null ? [] : [fr.done.gainedLess(counted)]),
  ];
}

function reportStage(state: AppState, actions: Actions, report: CleanReport): TemplateResult {
  const main = headline(report);
  const parts = reportParts(report);
  const alerts = warnings(report);
  // Arrêté par la personne, ou de lui-même par sécurité : il n'est pas allé au bout.
  const stopped = report.outcome === 'cancelled' || report.duplicates.stop !== null;
  // Une coche pour un grand vide allé au bout sans histoire ; sinon le trou noir, au repos.
  const whole = !stopped && main.kind !== 'nothing' && alerts.length === 0;
  // Une corbeille pleine n'a pas tout gardé : ses copies ne sont plus toutes récupérables.
  const copiesName = binWasFull(report) ? CLEAN_NAMES.duplicates : fr.done.duplicatesPart;

  // Le disque a pu gagner moins que le compte : on ne le dit que si l'écart se voit.
  const beyond = countedBeyondGain(report);
  const counted = beyond !== null && formatBytes(beyond) !== formatBytes(report.freedBytes) ? beyond : null;
  const notes = remarks(report, counted);

  const volume = mainVolume(state.overview?.volumes ?? []);
  const disk =
    main.kind === 'freed' && state.diskAfterClean && volume !== undefined
      ? html`<p>${fr.done.diskNow(driveName(volume.root), volume.free)}</p>`
      : nothing;

  const figure =
    main.kind === 'nothing'
      ? html`<h1>${fr.done.nothing}</h1>`
      : html`<p class="big">
          ${formatBytes(main.bytes)}<small>${main.kind === 'freed' ? fr.done.freed : fr.done.recycled}</small>
        </p>`;
  const below =
    main.kind === 'freed' && main.recoverable > 0
      ? html`<p class="rec">${fr.done.recoverable(main.recoverable)}</p>`
      : main.kind === 'recycled'
        ? html`<p class="rec">${fr.done.recycledHint}</p>`
        : nothing;

  const breakdown =
    main.kind === 'nothing' || parts.length === 0
      ? nothing
      : html`<div class="segbox">
            <div class="seg" aria-hidden="true">
              ${parts.map(
                (part) => html`<span class=${SEGMENT_CLASS[part.id]} ${styles({ 'flex-grow': String(part.bytes) })}></span>`,
              )}
            </div>
          </div>
          <div class="chips">
            ${parts.map(
              (part) =>
                html`<span class="chip">
                  <span class="dot ${SEGMENT_CLASS[part.id]}" aria-hidden="true"></span>
                  ${part.id === 'duplicates' ? copiesName : CLEAN_NAMES[part.id]}
                  <b>${formatBytes(part.bytes)}</b>
                </span>`,
            )}
          </div>`;

  return stage({
    key: 'report',
    body: html`<div class="hero bilan ${alerts.length > 0 || notes.length > 0 ? 'dense' : ''}">
      ${whole ? doneArt('cld') : hole('cls', 170)}
      <p class="eb" role="status">${stopped ? fr.done.stopped : fr.done.finished}</p>
      ${figure} ${below} ${breakdown} ${disk}
      ${alerts.length > 0
        ? html`<div class="alert" role="alert">${alerts.map((text) => html`<p>${text}</p>`)}</div>`
        : nothing}
      ${notes.length > 0 ? html`<div class="fines">${notes.map((text) => html`<p class="fine">${text}</p>`)}</div>` : nothing}
    </div>`,
    dock: html`<div class="dock">
      <div class="row2">
        <button type="button" class="ghost" @click=${actions.startScan}>${fr.done.rescan}</button>
        <button type="button" class="pill" @click=${actions.closeReport}>${fr.done.close}</button>
      </div>
    </div>`,
  });
}

/** L'entrée « Grand vide » pendant un grand vide, puis devant son bilan. */
export function cleanPane(state: AppState, actions: Actions, clean: CleanView): TemplateResult {
  return clean.report === null ? runningStage(state, actions, clean) : reportStage(state, actions, clean.report);
}
