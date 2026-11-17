// La fenêtre : boutons de fenêtre, menu latéral, et l'écran du module choisi.

import { html, nothing, type TemplateResult } from 'lit-html';
import { keyed } from 'lit-html/directives/keyed.js';

import { driveName, formatBytes } from '../format.ts';
import { fr } from '../i18n/fr.ts';
import { binOverThreshold, mainVolume } from '../model/scan.ts';
import type { Actions, AppState, Route } from '../state.ts';
import { closeIcon, hole, type IconName, mark, maximizeIcon, minimizeIcon, navIcon } from './art.ts';
import { confirmDialog } from './clean.ts';
import { gameView, pickerDialog } from './game.ts';
import { homeView, shownPane } from './home.ts';
import { journalDialog, journalView } from './journal.ts';
import { binView, duplicatesView, junkView, largeView } from './modules.ts';
import { privacyDialog, privacyView } from './privacy.ts';
import { settingsView } from './settings.ts';
import { startupView } from './startup.ts';
import { styles } from './style.ts';
import { uninstallDialog, uninstallView } from './uninstall.ts';

interface NavItem {
  route: Route;
  icon: IconName;
  label: string;
}

const GROUPS: ReadonlyArray<{ title: string | null; items: readonly NavItem[] }> = [
  { title: null, items: [{ route: 'home', icon: 'home', label: fr.nav.home }] },
  {
    title: fr.nav.cleaning,
    items: [
      { route: 'junk', icon: 'junk', label: fr.nav.junk },
      { route: 'duplicates', icon: 'duplicates', label: fr.nav.duplicates },
      { route: 'large', icon: 'large', label: fr.nav.large },
      { route: 'bin', icon: 'bin', label: fr.nav.bin },
    ],
  },
  {
    title: fr.nav.performance,
    items: [
      { route: 'startup', icon: 'startup', label: fr.nav.startup },
      { route: 'gameMode', icon: 'gameMode', label: fr.nav.gameMode },
    ],
  },
  { title: fr.nav.apps, items: [{ route: 'uninstall', icon: 'uninstall', label: fr.nav.uninstall }] },
  { title: fr.nav.protection, items: [{ route: 'privacy', icon: 'privacy', label: fr.nav.privacy }] },
];

function windowButtons(actions: Actions): TemplateResult {
  return html`<div class="win">
    <button type="button" class="wbtn" aria-label=${fr.window.minimize} @click=${actions.minimize}>
      ${minimizeIcon()}
    </button>
    <button type="button" class="wbtn" aria-label=${fr.window.maximize} @click=${actions.toggleMaximize}>
      ${maximizeIcon()}
    </button>
    <button type="button" class="wbtn wclose" aria-label=${fr.window.close} @click=${actions.close}>
      ${closeIcon()}
    </button>
  </div>`;
}

function binBadge(state: AppState): TemplateResult | typeof nothing {
  const bin = state.overview?.recycleBin;
  const settings = state.overview?.settings;
  if (bin == null || settings === undefined || !settings.recycleBinAlert) {
    return nothing;
  }
  if (!binOverThreshold(bin.bytes, settings.recycleBinThresholdGb)) {
    return nothing;
  }
  return html`<span class="badge"><i></i>${formatBytes(bin.bytes)}<span class="sr">${fr.nav.binOver}</span></span>`;
}

function navButton(state: AppState, actions: Actions, item: NavItem): TemplateResult {
  return html`<button
    type="button"
    class="nav"
    aria-current=${state.route === item.route ? 'page' : nothing}
    @click=${() => actions.navigate(item.route)}
  >
    <i class="nic">${navIcon(item.icon)}</i><span>${item.label}</span>${item.route === 'bin' ? binBadge(state) : nothing}
  </button>`;
}

function diskGauge(state: AppState): TemplateResult | typeof nothing {
  const volume = mainVolume(state.overview?.volumes ?? []);
  if (volume === undefined || volume.total <= 0) {
    return nothing;
  }
  const used = Math.min(100, Math.max(0, Math.round((1 - volume.free / volume.total) * 100)));
  return html`<div class="disk">
    <div class="dhead">
      <span>${fr.disk.title(volume.label, driveName(volume.root))}</span><span>${fr.disk.used(used)}</span>
    </div>
    <div class="tr" aria-hidden="true"><div class="fl" ${styles({ width: `${used}%` })}></div></div>
    <p class="dfree"><b>${fr.disk.free(volume.free)}</b> <span>${fr.disk.freeRest(volume.total)}</span></p>
  </div>`;
}

function sidebar(state: AppState, actions: Actions): TemplateResult {
  return html`<nav class="side" aria-label=${fr.nav.label}>
    <div class="brand" data-tauri-drag-region="deep">
      ${mark('sb', 24)}<span class="wm">SHINKU</span><span class="kj" lang="ja">真空</span>
    </div>
    ${GROUPS.map(
      (group) =>
        html`${group.title === null ? nothing : html`<p class="grp">${group.title}</p>`}
        ${group.items.map((item) => navButton(state, actions, item))}`,
    )}
    <div class="spring"></div>
    ${diskGauge(state)} ${navButton(state, actions, { route: 'journal', icon: 'journal', label: fr.nav.journal })}
    ${navButton(state, actions, { route: 'settings', icon: 'settings', label: fr.nav.settings })}
  </nav>`;
}

function page(state: AppState, actions: Actions): TemplateResult {
  switch (state.route) {
    case 'home':
      return homeView(state, actions);
    case 'junk':
      return junkView(state, actions);
    case 'duplicates':
      return duplicatesView(state, actions);
    case 'large':
      return largeView(state, actions);
    case 'bin':
      return binView(state);
    case 'gameMode':
      return gameView(state, actions);
    case 'startup':
      return startupView(state, actions);
    case 'uninstall':
      return uninstallView(state, actions);
    case 'privacy':
      return privacyView(state, actions);
    case 'journal':
      return journalView(state, actions);
    case 'settings':
      return settingsView(state, actions);
  }
}

/** L'écran d'attente, et celui des pannes de démarrage. */
function bootScreen(state: AppState): TemplateResult {
  const failed = state.boot === 'failed' || state.boot === 'outside';
  return html`<div class="loader" data-tauri-drag-region="deep">
    <div class="enter">${hole('ld', 170)}</div>
    <div class="lname"><span class="wm">SHINKU</span><span class="kj" lang="ja">真空</span></div>
    <div class="lstate">
      ${failed
        ? html`<p class="ltext strong" role="alert">${fr.boot.unavailable}</p>
            <p class="ltext">${state.boot === 'outside' ? fr.boot.outsideApp : fr.boot.unavailableHint}</p>`
        : html`<div class="indet" aria-hidden="true"><i></i></div>
            <p class="ltext" role="status">${fr.boot.preparing}</p>`}
    </div>
  </div>`;
}

/**
 * La clé de la page : quand elle change, toute la page est redessinée (et le focus d'un bouton est perdu). Le Journal
 * n'a pas de volet : un grand vide qui se termine pendant qu'il est ouvert ne doit pas le redessiner.
 */
export function pageKey(state: AppState): string {
  return state.route === 'journal' ? 'journal' : `${state.route}:${shownPane(state)}`;
}

export function appView(state: AppState, actions: Actions): TemplateResult {
  const ready = state.boot === 'ready';
  return html`<div class="app">
    <div class="topdrag" data-tauri-drag-region></div>
    ${windowButtons(actions)}
    ${ready
      ? html`${sidebar(state, actions)}
          <main class="main">${keyed(pageKey(state), page(state, actions))}</main>
          ${confirmDialog(state, actions)} ${pickerDialog(state, actions)} ${uninstallDialog(state, actions)}
          ${privacyDialog(state, actions)} ${journalDialog(state, actions)}`
      : bootScreen(state)}
    <div class="toast ${state.toastShown ? 'show' : ''}" role="status">${state.toast}</div>
  </div>`;
}
