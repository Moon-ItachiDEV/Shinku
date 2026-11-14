// Les pièces communes à plusieurs écrans.

import { html, nothing, type TemplateResult } from 'lit-html';
import { keyed } from 'lit-html/directives/keyed.js';

import { fr } from '../i18n/fr.ts';
import { backIcon } from './art.ts';

type Content = TemplateResult | typeof nothing;

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

/** Le grand bouton rond. Désactivé, il reste visible : l'action existe, mais pas encore. */
export function orb(label: string, onClick: (() => void) | null): TemplateResult {
  return html`<button type="button" class="orb" ?disabled=${onClick === null} @click=${onClick ?? nothing}>
    ${label}
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
