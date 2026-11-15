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

/**
 * Une durée de jeu : « 45 min », « 2 h 10 ». Moins d'une minute se lit « 1 min » ;
 * une heure pile s'écrit sans minutes (« 2 h »).
 */
export function formatDuration(milliseconds: number): string {
  const minutes = Math.max(1, Math.floor(Math.max(0, milliseconds) / MINUTE));
  if (minutes < 60) {
    return `${minutes}${NBSP}min`;
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}${NBSP}h` : `${hours}${NBSP}h${NBSP}${String(rest).padStart(2, '0')}`;
}

/** Heure locale : « 14:32 ». */
export function formatClock(timestamp: number): string {
  const date = new Date(timestamp);
  const two = (value: number) => String(value).padStart(2, '0');
  return `${two(date.getHours())}:${two(date.getMinutes())}`;
}

const MONTHS = ['janv.', 'févr.', 'mars', 'avr.', 'mai', 'juin', 'juil.', 'août', 'sept.', 'oct.', 'nov.', 'déc.'];

/**
 * Un début, pour finir la phrase « depuis … » : « 14:32 » le jour même,
 * « hier, 23:40 » la veille, sinon « le 14 nov., 23:40 » (avec l'année si ce
 * n'est pas l'année en cours). Jours et heures sont ceux de la machine.
 */
export function formatSince(then: number, now: number): string {
  const date = new Date(then);
  const today = new Date(now);
  const clock = formatClock(then);
  const dayOf = (value: Date): number => new Date(value.getFullYear(), value.getMonth(), value.getDate()).getTime();
  // Les jours se comptent sur le calendrier, pas en tranches de 24 h : un changement d'heure ne les décale pas.
  const days = Math.round((dayOf(today) - dayOf(date)) / DAY);
  if (days <= 0) {
    return clock;
  }
  if (days === 1) {
    return `hier, ${clock}`;
  }
  const day = date.getDate() === 1 ? '1er' : String(date.getDate());
  const year = date.getFullYear() === today.getFullYear() ? '' : ` ${date.getFullYear()}`;
  return `le ${day}${NBSP}${MONTHS[date.getMonth()] ?? ''}${year}, ${clock}`;
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
