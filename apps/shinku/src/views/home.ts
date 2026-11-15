// L'entrée « Grand vide » : l'accueil, l'analyse en cours, puis les résultats.

import { html, nothing, type TemplateResult } from 'lit-html';

import type { LoadIssue } from '../backend/types.ts';
import { formatBytes } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import {
  binOverThreshold,
  category,
  CLEAN_ROWS,
  type CleanRowId,
  counterBytes,
  duplicatesPercent,
  ringFraction,
  type ScanRow,
  type ScanRowId,
  scanRows,
  segments,
  selectable,
  selected,
  selectedBytes,
} from '../model/scan.ts';
import type { Actions, AppState } from '../state.ts';
import { checkIcon, chevronIcon, hole, navIcon, ring } from './art.ts';
import { backButton, orb, stage } from './parts.ts';
import { styles } from './style.ts';

const ROW_NAMES: Record<ScanRowId, string> = {
  junkFiles: fr.nav.junk,
  duplicates: fr.nav.duplicates,
  recycleBin: fr.nav.bin,
  largeFiles: fr.nav.large,
  startup: fr.nav.startup,
};

export function homeView(state: AppState, actions: Actions): TemplateResult {
  switch (state.pane) {
    case 'scan':
      return scanPane(state, actions);
    case 'results':
      return resultsPane(state, actions);
    case 'home':
      return welcomePane(state, actions);
  }
}

// ── Accueil ────────────────────────────────────────────────────────────────

/** Vrai si ce problème de lecture touche aux dossiers jamais touchés. */
function touchesFolders(issue: LoadIssue): boolean {
  return issue.kind === 'unreadable' || issue.fields.includes('excludedFolders');
}

function issueAlert(issue: LoadIssue, actions: Actions): TemplateResult {
  const serious = touchesFolders(issue);
  const text =
    issue.kind === 'unreadable' ? fr.home.issueUnreadable : serious ? fr.home.issueFolders : fr.home.issueMinor;
  return html`<div class="alert" role="alert">
    <p>${text}</p>
    <div class="row2">
      ${serious
        ? html`<button type="button" class="ghost small" @click=${() => actions.navigate('settings')}>
            ${fr.common.openSettings}
          </button>`
        : nothing}
      <button type="button" class="pill small" @click=${actions.acknowledgeIssue}>
        ${serious ? fr.home.continueAnyway : fr.common.understood}
      </button>
    </div>
  </div>`;
}

function welcomePane(state: AppState, actions: Actions): TemplateResult {
  const overview = state.overview;
  const bin = overview?.recycleBin ?? null;
  const settings = overview?.settings;
  const binOver =
    bin !== null &&
    settings !== undefined &&
    settings.recycleBinAlert &&
    binOverThreshold(bin.bytes, settings.recycleBinThresholdGb);

  const bar =
    binOver && bin !== null && settings !== undefined
      ? html`<div class="notice">
          <span class="ndot" aria-hidden="true"></span>
          <span>${fr.home.binNotice(bin.bytes, settings.recycleBinThresholdGb)}</span>
          <button type="button" @click=${() => actions.navigate('bin')}>${fr.home.seeBin}</button>
        </div>`
      : nothing;

  const issue = overview?.settingsIssue ?? null;
  const blocked = overview?.scanBlocked ?? false;
  const scanning = state.session.state === 'scanning';
  const summary = state.session.summary;

  let dock: TemplateResult;
  if (scanning) {
    dock = html`${orb(fr.home.follow, actions.showScan)}
      <p class="cap">${fr.common.scanning}</p>`;
  } else if (blocked) {
    dock = html`${orb(fr.common.analyse, null)}
      <p class="cap">${fr.home.checkFirst}</p>`;
  } else {
    const below =
      state.session.state === 'failed'
        ? html`<p class="cap">${fr.home.failed}</p>`
        : summary !== null
          ? html`<button type="button" class="back" @click=${actions.showResults}>
              ${fr.home.reviewResults(summary.finished)}
            </button>`
          : html`<p class="cap">${fr.common.nothingWithoutConsent}</p>`;
    dock = html`${orb(fr.common.analyse, actions.startScan)} ${below}`;
  }

  return html`<div class="stage enter">
    <div class="bar center" data-tauri-drag-region="deep">${bar}</div>
    <div class="hero ${issue !== null ? 'tight' : ''}">
      ${hole('ac', 250)}
      <h1>${fr.home.title}</h1>
      <p>${fr.home.intro}</p>
      ${issue !== null ? issueAlert(issue, actions) : nothing}
    </div>
    <div class="dock">${dock}</div>
  </div>`;
}

// ── Analyse ────────────────────────────────────────────────────────────────

/**
 * Le chiffre d'une ligne terminée. Hors du lot automatique, il ne compte pas
 * dans le grand chiffre « à libérer » : la ligne dit pourquoi.
 */
function doneText(row: Extract<ScanRow, { status: 'done' }>): string {
  if (row.lot === 'auto') {
    return formatBytes(row.bytes);
  }
  if (row.items === 0) {
    return fr.scan.none;
  }
  return row.lot === 'optional' ? fr.scan.optional(row.bytes) : fr.scan.toReview(row.bytes);
}

function rowStatus(row: ScanRow): { indicator: TemplateResult; text: string; tone: string } {
  switch (row.status) {
    case 'waiting':
      return { indicator: html`<span class="pip"></span>`, text: fr.scan.waiting, tone: 'wait' };
    case 'running': {
      const text =
        row.percent !== undefined
          ? fr.scan.comparing(row.percent)
          : row.bytes !== undefined && row.bytes > 0
            ? formatBytes(row.bytes)
            : fr.scan.working;
      return { indicator: html`<span class="pip on breathe"></span>`, text, tone: 'run' };
    }
    case 'done':
      return { indicator: html`<span class="tick">${checkIcon()}</span>`, text: doneText(row), tone: 'done' };
    case 'unavailable':
      return {
        indicator: html`<span class="pip off"></span>`,
        text: row.id === 'duplicates' ? fr.scan.notSearched : fr.scan.notMeasured,
        tone: 'wait',
      };
    case 'soon':
      return { indicator: html`<span class="pip off"></span>`, text: fr.common.soon, tone: 'wait' };
  }
}

function scanPane(state: AppState, actions: Actions): TemplateResult {
  const view = state.session;
  const finished = view.state === 'results';
  const rows = scanRows(view).map((row) => {
    const { indicator, text, tone } = rowStatus(row);
    return html`<li class="row static short">
      <span class="ind" aria-hidden="true">${indicator}</span>
      <span class="tx"><span class="t1">${ROW_NAMES[row.id]}</span></span>
      <span class="st ${tone}">${text}</span>
    </li>`;
  });

  const dock = finished
    ? html`<button type="button" class="orb enter" @click=${actions.showResults}>${fr.scan.seeResults}</button>`
    : html`${ring(
          'anr',
          ringFraction(view),
          html`<button type="button" class="core" @click=${actions.cancelScan}>${fr.common.stop}</button>`,
        )}
        ${view.summary !== null
          ? html`<button type="button" class="back" @click=${actions.showResults}>${fr.scan.seeEarlyResults}</button>`
          : nothing}`;

  return stage({
    bar: finished ? backButton(actions.showHome) : nothing,
    body: html`<div class="middle">
      ${hole('an', 132, !finished)}
      <p class="eb gap-s" role="status">${finished ? fr.scan.finished : fr.scan.running}</p>
      <p class="big mid">${formatBytes(counterBytes(view))}</p>
      <p class="eb gap-s">${fr.scan.toFree}</p>
      <div class="wrap gap-l"><ul class="list plain">${rows}</ul></div>
      <p class="cap gap-m">${fr.scan.walked(view.progress.folders, view.progress.files)}</p>
    </div>`,
    dock: html`<div class="dock">${dock}</div>`,
  });
}

// ── Résultats ──────────────────────────────────────────────────────────────

const SEGMENT_CLASS: Record<CleanRowId, string> = {
  junkFiles: 'seg-1',
  duplicates: 'seg-2',
  recycleBin: 'seg-3',
};

function rowHint(state: AppState, id: CleanRowId, checked: boolean): string {
  const entry = category(state.session.summary, id);
  switch (id) {
    case 'junkFiles':
      return fr.results.junkHint;
    case 'duplicates':
      if (entry?.state === 'pending') {
        return fr.results.duplicatesPending(duplicatesPercent(state.session));
      }
      if (entry?.state === 'unavailable') {
        return fr.results.duplicatesOff;
      }
      if (entry === undefined || entry.items === 0) {
        return fr.results.duplicatesNone;
      }
      // Des fichiers personnels : la ligne reste décochée tant que la personne ne l'a pas choisie.
      return checked ? fr.results.duplicatesHint : fr.results.duplicatesChoice;
    case 'recycleBin': {
      if (entry === undefined || entry.state === 'unavailable') {
        return fr.results.binUnknown;
      }
      if (entry.items === 0) {
        return fr.results.binEmpty;
      }
      const settings = state.overview?.settings;
      const over =
        settings !== undefined &&
        settings.recycleBinAlert &&
        binOverThreshold(entry.bytes, settings.recycleBinThresholdGb);
      return over ? fr.results.binItemsOver(entry.items) : fr.results.binItems(entry.items);
    }
  }
}

function resultsPane(state: AppState, actions: Actions): TemplateResult {
  const { summary, skipped, stale } = state.session;
  const grow = segments(summary, state.selection);

  const cleanRows = CLEAN_ROWS.map((id) => {
    const entry = category(summary, id);
    const measured = entry !== undefined && entry.state !== 'unavailable';
    const checked = selected(summary, state.selection, id);
    return html`<label class="row">
      <input
        type="checkbox"
        .checked=${checked}
        ?disabled=${!selectable(summary, id)}
        @change=${() => actions.toggle(id)}
      />
      <span class="box">${checkIcon()}</span>
      <span class="dot ${SEGMENT_CLASS[id]}" aria-hidden="true"></span>
      <span class="tx">
        <span class="t1">${ROW_NAMES[id]}</span>
        <span class="t2">${rowHint(state, id, checked)}</span>
      </span>
      <span class="val"><span>${measured ? formatBytes(entry.bytes) : '–'}</span></span>
    </label>`;
  });

  const large = category(summary, 'largeFiles');
  const reviewRows = html`<button type="button" class="row" @click=${() => actions.navigate('large')}>
      <span class="ricon">${navIcon('large', 16)}</span>
      <span class="tx">
        <span class="t1">${fr.nav.large}</span>
        <span class="t2">${fr.results.largeHint(large?.items ?? 0, state.large?.threshold ?? 3 * 1024 ** 3)}</span>
      </span>
      <span class="val"><span>${large !== undefined && large.items > 0 ? formatBytes(large.bytes) : ''}</span></span>
      ${chevronIcon()}
    </button>
    <button type="button" class="row" @click=${() => actions.navigate('startup')}>
      <span class="ricon">${navIcon('startup', 16)}</span>
      <span class="tx">
        <span class="t1">${fr.nav.startup}</span>
        <span class="t2">${fr.common.soon}</span>
      </span>
      ${chevronIcon()}
    </button>`;

  const notes = [
    skipped.folders > 0 ? fr.results.skipped(skipped.folders) : null,
    skipped.refusedRoots > 0 ? fr.results.refused(skipped.refusedRoots) : null,
  ].filter((note): note is string => note !== null);

  const dock =
    stale && summary !== null
      ? html`${orb(fr.results.rescan, actions.startScan)}
          <p class="cap">${fr.results.stale(summary.finished)}</p>`
      : html`${orb(fr.results.clean, null)}
          <p class="cap">${fr.results.cleanSoon}</p>`;

  return stage({
    bar: backButton(actions.showHome),
    body: html`<div class="scroll centered">
      <div class="wrap stagger">
        <div class="head center">
          <p class="eb">${fr.results.canFree}</p>
          <p class="big">${formatBytes(selectedBytes(summary, state.selection))}</p>
          <div class="segbox">
            <div class="seg" aria-hidden="true">
              ${CLEAN_ROWS.map(
                (id, index) =>
                  html`<span class=${SEGMENT_CLASS[id]} ${styles({ 'flex-grow': String(grow[index] ?? 0) })}></span>`,
              )}
            </div>
          </div>
        </div>
        <div class="list">${cleanRows}</div>
        <p class="sub">${fr.results.review}</p>
        <div class="list">${reviewRows}</div>
        ${notes.length > 0 ? html`<p class="note">${notes.join(' ')}</p>` : nothing}
      </div>
    </div>`,
    dock: html`<div class="dock">${dock}</div>`,
  });
}
