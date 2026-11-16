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
  return write(value, false);
}

/**
 * Le HTML tel que le navigateur le lirait : les valeurs d'attribut entre guillemets, et les liaisons qui
 * n'écrivent rien dans la page (`@click`, `.checked`, `?x`, une valeur `nothing`) retirées. Pour lire la page comme un arbre.
 */
export function markupOf(value: unknown): string {
  return write(value, true);
}

function write(value: unknown, markup: boolean): string {
  const again = (inner: unknown): string => write(inner, markup);
  if (value === nothing || value === null || value === undefined || value === false) {
    return '';
  }
  if (Array.isArray(value)) {
    return value.map(again).join('');
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
      return items.map((item, index) => again((last as (item: unknown, index: number) => unknown)(item, index))).join('');
    }
    // `keyed(clé, gabarit)` : seul le dernier argument s'écrit.
    return again(last);
  }
  if (typeof value === 'object' && 'strings' in value && 'values' in value) {
    const template = value as { strings: readonly string[]; values: readonly unknown[] };
    let out = '';
    template.strings.forEach((text, index) => {
      out += text;
      if (index >= template.values.length) {
        return;
      }
      const inner = template.values[index];
      const binding = markup ? /\s([.@?]?[\w:-]+)=$/.exec(out) : null;
      if (binding !== null) {
        const name = binding[1] ?? '';
        const silent = /^[.@?]/.test(name) || inner === nothing || inner === null || inner === undefined || inner === false;
        if (silent) {
          out = out.slice(0, out.length - binding[0].length);
          return;
        }
        out += `"${again(inner).replace(/"/g, '&quot;')}"`;
        return;
      }
      out += again(inner);
    });
    return out;
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

/**
 * Les gestes `@click` d'un gabarit, dans l'ordre : chacun avec le HTML écrit avant lui dans son gabarit (de quoi
 * reconnaître le bouton) et la fonction. Un test les déclenche comme le ferait un clic, sans navigateur.
 */
export function clicksOf(value: unknown): Array<{ context: string; fn: () => unknown }> {
  const found: Array<{ context: string; fn: () => unknown }> = [];
  const walk = (inner: unknown): void => {
    if (Array.isArray(inner)) {
      inner.forEach(walk);
      return;
    }
    if (typeof inner !== 'object' || inner === null) {
      return;
    }
    if ('_$litDirective$' in inner && 'values' in inner) {
      const { values } = inner as { values: unknown[] };
      const last = values.at(-1);
      if (typeof last === 'function' && values.length > 1) {
        for (const [index, item] of [...(values[0] as Iterable<unknown>)].entries()) {
          walk((last as (item: unknown, index: number) => unknown)(item, index));
        }
      } else {
        walk(last);
      }
      return;
    }
    if ('strings' in inner && 'values' in inner) {
      const template = inner as { strings: readonly string[]; values: readonly unknown[] };
      let out = '';
      template.strings.forEach((text, index) => {
        out += text;
        if (index >= template.values.length) {
          return;
        }
        const value = template.values[index];
        if (/\s@click=$/.test(out) && typeof value === 'function') {
          found.push({ context: out, fn: value as () => unknown });
          out = out.replace(/\s@click=$/, '');
          return;
        }
        walk(value);
        out += htmlOf(value);
      });
    }
  };
  walk(value);
  return found;
}
