export const DOM: Record<
  | 'title'
  | 'page'
  | 'loading'
  | 'intro'
  | 'erase'
  | 'browserCard'
  | 'browserName'
  | 'running'
  | 'browserRow'
  | 'cardRow'
  | 'windowsBlock'
  | 'blockTitle'
  | 'windowsRow'
  | 'rowTitle'
  | 'rowSub'
  | 'rowOutcome'
  | 'rowValue'
  | 'check'
  | 'rowInert'
  | 'rowEmpty'
  | 'lastTitle'
  | 'last'
  | 'lastBlock'
  | 'lastTrace'
  | 'lastHelp'
  | 'lastValue'
  | 'dialog'
  | 'askBlock'
  | 'askName'
  | 'askLine'
  | 'askConfirm'
  | 'askCancel',
  string
>;
export const EXPECTED: {
  title: string;
  intro: string;
  erase: string;
  confirm: string;
  cancel: string;
  direct: string;
  lastTitle: string;
  listChanged: string;
  askTitleOne: string;
  directOne: string;
  chrome: string;
  windows: string;
  history: string;
  recent: string;
  typed: string;
  clipboard: string;
  running: string;
  outcomes: Record<'erased' | 'nothing' | 'inUse' | 'changed' | 'partial' | 'failed', string>;
};

export interface PrivacyRow {
  block: string;
  title: string;
  sub: string;
  outcome: string;
  value: string;
  kind: 'check' | 'inert' | 'empty' | 'autre';
  checked: boolean;
  label: string;
  text: string;
}
export interface PrivacyScreen {
  title: string;
  ready: boolean;
  busy: boolean;
  intro: string;
  rows: PrivacyRow[];
  running: Array<{ block: string; text: string }>;
  warnings: string[];
  lastTitle: string;
  erase: { label: string; inert: boolean } | null;
  last: Array<{ block: string; trace: string; help: string; outcome: string }>;
  dialog: {
    open: boolean;
    title: string;
    blocks: Array<{ name: string; lines: string[] }>;
    text: string;
    focus: string;
  } | null;
}
export function readPrivacy(dom: Record<string, string>): PrivacyScreen;
