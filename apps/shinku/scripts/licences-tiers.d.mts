export type ExpressionTree = { kind: 'id'; name: string } | { kind: 'or' | 'and'; items: ExpressionTree[] };

export interface TextFile {
  name: string;
  content: string;
  notice?: boolean;
}

export interface Rejected {
  name: string;
  kind: 'link' | 'directory' | 'other' | 'large' | 'empty';
  reason: string;
}

export interface ReadFiles {
  files: TextFile[];
  rejected: Rejected[];
}

export interface TreePackage {
  name: string;
  version: string;
}

export interface MetadataPackage {
  name: string;
  version: string;
  source: string | null;
  license: string | null;
  repository: string | null;
  manifest_path: string;
}

export interface Package {
  ecosystem: 'rust' | 'npm';
  name: string;
  version: string;
  license: string | null;
  repository: string | null;
  dir: string;
  texts?: TextFile[];
  complement?: string | null;
}

export interface PackageWithTexts extends Package {
  texts: TextFile[];
  rejected?: Rejected[];
  complement: string | null;
}

export interface Complement {
  name: string;
  version: string;
  files: TextFile[];
  source: string;
}

export const ALLOWED_LICENSES: string[];
export const PAGE_DEV_PACKAGES: string[];
export const TREE_TARGET: string;
export const TREE_SELECTIONS: string[][];
export const MAX_TEXT_BYTES: number;
export const LICENSE_FILE_NAME: RegExp;
export const DEFAULT_OUTPUT: string;
export const APP_DIR: string;
export const REPO_DIR: string;

export function parseExpression(text: string): ExpressionTree;
export function licenseNames(tree: ExpressionTree): string[];
export function isSatisfied(tree: ExpressionTree, allowed?: string[]): boolean;
export function checkExpression(expression: string | null | undefined, allowed?: string[]): string | null;
export function cargoCalls(options?: { offline?: boolean }): { trees: string[][]; metadata: string[] };
export function parseCargoTree(text: string): TreePackage[];
export function unionPackages(lists: TreePackage[][]): TreePackage[];
export function comparePackages(a: TreePackage, b: TreePackage): number;
export function resolveRustPackages(lists: TreePackage[][], metadata: { packages: MetadataPackage[] }): { packages: Package[]; errors: string[] };
export function resolvePagePackages(
  lock: { packages: Record<string, { version?: string; license?: string; dev?: boolean }> },
  root: string,
  extra?: string[],
): { packages: Package[]; errors: string[] };
export function normalizeText(text: string): string;
export function isNotice(name: string): boolean;
export function licenseTexts(files: TextFile[]): TextFile[];
export function readLicenseFiles(dir: string): ReadFiles;
export function readComplements(dir: string): { complements: Map<string, Complement>; errors: string[] };
export function attachTexts(
  packages: Package[],
  complements: Map<string, Complement>,
  readFiles?: (dir: string) => ReadFiles,
): { packages: PackageWithTexts[]; errors: string[] };
export function checkPackages(packages: PackageWithTexts[], allowed?: string[]): string[];
export function needsSourceAddress(one: { license: string | null }): boolean;
export function numberTexts(packages: PackageWithTexts[]): { texts: string[]; perPackage: number[][] };
export function composeFile(input: { fixed: string; packages: PackageWithTexts[] }): string;
export function build(options?: { offline?: boolean; run?: (args: string[]) => string; appDir?: string }): {
  content: string;
  errors: string[];
  packages: PackageWithTexts[];
  stats: { rust: number; page: number; texts: number };
};
export function parseArguments(
  argv: string[],
  options?: { env?: Record<string, string | undefined>; output?: string },
): { verify: boolean; output: string; offline: boolean };
export function sha256(content: string): string;
export function writeAtomically(path: string, content: string): void;
export function runCli(
  argv: string[],
  context?: { run?: (args: string[]) => string; appDir?: string; env?: Record<string, string | undefined>; log?: (line: string) => void },
): number;
