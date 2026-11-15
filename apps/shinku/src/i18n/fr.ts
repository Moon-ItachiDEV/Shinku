// Tous les textes de l'interface, en français.
//
// Une seule langue existe pour l'instant. Les écrans ne contiennent aucun
// texte en dur : ajouter l'anglais reviendra à écrire un second fichier de la
// même forme.

import {
  NBSP,
  countOf,
  formatBytes,
  formatClock,
  formatCount,
  formatLimit,
  frenchSpacing,
  plural,
} from '../format.ts';

/** Les mêmes textes, une fois les espaces insécables posées. */
type Typeset<T> = T extends string
  ? string
  : T extends (...args: infer Arguments) => string
    ? (...args: Arguments) => string
    : { readonly [Key in keyof T]: Typeset<T[Key]> };

/**
 * Pose les espaces insécables de la typographie française sur tous les
 * textes, y compris ceux qui sont calculés. Les textes ci-dessous s'écrivent
 * donc avec des espaces ordinaires.
 */
function typeset<T>(messages: T): Typeset<T> {
  if (typeof messages === 'string') {
    return frenchSpacing(messages) as Typeset<T>;
  }
  if (typeof messages === 'function') {
    const write = messages as unknown as (...args: unknown[]) => string;
    return ((...args: unknown[]) => frenchSpacing(write(...args))) as Typeset<T>;
  }
  const group = messages as Record<string, unknown>;
  return Object.fromEntries(Object.entries(group).map(([key, value]) => [key, typeset(value)])) as Typeset<T>;
}

export const fr = typeset({
  window: {
    minimize: 'Réduire',
    maximize: 'Agrandir',
    close: 'Fermer',
  },

  nav: {
    label: 'Modules de Shinku',
    home: 'Grand vide',
    cleaning: 'Nettoyage',
    performance: 'Performance',
    apps: 'Applications',
    protection: 'Protection',
    junk: 'Fichiers inutiles',
    duplicates: 'Doublons',
    large: 'Gros fichiers',
    bin: 'Corbeille',
    startup: 'Démarrage',
    gameMode: 'Mode jeu',
    uninstall: 'Désinstallation',
    privacy: 'Confidentialité',
    settings: 'Paramètres',
    binOver: ', au-delà du seuil',
  },

  disk: {
    title: (label: string, drive: string) => `${label === '' ? 'Disque local' : label} (${drive})`,
    used: (value: number) => `${value}${NBSP}%`,
    free: (free: number, total: number) => `${formatBytes(free)} libres sur ${formatBytes(total)}`,
    unknown: 'Espace disque inconnu',
  },

  common: {
    back: 'Revenir',
    analyse: 'Analyser',
    stop: 'Arrêter',
    soon: 'Bientôt',
    scanning: 'Analyse en cours',
    nothingWithoutConsent: 'Rien n’est supprimé sans ton accord',
    showMore: 'Afficher la suite',
    loading: 'Chargement…',
    kept: 'Conservé',
    synced: 'Synchronisé',
    understood: 'Compris',
    openSettings: 'Ouvrir les Paramètres',
  },

  boot: {
    preparing: 'Préparation de Shinku',
    unavailable: 'Shinku n’a pas pu démarrer',
    unavailableHint: 'Ferme la fenêtre puis relance Shinku.',
    outsideApp: 'Cette page est l’interface de Shinku : elle s’ouvre depuis l’application.',
  },

  home: {
    title: 'Grand vide',
    intro: 'Fichiers inutiles, doublons, corbeille et gros fichiers : tout est analysé en une fois.',
    binNotice: (bytes: number, thresholdGb: number) =>
      `La corbeille contient ${formatBytes(bytes)}, au-delà de ton seuil de ${thresholdGb}${NBSP}Go.`,
    seeBin: 'Voir la corbeille',
    follow: 'Suivre l’analyse',
    reviewResults: (finished: number) => `Revoir les résultats de ${formatClock(finished)}`,
    failed: 'La dernière analyse s’est arrêtée sur une erreur. Tu peux la relancer.',
    checkFirst: 'Vérifie d’abord tes réglages',
    issueUnreadable:
      'Tes réglages n’ont pas pu être relus. Shinku repart des valeurs par défaut, et ta liste de dossiers jamais touchés est vide. Vérifie-la avant d’analyser.',
    issueFolders:
      'Certains de tes dossiers jamais touchés n’ont pas pu être relus. Vérifie la liste avant d’analyser.',
    issueMinor: 'Certains réglages étaient illisibles : ils sont revenus à leur valeur par défaut.',
    continueAnyway: 'Continuer',
  },

  scan: {
    running: 'Analyse en cours',
    finished: 'Analyse terminée',
    toFree: 'à libérer',
    waiting: 'En attente',
    working: 'Analyse…',
    comparing: (done: number) => `Comparaison… ${done}${NBSP}%`,
    notMeasured: 'Non mesurée',
    notSearched: 'Non recherchés',
    none: 'Rien',
    optional: (bytes: number) => `${formatBytes(bytes)} au choix`,
    toReview: (bytes: number) => `${formatBytes(bytes)} à vérifier`,
    walked: (folders: number, files: number) =>
      `${countOf(folders, 'dossier', 'dossiers')}, ${countOf(files, 'fichier', 'fichiers')}`,
    seeResults: 'Voir les résultats',
    seeEarlyResults: 'Voir les résultats déjà prêts',
  },

  results: {
    canFree: 'Le grand vide peut libérer',
    junkHint: 'Caches, fichiers temporaires et journaux',
    duplicatesHint: 'Gardés dans la corbeille, récupérables',
    duplicatesChoice: 'Copies de tes fichiers : à toi de les cocher',
    duplicatesPending: (done: number) => `Recherche en cours… ${done}${NBSP}%`,
    duplicatesNone: 'Aucun doublon trouvé',
    duplicatesOff: 'Non recherchés dans cette analyse',
    binItems: (items: number) => countOf(items, 'élément', 'éléments'),
    binItemsOver: (items: number) => `${countOf(items, 'élément', 'éléments')}, au-delà de ton seuil`,
    binEmpty: 'Vide',
    binUnknown: 'Non mesurée',
    review: 'À vérifier toi-même',
    largeHint: (count: number, threshold: number) =>
      count === 0
        ? `Aucun fichier de plus de ${formatLimit(threshold)}`
        : `${countOf(count, 'fichier', 'fichiers')} de plus de ${formatLimit(threshold)}`,
    clean: 'Faire le vide',
    cleanSoon: 'Le nettoyage arrive dans une prochaine version : rien n’est supprimé.',
    stale: (finished: number) => `Résultats de ${formatClock(finished)}. Relance une analyse pour des chiffres à jour.`,
    rescan: 'Réanalyser',
    skipped: (folders: number) =>
      `${countOf(folders, 'dossier n’a pas pu être lu', 'dossiers n’ont pas pu être lus')} : ${plural(folders, 'il n’est pas compté', 'ils ne sont pas comptés')}.`,
    refused: (roots: number) =>
      `${countOf(roots, 'emplacement a été écarté', 'emplacements ont été écartés')} par prudence.`,
  },

  junk: {
    intro: 'Caches, fichiers temporaires, journaux et restes de mises à jour : ce que Windows et tes apps laissent traîner.',
    scanTitle: 'Analyse des dossiers',
    scanText: 'Shinku parcourt les caches, les dossiers temporaires et les journaux.',
    found: (categories: number) =>
      categories === 0
        ? 'Rien à nettoyer'
        : `${countOf(categories, 'catégorie trouvée', 'catégories trouvées')}`,
    hint: 'Ces fichiers se recréent tout seuls si une app en a besoin.',
    files: (files: number) => countOf(files, 'fichier', 'fichiers'),
    spared: (files: number, bytes: number) =>
      `${countOf(files, 'fichier trop récent est laissé', 'fichiers trop récents sont laissés')} de côté (${formatBytes(bytes)}).`,
    remove: 'Supprimer',
    soon: (bytes: number) => `${formatBytes(bytes)} trouvés. Le nettoyage arrive dans une prochaine version.`,
    soonEmpty: 'Le nettoyage arrive dans une prochaine version.',
  },

  duplicates: {
    intro: 'Photos, musiques et documents copiés plusieurs fois. Shinku garde toujours l’exemplaire le plus ancien.',
    scanTitle: 'Comparaison des fichiers',
    scanText: 'Les fichiers de même taille sont comparés par leur contenu.',
    found: (groups: number) =>
      groups === 0 ? 'Aucun doublon' : `${countOf(groups, 'groupe de doublons', 'groupes de doublons')}`,
    hint: 'Dans chaque groupe, l’exemplaire le plus ancien est conservé.',
    hintNone: 'Tes bibliothèques ne contiennent pas de fichier en double.',
    pendingTitle: 'Recherche des doublons',
    pendingText: (done: number) => `Les fichiers qui se ressemblent sont en cours de lecture : ${done}${NBSP}%.`,
    offText: 'Les doublons n’ont pas été recherchés dans cette analyse.',
    copies: (count: number, size: number) =>
      `${countOf(count, 'exemplaire', 'exemplaires')} de ${formatBytes(size)}`,
    skipped: (files: number) =>
      `${countOf(files, 'fichier n’a pas pu être comparé', 'fichiers n’ont pas pu être comparés')} (verrouillé ou modifié pendant l’analyse).`,
    remove: 'Supprimer',
    soon: (bytes: number) => `${formatBytes(bytes)} de copies. Le nettoyage arrive dans une prochaine version.`,
    soonEmpty: 'Le nettoyage arrive dans une prochaine version.',
  },

  large: {
    intro: 'Les fichiers les plus lourds de ton disque, avec la date de leur dernière modification.',
    scanTitle: 'Mesure de ton disque',
    scanText: 'Shinku cherche les fichiers les plus lourds de ton disque.',
    found: (count: number, threshold: number) =>
      count === 0
        ? `Aucun fichier de plus de ${formatLimit(threshold)}`
        : `${countOf(count, 'fichier', 'fichiers')} de plus de ${formatLimit(threshold)}`,
    hint: 'Vérifie chaque fichier avant de le supprimer : Shinku ne coche jamais un gros fichier à ta place.',
    hintNone: 'Rien de très lourd sur ton disque.',
    modified: (ago: string) => `modifié ${ago}`,
    truncated: (listed: number) => `Seuls les ${formatCount(listed)} plus gros sont listés.`,
    remove: 'Supprimer',
    soon: (bytes: number) => `${formatBytes(bytes)} au total. La suppression arrive dans une prochaine version.`,
    soonEmpty: 'La suppression arrive dans une prochaine version.',
  },

  bin: {
    title: 'Corbeille',
    titleOver: 'Ta corbeille déborde',
    content: (bytes: number, items: number) =>
      `${formatBytes(bytes)} dans ${countOf(items, 'élément', 'éléments')}.`,
    contentOver: (bytes: number, items: number, thresholdGb: number) =>
      `${formatBytes(bytes)} dans ${countOf(items, 'élément', 'éléments')}, au-delà de ton seuil de ${thresholdGb}${NBSP}Go.`,
    empty: 'Ta corbeille est vide.',
    unknown: 'La corbeille n’a pas pu être mesurée.',
    emptyIt: 'Vider la corbeille',
    soon: 'Le vidage arrive dans une prochaine version.',
  },

  soon: {
    cap: 'Ce module arrive dans une prochaine version.',
    startup: 'Les apps qui se lancent avec Windows, et le temps qu’elles te coûtent à chaque allumage.',
    gameMode:
      'Met en pause ce qui tourne en arrière-plan pendant que tu joues. Tout reprend quand tu quittes le jeu.',
    uninstall:
      'Supprime une app avec ce qu’elle laisse derrière elle : dossiers, raccourcis et entrées du registre.',
    privacy:
      'Historique, cookies de suivi, fichiers récents et presse-papiers : les traces de ton activité sur ce PC.',
  },

  settings: {
    title: 'Paramètres',
    intro: 'Les changements s’appliquent tout de suite.',
    appearance: 'Apparence',
    themeLabel: 'Mode d’affichage',
    themeHint: 'S’applique tout de suite à toute l’interface.',
    light: 'Clair',
    dark: 'Sombre',
    system: 'Système',
    security: 'Sécurité',
    foldersLabel: 'Dossiers jamais touchés',
    foldersHint: 'Shinku n’analyse pas ces dossiers.',
    foldersEmpty: 'Aucun dossier pour l’instant.',
    foldersNote: 'Changer cette liste efface les résultats en cours : relance ensuite une analyse.',
    addFolder: 'Ajouter un dossier',
    removeFolder: (path: string) => `Retirer ${path}`,
    later: 'Les autres réglages arriveront avec leurs modules : lancement avec Windows, analyse automatique, corbeille et mode jeu.',
    version: (version: string) => `Shinku version ${version}`,
  },

  toast: {
    foldersChanged: 'Liste mise à jour. Les résultats précédents ont été effacés.',
    settingsNotSaved: 'Les réglages n’ont pas pu être enregistrés. Rien n’a changé.',
    scanNotStarted: 'L’analyse n’a pas pu démarrer.',
    scanFailed: 'L’analyse s’est arrêtée sur une erreur.',
    settingsUnchecked: 'Vérifie d’abord tes réglages.',
    notAFolder: 'Ce chemin ne désigne pas un dossier lisible.',
    notAbsolute: 'Ce chemin n’est pas valide.',
    notUnicode: 'Le nom de ce dossier contient des caractères que Shinku ne sait pas enregistrer.',
    unexpected: 'Une erreur inattendue s’est produite.',
  },
});

export type Messages = typeof fr;
