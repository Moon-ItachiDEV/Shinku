export const DOM: Record<
  | 'title'
  | 'page'
  | 'loading'
  | 'row'
  | 'sentence'
  | 'date'
  | 'link'
  | 'forget'
  | 'licences'
  | 'side'
  | 'nav',
  string
>;
export const EXPECTED: Record<'title' | 'forget' | 'licences' | 'uninstalled' | 'cleaned' | 'freed' | 'binned' | 'leftovers', string>;
export function readJournal(dom: Record<string, string>): {
  title: string;
  ready: boolean;
  loading: boolean;
  rows: Array<{ sentence: string; date: string; iso: string; link: string }>;
  forget: string;
};
export function readAbout(dom: Record<string, string>): { licences: string | null; visible: boolean };
export function readMenu(dom: Record<string, string>): {
  entries: number;
  scroll: number | null;
  client: number | null;
  lastBottom: number | null;
  height: number;
};
export function judgeDated(
  row: { iso: string; sentence: string },
  from: number,
  now: number,
  what?: string,
): string | null;
export function judgeClean(
  rows: Array<{ iso: string; sentence: string }>,
  expected: Record<string, string>,
  from: number,
  now: number,
): string | null;
