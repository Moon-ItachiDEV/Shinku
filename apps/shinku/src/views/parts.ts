// Les pièces communes à plusieurs écrans.

import { html, nothing, type TemplateResult } from 'lit-html';
import { keyed } from 'lit-html/directives/keyed.js';

import { fr } from '../i18n/fr.ts';
import type { CleanRowId } from '../model/scan.ts';
import { backIcon, playIcon } from './art.ts';

type Content = TemplateResult | typeof nothing;

/** Le nom des trois catégories que le grand vide nettoie. */
export const CLEAN_NAMES: Record<CleanRowId, string> = {
  junkFiles: fr.nav.junk,
  duplicates: fr.nav.duplicates,
  recycleBin: fr.nav.bin,
};

/** Leur couleur : la même sur la barre des Résultats, la confirmation et le bilan. */
export const SEGMENT_CLASS: Record<CleanRowId, string> = {
  junkFiles: 'seg-1',
  duplicates: 'seg-2',
  recycleBin: 'seg-3',
};

/**
 * Un écran : une barre en haut (qui sert aussi à déplacer la fenêtre), un
 * contenu, et le socle des boutons en bas.
 *
 * `key` nomme l'écran : quand il change, l'écran est reconstruit et son
 * animation d'entrée rejouée, au lieu d'être mis à jour sur place.
 */
export function stage(parts: { key?: string; bar?: Content; body: Content; dock?: Content }): TemplateResult {
  return html`${keyed(
    parts.key ?? '',
    html`<div class="stage enter">
      <div class="bar" data-tauri-drag-region="deep">${parts.bar ?? nothing}</div>
      ${parts.body} ${parts.dock ?? nothing}
    </div>`,
  )}`;
}

export function backButton(onClick: () => void, label: string = fr.common.back): TemplateResult {
  return html`<button type="button" class="back" @click=${onClick}>${backIcon()}${label}</button>`;
}

/**
 * Le grand bouton rond. Désactivé, il reste visible : l'action existe, mais pas encore.
 * `play` : un triangle au-dessus de l'étiquette, pour lancer une analyse.
 * `enter` : le bouton apparaît en fondu.
 */
export function orb(
  label: string,
  onClick: (() => void) | null,
  options: { play?: boolean; enter?: boolean } = {},
): TemplateResult {
  return html`<button
    type="button"
    class="orb ${options.enter === true ? 'enter' : ''}"
    ?disabled=${onClick === null}
    @click=${onClick ?? nothing}
  >
    ${options.play === true ? playIcon() : nothing}<span>${label}</span>
  </button>`;
}

/** Le bouton d'arrêt, au centre de l'anneau d'avancement. `disabled` : l'arrêt est déjà demandé. */
export function stopCore(onClick: () => void, disabled = false): TemplateResult {
  return html`<button type="button" class="core" ?disabled=${disabled} @click=${onClick}>
    <span class="sq"></span><span>${fr.common.stop}</span>
  </button>`;
}

/** Un écran d'introduction : une illustration, un titre, une phrase. */
export function hero(art: TemplateResult, title: string, text: string, extra: Content = nothing): TemplateResult {
  return html`<div class="hero">
    ${art}
    <h1>${title}</h1>
    <p>${text}</p>
    ${extra}
  </div>`;
}
