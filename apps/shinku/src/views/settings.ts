// Les Paramètres. Seuls les réglages qui ont un effet aujourd'hui sont
// montrés : le mode d'affichage et les dossiers jamais touchés.

import { html, nothing, type TemplateResult } from 'lit-html';

import type { Theme } from '../backend/types.ts';
import { fr } from '../i18n/fr.ts';
import type { Actions, AppState } from '../state.ts';
import { crossIcon, folderIcon } from './art.ts';
import { stage } from './parts.ts';

/** Le repère de focus du bouton des licences (essai de la fenêtre). */
export const settingsFocus = { licences: 'settings-licences' };

const THEMES: ReadonlyArray<{ theme: Theme; label: string }> = [
  { theme: 'light', label: fr.settings.light },
  { theme: 'dark', label: fr.settings.dark },
  { theme: 'system', label: fr.settings.system },
];

export function settingsView(state: AppState, actions: Actions): TemplateResult {
  const settings = state.overview?.settings;
  const current = settings?.theme ?? 'system';
  const folders = settings?.excludedFolders ?? [];
  // La liste protège le grand vide en cours : elle ne change pas sous ses pieds.
  const cleaning = state.session.state === 'cleaning';

  const swatches = THEMES.map(
    ({ theme, label }) =>
      html`<button
        type="button"
        class="pal"
        aria-pressed=${theme === current ? 'true' : 'false'}
        @click=${() => actions.setTheme(theme)}
      >
        <span class="pv pv-${theme}" aria-hidden="true"><span class="pvside"></span><span class="pvorb"></span></span>
        <span class="pn">${label}</span>
      </button>`,
  );

  const folderList =
    folders.length === 0
      ? html`<p class="d empty">${fr.settings.foldersEmpty}</p>`
      : html`<ul class="folders">
          ${folders.map(
            (path) =>
              html`<li>
                <span class="ficon">${folderIcon()}</span>
                <span class="fpath" title=${path}>${path}</span>
                <button
                  type="button"
                  class="icb"
                  aria-label=${fr.settings.removeFolder(path)}
                  ?disabled=${cleaning}
                  @click=${() => actions.removeFolder(path)}
                >
                  ${crossIcon()}
                </button>
              </li>`,
          )}
        </ul>`;

  return stage({
    body: html`<div class="scroll">
      <div class="page">
        <h1>${fr.settings.title}</h1>
        <p class="pintro">${fr.settings.intro}</p>
        <div class="pgrid">
          <section class="psec">
            <h2 class="sub flat">${fr.settings.appearance}</h2>
            <div class="card">
              <div class="sblock">
                <div class="lab">
                  <span class="l">${fr.settings.themeLabel}</span>
                  <span class="d">${fr.settings.themeHint}</span>
                </div>
                <div class="pals" role="group" aria-label=${fr.settings.themeLabel}>${swatches}</div>
              </div>
            </div>
          </section>
          <section class="psec">
            <h2 class="sub flat">${fr.settings.security}</h2>
            <div class="card">
              <div class="sblock">
                <div class="lab">
                  <span class="l">${fr.settings.foldersLabel}</span>
                  <span class="d">${fr.settings.foldersHint}</span>
                </div>
                ${folderList}
                <button
                  type="button"
                  class="ghost small add"
                  ?disabled=${state.pickingFolder || cleaning}
                  @click=${actions.addFolder}
                >
                  ${fr.settings.addFolder}
                </button>
                <p class="d fnote">${fr.settings.foldersNote}</p>
              </div>
            </div>
          </section>
          <section class="psec">
            <h2 class="sub flat">${fr.settings.about}</h2>
            <div class="card">
              <div class="sblock">
                <div class="lab">
                  <span class="l">${fr.settings.version(state.overview?.version ?? '')}</span>
                  <span class="d">${fr.settings.aboutFree}</span>
                </div>
                <p class="d">${fr.settings.aboutParts}</p>
                <button
                  type="button"
                  class="ghost small add"
                  data-fk=${settingsFocus.licences}
                  aria-disabled=${state.licencesBusy ? 'true' : nothing}
                  @click=${actions.openLicences}
                >
                  ${fr.settings.licences}
                </button>
                ${state.licencesAnswer === null
                  ? nothing
                  : html`<p class="d fnote" role="status">
                      ${state.licencesAnswer === 'missing' ? fr.settings.licencesMissing : fr.settings.licencesFailed}
                    </p>`}
              </div>
            </div>
          </section>
        </div>
        <p class="plater">${fr.settings.later}</p>
      </div>
    </div>`,
  });
}
