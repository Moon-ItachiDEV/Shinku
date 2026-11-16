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
export const EXPECTED: Record<'title' | 'forget' | 'licences' | 'uninstalled' | 'cleaned' | 'leftovers', string>;
export function readJournal(dom: Record<string, string>): {
  title: string;
  ready: boolean;
  loading: boolean;
  rows: Array<{ sentence: string; date: string; link: string }>;
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
