// Les pages des modules. Elles montrent ce que l'analyse a trouvé, sans rien
// supprimer elles-mêmes : pour l'instant, tout ce qui part passe par le grand
// vide et sa confirmation. Leurs boutons d'action sont visibles mais inactifs,
// et l'écran dit où aller.

import { html, nothing, type TemplateResult } from 'lit-html';

import type { DuplicateGroup, JunkReport, KeepChoice, LargeFile } from '../backend/types.ts';
import { formatAgo, formatBytes, splitPath } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import { sharedFolders } from '../model/duplicates.ts';
import { binOverThreshold, category, duplicatesPercent, ringFraction } from '../model/scan.ts';
import type { Actions, AppState, Route } from '../state.ts';
import { binArt, moduleArt, type ModuleArt, ring } from './art.ts';
import { hero, orb, stage } from './parts.ts';

type Content = TemplateResult | typeof nothing;

/** L'introduction d'un module qui sait analyser : rien n'a encore été cherché. */
function introStage(state: AppState, actions: Actions, art: ModuleArt, title: string, text: string): TemplateResult {
  const blocked = state.overview?.scanBlocked ?? false;
  // Tant qu'un grand vide tourne, aucune analyse ne démarre.
  const cleaning = state.session.state === 'cleaning';
  return stage({
    key: 'intro',
    body: hero(moduleArt(art), title, text),
    dock: html`<div class="dock">
      ${orb(fr.common.analyse, blocked || cleaning ? null : actions.startScan)}
      <p class="cap">
        ${cleaning ? fr.clean.title : blocked ? fr.home.checkFirst : fr.common.nothingWithoutConsent}
      </p>
    </div>`,
  });
}

/** Le module pendant l'analyse : son illustration est balayée, un anneau tourne. */
function scanStage(state: AppState, actions: Actions, art: ModuleArt, title: string, text: string): TemplateResult {
  return stage({
    key: 'scan',
    body: hero(moduleArt(art, true), title, text),
    dock: html`<div class="dock">
      ${ring(
        `${art}-r`,
        ringFraction(state.session),
        html`<button type="button" class="core" @click=${actions.cancelScan}>${fr.common.stop}</button>`,
      )}
      <p class="cap">${fr.common.scanning}</p>
    </div>`,
  });
}

/** La liste des trouvailles d'un module, avec son bouton d'action, encore inactif. */
function listStage(parts: {
  name: string;
  title: string;
  hint: string;
  body: Content;
  action: string;
  cap: string;
}): TemplateResult {
  return stage({
    key: 'list',
    body: html`<div class="scroll">
      <div class="wrap stagger">
        <div class="head">
          <p class="eb">${parts.name}</p>
          <h1>${parts.title}</h1>
          <p>${parts.hint}</p>
        </div>
        ${parts.body}
      </div>
    </div>`,
    dock: html`<div class="dock">
      ${orb(parts.action, null)}
      <p class="cap">${parts.cap}</p>
    </div>`,
  });
}

const loadingNote = (): TemplateResult => html`<p class="note">${fr.common.loading}</p>`;

function moreButton(state: AppState, actions: Actions, list: 'large' | 'duplicates'): TemplateResult {
  return html`<div class="more">
    <button type="button" class="ghost small" ?disabled=${state.loadingMore} @click=${() => actions.loadMore(list)}>
      ${state.loadingMore ? fr.common.loading : fr.common.showMore}
    </button>
  </div>`;
}

// ── Fichiers inutiles ──────────────────────────────────────────────────────

function junkBody(report: JunkReport): TemplateResult {
  const found = report.categories.filter((entry) => entry.files > 0);
  return html`${found.map(
    (entry) =>
      html`<p class="sub split"><span>${entry.label}</span><span>${formatBytes(entry.bytes)}</span></p>
        <ul class="list plain">
          ${entry.lines.map(
            (line) =>
              html`<li class="row static">
                <span class="tx">
                  <span class="t1">${line.label}</span>
                  <span class="t2">${fr.junk.files(line.files)}</span>
                </span>
                <span class="val"><span>${formatBytes(line.bytes)}</span></span>
              </li>`,
          )}
        </ul>`,
  )}
  ${report.sparedFiles > 0
    ? html`<p class="note">${fr.junk.spared(report.sparedFiles, report.sparedBytes)}</p>`
    : nothing}`;
}

export function junkView(state: AppState, actions: Actions): TemplateResult {
  const entry = category(state.session.summary, 'junkFiles');
  if (entry === undefined) {
    return state.session.state === 'scanning'
      ? scanStage(state, actions, 'junk', fr.junk.scanTitle, fr.junk.scanText)
      : introStage(state, actions, 'junk', fr.nav.junk, fr.junk.intro);
  }
  const report = state.junk;
  const categories = report?.categories.filter((found) => found.files > 0).length ?? 0;
  return listStage({
    name: fr.nav.junk,
    title: report === null ? formatBytes(entry.bytes) : fr.junk.found(categories),
    hint: fr.junk.hint,
    body: report === null ? loadingNote() : junkBody(report),
    action: fr.junk.remove,
    cap: entry.bytes > 0 ? fr.junk.viaClean(entry.bytes) : fr.junk.nothing,
  });
}

// ── Gros fichiers ──────────────────────────────────────────────────────────

function largeRow(file: LargeFile, now: number): TemplateResult {
  const { name, parent } = splitPath(file.path);
  return html`<li class="row static">
    <span class="tx">
      <span class="t1 path" title=${file.path}>${name}</span>
      <span class="t2 path">${parent}</span>
      <span class="t2">${fr.large.modified(formatAgo(file.modified, now))}</span>
    </span>
    ${file.synced ? html`<span class="tag">${fr.common.synced}</span>` : nothing}
    <span class="val"><span>${formatBytes(file.onDisk)}</span></span>
  </li>`;
}

export function largeView(state: AppState, actions: Actions): TemplateResult {
  const entry = category(state.session.summary, 'largeFiles');
  if (entry === undefined) {
    return state.session.state === 'scanning'
      ? scanStage(state, actions, 'large', fr.large.scanTitle, fr.large.scanText)
      : introStage(state, actions, 'large', fr.nav.large, fr.large.intro);
  }
  const page = state.large;
  const body =
    page === null
      ? loadingNote()
      : html`${page.files.length > 0
          ? html`<ul class="list plain">
              ${page.files.map((file) => largeRow(file, state.now))}
            </ul>`
          : nothing}
        ${page.files.length < page.listed ? moreButton(state, actions, 'large') : nothing}
        ${page.files.length >= page.listed && page.listed < page.count
          ? html`<p class="note">${fr.large.truncated(page.listed)}</p>`
          : nothing}`;
  return listStage({
    name: fr.nav.large,
    title: fr.large.found(entry.items, page?.threshold ?? 3 * 1024 ** 3),
    hint: entry.items > 0 ? fr.large.hint : fr.large.hintNone,
    body,
    action: fr.large.remove,
    cap: entry.bytes > 0 ? fr.large.soon(entry.bytes) : fr.large.soonEmpty,
  });
}

// ── Doublons ───────────────────────────────────────────────────────────────

/** Ce que l'écran peut faire d'un choix : `null` tant qu'il ne peut pas en prendre. */
type Keep = ((choice: KeepChoice) => void) | null;

/** Combien de dossiers l'écran propose de garder d'un seul geste. */
const FOLDER_ROWS = 4;

/** Les dossiers qui rangent un exemplaire de plusieurs groupes : un bouton les garde partout. */
function folderRows(groups: readonly DuplicateGroup[], keep: Keep, busy: boolean, partial: boolean): Content {
  const folders = keep === null ? [] : sharedFolders(groups, FOLDER_ROWS);
  if (keep === null || folders.length === 0) {
    return nothing;
  }
  return html`<p class="sub">${fr.duplicates.folders}</p>
    <ul class="list plain folders">
      ${folders.map((entry) => {
        const all = entry.kept === entry.groups;
        return html`<li class="row static">
          <span class="tx">
            <span class="t1 path" title=${entry.folder}>${entry.folder}</span>
            <span class="t2">${fr.duplicates.folderGroups(entry.groups)}</span>
          </span>
          <button
            type="button"
            class="ghost small"
            aria-pressed=${all ? 'true' : 'false'}
            aria-disabled=${busy ? 'true' : 'false'}
            @click=${() => (all || busy ? undefined : keep(entry.choice))}
          >
            ${all ? fr.duplicates.folderKept : fr.duplicates.keepFolder}
          </button>
        </li>`;
      })}
    </ul>
    ${partial ? html`<p class="note folders-note">${fr.duplicates.foldersPartial}</p>` : nothing}`;
}

function duplicateGroup(group: DuplicateGroup, keep: Keep, busy: boolean): TemplateResult {
  const kept = group.files.find((file) => file.keep) ?? group.files[0];
  const name = kept === undefined ? '' : splitPath(kept.path).name;
  return html`<ul class="list plain group">
    <li class="row static">
      <span class="tx">
        <span class="t1 path">${name}</span>
        <span class="t2">${fr.duplicates.copies(group.files.length, group.size)}</span>
      </span>
      <span class="val"><span>${formatBytes(group.reclaimable)}</span></span>
    </li>
    ${group.files.map((file, index) => {
      const line = html`<span class="tx"><span class="t2 path" title=${file.path}>${file.path}</span></span>
        ${file.synced ? html`<span class="tag">${fr.common.synced}</span>` : nothing}
        ${file.keep
          ? html`<span class="tag strong">${fr.common.kept}</span>`
          : keep === null
            ? nothing
            : html`<span class="tag offer">${fr.duplicates.keepThis}</span>`}`;
      if (keep === null) {
        return html`<li class="row static slim">${line}</li>`;
      }
      const choice = { digest: group.digest, size: group.size, index, wholeFolder: false };
      return html`<li class="pick">
        <button
          type="button"
          class="row slim"
          aria-pressed=${file.keep ? 'true' : 'false'}
          aria-disabled=${busy ? 'true' : 'false'}
          @click=${() => (file.keep || busy ? undefined : keep(choice))}
        >
          ${line}
        </button>
      </li>`;
    })}
  </ul>`;
}

export function duplicatesView(state: AppState, actions: Actions): TemplateResult {
  const entry = category(state.session.summary, 'duplicates');
  if (entry === undefined) {
    return state.session.state === 'scanning'
      ? scanStage(state, actions, 'duplicates', fr.duplicates.scanTitle, fr.duplicates.scanText)
      : introStage(state, actions, 'duplicates', fr.nav.duplicates, fr.duplicates.intro);
  }
  if (entry.state === 'pending') {
    // Le reste du rapport est prêt, mais les doublons se cherchent encore.
    return stage({
      key: 'pending',
      body: hero(
        moduleArt('duplicates', true),
        fr.duplicates.pendingTitle,
        fr.duplicates.pendingText(duplicatesPercent(state.session)),
      ),
      dock: html`<div class="dock">
        ${ring(
          'duplicates-r',
          ringFraction(state.session),
          html`<button type="button" class="core" @click=${actions.cancelScan}>${fr.common.stop}</button>`,
        )}
        <p class="cap">${fr.common.scanning}</p>
      </div>`,
    });
  }
  const page = state.duplicates;
  const off = entry.state === 'unavailable';
  // Un choix ne se prend que sur des résultats qui servent encore à nettoyer.
  const keep: Keep = state.session.state === 'results' && !state.session.stale ? actions.keepDuplicate : null;
  const busy = state.keeping || state.loadingMore;
  const body =
    page === null
      ? off
        ? nothing
        : loadingNote()
      : html`${folderRows(page.groups, keep, busy, page.groups.length < page.groupCount)}
        ${page.groups.map((group) => duplicateGroup(group, keep, busy))}
        ${page.groups.length < page.groupCount ? moreButton(state, actions, 'duplicates') : nothing}
        ${page.skippedFiles > 0 ? html`<p class="note">${fr.duplicates.skipped(page.skippedFiles)}</p>` : nothing}`;
  return listStage({
    name: fr.nav.duplicates,
    title: fr.duplicates.found(page?.groupCount ?? (entry.items > 0 ? 1 : 0)),
    hint: off ? fr.duplicates.offText : entry.items > 0 ? (keep === null ? fr.duplicates.hintFixed : fr.duplicates.hint) : fr.duplicates.hintNone,
    body,
    action: fr.duplicates.remove,
    cap: entry.bytes > 0 ? fr.duplicates.viaClean(entry.bytes) : fr.duplicates.nothing,
  });
}

// ── Corbeille ──────────────────────────────────────────────────────────────

export function binView(state: AppState): TemplateResult {
  const bin = state.overview?.recycleBin ?? null;
  const settings = state.overview?.settings;
  const thresholdGb = settings?.recycleBinThresholdGb ?? 5;
  const over = bin !== null && (settings?.recycleBinAlert ?? true) && binOverThreshold(bin.bytes, thresholdGb);
  // La corbeille dessinée est pleine à deux fois le seuil d'alerte.
  const level = bin === null || bin.bytes === 0 ? 0 : Math.max(0.12, bin.bytes / (2 * thresholdGb * 1024 ** 3));
  const text =
    bin === null
      ? fr.bin.unknown
      : bin.items === 0
        ? fr.bin.empty
        : over
          ? fr.bin.contentOver(bin.bytes, bin.items, thresholdGb)
          : fr.bin.content(bin.bytes, bin.items);
  return stage({
    body: hero(binArt('bin', level), over ? fr.bin.titleOver : fr.bin.title, text),
    dock: html`<div class="dock">
      ${orb(fr.bin.emptyIt, null)}
      ${bin === null ? nothing : html`<p class="cap">${bin.items > 0 ? fr.bin.viaClean : fr.bin.nothing}</p>`}
    </div>`,
  });
}

// ── Modules à venir ────────────────────────────────────────────────────────

type SoonRoute = Extract<Route, 'startup' | 'gameMode' | 'uninstall' | 'privacy'>;

export function soonView(route: SoonRoute): TemplateResult {
  return stage({
    body: hero(moduleArt(route), fr.nav[route], fr.soon[route]),
    dock: html`<div class="dock">
      ${orb(fr.common.soon, null)}
      <p class="cap">${fr.soon.cap}</p>
    </div>`,
  });
}
