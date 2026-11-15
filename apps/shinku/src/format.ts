// Mise en forme des nombres, des tailles et des dates, à la française.
//
// Les tailles suivent l'Explorateur de Windows : unités binaires
// (1 Go = 1024 Mo) et virgule décimale. C'est la même règle que `ByteSize`
// dans le cœur de Shinku.

/** Espace insécable : entre un nombre et son unité. */
export const NBSP = '\u00a0';
/** Espace fine insécable : séparateur de milliers. */
export const NNBSP = '\u202f';

const UNITS = ['Ko', 'Mo', 'Go', 'To'] as const;

/** `3972844749` → « 3,7 Go ». */
export function formatBytes(bytes: number): string {
  const whole = Math.max(0, Math.floor(bytes));
  if (whole < 1024) {
    return `${whole}${NBSP}${plural(whole, 'octet', 'octets')}`;
  }
  let value = whole / 1024;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const text = value >= 100 ? value.toFixed(0) : value.toFixed(1).replace('.', ',');
  return `${text}${NBSP}${UNITS[unit]}`;
}

/** Un seuil rond s'écrit sans décimale : « 3 Go », pas « 3,0 Go ». */
export function formatLimit(bytes: number): string {
  return formatBytes(bytes).replace(`,0${NBSP}`, NBSP);
}

/**
 * Les espaces de la typographie française : insécable avant « : », fine
 * insécable avant « ; », « ! » et « ? ». Sans elles, un signe de ponctuation
 * peut se retrouver seul au début d'une ligne.
 */
export function frenchSpacing(text: string): string {
  return text.replace(/ :/g, `${NBSP}:`).replace(/ ([;!?])/g, `${NNBSP}$1`);
}

/** `1284` → « 1 284 » (espace fine insécable). */
export function formatCount(count: number): string {
  const digits = String(Math.max(0, Math.floor(count)));
  let grouped = '';
  for (let index = 0; index < digits.length; index += 1) {
    if (index > 0 && (digits.length - index) % 3 === 0) {
      grouped += NNBSP;
    }
    grouped += digits[index];
  }
  return grouped;
}

/** En français, zéro et un prennent le singulier : « 0 fichier », « 2 fichiers ». */
export function plural(count: number, one: string, many: string): string {
  return count < 2 ? one : many;
}

/** « 1 284 éléments ». */
export function countOf(count: number, one: string, many: string): string {
  return `${formatCount(count)}${NBSP}${plural(count, one, many)}`;
}

/** « Chrome », « Chrome et Discord », « Chrome, Discord et Steam ». */
export function listOf(items: readonly string[]): string {
  const last = items[items.length - 1];
  if (last === undefined) {
    return '';
  }
  return items.length === 1 ? last : `${items.slice(0, -1).join(', ')} et ${last}`;
}

/** Part faite, en pour cent entiers, bornée entre 0 et 100. */
export function percent(done: number, total: number): number {
  if (total <= 0) {
    return 0;
  }
  return Math.min(100, Math.max(0, Math.floor((done / total) * 100)));
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Temps écoulé : « à l'instant », « il y a 8 mois », « il y a 2 ans ». */
export function formatAgo(then: number, now: number): string {
  const elapsed = now - then;
  if (elapsed < MINUTE) {
    return 'à l’instant';
  }
  if (elapsed < HOUR) {
    return `il y a ${Math.floor(elapsed / MINUTE)}${NBSP}min`;
  }
  if (elapsed < DAY) {
    return `il y a ${Math.floor(elapsed / HOUR)}${NBSP}h`;
  }
  const days = Math.floor(elapsed / DAY);
  if (days < 31) {
    return `il y a ${days}${NBSP}${plural(days, 'jour', 'jours')}`;
  }
  if (days < 365) {
    // « mois » est invariable.
    return `il y a ${Math.floor(days / 30.44)}${NBSP}mois`;
  }
  const years = Math.floor(days / 365.25);
  return `il y a ${Math.max(1, years)}${NBSP}${plural(years, 'an', 'ans')}`;
}

/** Heure locale : « 14:32 ». */
export function formatClock(timestamp: number): string {
  const date = new Date(timestamp);
  const two = (value: number) => String(value).padStart(2, '0');
  return `${two(date.getHours())}:${two(date.getMinutes())}`;
}

/** Sépare un chemin en nom et dossier parent : pour Windows (`\`) et pour le développement (`/`). */
export function splitPath(path: string): { name: string; parent: string } {
  const trimmed = path.replace(/[\\/]+$/, '');
  const cut = Math.max(trimmed.lastIndexOf('\\'), trimmed.lastIndexOf('/'));
  if (cut < 0) {
    return { name: trimmed, parent: '' };
  }
  const parent = trimmed.slice(0, cut);
  // « C: » seul redevient « C:\ », et la racine Unix reste « / ».
  const shown = parent === '' ? '/' : /^[A-Za-z]:$/.test(parent) ? `${parent}\\` : parent;
  return { name: trimmed.slice(cut + 1), parent: shown };
}

/** « C:\ » → « C: » : la lettre d'un disque, pour la jauge. */
export function driveName(root: string): string {
  const match = /^([A-Za-z]:)/.exec(root);
  return match?.[1] ?? root;
}
