// Un arbre HTML minimal et un sélecteur CSS minimal, pour chercher dans le HTML d'un gabarit les éléments
// que l'essai de la fenêtre cherchera dans la vraie page. Sans navigateur, sans dépendance.
//
// Le sélecteur sait : une balise, des classes, des attributs (`[a]`, `[a="v"]`, `[a^="v"]`), `:has(...)`,
// et les combinateurs « descendant » et « > ». Une autre syntaxe lève une erreur : mieux vaut un essai qui
// refuse qu'un essai qui croit avoir trouvé.

export interface Node {
  tag: string;
  attrs: Record<string, string>;
  children: Node[];
  parent: Node | null;
  /** Le texte écrit directement dans cet élément, balises retirées des enfants. */
  text: string;
}

const VOID = new Set(['input', 'br', 'img', 'hr', 'meta', 'link', 'source']);

export function parse(markup: string): Node {
  const root: Node = { tag: '#root', attrs: {}, children: [], parent: null, text: '' };
  let current = root;
  const token = /<!--[\s\S]*?-->|<\/([a-zA-Z][\w-]*)\s*>|<([a-zA-Z][\w-]*)((?:[^>"']|"[^"]*"|'[^']*')*?)(\/?)>/g;
  let last = 0;
  for (let found = token.exec(markup); found !== null; found = token.exec(markup)) {
    current.text += markup.slice(last, found.index);
    last = found.index + found[0].length;
    if (found[0].startsWith('<!--')) {
      continue;
    }
    const closing = found[1];
    if (closing !== undefined) {
      let up: Node | null = current;
      while (up !== null && up.tag !== closing.toLowerCase()) {
        up = up.parent;
      }
      if (up !== null && up.parent !== null) {
        current = up.parent;
      }
      continue;
    }
    const tag = (found[2] ?? '').toLowerCase();
    const node: Node = { tag, attrs: {}, children: [], parent: current, text: '' };
    const attrs = /([^\s=/]+)(?:=("[^"]*"|'[^']*'|[^\s"'>]+))?/g;
    for (let one = attrs.exec(found[3] ?? ''); one !== null; one = attrs.exec(found[3] ?? '')) {
      const raw = one[2] ?? '';
      node.attrs[(one[1] ?? '').toLowerCase()] = raw.replace(/^["']|["']$/g, '').replace(/&quot;/g, '"');
    }
    current.children.push(node);
    if (found[4] !== '/' && !VOID.has(tag)) {
      current = node;
    }
  }
  return root;
}

/** Le texte visible d'un élément et de tout ce qu'il contient, blancs réduits. */
export function textOfNode(node: Node): string {
  return (node.text + node.children.map(textOfNode).join(' ')).replace(/\s+/g, ' ').trim();
}

interface Compound {
  tag: string | null;
  classes: string[];
  attrs: Array<{ name: string; op: '' | '=' | '^='; value: string }>;
  has: Chain[];
}
type Chain = Array<{ combinator: ' ' | '>'; compound: Compound }>;

function splitTop(text: string): string[] {
  // Coupe sur les blancs et « > » hors crochets et parenthèses, en gardant « > » comme élément.
  const parts: string[] = [];
  let depth = 0;
  let quote = '';
  let word = '';
  for (const character of text.trim()) {
    if (quote !== '') {
      word += character;
      if (character === quote) {
        quote = '';
      }
    } else if (character === '"' || character === "'") {
      quote = character;
      word += character;
    } else if (character === '[' || character === '(') {
      depth += 1;
      word += character;
    } else if (character === ']' || character === ')') {
      depth -= 1;
      word += character;
    } else if (depth === 0 && (character === ' ' || character === '>')) {
      if (word !== '') {
        parts.push(word);
        word = '';
      }
      if (character === '>') {
        parts.push('>');
      }
    } else {
      word += character;
    }
  }
  if (word !== '') {
    parts.push(word);
  }
  return parts;
}

function parseCompound(text: string): Compound {
  const compound: Compound = { tag: null, classes: [], attrs: [], has: [] };
  let rest = text;
  const tag = /^[a-zA-Z][\w-]*/.exec(rest);
  if (tag !== null) {
    compound.tag = tag[0].toLowerCase();
    rest = rest.slice(tag[0].length);
  }
  while (rest !== '') {
    let found: RegExpExecArray | null;
    if ((found = /^\.([\w-]+)/.exec(rest)) !== null) {
      compound.classes.push(found[1] ?? '');
    } else if ((found = /^\[([\w:-]+)(?:(\^?=)(?:"([^"]*)"|([^\]]*)))?\]/.exec(rest)) !== null) {
      compound.attrs.push({ name: (found[1] ?? '').toLowerCase(), op: (found[2] ?? '') as '' | '=' | '^=', value: found[3] ?? found[4] ?? '' });
    } else if ((found = /^:has\(/.exec(rest)) !== null) {
      let depth = 1;
      let index = found[0].length;
      while (index < rest.length && depth > 0) {
        depth += rest[index] === '(' ? 1 : rest[index] === ')' ? -1 : 0;
        index += 1;
      }
      const inner = parseChain(rest.slice(found[0].length, index - 1));
      if (inner.length !== 1) {
        // Un navigateur lit « :has(A B) » dans l'élément ; ce lecteur-ci ne sait lire qu'un composé : il refuse le reste.
        throw new Error(`sélecteur non pris en charge : « ${text} » (:has ne prend qu'un composé, sans combinateur)`);
      }
      compound.has.push(inner);
      rest = rest.slice(index);
      continue;
    } else {
      throw new Error(`sélecteur non pris en charge : « ${text} » (à partir de « ${rest} »)`);
    }
    rest = rest.slice(found[0].length);
  }
  return compound;
}

function parseChain(selector: string): Chain {
  const chain: Chain = [];
  let combinator: ' ' | '>' = ' ';
  for (const part of splitTop(selector)) {
    if (part === '>') {
      combinator = '>';
    } else {
      chain.push({ combinator, compound: parseCompound(part) });
      combinator = ' ';
    }
  }
  if (chain.length === 0) {
    throw new Error(`sélecteur vide : « ${selector} »`);
  }
  return chain;
}

function descendants(node: Node): Node[] {
  return node.children.flatMap((child) => [child, ...descendants(child)]);
}

function matchesCompound(node: Node, compound: Compound): boolean {
  if (compound.tag !== null && node.tag !== compound.tag) {
    return false;
  }
  const classes = (node.attrs['class'] ?? '').split(/\s+/);
  if (!compound.classes.every((name) => classes.includes(name))) {
    return false;
  }
  for (const { name, op, value } of compound.attrs) {
    const actual = node.attrs[name];
    if (actual === undefined || (op === '=' && actual !== value) || (op === '^=' && !actual.startsWith(value))) {
      return false;
    }
  }
  return compound.has.every((chain) => queryChain(node, chain).length > 0);
}

function matchesChain(node: Node, chain: Chain, index: number): boolean {
  const step = chain[index];
  if (step === undefined || !matchesCompound(node, step.compound)) {
    return false;
  }
  if (index === 0) {
    return true;
  }
  if (step.combinator === '>') {
    return node.parent !== null && matchesChain(node.parent, chain, index - 1);
  }
  for (let up = node.parent; up !== null; up = up.parent) {
    if (matchesChain(up, chain, index - 1)) {
      return true;
    }
  }
  return false;
}

function queryChain(scope: Node, chain: Chain): Node[] {
  return descendants(scope).filter((node) => matchesChain(node, chain, chain.length - 1));
}

/** Les éléments sous `scope` que ce sélecteur désigne, dans l'ordre du document. */
export function queryAll(scope: Node, selector: string): Node[] {
  return queryChain(scope, parseChain(selector));
}

export function query(scope: Node, selector: string): Node | null {
  return queryAll(scope, selector)[0] ?? null;
}
