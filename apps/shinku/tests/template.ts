// De quoi lire un gabarit lit-html sans navigateur : le texte HTML qu'il écrit,
// valeurs comprises, pour y chercher des textes et des attributs.
//
// lit-html a besoin d'un `document` à son import : les tests importent les
// écrans après `fakeDocument()`.

import { nothing } from 'lit-html';

export function fakeDocument(): void {
  Object.assign(globalThis, {
    document: { createTreeWalker: () => ({}), addEventListener: () => {}, activeElement: null, body: {}, hidden: false },
  });
}

/** Le HTML d'un gabarit, ses valeurs écrites à la place de leurs trous (un bouton `@click` s'écrit `[geste]`). */
export function htmlOf(value: unknown): string {
  if (value === nothing || value === null || value === undefined || value === false) {
    return '';
  }
  if (Array.isArray(value)) {
    return value.map(htmlOf).join('');
  }
  if (typeof value === 'function') {
    return '[geste]';
  }
  if (typeof value === 'object' && '_$litDirective$' in value && 'values' in value) {
    const { values } = value as { values: unknown[] };
    const last = values.at(-1);
    if (values.length === 1 && typeof last === 'function') {
      // `ref(fonction)` : ni texte ni attribut.
      return '';
    }
    if (typeof last === 'function') {
      // `repeat(éléments, [clé,] gabarit)` : le gabarit s'écrit pour chaque élément.
      const items = [...(values[0] as Iterable<unknown>)];
      return items.map((item, index) => htmlOf((last as (item: unknown, index: number) => unknown)(item, index))).join('');
    }
    // `keyed(clé, gabarit)` : seul le dernier argument s'écrit.
    return htmlOf(last);
  }
  if (typeof value === 'object' && 'strings' in value && 'values' in value) {
    const template = value as { strings: readonly string[]; values: readonly unknown[] };
    return template.strings.map((text, index) => text + (index < template.values.length ? htmlOf(template.values[index]) : '')).join('');
  }
  return String(value);
}

/** Le texte visible d'un gabarit : sans balises, blancs réduits. */
export function textOf(value: unknown): string {
  return htmlOf(value)
    .replace(/<[^>]*>/g, ' ')
    .replace(/[ \t\r\n]+/g, ' ')
    .trim();
}
