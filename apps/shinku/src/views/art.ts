// Icônes et illustrations, reprises du canevas de design.
//
// Tout est dessiné en SVG dans la page : aucune image n'est chargée. Les
// couleurs qui changent avec le mode d'affichage passent par des classes
// (voir app.css) ; les dégradés d'accent sont les mêmes dans les deux modes.
//
// Chaque dessin reçoit un préfixe `p` : il rend uniques les identifiants de
// ses dégradés quand plusieurs dessins sont affichés en même temps.

import { html, nothing, svg, type TemplateResult } from 'lit-html';

import { styles } from './style.ts';

function icon(paths: TemplateResult, size = 17, strokeWidth = 1.6): TemplateResult {
  return html`<svg
    class="ic"
    width=${size}
    height=${size}
    viewBox="0 0 24 24"
    aria-hidden="true"
    fill="none"
    stroke="currentColor"
    stroke-width=${strokeWidth}
    stroke-linecap="round"
    stroke-linejoin="round"
  >
    ${paths}
  </svg>`;
}

const ICON_PATHS = {
  home: svg`<circle cx="12" cy="12.5" r="4.4"></circle><ellipse cx="12" cy="12.5" rx="10" ry="3" transform="rotate(-9 12 12.5)"></ellipse>`,
  junk: svg`<path d="M7 3h7l5 5v13H7z"></path><path d="M14 3v5h5"></path><path d="M10 13h6M10 17h4"></path>`,
  duplicates: svg`<path d="M9 7h6l4 4v10H9z"></path><path d="M15 7v4h4"></path><path d="M5 17V3h7"></path>`,
  large: svg`<rect x="3" y="3" width="10" height="11" rx="1.5"></rect><rect x="15" y="3" width="6" height="7" rx="1.5"></rect><rect x="15" y="12" width="6" height="9" rx="1.5"></rect><rect x="3" y="16" width="10" height="5" rx="1.5"></rect>`,
  bin: svg`<path d="M4 7h16"></path><path d="M9 7V4.5h6V7"></path><path d="M6 7l1 13h10l1-13"></path><path d="M10 11v5M14 11v5"></path>`,
  startup: svg`<path d="M12 3v8"></path><path d="M6.3 6.8a8 8 0 1 0 11.4 0"></path>`,
  gameMode: svg`<path d="M7 9h10a4 4 0 0 1 3.9 4.9l-1 4.2a2.4 2.4 0 0 1-4.2.9L14 17h-4l-1.7 2a2.4 2.4 0 0 1-4.2-.9l-1-4.2A4 4 0 0 1 7 9z"></path><path d="M8 12v3M6.5 13.5h3"></path><path d="M15.5 12.5h.01M17.5 14.5h.01"></path>`,
  uninstall: svg`<rect x="4" y="4" width="7" height="7" rx="2"></rect><rect x="13" y="4" width="7" height="7" rx="2"></rect><rect x="4" y="13" width="7" height="7" rx="2"></rect><rect x="13" y="13" width="7" height="7" rx="2" stroke-dasharray="2 2.4"></rect>`,
  privacy: svg`<path d="M12 3l7 3v5.5c0 4.2-3 7.8-7 9.5-4-1.7-7-5.3-7-9.5V6z"></path><path d="M9 12l2.2 2.2L15.5 10"></path>`,
  settings: svg`<path d="M4 7h10M18 7h2M4 17h4M12 17h8"></path><path d="M14 5v4M8 15v4"></path>`,
  folder: svg`<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"></path>`,
  cross: svg`<path d="M6 6l12 12M18 6L6 18"></path>`,
} as const;

export type IconName = keyof typeof ICON_PATHS;

export function navIcon(name: IconName, size = 17): TemplateResult {
  return icon(ICON_PATHS[name], size);
}

export const folderIcon = (): TemplateResult => icon(ICON_PATHS.folder, 16);
export const crossIcon = (): TemplateResult => icon(ICON_PATHS.cross, 14);

export const checkIcon = (): TemplateResult => html`<svg class="ic" width="11" height="11" viewBox="0 0 12 12" aria-hidden="true">
  <path
    d="M2.5 6.2l2.3 2.3 4.7-5"
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
  ></path>
</svg>`;

export const chevronIcon = (): TemplateResult => html`<svg
  class="ic chev"
  width="16"
  height="16"
  viewBox="0 0 24 24"
  aria-hidden="true"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M9 6l6 6-6 6"></path>
</svg>`;

export const backIcon = (): TemplateResult => html`<svg
  class="ic"
  width="16"
  height="16"
  viewBox="0 0 24 24"
  aria-hidden="true"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
>
  <path d="M15 6l-6 6 6 6"></path>
</svg>`;

export const minimizeIcon = (): TemplateResult => html`<svg class="ic" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
  <path d="M0 5h10" stroke="currentColor" stroke-width="1"></path>
</svg>`;

export const maximizeIcon = (): TemplateResult => html`<svg class="ic" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
  <rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" stroke-width="1"></rect>
</svg>`;

export const closeIcon = (): TemplateResult => html`<svg class="ic" width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
  <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" stroke-width="1"></path>
</svg>`;

/** Le logo : un petit trou noir. */
export function mark(p: string, size = 22): TemplateResult {
  return html`<svg class="ic" width=${size} height=${size} viewBox="0 0 24 24" aria-hidden="true">
    <defs>
      <linearGradient id="${p}-mk" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#FBE3C8"></stop>
        <stop offset="0.55" stop-color="#F0A77E"></stop>
        <stop offset="1" stop-color="#C98BC0"></stop>
      </linearGradient>
    </defs>
    <g transform="rotate(-9 12 12.5)">
      <ellipse
        cx="12"
        cy="12.5"
        rx="10.6"
        ry="2.7"
        fill="none"
        stroke="url(#${p}-mk)"
        stroke-width="1.3"
        stroke-opacity="0.7"
      ></ellipse>
    </g>
    <circle class="f-hole" cx="12" cy="12.5" r="5.3" stroke="url(#${p}-mk)" stroke-width="1.6"></circle>
    <g transform="rotate(-9 12 12.5)">
      <path d="M1.4 12.5A10.6 2.7 0 0 0 22.6 12.5" fill="none" stroke="url(#${p}-mk)" stroke-width="1.3"></path>
    </g>
  </svg>`;
}

/** Le trou noir de l'Accueil et de l'Analyse. `fast` : il tourne plus vite pendant une analyse. */
export function hole(p: string, width: number, fast = false): TemplateResult {
  return html`<svg class="ill ${fast ? 'fast' : ''}" viewBox="0 0 200 200" width=${width} aria-hidden="true">
    <defs>
      <radialGradient id="${p}-halo" cx="0.5" cy="0.5" r="0.5">
        <stop offset="0.22" stop-color="#F0A77E" stop-opacity="0.22"></stop>
        <stop offset="0.6" stop-color="#9D93E8" stop-opacity="0.08"></stop>
        <stop offset="1" stop-color="#9D93E8" stop-opacity="0"></stop>
      </radialGradient>
      <linearGradient id="${p}-disk" gradientUnits="userSpaceOnUse" x1="14" y1="0" x2="186" y2="0">
        <stop offset="0" stop-color="#9D93E8" stop-opacity="0"></stop>
        <stop offset="0.2" stop-color="#C98BC0" stop-opacity="0.8"></stop>
        <stop offset="0.48" stop-color="#FBE3C8"></stop>
        <stop offset="0.8" stop-color="#F0A77E" stop-opacity="0.85"></stop>
        <stop offset="1" stop-color="#9D93E8" stop-opacity="0"></stop>
      </linearGradient>
      <linearGradient id="${p}-lens" gradientUnits="userSpaceOnUse" x1="62" y1="0" x2="138" y2="0">
        <stop offset="0" stop-color="#C98BC0" stop-opacity="0"></stop>
        <stop offset="0.2" stop-color="#C98BC0" stop-opacity="0.8"></stop>
        <stop offset="0.5" stop-color="#FBE3C8"></stop>
        <stop offset="0.82" stop-color="#F0A77E" stop-opacity="0.8"></stop>
        <stop offset="1" stop-color="#F0A77E" stop-opacity="0"></stop>
      </linearGradient>
      <linearGradient id="${p}-ring" x1="0" y1="0" x2="1" y2="1">
        <stop offset="0" stop-color="#FBE3C8"></stop>
        <stop offset="0.55" stop-color="#F0A77E"></stop>
        <stop offset="1" stop-color="#C98BC0"></stop>
      </linearGradient>
      <filter id="${p}-soft" x="-40%" y="-60%" width="180%" height="220%">
        <feGaussianBlur stdDeviation="2.4"></feGaussianBlur>
      </filter>
      <clipPath id="${p}-top"><rect x="0" y="0" width="200" height="104"></rect></clipPath>
      <clipPath id="${p}-bot"><rect x="0" y="104" width="200" height="96"></rect></clipPath>
    </defs>
    <circle cx="100" cy="104" r="96" fill="url(#${p}-halo)"></circle>
    <g transform="rotate(-9 100 104)">
      <g clip-path="url(#${p}-top)">
        <ellipse cx="100" cy="104" rx="88" ry="17" fill="none" stroke="url(#${p}-disk)" stroke-opacity="0.3" stroke-width="0.6"></ellipse>
        <ellipse cx="100" cy="104" rx="70" ry="13.5" fill="none" stroke="url(#${p}-disk)" stroke-opacity="0.55" stroke-width="7"></ellipse>
        <ellipse class="flow" cx="100" cy="104" rx="79" ry="15.2" fill="none" stroke="url(#${p}-disk)" stroke-opacity="0.6" stroke-width="0.7" stroke-dasharray="4 7 12 6 3 8"></ellipse>
      </g>
      <path d="M64 104A36 36 0 0 1 136 104" fill="none" stroke="url(#${p}-lens)" stroke-width="5" stroke-opacity="0.35" filter="url(#${p}-soft)"></path>
      <path d="M64 104A36 36 0 0 1 136 104" fill="none" stroke="url(#${p}-lens)" stroke-width="2.2" stroke-linecap="round"></path>
      <path d="M69 104A31 31 0 0 0 131 104" fill="none" stroke="url(#${p}-lens)" stroke-opacity="0.6" stroke-width="1"></path>
      <circle class="breathe" cx="100" cy="104" r="30.5" fill="none" stroke="url(#${p}-ring)" stroke-width="1.8"></circle>
      <circle class="f-hole" cx="100" cy="104" r="29"></circle>
      <g clip-path="url(#${p}-bot)">
        <ellipse cx="100" cy="104" rx="88" ry="17" fill="none" stroke="url(#${p}-disk)" stroke-opacity="0.3" stroke-width="0.6"></ellipse>
        <ellipse cx="100" cy="104" rx="70" ry="13.5" fill="none" stroke="url(#${p}-disk)" stroke-width="7" stroke-opacity="0.9"></ellipse>
        <ellipse class="flow" cx="100" cy="104" rx="79" ry="15.2" fill="none" stroke="url(#${p}-disk)" stroke-opacity="0.6" stroke-width="0.7" stroke-dasharray="4 7 12 6 3 8"></ellipse>
        <ellipse cx="100" cy="104" rx="58" ry="11" fill="none" stroke="#FFEBD6" stroke-opacity="0.55" stroke-width="0.9"></ellipse>
      </g>
    </g>
  </svg>`;
}

function moduleDefs(p: string): TemplateResult {
  return svg`<defs>
    <radialGradient id="${p}-halo" cx="0.5" cy="0.5" r="0.5">
      <stop offset="0" stop-color="#9D93E8" stop-opacity="0.16"></stop>
      <stop offset="0.55" stop-color="#F0A77E" stop-opacity="0.05"></stop>
      <stop offset="1" stop-color="#9D93E8" stop-opacity="0"></stop>
    </radialGradient>
    <linearGradient id="${p}-glass" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="0.11"></stop>
      <stop offset="1" stop-color="#FFFFFF" stop-opacity="0.025"></stop>
    </linearGradient>
    <linearGradient id="${p}-edge" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#FBE3C8"></stop>
      <stop offset="0.5" stop-color="#F0A77E"></stop>
      <stop offset="1" stop-color="#9D93E8"></stop>
    </linearGradient>
    <linearGradient id="${p}-warm" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#F0A77E" stop-opacity="0.8"></stop>
      <stop offset="1" stop-color="#C98BC0" stop-opacity="0.35"></stop>
    </linearGradient>
    <linearGradient id="${p}-orbit" gradientUnits="userSpaceOnUse" x1="14" y1="0" x2="206" y2="0">
      <stop offset="0" stop-color="#9D93E8" stop-opacity="0"></stop>
      <stop offset="0.3" stop-color="#C98BC0" stop-opacity="0.7"></stop>
      <stop offset="0.55" stop-color="#FBE3C8"></stop>
      <stop offset="0.8" stop-color="#F0A77E" stop-opacity="0.6"></stop>
      <stop offset="1" stop-color="#9D93E8" stop-opacity="0"></stop>
    </linearGradient>
    <linearGradient id="${p}-sweep" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#F0A77E" stop-opacity="0"></stop>
      <stop offset="0.5" stop-color="#FBE3C8" stop-opacity="0.9"></stop>
      <stop offset="1" stop-color="#F0A77E" stop-opacity="0"></stop>
    </linearGradient>
  </defs>`;
}

/** Le cadre commun des illustrations de module. `scan` : un rayon balaie le dessin. */
function frame(p: string, inner: TemplateResult, scan: boolean): TemplateResult {
  const sweep = scan
    ? svg`<g class="sweep">
        <rect x="40" y="22" width="140" height="1.6" rx="0.8" fill="url(#${p}-sweep)"></rect>
        <rect x="60" y="18" width="100" height="10" rx="5" fill="url(#${p}-sweep)" opacity="0.18"></rect>
      </g>`
    : nothing;
  return html`<svg class="ill" viewBox="0 0 220 180" width="210" aria-hidden="true">
    ${moduleDefs(p)}
    <circle cx="110" cy="92" r="88" fill="url(#${p}-halo)"></circle>
    <ellipse cx="110" cy="146" rx="94" ry="13" fill="none" stroke="url(#${p}-orbit)" stroke-width="1.1" stroke-opacity="0.7" transform="rotate(-5 110 146)"></ellipse>
    <g class="float">${inner}</g>
    ${sweep}
  </svg>`;
}

function junkArt(p: string, scan: boolean): TemplateResult {
  return frame(
    p,
    svg`<g transform="rotate(-13 74 92)"><rect class="s-hair" x="44" y="48" width="62" height="80" rx="9" fill="url(#${p}-glass)" stroke-opacity="0.16" stroke-width="1"></rect></g>
      <g transform="rotate(11 150 90)"><rect class="s-hair" x="118" y="46" width="62" height="80" rx="9" fill="url(#${p}-glass)" stroke-opacity="0.16" stroke-width="1"></rect></g>
      <rect class="f-ill" x="76" y="34" width="68" height="90" rx="10" stroke="url(#${p}-edge)" stroke-width="1.4"></rect>
      <rect x="76" y="34" width="68" height="90" rx="10" fill="url(#${p}-glass)"></rect>
      <path class="s-hair" d="M90 60h40M90 72h40M90 84h24" stroke-opacity="0.3" stroke-width="2.2" stroke-linecap="round"></path>
      <circle cx="96" cy="104" r="3" fill="#F0A77E"></circle>
      <path d="M104 104h22" stroke="#F0A77E" stroke-opacity="0.6" stroke-width="2.2" stroke-linecap="round"></path>
      <circle cx="62" cy="34" r="2" fill="#FBE3C8" fill-opacity="0.6"></circle>
      <circle cx="166" cy="30" r="1.6" fill="#C98BC0" fill-opacity="0.7"></circle>
      <circle cx="182" cy="62" r="1.2" fill="#FBE3C8" fill-opacity="0.5"></circle>`,
    scan,
  );
}

function duplicatesArt(p: string, scan: boolean): TemplateResult {
  return frame(
    p,
    svg`<rect class="s-hair" x="92" y="28" width="80" height="100" rx="11" fill="none" stroke-opacity="0.22" stroke-width="1.2" stroke-dasharray="4 5"></rect>
      <rect class="f-ill" x="58" y="46" width="80" height="100" rx="11" stroke="url(#${p}-edge)" stroke-width="1.4"></rect>
      <rect x="68" y="56" width="60" height="50" rx="7" fill="url(#${p}-glass)"></rect>
      <circle cx="112" cy="70" r="6" fill="#F0A77E"></circle>
      <path class="s-hair" d="M70 102l17-19 11 11 8-7 20 15" fill="none" stroke-opacity="0.5" stroke-width="1.6" stroke-linejoin="round" stroke-linecap="round"></path>
      <path class="s-hair" d="M70 120h42M70 131h26" stroke-opacity="0.26" stroke-width="2.2" stroke-linecap="round"></path>`,
    scan,
  );
}

function largeArt(p: string, scan: boolean): TemplateResult {
  return frame(
    p,
    svg`<rect class="f-ill" x="50" y="34" width="76" height="100" rx="11" stroke="url(#${p}-edge)" stroke-width="1.4"></rect>
      <rect x="50" y="34" width="76" height="100" rx="11" fill="url(#${p}-warm)" fill-opacity="0.22"></rect>
      <path class="s-cream" d="M66 112h30" stroke-opacity="0.7" stroke-width="2.4" stroke-linecap="round"></path>
      <rect class="s-hair" x="132" y="34" width="40" height="44" rx="9" fill="url(#${p}-glass)" stroke-opacity="0.16" stroke-width="1"></rect>
      <rect class="s-hair" x="132" y="84" width="40" height="26" rx="8" fill="url(#${p}-glass)" stroke-opacity="0.16" stroke-width="1"></rect>
      <rect class="s-hair" x="132" y="116" width="40" height="18" rx="7" fill="url(#${p}-glass)" stroke-opacity="0.16" stroke-width="1"></rect>`,
    scan,
  );
}

/** La corbeille. `level` : son remplissage, de 0 (vide) à 1 (pleine). */
export function binArt(p: string, level: number): TemplateResult {
  const body = 'M74 56L81 136Q82 146 92 146L128 146Q138 146 139 136L146 56Z';
  const scale = Math.min(1, Math.max(0, level));
  return frame(
    p,
    svg`<clipPath id="${p}-bin"><path d=${body}></path></clipPath>
      <path class="f-ill" d=${body}></path>
      <g clip-path="url(#${p}-bin)">
        <g class="drain" ${styles({ transform: `scaleY(${scale.toFixed(3)})` })}>
          <rect x="70" y="72" width="80" height="80" fill="url(#${p}-warm)"></rect>
          <rect x="70" y="72" width="80" height="2" fill="#FBE3C8" fill-opacity="0.8"></rect>
        </g>
      </g>
      <path d=${body} fill="url(#${p}-glass)" stroke="url(#${p}-edge)" stroke-width="1.4"></path>
      <path class="s-hair" d="M98 72v58M122 72v58" stroke-opacity="0.1" stroke-width="1.4" stroke-linecap="round"></path>
      <path d="M66 48h88" stroke="url(#${p}-edge)" stroke-linecap="round" stroke-width="2.6"></path>
      <path d="M98 48v-8h24v8" fill="none" stroke="url(#${p}-edge)" stroke-width="1.4" stroke-linejoin="round"></path>`,
    false,
  );
}

function startupArt(p: string): TemplateResult {
  return frame(
    p,
    svg`<circle class="f-ill s-hair" cx="110" cy="86" r="52" stroke-opacity="0.16" stroke-width="1"></circle>
      <circle cx="110" cy="86" r="52" fill="url(#${p}-glass)"></circle>
      <path d="M110 34A52 52 0 1 1 61 103" fill="none" stroke="url(#${p}-edge)" stroke-width="3" stroke-linecap="round"></path>
      <path class="s-text" d="M110 62v22" stroke-width="3.2" stroke-linecap="round"></path>
      <path class="s-text" d="M96.5 70.5a20 20 0 1 0 27 0" fill="none" stroke-opacity="0.8" stroke-width="3.2" stroke-linecap="round"></path>`,
    false,
  );
}

function gameModeArt(p: string): TemplateResult {
  const pad =
    'M7 9h10a4 4 0 0 1 3.9 4.9l-1 4.2a2.4 2.4 0 0 1-4.2.9L14 17h-4l-1.7 2a2.4 2.4 0 0 1-4.2-.9l-1-4.2A4 4 0 0 1 7 9z';
  return frame(
    p,
    svg`<g transform="translate(48 12) scale(5.2)">
        <path class="f-ill" d=${pad} stroke="url(#${p}-edge)" stroke-width="0.27" stroke-linejoin="round"></path>
        <path d=${pad} fill="url(#${p}-glass)"></path>
        <path class="s-text" d="M8 12v3M6.5 13.5h3" fill="none" stroke-opacity="0.75" stroke-width="0.6" stroke-linecap="round"></path>
        <circle cx="15.6" cy="12.6" r="0.75" fill="#F0A77E"></circle>
        <circle cx="17.5" cy="14.4" r="0.75" fill="#C98BC0"></circle>
      </g>`,
    false,
  );
}

function uninstallArt(p: string): TemplateResult {
  return frame(
    p,
    svg`<rect class="f-ill" x="62" y="50" width="44" height="44" rx="12" stroke="url(#${p}-edge)" stroke-width="1.4"></rect>
      <rect class="s-hair" x="62" y="100" width="44" height="44" rx="12" fill="url(#${p}-glass)" stroke-opacity="0.16" stroke-width="1"></rect>
      <rect class="s-hair" x="112" y="100" width="44" height="44" rx="12" fill="url(#${p}-glass)" stroke-opacity="0.16" stroke-width="1"></rect>
      <rect class="s-hair" x="112" y="50" width="44" height="44" rx="12" fill="none" stroke-opacity="0.2" stroke-width="1.2" stroke-dasharray="4 5"></rect>
      <g transform="rotate(14 146 44)" opacity="0.7">
        <rect class="s-cream" x="126" y="24" width="40" height="40" rx="11" fill="url(#${p}-warm)" fill-opacity="0.4" stroke-opacity="0.5" stroke-width="1"></rect>
      </g>
      <rect x="172" y="22" width="6" height="6" rx="1.5" fill="#F0A77E" fill-opacity="0.6" transform="rotate(20 175 25)"></rect>
      <rect x="180" y="40" width="4" height="4" rx="1" fill="#C98BC0" fill-opacity="0.6"></rect>
      <rect x="164" y="10" width="3.5" height="3.5" rx="1" fill="#FBE3C8" fill-opacity="0.5"></rect>`,
    false,
  );
}

function privacyArt(p: string): TemplateResult {
  const shield = 'M12 3l7 3v5.5c0 4.2-3 7.8-7 9.5-4-1.7-7-5.3-7-9.5V6z';
  return frame(
    p,
    svg`<g transform="translate(43.5 6) scale(5.55)">
        <path class="f-ill" d=${shield} stroke="url(#${p}-edge)" stroke-width="0.25" stroke-linejoin="round"></path>
        <path d=${shield} fill="url(#${p}-glass)"></path>
      </g>
      <circle cx="110" cy="78" r="9" fill="#F0A77E"></circle>
      <path d="M106 84L103.5 104H116.5L114 84Z" fill="#F0A77E" fill-opacity="0.85"></path>`,
    false,
  );
}

export type ModuleArt = 'junk' | 'duplicates' | 'large' | 'startup' | 'gameMode' | 'uninstall' | 'privacy';

/** L'illustration d'un module. `scan` : un rayon la balaie pendant l'analyse. */
export function moduleArt(name: ModuleArt, scan = false): TemplateResult {
  // Le préfixe distingue aussi la version « analyse » : les deux peuvent se
  // croiser le temps d'une transition.
  const p = scan ? `${name}-s` : name;
  switch (name) {
    case 'junk':
      return junkArt(p, scan);
    case 'duplicates':
      return duplicatesArt(p, scan);
    case 'large':
      return largeArt(p, scan);
    case 'startup':
      return startupArt(p);
    case 'gameMode':
      return gameModeArt(p);
    case 'uninstall':
      return uninstallArt(p);
    case 'privacy':
      return privacyArt(p);
  }
}

/**
 * L'anneau d'avancement autour d'un bouton rond. `fraction` : la part faite,
 * de 0 à 1, ou `null` quand elle est inconnue (l'anneau tourne alors).
 */
export function ring(p: string, fraction: number | null, core: TemplateResult): TemplateResult {
  const LENGTH = 320.4;
  const arc =
    fraction === null
      ? svg`<g class="spin"><circle class="ri" cx="56" cy="56" r="51" fill="none" stroke="url(#${p}-g)" stroke-width="2.4" stroke-linecap="round"></circle></g>`
      : svg`<circle
          class="rp"
          cx="56"
          cy="56"
          r="51"
          fill="none"
          stroke="url(#${p}-g)"
          stroke-width="2.4"
          stroke-linecap="round"
          transform="rotate(-90 56 56)"
          ${styles({ 'stroke-dashoffset': (LENGTH * (1 - Math.min(1, Math.max(0, fraction)))).toFixed(1) })}
        ></circle>`;
  return html`<div class="ring">
    <svg width="112" height="112" viewBox="0 0 112 112" aria-hidden="true">
      <defs>
        <linearGradient id="${p}-g" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stop-color="#FBE3C8"></stop>
          <stop offset="0.5" stop-color="#F0A77E"></stop>
          <stop offset="1" stop-color="#9D93E8"></stop>
        </linearGradient>
      </defs>
      <circle class="s-hair" cx="56" cy="56" r="51" fill="none" stroke-opacity="0.07" stroke-width="2"></circle>
      ${arc}
    </svg>
    ${core}
  </div>`;
}
