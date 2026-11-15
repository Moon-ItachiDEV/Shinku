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

/** « 9 utilisés par une app » : un nombre et ce qui lui est arrivé, accordé. */
const some = (count: number, one: string, many: string): string =>
  `${formatCount(count)}${NBSP}${plural(count, one, many)}`;

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
    cleanFailed: 'Le dernier grand vide s’est arrêté sur une erreur. Relance une analyse pour voir ce qui reste.',
    interrupted: (ago: string, freed: number, recycled: number) => {
      // Le journal est tenu de loin en loin : ses chiffres sont un minimum.
      const gone =
        freed > 0 && recycled > 0
          ? ` Au moins ${formatBytes(freed)} étaient déjà libérés, et ${formatBytes(recycled)} partis à la corbeille.`
          : freed > 0
            ? ` Au moins ${formatBytes(freed)} étaient déjà libérés.`
            : recycled > 0
              ? ` Au moins ${formatBytes(recycled)} étaient déjà partis à la corbeille.`
              : '';
      return `Le grand vide lancé ${ago} a été coupé avant la fin : Shinku a été fermé, ou le PC éteint.${gone} Relance une analyse pour voir ce qui reste.`;
    },
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
    duplicatesHint: 'Les copies vont à la corbeille, un exemplaire reste',
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
    cleanConfirm: 'Rien ne part avant ta confirmation',
    cleanPending: 'Disponible dès la fin de l’analyse',
    cleanUnchecked: 'Coche au moins une ligne pour faire le vide',
    cleanEmpty: 'Rien à nettoyer pour l’instant',
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
    viaClean: (bytes: number) => `${formatBytes(bytes)} trouvés. Pour les supprimer, passe par le grand vide.`,
    nothing: 'Rien à supprimer.',
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
    viaClean: (bytes: number) =>
      `${formatBytes(bytes)} de copies. Coche Doublons dans le grand vide pour les retirer.`,
    nothing: 'Aucune copie à retirer.',
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
    viaClean: 'Pour l’instant, la corbeille se vide avec le grand vide.',
    nothing: 'Rien à vider.',
  },

  confirm: {
    title: 'Faire le vide ?',
    intro: 'Voici ce qui part si tu confirmes, dans cet ordre.',
    bin: (items: number) => `${countOf(items, 'élément supprimé', 'éléments supprimés')} définitivement`,
    junk: (files: number) => `${countOf(files, 'fichier supprimé', 'fichiers supprimés')} sans passer par la corbeille`,
    duplicates: (copies: number) =>
      `${countOf(copies, 'copie envoyée', 'copies envoyées')} à la corbeille, un exemplaire reste`,
    openApps: (names: string, count: number, bytes: number) =>
      `${names} ${plural(count, 'est ouvert', 'sont ouverts')} : ${plural(count, 'ses', 'leurs')} fichiers (${formatBytes(bytes)}) resteront en place. Ferme-${plural(count, 'le', 'les')} d’abord pour tout nettoyer.`,
    syncedCopies: (copies: number) =>
      `${countOf(copies, 'copie est rangée', 'copies sont rangées')} dans un dossier synchronisé : ${plural(copies, 'elle partira', 'elles partiront')} aussi de ton espace en ligne.`,
    otherDiskCopies: (copies: number) =>
      `${countOf(copies, 'copie est rangée', 'copies sont rangées')} sur un autre disque que l’exemplaire gardé : vérifie que ce n’est pas une sauvegarde.`,
    cancel: 'Annuler',
    confirm: 'Faire le vide',
  },

  clean: {
    title: 'Grand vide en cours',
    preparing: 'Préparation…',
    recycleBin: 'Vidage de la corbeille…',
    junkFiles: 'Suppression des fichiers inutiles…',
    duplicates: 'Envoi des doublons à la corbeille…',
    finishing: 'Dernières vérifications…',
    at: (done: number) => `${done}${NBSP}%`,
    soFar: (freed: number, recycled: number) =>
      freed > 0 && recycled > 0
        ? `${formatBytes(freed)} libérés, ${formatBytes(recycled)} à la corbeille`
        : freed > 0
          ? `${formatBytes(freed)} libérés`
          : recycled > 0
            ? `${formatBytes(recycled)} à la corbeille`
            : '',
    stopping: 'Arrêt demandé : Shinku termine ce qui est en cours.',
  },

  done: {
    finished: 'Grand vide terminé',
    stopped: 'Grand vide arrêté',
    freed: 'libérés',
    recycled: 'à la corbeille',
    recoverable: (bytes: number) => `+ ${formatBytes(bytes)} récupérables dans la corbeille`,
    recycledHint: 'Récupérables tant que tu ne vides pas la corbeille',
    nothing: 'Rien n’a été supprimé',
    duplicatesPart: 'Doublons, récupérables',
    diskNow: (drive: string, free: number) => `Ton disque ${drive} a maintenant ${formatBytes(free)} d’espace libre.`,
    cancelled: 'Tu as arrêté le grand vide en route. Ce qui est déjà parti reste parti.',
    binFailed: 'La corbeille n’a pas pu être vidée en entier.',
    junkFailed:
      'Les fichiers inutiles sont restés en place : un de tes dossiers jamais touchés n’a pas pu être appliqué, et dans le doute Shinku ne supprime rien.',
    duplicatesFailed:
      'Les doublons sont restés en place : un de tes dossiers jamais touchés n’a pas pu être appliqué, et dans le doute Shinku n’y touche pas.',
    stopNotInBin: (name: string) =>
      `Shinku s’est arrêté par sécurité : après avoir envoyé «${NBSP}${name}${NBSP}» à la corbeille, il ne l’y a pas retrouvé. Les copies suivantes n’ont pas été touchées.`,
    stopKeptChanged: (name: string) =>
      `Shinku s’est arrêté par sécurité : l’exemplaire gardé de «${NBSP}${name}${NBSP}» a changé pendant le grand vide. Sa copie est dans la corbeille, d’où tu peux la restaurer. Les copies suivantes n’ont pas été touchées.`,
    leftOpen: (names: string, count: number, bytes: number) =>
      `${names} ${plural(count, 'était ouvert', 'étaient ouverts')} : ${plural(count, 'ses', 'leurs')} fichiers sont restés (${formatBytes(bytes)}).`,
    skipped: (files: number, detail: string) =>
      `${countOf(files, 'fichier est resté', 'fichiers sont restés')} en place : ${detail}.`,
    skippedInUse: (files: number) => `${some(files, 'utilisé', 'utilisés')} par une app`,
    skippedChanged: (files: number) => `${some(files, 'modifié', 'modifiés')} depuis l’analyse`,
    skippedDenied: (files: number) => `${some(files, 'refusé', 'refusés')} par Windows`,
    skippedUnrecyclable: (files: number) => `${formatCount(files)} que la corbeille ne peut pas recevoir`,
    skippedOther: (files: number) => `${formatCount(files)} pour une autre raison`,
    binWasFull:
      'Ta corbeille était pleine : elle a déjà abandonné une partie de son contenu pour faire de la place.',
    gainedLess: (counted: number) =>
      `Les fichiers supprimés occupaient ${formatBytes(counted)}, mais ton disque a gagné un peu moins : d’autres programmes écrivaient en même temps, ou certains fichiers existaient sous deux noms.`,
    close: 'Terminé',
    rescan: 'Analyser à nouveau',
  },

  soon: {
    cap: 'Ce module arrive dans une prochaine version.',
    startup: 'Les apps qui se lancent avec Windows, et le temps qu’elles te coûtent à chaque allumage.',
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
    foldersHint: 'Shinku n’analyse pas ces dossiers, et n’y supprime rien.',
    foldersEmpty: 'Aucun dossier pour l’instant.',
    foldersNote: 'Changer cette liste efface les résultats en cours : relance ensuite une analyse.',
    addFolder: 'Ajouter un dossier',
    removeFolder: (path: string) => `Retirer ${path}`,
    later: 'Les autres réglages arriveront avec leurs modules : lancement avec Windows, analyse automatique et corbeille.',
    version: (version: string) => `Shinku version ${version}`,
  },

  game: {
    eyebrow: 'Mode jeu',
    titleOff: 'Le mode jeu est éteint',
    titleIdle: 'Shinku veille sur tes parties',
    titlePlaying: (name: string) => `En jeu : ${name}`,
    titlePending: 'Remise en place en cours',
    textOff:
      'Allume-le une fois. Ensuite, lance ton jeu comme d’habitude : Shinku le reconnaît tout seul et ne lance jamais rien à ta place.',
    textIdle:
      'Lance ton jeu comme d’habitude. Shinku applique tes réglages, puis remet tout en place quand tu le quittes.',
    // `since` : « 14:32 », « hier, 23:40 » ou « le 14 nov., 23:40 » (voir formatSince).
    textPlaying: (since: string) => `Mode jeu actif depuis ${since}. Tout revient en place quand tu quittes le jeu.`,
    textReleased: 'Remis en place pour cette partie. Le mode jeu reprendra à la prochaine.',
    textPending: 'Shinku n’a pas encore pu tout remettre en place. Il réessaie tout seul.',
    release: 'Tout remettre maintenant',
    notAGame: 'Ce n’est pas un jeu',
    notAGameNamed: (name: string) => `Ce n’est pas un jeu : ${name}`,
    lastOk: (name: string, duration: string) =>
      `Dernière partie : ${name}, ${duration}. Tout a été remis en place.`,
    lastIncomplete: (name: string, duration: string) =>
      `Dernière partie : ${name}, ${duration}. Tout n’a pas pu être remis en place : vérifie le mode d’alimentation de Windows.`,
    powerApplied: 'Alimentation en performances élevées',
    powerAlreadyFast: 'Alimentation déjà en performances élevées',
    powerOnBattery: 'Alimentation inchangée : tu es sur batterie',
    powerCustom: 'Alimentation inchangée : ton PC a son propre mode',
    powerRefused: 'Windows a refusé de changer l’alimentation',
    quietCount: (count: number) => countOf(count, 'app en veilleuse', 'apps en veilleuse'),
    auto: 'Mode jeu automatique',
    autoHint:
      'S’active quand un jeu passe au premier plan, sans rien lancer. Shinku démarre alors avec Windows, en arrière-plan et sans fenêtre.',
    agentMissing: 'Cette installation de Shinku n’a pas son programme de veille. Réinstalle Shinku pour l’avoir.',
    agentStopped: 'La veille de Shinku ne démarre pas. Ferme Shinku, puis rouvre-le.',
    startupDisabled:
      'Shinku est désactivé dans les apps de démarrage de Windows : le mode jeu ne veillera qu’après avoir ouvert Shinku.',
    during: 'Pendant que tu joues',
    power: 'Alimentation en performances élevées',
    powerHint:
      `Windows passe sur son mode «${NBSP}Performances élevées${NBSP}» le temps de la partie, puis reprend ton mode habituel. Jamais sur batterie. Dans ce mode, le PC ne se met pas en veille tout seul.`,
    powerUnavailable: `Ce PC ne propose pas le mode «${NBSP}Performances élevées${NBSP}» : son alimentation se règle dans Windows.`,
    powerUnsuited:
      'Sur ce processeur, AMD recommande de garder le mode normal de Windows : Shinku n’y touche pas.',
    powerOpen: 'Réglages d’alimentation de Windows',
    onBattery: 'Sur batterie',
    quiet: 'Apps d’arrière-plan en veilleuse',
    quietHint: 'Elles restent ouvertes, mais passent après ton jeu. Celle que tu regardes retrouve sa vitesse.',
    open: 'Ouverte',
    addedApp: 'Ajoutée par toi',
    quietAlso: (list: string) => `Aussi mises en veilleuse si tu les ouvres : ${list}.`,
    quietAdvice: 'Évite d’y mettre une app qui joue du son ou de la vidéo pendant ta partie.',
    quietAdd: 'Ajouter une app ouverte',
    games: 'Tes jeux',
    gamesHint: 'Les jeux installés avec Steam, Epic, Xbox, GOG, Riot, Ubisoft ou EA sont reconnus tout seuls.',
    gamesEmpty: 'Aucun jeu pour l’instant. Ils apparaîtront ici après ta première partie.',
    added: 'Ajouté par toi',
    recognised: 'Reconnu tout seul',
    lastPlayed: (ago: string) => `joué ${ago}`,
    remove: (name: string) => `Retirer ${name}`,
    addGame: 'Ajouter un jeu ouvert',
    addGameHint: 'Un jeu n’est pas reconnu (Battle.net, Minecraft, Roblox…) ? Ouvre-le, puis ajoute-le ici.',
    never: 'Jamais traités comme des jeux',
    restore: 'Rétablir',
    restoreNamed: (name: string) => `Rétablir ${name}`,
    pickGame: 'Choisis ton jeu',
    pickApp: 'Choisis une app',
    pickHint: 'Seules les apps ouvertes en ce moment sont listées.',
    pickEmpty: 'Aucune app ouverte à ajouter. Ouvre-la, puis reviens ici.',
    cancel: 'Annuler',
    windows: 'Windows s’en occupe déjà',
    winMode: 'Mode Jeu de Windows',
    winModeHint:
      'Il donne la priorité au jeu et retient les mises à jour pendant la partie. Shinku ne le refait pas.',
    winOpen: 'Réglages de Windows',
    winNotif: 'Notifications',
    winNotifHint: 'Windows 11 les coupe tout seul pendant un jeu en plein écran.',
    promise:
      'Shinku ne promet pas d’images par seconde en plus : il écarte ce qui gêne ton jeu, et remet tout en place après.',
    toast: {
      unknownApp: 'Cette app n’est plus dans la liste.',
      unknownGame: 'Ce jeu n’est plus dans la liste.',
      notAGame: 'Shinku ne traite jamais cette app comme un jeu.',
      protectedApp: 'Cette app ne peut pas être mise en veilleuse.',
      listFull: 'La liste est pleine.',
    },
  },

  toast: {
    foldersChanged: 'Liste mise à jour. Les résultats précédents ont été effacés.',
    settingsNotSaved: 'Les réglages n’ont pas pu être enregistrés. Rien n’a changé.',
    scanNotStarted: 'L’analyse n’a pas pu démarrer.',
    scanFailed: 'L’analyse s’est arrêtée sur une erreur.',
    cleanFailed: 'Le grand vide s’est arrêté sur une erreur.',
    cleanRunning: 'Un grand vide est en cours : attends qu’il se termine.',
    cleanNotStarted: 'Le grand vide n’a pas pu démarrer. Rien n’a été supprimé.',
    noResults: 'L’analyse n’est pas terminée : le grand vide attend ses résultats.',
    staleResults: 'Ces résultats ont plus de dix minutes : relance une analyse avant de faire le vide.',
    otherAnalysis: 'Les résultats ont changé entre-temps : vérifie-les avant de faire le vide.',
    nothingToClean: 'Il n’y a plus rien à nettoyer dans ce que tu as coché.',
    settingsUnchecked: 'Vérifie d’abord tes réglages.',
    notAFolder: 'Ce chemin ne désigne pas un dossier lisible.',
    notAbsolute: 'Ce chemin n’est pas valide.',
    notUnicode: 'Le nom de ce dossier contient des caractères que Shinku ne sait pas enregistrer.',
    unexpected: 'Une erreur inattendue s’est produite.',
  },
});

export type Messages = typeof fr;
