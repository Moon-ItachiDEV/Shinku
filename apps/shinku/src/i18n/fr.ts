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

const EXACT = Symbol('exact');

/** Une part de texte que la typographie ne touche pas : un nom que le cœur a déjà mis en forme (`registered`). */
class Verbatim {
  readonly text: string;
  constructor(text: string) {
    this.text = text;
  }
}
const verbatim = (text: string): Verbatim => new Verbatim(text);

/**
 * Un texte calculé dont certaines parts sont `verbatim` : la typographie française s'applique aux autres, pas à
 * celles-là (« Alerte ! » reste avec son espace ordinaire, comme dans la boîte de l'assistant).
 */
function exact<Arguments extends unknown[]>(
  write: (...args: Arguments) => Array<string | Verbatim>,
): (...args: Arguments) => string {
  const compose = (...args: Arguments): string =>
    write(...args)
      .map((part) => (part instanceof Verbatim ? part.text : frenchSpacing(part)))
      .join('');
  return Object.assign(compose, { [EXACT]: true });
}

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
  if (typeof messages === 'function' && EXACT in messages) {
    // Déjà composé par `exact` : ses parts verbatim ne sont jamais retouchées.
    return messages as Typeset<T>;
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
    free: (free: number) => formatBytes(free),
    freeRest: (total: number) => `libres sur ${formatBytes(total)}`,
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
    startupNotMeasured: 'Non mesuré',
    startupCount: (count: number) => (count === 0 ? 'Aucune app active' : countOf(count, 'app active', 'apps actives')),
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
    startupHint: (count: number) =>
      count === 0
        ? 'Aucune app active au démarrage'
        : `${countOf(count, 'app active', 'apps actives')} au démarrage de Windows`,
    startupNotMeasured: 'Non mesuré',
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
    intro:
      'Photos, musiques et documents copiés plusieurs fois. Tu choisis l’exemplaire à garder ; sans choix, c’est le plus ancien.',
    scanTitle: 'Comparaison des fichiers',
    scanText: 'Les fichiers de même taille sont comparés par leur contenu.',
    found: (groups: number) =>
      groups === 0 ? 'Aucun doublon' : `${countOf(groups, 'groupe de doublons', 'groupes de doublons')}`,
    hint: 'Dans chaque groupe, un seul exemplaire reste : le plus ancien, ou celui que tu choisis en cliquant sur sa ligne. Une nouvelle analyse repart du plus ancien.',
    hintFixed: 'Dans chaque groupe, un seul exemplaire reste.',
    folders: 'Garder tout un dossier',
    folderGroups: (groups: number) => `Présent dans ${countOf(groups, 'groupe', 'groupes')}`,
    foldersPartial: 'Garder un dossier vaut aussi pour les groupes pas encore affichés.',
    keepFolder: 'Garder ce dossier',
    folderKept: 'Dossier gardé',
    keepThis: 'Garder celui-ci',
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
    uninstall:
      'Retire une app avec le désinstalleur de son éditeur, puis regarde ce qu’elle a laissé dans ton compte.',
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
    textLeaving: 'Shinku remet tout en place…',
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
    quietHint: 'Elles restent ouvertes, mais passent après ton jeu. Celle que tu utilises retrouve sa vitesse.',
    open: 'Ouverte',
    addedApp: 'Ajoutée par toi',
    quietAlso: (list: string) => `Aussi mises en veilleuse si tu les ouvres : ${list}.`,
    quietAdvice: 'Évite d’y mettre une app qui joue du son ou de la vidéo pendant ta partie.',
    quietAdd: 'Ajouter une app ouverte',
    games: 'Tes jeux',
    gamesHint: 'Les jeux rangés dans le dossier habituel de Steam, Epic, Xbox, GOG, Riot, Ubisoft ou EA sont reconnus tout seuls.',
    gamesEmpty: 'Aucun jeu pour l’instant. Ils apparaîtront ici après ta première partie.',
    added: 'Ajouté par toi',
    recognised: 'Reconnu tout seul',
    lastPlayed: (ago: string) => `joué ${ago}`,
    remove: (name: string) => `Retirer ${name}`,
    addGame: 'Ajouter un jeu ouvert',
    addGameHint: 'Un jeu n’est pas reconnu (Battle.net, Minecraft…) ? Ouvre-le, puis ajoute-le ici.',
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
      'Shinku ne promet pas d’images par seconde en plus : il fait passer les apps que tu as choisies après ton jeu, puis remet tout en place.',
    toast: {
      unknownApp: 'Cette app n’est plus dans la liste.',
      unknownGame: 'Ce jeu n’est plus dans la liste.',
      notAGame: 'Shinku ne traite jamais cette app comme un jeu.',
      protectedApp: 'Cette app ne peut pas être mise en veilleuse.',
      listFull: 'La liste est pleine.',
    },
  },

  startup: {
    eyebrow: 'Démarrage',
    // Les apps de la liste, coupées comprises. Aucune : la page le dit, sans compter.
    title: (count: number) =>
      count === 0 ? 'Aucune app inscrite ici' : `${countOf(count, 'app inscrite', 'apps inscrites')} au démarrage`,
    // `running` : lues actives ; `cut` : lues coupées. Les entrées que Shinku ne sait pas lire ne sont dans aucun des deux.
    summary: (running: number, cut: number) =>
      `${
        running === 0
          ? 'Aucune ne se lance'
          : `${formatCount(running)}${NBSP}${plural(running, 'se lance', 'se lancent')}`
      } à l’ouverture de ta session, ${
        cut === 0 ? 'aucune n’est coupée' : `${formatCount(cut)}${NBSP}${plural(cut, 'est coupée', 'sont coupées')}`
      }.`,
    scope:
      'Shinku liste ce que règle le Gestionnaire des tâches, sauf les apps du Microsoft Store. Les tâches planifiées et les services ne sont pas ici.',
    openWindows: 'Apps de démarrage de Windows',
    user: 'Pour ton compte',
    machine: 'Pour tous les comptes de ce PC',
    machineHint: 'Ces apps se règlent dans Windows pour l’instant : il faut l’accord d’un administrateur.',
    // Sous le titre de la carte du PC, selon ce que Windows laisse faire (`unavailable` : `machineHint` plus haut).
    pc: {
      consent: 'Windows te demande ton accord à chaque changement, puis Shinku te redit quelle app il règle.',
      password:
        'Windows demandera le mot de passe d’un administrateur à chaque changement, puis Shinku redira quelle app il règle.',
      direct: 'Shinku te demandera de confirmer chaque changement.',
      // Une phrase de plus, dès qu'il y a un accès.
      caution:
        'Certaines servent au son, au pavé tactile ou aux alertes de sécurité. Si quelque chose te manque après, remets l’app.',
    },
    // Sous le nom lisible d'une ligne du PC ; `registered` est déjà mis en forme par le cœur.
    registered: exact((registered: string) => [`Inscrite sous «${NBSP}`, verbatim(registered), `${NBSP}»`]),
    // Dans la ligne de l'app dont le changement attend la personne.
    waiting: exact((registered: string) => [
      `En attente de ta réponse… Une petite fenêtre «${NBSP}Shinku${NBSP}» te demandera de confirmer «${NBSP}`,
      verbatim(registered),
      `${NBSP}». Si tu ne la vois pas, regarde dans la barre des tâches.`,
    ]),
    // Sous la phrase d'accès : des lignes du PC n'ont pas d'interrupteur.
    someStuck: 'Shinku ne sait pas régler celles qui n’ont pas d’interrupteur : passe par Windows.',
    unreadable: 'Shinku n’a pas pu lire toute cette liste.',
    note: 'Couper une app ne la désinstalle pas et ne supprime rien : elle ne se lance plus toute seule, et tu peux toujours l’ouvrir toi-même. Certaines apps se réinscrivent d’elles-mêmes : règle-le aussi dans leurs options. L’éditeur affiché est celui que le fichier déclare.',
    unknownPublisher: 'Éditeur inconnu',
    hostedBy: (host: string) => `Lancée par ${host}`,
    alias: (alias: string) => `inscrite sous «${NBSP}${alias}${NBSP}»`,
    folder: 'raccourci du dossier Démarrage',
    // `date` : « le 15 nov. » (voir formatDay).
    missing: 'Semble désinstallée',
    cameBack: (date: string) => `S’est remise toute seule depuis ${date}`,
    cutByShinku: (date: string) => `Coupée par Shinku ${date}`,
    cutOn: (date: string) => `Coupée ${date}`,
    cut: 'Coupée',
    active: 'Active',
    sync: 'Synchronise tes fichiers',
    // `rank` : 2 pour la deuxième ligne de même nom et de même alias dans une carte, et ainsi de suite.
    // `registered` : une ligne du PC cite son nom d'inscription tel que le cœur l'a rendu, à la place de l'alias.
    switchLabel: exact((name: string, alias: string | null, rank: number = 1, registered: string | null = null) => [
      `Lancer ${name} à l’ouverture de session`,
      ...(registered !== null ? [' (', verbatim(registered), ')'] : alias === null ? [] : [` (${alias})`]),
      rank > 1 ? ` (${rank})` : '',
    ]),
    own: 'Réglé par le Mode jeu',
    ownCut: 'Coupée dans Windows',
    seeGameMode: 'Voir le Mode jeu',
    unreadableEntry: 'Shinku ne sait pas lire son réglage',
    notAdjustableHere: 'Shinku ne peut pas la régler ici',
    openInWindows: 'Ouvrir dans Windows',
    // Le nom accessible du bouton d'une ligne : il cite l'app, et contient le texte visible.
    openInWindowsFor: (name: string) => `Ouvrir dans Windows, pour ${name}`,
    undo: 'Annuler',
    retry: 'Réessayer',
    // Entre l'éditeur, le programme porteur, le nom d'inscription et le raccourci.
    separator: ' · ',
    // L'initiale d'un nom qui ne commence pas par une lettre.
    initialFallback: '#',
    say: {
      cut: (name: string, sync: boolean) =>
        `${name} est coupée. Si elle est ouverte, elle le reste : elle ne se lancera plus toute seule à ta prochaine ouverture de session.${
          sync ? ' Tes fichiers ne se synchroniseront plus tant que tu ne l’ouvres pas.' : ''
        }`,
      restored: (name: string) => `${name} se lancera de nouveau à l’ouverture de ta session.`,
      // Une app de tous les comptes : « ta session » ne dirait pas tout.
      cutPc: (name: string, sync: boolean) =>
        `${name} est coupée. Si elle est ouverte, elle le reste : elle ne se lancera plus toute seule à la prochaine ouverture de session, pour aucun compte de ce PC.${
          sync ? ' Tes fichiers ne se synchroniseront plus tant que tu ne l’ouvres pas.' : ''
        }`,
      restoredPc: (name: string) => `${name} se lancera de nouveau à l’ouverture de session de chaque compte.`,
      failed: 'Le changement n’a pas pu être fait.',
      unknownEntry: 'Cette entrée n’existe plus.',
      notAdjustable: 'Cette entrée ne se règle pas ici.',
      cancelled: 'Rien n’a changé.',
      needsAdmin: 'Il faut un administrateur pour régler cette app.',
    },
  },

  uninstall: {
    eyebrow: 'Désinstallation',
    // Toutes les apps de la liste, filtrées ou non.
    title: (count: number) => (count === 0 ? 'Aucune app classique installée' : countOf(count, 'app installée', 'apps installées')),
    intro: 'Shinku ouvre le désinstalleur de l’app, puis te montre ce qu’elle a laissé derrière elle.',
    store: 'Les apps du Microsoft Store ne sont pas ici : elles se retirent dans Windows.',
    openWindows: 'Apps installées de Windows',
    searchLabel: 'Chercher une app',
    clearSearch: 'Effacer la recherche',
    sortLabel: 'Trier par',
    sortName: 'Nom',
    sortSize: 'Taille',
    sortDate: 'Date',
    noMatch: (query: string) => `Aucune app ne correspond à «${NBSP}${query}${NBSP}».`,
    incomplete: 'Shinku n’a pas pu lire toute la liste des apps.',
    retry: 'Réessayer',
    note: 'Shinku ne supprime jamais une app lui-même et ne touche pas au registre : c’est le désinstalleur de l’éditeur qui travaille. La taille est celle que l’app déclare.',
    unknownPublisher: 'Éditeur inconnu',
    version: (version: string) => `version ${version}`,
    // `day` : « le 15 nov. » (voir formatDay). Affichée quand la liste est triée par date.
    installedOn: (day: string) => `installée ${day}`,
    separator: ' · ',
    // L'initiale d'un nom qui ne commence pas par une lettre.
    initialFallback: '#',
    runtimeChip: 'Souvent utile à d’autres apps',
    driverChip: 'Ressemble à un pilote',
    uninstall: 'Désinstaller',
    // Le nom accessible du bouton d'une ligne : il cite l'app, et contient le texte visible.
    uninstallLabel: (name: string) => `Désinstaller ${name}`,
    refused: 'Protégée par Windows',
    refusedTitle: 'Windows ne permet pas de la retirer',
    own: 'C’est Shinku',
    needed: 'Shinku en a besoin',
    neededTitle: 'Shinku en a besoin pour s’afficher',
    confirm: {
      title: (name: string) => `Désinstaller ${name} ?`,
      text: (name: string) =>
        `Shinku ouvre le désinstalleur de ${name}. C’est lui qui retire l’app, et il peut te poser des questions. Ensuite, Shinku te montre ce qui reste.`,
      runtime: 'D’autres apps peuvent en avoir besoin pour fonctionner.',
      driver: 'Ça ressemble à un pilote : un appareil de ton PC peut cesser de marcher.',
      machine: 'Windows va te demander ton accord.',
      cancel: 'Annuler',
      confirm: 'Désinstaller',
    },
    toast: {
      broken: (name: string) => `Le désinstalleur de ${name} est introuvable. Essaie depuis Windows.`,
      vanished: (name: string) => `${name} n’est plus installée.`,
      failed: 'La désinstallation n’a pas pu démarrer.',
      kept: (name: string) => `${name} est toujours installée.`,
      keptUnknown: 'Cette app est toujours installée.',
      removed: (count: number, bytes: number) =>
        `${countOf(count, 'élément mis', 'éléments mis')} à la corbeille${bytes > 0 ? `, ${formatBytes(bytes)}` : ''}.`,
      nothingMoved: 'Rien n’a pu être mis à la corbeille.',
    },
    scene: {
      title: (name: string) => `Désinstallation de ${name}`,
      launching: 'Windows te demande peut-être ton accord.',
      waiting:
        'Si une fenêtre s’ouvre, suis ses étapes. Certaines apps se retirent sans rien afficher. Shinku attend.',
      waitingLate:
        'Rien ne se passe ? Regarde dans la barre des tâches : la fenêtre est peut-être derrière. Sinon, reviens à la liste.',
      settling: 'Presque fini…',
      back: 'Revenir à la liste',
    },
    leftovers: {
      title: (name: string) => `${name} est désinstallée`,
      family:
        'Une app du même nom est encore installée. Shinku ne propose rien, pour ne pas toucher à ce qui lui sert.',
      unsure: 'Shinku n’a pas pu vérifier à qui sont les dossiers : il ne propose rien.',
      none: 'Shinku n’a rien trouvé à te proposer. Dans ton compte, il ne cherche que les dossiers qui portent exactement le nom de l’app, et les raccourcis qui menaient vers elle.',
      allGone: 'Il ne reste plus rien à te proposer.',
      found: (count: number) =>
        `Shinku a trouvé ${countOf(count, 'élément qui semble', 'éléments qui semblent')} venir d’elle. Rien n’est coché : regarde avant de choisir.`,
      folder: 'Dossier de l’app',
      data: 'Réglages et données de l’app. Peut contenir ce que tu as fait avec elle : sauvegardes de jeu, messages, projets.',
      shortcut: 'Raccourci qui ne mène plus nulle part',
      // La taille et le nombre de fichiers d'un reste ; l'un ou l'autre peut manquer.
      files: (files: number) => countOf(files, 'fichier', 'fichiers'),
      size: (bytes: number, files: number) => `${formatBytes(bytes)} · ${countOf(files, 'fichier', 'fichiers')}`,
      trash: 'Mettre à la corbeille',
      rescan: 'Chercher à nouveau',
      done: 'Terminé',
      // `atLeast` : la taille d'un des éléments cochés n'est pas connue, le total est un minimum.
      caption: (count: number, bytes: number, atLeast: boolean) =>
        `${countOf(count, 'élément', 'éléments')}${bytes > 0 ? `, ${atLeast ? 'au moins ' : ''}${formatBytes(bytes)}` : ''}.`,
      captionNothing: 'Rien n’est coché',
      binNote: 'Ta corbeille peut abandonner d’anciens éléments pour faire de la place.',
      inUse: 'Encore utilisé : ferme ce qui l’utilise, puis réessaie.',
      destroyed: 'Windows l’a supprimé pour de bon : il n’est pas dans la Corbeille.',
      refused: 'La corbeille ne peut pas le recevoir.',
      partial: 'Une partie est restée.',
      other: 'N’a pas pu partir.',
    },
    announce: {
      list: 'Retour à la liste des apps.',
      launching: (name: string) => `Désinstallation de ${name}. Windows te demande peut-être ton accord.`,
      waiting: (name: string) => `Désinstallation de ${name}. Shinku attend la fin du désinstalleur.`,
      settling: (name: string) => `Désinstallation de ${name}. Presque fini.`,
      removed: (name: string, count: number) =>
        count === 0
          ? `${name} est désinstallée. Shinku n’a rien à te proposer.`
          : `${name} est désinstallée. ${countOf(count, 'reste à regarder', 'restes à regarder')}.`,
    },
  },

  privacy: {
    eyebrow: 'Protection',
    title: 'Confidentialité',
    intro: 'Des traces de ton activité sur ce PC, dans ton compte. Rien n’est coché d’avance.',
    erase: 'Effacer ce qui est coché',
    nothingNow: 'Rien à effacer pour l’instant.',
    eraseHint: 'Coche au moins une trace pour pouvoir l’effacer.',
    checked: (count: number) => `${countOf(count, 'trace cochée', 'traces cochées')}.`,
    retry: 'Réessayer',
    working: 'Effacement en cours. Les cases et les boutons attendent la fin.',

    browser: { chrome: 'Chrome', edge: 'Edge', brave: 'Brave', vivaldi: 'Vivaldi' },
    windows: 'Windows',
    mainProfile: 'Profil principal',
    numberedProfile: (slot: number) => `Profil ${slot}`,
    /** « Chrome · Léa » : le nom du profil est celui du navigateur, il n'est jamais retouché. */
    blockTitle: exact((app: string, profile: string) => [`${app}`, ' · ', verbatim(`${profile}`)]),
    /** Le nom accessible d'une case de Windows : « Windows, Presse-papiers ». */
    checkLabel: exact((block: string, trace: string) => [verbatim(`${block}`), ', ', `${trace}`]),
    /** Celui d'une case de navigateur : « Chrome, Léa, historique de navigation ». */
    checkLabelProfile: exact((app: string, profile: string, trace: string) => [`${app}`, ', ', verbatim(`${profile}`), ', ', `${trace}`]),
    /** La puce de la confirmation : « Léa : historique de navigation ». */
    /** Ce qui suit le nom du profil dans la puce (le nom, lui, est posé à part et isolé par `<bdi>`). */
    askTail: (trace: string) => ` : ${trace}`,
    /** Deux profils du même nom : « Léa (Profil 3) ». */
    twinProfile: exact((name: string, folder: string) => [verbatim(`${name}`), ' (', `${folder}`, ')']),
    askLine: exact((profile: string, trace: string) => [verbatim(`${profile}`), ' : ', `${trace}`]),
    browsers: {
      title: 'Navigateurs',
      intro:
        'Effacer l’historique d’un profil, c’est perdre : les pages visitées, la liste des téléchargements (pas les fichiers ; un téléchargement interrompu ne pourra plus reprendre), les suggestions de la barre d’adresse. Les icônes de tes favoris reviennent à la prochaine visite.',
    },

    trace: {
      history: 'Historique de navigation',
      recent: 'Fichiers et dossiers récents',
      typed: 'Ce que tu as tapé dans Windows',
      clipboard: 'Presse-papiers',
    },
    // Ce qu'on perd, écrit sous chaque ligne.
    loses: {
      recent:
        'Les « Récents » de l’Explorateur et les fichiers récents des apps dans la barre des tâches.',
      typed:
        'Les propositions de « Exécuter », de la barre d’adresse et de la recherche de l’Explorateur. Ferme d’abord les fenêtres de l’Explorateur : une fenêtre ouverte peut réécrire ce qu’elle a en mémoire.',
      clipboard: 'Ce que tu viens de copier, et l’historique (Windows+V).',
    },
    count: {
      history: (count: number) => countOf(count, 'élément', 'éléments'),
      recent: (count: number) => countOf(count, 'raccourci', 'raccourcis'),
      typed: (count: number) => countOf(count, 'élément', 'éléments'),
    },
    nothing: 'Rien à effacer',
    unreadable: 'Shinku n’a pas pu lire cette trace : elle n’est pas proposée.',
    running: (app: string) =>
      `Ferme ${app} pour effacer son historique. S’il n’a plus de fenêtre, il tourne peut-être en arrière-plan : quitte-le depuis la zone de notification.`,

    // Ce qui reste malgré l'effacement, écrit sous chaque bloc.
    remainsLabel: 'Ce qui reste',
    remainsBrowser:
      'Tes onglets ouverts et récemment fermés. Ce que la synchronisation garde dans ton compte en ligne, et peut faire revenir. Ce que tes extensions ont noté. Les cookies et les données des sites : pour eux, passe par le navigateur (Ctrl+Maj+Suppr).',
    remainsEdge: 'Edge peut réimporter l’historique de Chrome à son lancement, si tu l’as réglé ainsi.',
    remainsWindows:
      'Les éléments épinglés. Les éléments en nuage de « Recommandé ». Ce que tes autres appareils ou un gestionnaire de presse-papiers ont gardé.',
    firefox: 'Firefox : efface depuis Firefox lui-même (Ctrl+Maj+Suppr).',
    cache: 'Le cache des navigateurs se vide avec « Faire le vide », sur l’accueil.',

    inWindows: {
      title: 'Dans Windows',
      intro: 'Shinku n’efface pas ces traces lui-même : Windows le fait mieux. Chaque bouton ouvre sa page dans les paramètres de Windows.',
      open: 'Ouvrir dans Windows',
      pages: {
        activityHistory: {
          name: 'Historique d’activité',
          text: 'Les activités que Windows garde de ce que tu as fait sur ce PC.',
        },
        searchPermissions: {
          name: 'Historique de recherche',
          text: 'Ce que tu as cherché dans Windows.',
        },
        diagnostics: {
          name: 'Diagnostics',
          text: 'Les données de diagnostic que Windows envoie. Selon ton édition de Windows, la page propose ou non de les supprimer.',
        },
      },
    },

    untouched: {
      title: 'Ce que Shinku ne touche pas',
      intro: 'Jamais, quel que soit le navigateur :',
      items: {
        passwords: 'Les mots de passe',
        bookmarks: 'Les favoris',
        extensions: 'Les extensions et leurs données',
        tabs: 'Les onglets',
        cookies: 'Les cookies et les données des sites',
        pinned: 'Les éléments épinglés',
        backups: 'Les copies gardées dans tes sauvegardes et tes points de restauration',
      },
      outside:
        'Pas encore pris en charge : Opera et Opera GX, les versions Beta, Dev et Canary des navigateurs, les fichiers récents d’Office et des autres apps, les autres comptes de ce PC.',
    },

    confirm: {
      title: 'Effacer ces traces ?',
      titleOne: 'Effacer cette trace ?',
      intro: 'Tu vas effacer :',
      direct: 'Elles sont supprimées directement, sans passer par la Corbeille.',
      directOne: 'Elle est supprimée directement, sans passer par la Corbeille.',
      confirm: 'Effacer',
      cancel: 'Annuler',
    },

    lastTitle: 'Dernier effacement',
    // Dans la carte « Dernier effacement » et sous une ligne : l'issue en deux mots, puis l'aide quand il y en a une.
    outcomeShort: {
      erased: 'Effacé',
      nothing: 'Rien à effacer',
      inUse: 'En cours d’utilisation',
      changed: 'Profil changé',
      partial: 'Effacé en partie',
      failed: 'Pas pu être effacé',
      unknown: 'Résultat inconnu',
    },
    outcomeShortWindows: { changed: 'A changé' },
    help: {
      closeBrowser: (app: string) => `Ferme ${app} et recommence.`,
      closeApp: 'Ferme l’app et recommence.',
      lookAgain: 'Regarde de nouveau.',
      someLeft: 'Il en reste : regarde de nouveau.',
    },
    lastLine: exact((block: string, trace: string, outcome: string) => [verbatim(`${block}`), ', ', `${trace}`, ' : ', `${outcome}`]),
    /** `issues` : les phrases des issues, déjà jointes (voir `announceLines`). */
    announce: (issues: string) => `Effacement terminé. ${issues}.`,
    refused: {
      busy: 'Un grand vide, une mise à la corbeille de restes ou un autre effacement est en cours : attends qu’il se termine, puis recommence. Rien n’a été effacé.',
      listChanged: 'La liste a changé : regarde de nouveau.',
      tooMany: 'Trop de traces cochées d’un coup (64 au plus) : décoche-en quelques-unes, puis recommence. Rien n’a été effacé.',
    },
  },

  toast: {
    foldersChanged: 'Liste mise à jour. Les résultats précédents ont été effacés.',
    keepRefused: 'Ce choix n’a pas pu être noté : relance une analyse.',
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
