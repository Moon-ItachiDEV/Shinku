// Le choix de l'exemplaire gardé dans un groupe de doublons, sans rien
// afficher. Le cœur fait foi : `keepIn` répète sa règle pour que l'écran suive
// sans tout recharger, et pour que la démo se comporte comme lui.

import type { DuplicateGroup, KeepChoice } from '../backend/types.ts';
import { splitPath } from '../format.ts';

/** Le dossier qui range un exemplaire. */
export const folderOf = (path: string): string => splitPath(path).parent;

const sameGroup = (group: DuplicateGroup, choice: KeepChoice): boolean =>
  group.digest === choice.digest && group.size === choice.size;

function keepOnly(group: DuplicateGroup, kept: number): DuplicateGroup {
  const files = group.files.map((file, rank) => ({ ...file, keep: rank === kept }));
  const reclaimable = files.reduce((total, file) => total + (file.keep ? 0 : file.onDisk), 0);
  return { ...group, files, reclaimable };
}

/**
 * Les mêmes groupes, où l'exemplaire choisi est gardé. Avec `wholeFolder`, le
 * choix vaut pour tous les groupes qui ont un exemplaire dans le même dossier :
 * un groupe qui y garde déjà un exemplaire ne bouge pas, les autres y gardent
 * leur plus ancien. Chaque groupe garde toujours un exemplaire et un seul.
 */
export function keepIn(groups: readonly DuplicateGroup[], choice: KeepChoice): DuplicateGroup[] {
  const chosen = groups.find((group) => sameGroup(group, choice))?.files[choice.index];
  const kept = chosen === undefined ? undefined : folderOf(chosen.path);
  return groups.map((group) => {
    if (sameGroup(group, choice)) {
      return choice.index < group.files.length ? keepOnly(group, choice.index) : group;
    }
    if (!choice.wholeFolder || kept === undefined) {
      return group;
    }
    // Un exemplaire déjà gardé dans ce dossier a pu être choisi à la main.
    if (group.files.some((file) => file.keep && folderOf(file.path) === kept)) {
      return group;
    }
    const rank = group.files.findIndex((file) => folderOf(file.path) === kept);
    return rank < 0 ? group : keepOnly(group, rank);
  });
}

/** Un dossier qui range un exemplaire de plusieurs groupes. */
export interface SharedFolder {
  folder: string;
  /** Combien de groupes y ont un exemplaire. */
  groups: number;
  /** Combien de ces groupes gardent déjà cet exemplaire-là. */
  kept: number;
  /** Le choix qui garde ce dossier partout. */
  choice: KeepChoice;
}

/**
 * Les dossiers où plusieurs groupes ont un exemplaire, les plus fournis
 * d'abord, puis dans l'ordre de la liste : ceux qu'il vaut la peine de garder
 * d'un seul geste.
 */
export function sharedFolders(groups: readonly DuplicateGroup[], limit: number): SharedFolder[] {
  const found = new Map<string, SharedFolder>();
  for (const group of groups) {
    const keptIn = group.files.find((file) => file.keep);
    const keptFolder = keptIn === undefined ? undefined : folderOf(keptIn.path);
    const seen = new Set<string>();
    group.files.forEach((file, index) => {
      const folder = folderOf(file.path);
      if (seen.has(folder)) {
        return;
      }
      seen.add(folder);
      // Le groupe garde ce dossier dès que son exemplaire gardé s'y trouve.
      const kept = folder === keptFolder ? 1 : 0;
      const known = found.get(folder);
      if (known === undefined) {
        found.set(folder, {
          folder,
          groups: 1,
          kept,
          choice: { digest: group.digest, size: group.size, index, wholeFolder: true },
        });
      } else {
        known.groups += 1;
        known.kept += kept;
      }
    });
  }
  return [...found.values()]
    .filter((entry) => entry.groups > 1)
    .sort((a, b) => b.groups - a.groups)
    .slice(0, limit);
}
