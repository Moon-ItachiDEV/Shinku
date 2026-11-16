// L'application : elle tient l'état, parle au cœur et redessine les écrans.
//
// Un seul sens de circulation : un geste appelle une action, l'action
// interroge le cœur puis change l'état, et l'état redessine tout. Pendant une
// analyse ou un grand vide, l'application redemande une vue de la session
// quelques fois par seconde ; le cœur ne pousse jamais rien vers la fenêtre.

import { render } from 'lit-html';

import type {
  AppsView,
  Backend,
  BackendError,
  DuplicateGroup,
  DuplicatesPage,
  GameOption,
  GameStatus,
  GameView,
  KeepChoice,
  LeftoverRow,
  SessionView,
  Settings,
  StartupView,
  Theme,
  WindowsPage,
} from './backend/types.ts';
import { fr } from './i18n/fr.ts';
import { cleanRequest, confirmedRequest, readiness, requestedSteps } from './model/clean.ts';
import {
  busyKey,
  focusKey,
  GAME_POLL_DELAY,
  GESTURE_TIMEOUT,
  isLeaving,
  LEAVE_GRACE,
  neighbour,
  noteStop,
  optionOf,
  RELEASE_GRACE,
  type Releasing,
  shownApps,
  VIEW_FAILURES,
  withExpectedRelease,
  withOption,
  withQuietApp,
  withReleased,
} from './model/game.ts';
import { reportKey, toggled } from './model/scan.ts';
import {
  entryById,
  isWaiting,
  SAY_DELAY,
  STARTUP_RELOAD_GAP,
  TITLE_KEY,
  focusTarget,
  sayStale,
  type Say,
  sayDone,
  sayRefused,
  shouldPoll,
  startupFocus,
  STARTUP_POLL_DELAY,
  withEntry,
  withEntryEnabled,
} from './model/startup.ts';
import {
  announcement,
  APPS_POLL_DELAY,
  APPS_RELOAD_GAP,
  appById,
  askFor,
  type AppsSort,
  checkable,
  checkedRows,
  focusAfterRemoval,
  focusTarget as appsFocusTarget,
  followedName,
  isPolling,
  mergedFailures,
  noneBecause,
  proposed,
  refusalOf,
  removalOf,
  removalToast,
  screenOf,
  listedApps,
  uninstallFocus,
} from './model/uninstall.ts';
import { type Actions, type AppState, initialState, type Route } from './state.ts';
import { applyTheme } from './theme.ts';
import { appView } from './views/shell.ts';

/** Délai entre deux vues de session pendant une analyse ou un grand vide, en millisecondes. */
const POLL_DELAY = 150;
/** Délai entre deux rafraîchissements de l'heure et de l'âge des résultats affichés. */
const CLOCK_DELAY = 30_000;
/** Taille d'une page de résultats. */
const PAGE_SIZE = 50;
/** Durée d'affichage d'un message bref. */
const TOAST_DELAY = 6000;

function isBackendError(error: unknown): error is BackendError {
  return typeof error === 'object' && error !== null && 'kind' in error;
}

function errorMessage(error: unknown): string {
  if (!isBackendError(error)) {
    return fr.toast.unexpected;
  }
  switch (error.kind) {
    case 'settingsUnchecked':
      return fr.toast.settingsUnchecked;
    case 'scanNotStarted':
      return fr.toast.scanNotStarted;
    case 'settingsNotSaved':
      return fr.toast.settingsNotSaved;
    case 'folderRefused':
      return fr.toast[error.reason];
    case 'cleanRunning':
      return fr.toast.cleanRunning;
    case 'cleanRefused':
      return fr.toast[error.reason];
    case 'cleanNotStarted':
      return fr.toast.cleanNotStarted;
    case 'gameRefused':
      return fr.game.toast[error.reason];
    default:
      return fr.toast.unexpected;
  }
}

const NO_ACTIONS: Actions = {
  navigate: () => {},
  startScan: () => {},
  cancelScan: () => {},
  showScan: () => {},
  showResults: () => {},
  showHome: () => {},
  toggle: () => {},
  askClean: () => {},
  confirmClean: () => {},
  dismissClean: () => {},
  stopClean: () => {},
  closeReport: () => {},
  loadMore: () => {},
  keepDuplicate: () => {},
  setTheme: () => {},
  addFolder: () => {},
  removeFolder: () => {},
  acknowledgeIssue: () => {},
  setGameOption: () => {},
  setQuietApp: () => {},
  releaseGame: () => {},
  setGameStatus: () => {},
  openPicker: () => {},
  closePicker: () => {},
  pickOpenApp: () => {},
  setStartupEntry: () => {},
  undoStartup: () => {},
  holdStartupSay: () => {},
  retryStartup: () => {},
  setAppsQuery: () => {},
  setAppsSort: () => {},
  retryApps: () => {},
  askUninstall: () => {},
  confirmUninstall: () => {},
  dismissUninstall: () => {},
  stopUninstall: () => {},
  rescanLeftovers: () => {},
  toggleLeftover: () => {},
  removeLeftovers: () => {},
  openWindowsSettings: () => {},
  minimize: () => {},
  toggleMaximize: () => {},
  close: () => {},
};

/** La page a été ouverte hors de Shinku : on l'explique, et rien d'autre. */
export function renderOutside(root: HTMLElement): void {
  render(appView({ ...initialState(Date.now()), boot: 'outside' }, NO_ACTIONS), root);
}

/** Ce qui dessine l'état dans la page : `render` de lit-html, sauf dans les tests. */
export type Draw = (view: ReturnType<typeof appView>, root: HTMLElement) => void;

export class App {
  private state: AppState = initialState(Date.now());
  private readonly backend: Backend;
  private readonly root: HTMLElement;
  private readonly draw: Draw;
  private renderQueued = false;
  private polling = false;
  private toastTimer: number | undefined;
  /**
   * Change à chaque geste qui lance ou arrête une analyse ou un grand vide.
   * Une vue de session demandée avant ce geste et arrivée après lui est ignorée.
   */
  private sessionEpoch = 0;
  /** Change à chaque nouveau rapport : les listes d'un ancien rapport sont ignorées. */
  private dataEpoch = 0;
  private lastReportKey = '';
  private readonly loading = new Set<string>();
  private lastOverviewRefresh = 0;
  private gameTimer: number | undefined;
  /** Redessine l'en-tête quand le délai de son attente est atteint : le sondage peut être arrêté. */
  private leaveTimer: number | undefined;
  /**
   * Change au départ et à la réception de chaque geste du mode jeu : une vue
   * demandée avant l'un ou l'autre et arrivée après est ignorée.
   */
  private gameEpoch = 0;
  /** Le numéro du dernier geste parti : seule sa réponse change la vue. */
  private gameSeq = 0;
  /** Les gestes du mode jeu en vol : tant qu'il y en a, aucune vue demandée par sondage n'est adoptée. */
  private gamePending = 0;
  /** Des gestes se sont croisés (l'un est parti avant la réponse de l'autre) : la vue est relue quand le dernier a répondu. */
  private gamesCrossed = false;
  private gameLoading = false;
  /** Les demandes de la vue du mode jeu échouées de suite. */
  private gameFailures = 0;
  /** « Tout remettre maintenant » est parti : la page tient la partie pour remise en place le temps que l'agent le note. */
  private releasing: Releasing | null = null;
  /** Change à chaque ouverture ou fermeture du sélecteur : une liste arrivée trop tard est ignorée. */
  private pickerEpoch = 0;
  /** La page Démarrage est affichée : sa vue se charge à l'arrivée et au retour du focus, jamais en boucle. */
  private startupVisit = 0;
  private startupWatching = false;
  /** Change au départ et à la réponse de chaque changement : une vue demandée avant lui et arrivée après est ignorée. */
  private startupAction = 0;
  /** Le numéro de la dernière demande de vue : seule sa réponse compte. */
  private startupAsk = 0;
  /** Les changements en vol : tant qu'il y en a un, aucune vue demandée n'est adoptée. */
  private startupPending = 0;
  private startupSayTimer: number | undefined;
  /** La relecture de la vue tant que le cœur attend la personne : une seule minuterie, comme `appsTimer`. */
  private startupTimer: number | undefined;
  /** « Réessayer » vient d'être pressé : le focus suivra la vue qui arrive. */
  private startupRetried = false;
  /** La zone de retour a le focus ou le pointeur : son délai de 12 secondes est suspendu. */
  private sayFocus = false;
  private sayHover = false;
  /** Une vue est demandée, ou vient d'arriver : le retour de la fenêtre n'en relance pas une deuxième. */
  private startupLoading = false;
  private startupLoadedAt = 0;
  /** La page Désinstallation est affichée : sa vue se charge à l'arrivée et au retour du focus. */
  private appsWatching = false;
  private appsVisit = 0;
  /** Change au départ et à la réponse de chaque geste : une vue demandée avant lui et arrivée après est ignorée. */
  private appsAction = 0;
  /** Le numéro de la dernière demande de vue : seule sa réponse compte. */
  private appsAsk = 0;
  private appsLoading = false;
  private appsLoadedAt = 0;
  /** La minuterie d'interrogation : une seule, jamais deux. */
  private appsTimer: number | undefined;
  /** « Réessayer » vient d'être pressé : le focus suivra la vue qui arrive. */
  private appsRetried = false;
  /** Le nom de la dernière app suivie : `outcome: 'kept'` ne le porte pas. */
  private appsLastName: string | null = null;

  constructor(backend: Backend, root: HTMLElement, draw: Draw = render) {
    this.backend = backend;
    this.root = root;
    this.draw = draw;
  }

  // ── État et dessin ───────────────────────────────────────────────────────

  private set(patch: Partial<AppState>): void {
    this.state = { ...this.state, ...patch };
    if (!this.renderQueued) {
      this.renderQueued = true;
      queueMicrotask(() => {
        this.renderQueued = false;
        this.draw(appView(this.state, this.actions), this.root);
      });
    }
  }

  private toast(message: string): void {
    window.clearTimeout(this.toastTimer);
    this.set({ toast: message, toastShown: true });
    this.toastTimer = window.setTimeout(() => this.set({ toastShown: false }), TOAST_DELAY);
  }

  // ── Démarrage ────────────────────────────────────────────────────────────

  async start(): Promise<void> {
    this.draw(appView(this.state, this.actions), this.root);
    try {
      const [overview, session] = await Promise.all([this.backend.overview(), this.backend.sessionView()]);
      applyTheme(overview.settings.theme);
      this.lastOverviewRefresh = Date.now();
      this.set({ boot: 'ready', overview });
      this.adopt(session);
    } catch {
      this.set({ boot: 'failed' });
      return;
    }
    // L'heure avance, les résultats vieillissent, et le disque change pendant
    // que la fenêtre n'est pas regardée.
    window.setInterval(() => {
      this.set({ now: Date.now() });
      void this.refreshResults();
    }, CLOCK_DELAY);
    window.addEventListener('focus', () => {
      void this.refreshOverview(5000);
      void this.refreshGame();
      void this.refreshStartup();
      void this.refreshApps('focus');
    });
    // Une fenêtre réduite ou cachée ne demande rien ; elle relit la vue dès qu'on la regarde de nouveau.
    document.addEventListener('visibilitychange', () => {
      void this.refreshGame();
      void this.refreshStartup();
      void this.refreshApps('focus');
    });
  }

  /** Relit le disque, la corbeille et les réglages. Rend `false` si rien n'a été relu. */
  private async refreshOverview(minimumAge = 0): Promise<boolean> {
    if (Date.now() - this.lastOverviewRefresh < minimumAge) {
      return false;
    }
    this.lastOverviewRefresh = Date.now();
    try {
      this.set({ overview: await this.backend.overview() });
      return true;
    } catch {
      // La jauge garde ses derniers chiffres.
      return false;
    }
  }

  /**
   * Des résultats affichés vieillissent sans que rien ne bouge : passé dix
   * minutes, le cœur les dit trop anciens pour nettoyer. C'est lui qui tient
   * l'horloge ; l'écran lui redemande donc sa vue de temps en temps.
   */
  private async refreshResults(): Promise<void> {
    const { session } = this.state;
    if (session.state !== 'results' || session.stale) {
      return;
    }
    const epoch = this.sessionEpoch;
    try {
      const fresh = await this.backend.sessionView();
      if (epoch === this.sessionEpoch) {
        this.adopt(fresh);
      }
    } catch {
      // La prochaine demande le dira.
    }
  }

  /** Après une analyse ou un grand vide : la corbeille et le disque ont bougé. */
  private async refreshAfterWork(): Promise<void> {
    const fresh = await this.refreshOverview();
    if (fresh && this.state.session.state === 'cleaned') {
      // Le bilan peut maintenant dire ce que le disque a de libre.
      this.set({ diskAfterClean: true });
    }
  }

  // ── Session d'analyse ────────────────────────────────────────────────────

  /** Prend en compte une vue de session : écran à montrer, listes à recharger, suivi. */
  private adopt(session: SessionView): void {
    const previous = this.state.session;
    const patch: Partial<AppState> = { session };

    const key = reportKey(session);
    if (key !== this.lastReportKey) {
      // Le rapport a changé : les listes affichées ne valent plus rien.
      this.lastReportKey = key;
      this.dataEpoch += 1;
      this.loading.clear();
      patch.junk = null;
      patch.large = null;
      patch.duplicates = null;
      patch.loadingMore = false;
      patch.keeping = false;
    }

    if (session.state === 'idle' || session.state === 'failed' || session.clean !== null) {
      // Ni analyse à suivre, ni résultats à montrer. Un grand vide en cours et
      // son bilan s'affichent d'eux-mêmes (voir `shownPane`).
      patch.pane = 'home';
    } else if (this.state.pane === 'results' && session.summary === null) {
      patch.pane = 'scan';
    }
    if (session.state !== 'results' || session.stale) {
      // La confirmation ne vaut que pour des résultats complets et récents.
      patch.confirm = null;
    }
    if (session.state !== 'cleaning') {
      patch.stopping = false;
    }
    if (session.state !== 'cleaned') {
      patch.diskAfterClean = false;
    }
    if (session.state === 'failed' && previous.state !== 'failed') {
      this.toast(session.clean === null ? fr.toast.scanFailed : fr.toast.cleanFailed);
    }
    this.set(patch);

    if (session.state === 'scanning' || session.state === 'cleaning') {
      void this.poll();
    }
    const scanEnded = previous.state === 'scanning' && session.state !== 'scanning';
    // Un grand vide très court peut être déjà terminé à la première vue reçue.
    const cleanEnded =
      (previous.state === 'cleaning' && session.state !== 'cleaning') ||
      (session.state === 'cleaned' && previous.state !== 'cleaned');
    if (scanEnded || cleanEnded) {
      void this.refreshAfterWork();
    }
    this.ensureData();
  }

  private async poll(): Promise<void> {
    if (this.polling) {
      return;
    }
    this.polling = true;
    try {
      while (this.state.session.state === 'scanning' || this.state.session.state === 'cleaning') {
        await new Promise((resolve) => window.setTimeout(resolve, POLL_DELAY));
        const epoch = this.sessionEpoch;
        const session = await this.backend.sessionView();
        if (epoch === this.sessionEpoch) {
          this.adopt(session);
        }
      }
    } catch {
      this.toast(fr.toast.unexpected);
    } finally {
      this.polling = false;
    }
  }

  /** Charge une liste une seule fois par rapport, même si l'écran est redessiné entre-temps. */
  private load<T>(name: string, request: () => Promise<T>, store: (value: T) => void): void {
    if (this.loading.has(name)) {
      return;
    }
    this.loading.add(name);
    const epoch = this.dataEpoch;
    request().then(
      (value) => {
        if (epoch === this.dataEpoch) {
          store(value);
        }
      },
      () => {
        if (epoch === this.dataEpoch) {
          this.loading.delete(name);
        }
      },
    );
  }

  /** Charge ce que l'écran affiché a besoin de montrer, si ce n'est pas déjà là. */
  private ensureData(): void {
    const { route, pane, session } = this.state;
    if (session.summary === null) {
      return;
    }
    if (route === 'junk') {
      this.load('junk', () => this.backend.junk(), (junk) => this.set({ junk }));
    }
    // L'écran Résultats cite le seuil des gros fichiers : il lit la première page lui aussi.
    if (route === 'large' || (route === 'home' && pane === 'results')) {
      this.load('large', () => this.backend.largeFiles(0, PAGE_SIZE), (large) => this.set({ large }));
    }
    if (route === 'duplicates') {
      this.load(
        'duplicates',
        () => this.backend.duplicates(0, PAGE_SIZE),
        (duplicates) => this.set({ duplicates }),
      );
    }
  }

  private async loadMore(list: 'large' | 'duplicates'): Promise<void> {
    // Une page lue pendant qu'un choix s'enregistre pourrait dater d'avant lui.
    if (this.state.loadingMore || this.state.keeping) {
      return;
    }
    const epoch = this.dataEpoch;
    this.set({ loadingMore: true });
    try {
      if (list === 'large') {
        const known = this.state.large;
        const page = await this.backend.largeFiles(known?.files.length ?? 0, PAGE_SIZE);
        if (epoch === this.dataEpoch && known !== null && page !== null) {
          this.set({ large: { ...page, offset: 0, files: [...known.files, ...page.files] } });
        }
      } else {
        const known = this.state.duplicates;
        const page = await this.backend.duplicates(known?.groups.length ?? 0, PAGE_SIZE);
        if (epoch === this.dataEpoch && known !== null && page !== null) {
          this.set({ duplicates: { ...page, offset: 0, groups: [...known.groups, ...page.groups] } });
        }
      }
    } catch (error) {
      this.toast(errorMessage(error));
    } finally {
      if (epoch === this.dataEpoch) {
        this.set({ loadingMore: false });
      }
    }
  }

  /**
   * La personne choisit l'exemplaire à garder d'un doublon, ou tout un dossier.
   * Rien n'est supprimé : le cœur note le choix dans son rapport, et le grand
   * vide le suivra.
   */
  private async keepDuplicate(choice: KeepChoice): Promise<void> {
    const { session, duplicates, keeping, loadingMore, cleanBusy, confirm } = this.state;
    if (session.state !== 'results' || session.stale || duplicates === null || keeping || loadingMore) {
      return;
    }
    if (cleanBusy || confirm !== null) {
      // La confirmation du grand vide annonce des chiffres : ils ne bougent plus.
      return;
    }
    const epoch = this.dataEpoch;
    this.set({ keeping: true });
    try {
      const noted = await this.backend.keepDuplicate(choice);
      if (epoch !== this.dataEpoch) {
        return;
      }
      if (!noted) {
        this.toast(fr.toast.keepRefused);
      }
      // Le cœur fait foi : les groupes affichés sont relus chez lui, choix noté ou non.
      const shown = this.state.duplicates?.groups.length ?? 0;
      const groups: DuplicateGroup[] = [];
      let last: DuplicatesPage | null = null;
      do {
        last = await this.backend.duplicates(groups.length, Math.min(PAGE_SIZE, Math.max(1, shown - groups.length)));
        if (epoch !== this.dataEpoch) {
          return;
        }
        groups.push(...(last?.groups ?? []));
      } while (last !== null && last.groups.length > 0 && groups.length < shown);
      this.set({ duplicates: last === null ? null : { ...last, offset: 0, groups } });
      // Le total des copies a changé : l'Accueil et la légende le relisent.
      const sessionEpoch = this.sessionEpoch;
      const fresh = await this.backend.sessionView();
      if (sessionEpoch === this.sessionEpoch) {
        this.adopt(fresh);
      }
    } catch (error) {
      this.toast(errorMessage(error));
    } finally {
      if (epoch === this.dataEpoch) {
        this.set({ keeping: false });
      }
    }
  }

  /** Envoie un geste qui change la session, et n'en garde la réponse que si aucun autre ne l'a suivi. */
  private async command(request: () => Promise<SessionView>): Promise<SessionView | null> {
    this.sessionEpoch += 1;
    const epoch = this.sessionEpoch;
    try {
      const session = await request();
      if (epoch !== this.sessionEpoch) {
        return null;
      }
      this.adopt(session);
      return session;
    } catch (error) {
      this.toast(errorMessage(error));
      return null;
    }
  }

  private async startScan(): Promise<void> {
    const session = await this.command(() => this.backend.startScan());
    if (session === null) {
      return;
    }
    if (session.state === 'scanning') {
      // « Grand vide » montre désormais l'analyse, d'où qu'elle ait été lancée.
      this.set({ pane: 'scan' });
    }
    // Dès qu'une analyse démarre, le cœur cesse d'annoncer un grand vide interrompu.
    const overview = this.state.overview;
    if (overview !== null && overview.interruptedClean !== null) {
      this.set({ overview: { ...overview, interruptedClean: null } });
    }
  }

  /** Arrête l'analyse et oublie ses résultats ; referme aussi le bilan d'un grand vide terminé. */
  private async cancelScan(): Promise<void> {
    await this.command(() => this.backend.cancelScan());
  }

  // ── Grand vide ───────────────────────────────────────────────────────────

  /**
   * « Faire le vide » : rien ne part encore. Le cœur dit ce que le grand vide
   * ferait du disque tel qu'il est maintenant, et la confirmation l'affiche.
   */
  private async askClean(): Promise<void> {
    // Un choix d'exemplaire en route changerait les chiffres de la confirmation.
    if (this.state.cleanBusy || this.state.confirm !== null || this.state.keeping) {
      return;
    }
    if (readiness(this.state.session, this.state.selection) !== 'ready') {
      return;
    }
    const epoch = this.sessionEpoch;
    this.set({ cleanBusy: true });
    try {
      const preview = await this.backend.cleanPreview();
      const { session, selection } = this.state;
      if (epoch !== this.sessionEpoch || session.summary === null || readiness(session, selection) !== 'ready') {
        // Un autre geste est passé entre-temps : cette réponse ne vaut plus.
        return;
      }
      if (preview.analysis !== session.summary.started) {
        this.toast(fr.toast.otherAnalysis);
        void this.refreshResults();
        return;
      }
      const request = confirmedRequest(preview, cleanRequest(session.summary, selection));
      if (requestedSteps(request).length === 0) {
        // Ce qui était coché s'est vidé depuis l'analyse.
        this.toast(fr.toast.nothingToClean);
        void this.refreshOverview();
        return;
      }
      this.set({ confirm: { preview, request } });
    } catch (error) {
      this.toast(errorMessage(error));
      // L'écran apprend pourquoi : des résultats devenus trop anciens, par exemple.
      void this.refreshResults();
    } finally {
      this.set({ cleanBusy: false });
    }
  }

  /** La personne a confirmé : la demande part telle que la confirmation l'affichait. */
  private async confirmClean(): Promise<void> {
    const confirmation = this.state.confirm;
    if (confirmation === null || this.state.cleanBusy) {
      return;
    }
    this.set({ cleanBusy: true });
    try {
      const session = await this.command(() => this.backend.startClean(confirmation.request));
      if (session === null) {
        // Refusé : rien n'est parti. L'écran reprend ce que le cœur a vraiment.
        await this.command(() => this.backend.sessionView());
      }
    } finally {
      this.set({ cleanBusy: false, confirm: null });
    }
  }

  private dismissClean(): void {
    // Une fois la demande partie, refermer la confirmation ne l'annulerait pas.
    if (!this.state.cleanBusy) {
      this.set({ confirm: null });
    }
  }

  /** Demande l'arrêt : le cœur finit le fichier en cours, puis rend un bilan partiel. */
  private async stopClean(): Promise<void> {
    if (this.state.session.state !== 'cleaning' || this.state.stopping) {
      return;
    }
    this.set({ stopping: true });
    const session = await this.command(() => this.backend.cancelClean());
    if (session === null && this.state.session.state === 'cleaning') {
      // La demande n'a pas abouti : le bouton redevient disponible.
      this.set({ stopping: false });
    }
  }

  // ── Réglages ─────────────────────────────────────────────────────────────

  private setSettings(settings: Settings): void {
    const overview = this.state.overview;
    if (overview !== null) {
      this.set({ overview: { ...overview, settings } });
    }
  }

  private async setTheme(theme: Theme): Promise<void> {
    const before = this.state.overview?.settings;
    if (before === undefined || before.theme === theme) {
      return;
    }
    // Le changement se voit tout de suite ; il est annulé si l'enregistrement échoue.
    applyTheme(theme);
    this.setSettings({ ...before, theme });
    try {
      this.setSettings(await this.backend.setTheme(theme));
    } catch (error) {
      applyTheme(before.theme);
      const current = this.state.overview?.settings;
      if (current !== undefined) {
        this.setSettings({ ...current, theme: before.theme });
      }
      this.toast(errorMessage(error));
    }
  }

  /** Change les dossiers jamais touchés : le cœur oublie alors les résultats en cours. */
  private async changeFolders(request: () => Promise<Settings | null>): Promise<void> {
    const before = this.state.overview?.settings.excludedFolders ?? [];
    const hadResults = this.state.session.state !== 'idle';
    try {
      const settings = await request();
      if (settings === null) {
        return;
      }
      this.setSettings(settings);
      const after = settings.excludedFolders;
      const changed = before.length !== after.length || before.some((path, index) => path !== after[index]);
      await this.command(() => this.backend.sessionView());
      if (changed && hadResults) {
        this.toast(fr.toast.foldersChanged);
      }
    } catch (error) {
      this.toast(errorMessage(error));
    }
  }

  private async addFolder(): Promise<void> {
    if (this.state.pickingFolder) {
      return;
    }
    this.set({ pickingFolder: true });
    try {
      await this.changeFolders(() => this.backend.addExcludedFolder());
    } finally {
      this.set({ pickingFolder: false });
    }
  }

  private async acknowledgeIssue(): Promise<void> {
    try {
      this.set({ overview: await this.backend.acknowledgeSettingsIssue() });
    } catch (error) {
      this.toast(errorMessage(error));
    }
  }

  // ── Mode jeu ─────────────────────────────────────────────────────────────

  /**
   * Tant que la page Mode jeu est affichée, la vue est redemandée toutes les
   * deux secondes : l'agent écrit seul, et la page doit le montrer. Hors de la
   * page, rien n'est demandé, et rien n'est gardé : la vue d'une visite ne sert
   * pas à la suivante.
   */
  private watchGame(): void {
    if (this.state.route !== 'gameMode') {
      this.stopWatchingGame();
      return;
    }
    if (this.gameTimer !== undefined) {
      // Déjà sur la page : une seule minuterie, on relit seulement la vue.
      void this.refreshGame();
      return;
    }
    // Une visite neuve : ni la vue ni l'arrêt noté de la précédente.
    this.set({ game: null, gameStoppedSince: null, gameFailed: false });
    this.endLeaving();
    this.gameFailures = 0;
    this.releasing = null;
    void this.refreshGame();
    this.gameTimer = window.setInterval(() => {
      if (this.state.route !== 'gameMode') {
        // La route a changé sans passer par `navigate` : on cesse de demander.
        this.stopWatchingGame();
        return;
      }
      void this.refreshGame();
    }, GAME_POLL_DELAY);
  }

  private stopWatchingGame(): void {
    if (this.gameTimer === undefined) {
      return;
    }
    window.clearInterval(this.gameTimer);
    this.gameTimer = undefined;
    this.set({ game: null, gameStoppedSince: null, gameFailed: false });
    this.endLeaving();
  }

  private adoptGame(received: GameView): void {
    if (this.state.route !== 'gameMode') {
      return;
    }
    const now = Date.now();
    // L'agent n'a peut-être pas encore noté ce que la personne vient de demander : la page ne revient pas en arrière.
    const view = withExpectedRelease(received, this.releasing, now);
    if (this.releasing !== null && view === received) {
      this.releasing = null;
    }
    this.set({ game: view, gameStoppedSince: noteStop(this.state.gameStoppedSince, view, now), now });
    this.settleLeaving();
  }

  /** L'attente de l'en-tête (« Ce n'est pas un jeu ») est-elle finie ? Si oui, elle tombe et le focus va où il faut. */
  private settleLeaving(): void {
    const leaving = this.state.gameLeaving;
    const view = this.state.game;
    const now = Date.now();
    if (leaving === null || view === null || isLeaving(view, leaving, now, true)) {
      return;
    }
    this.endLeaving();
    this.set({ now });
    // La partie a disparu : le focus va au titre. Sinon (délai dépassé, remise en place en attente, agent arrêté…),
    // les boutons de l'en-tête sont encore là : il va à celui qui avait le focus.
    this.focusKey(view.session?.path === leaving.path ? focusKey.headerNotGame : focusKey.title);
  }

  private endLeaving(): void {
    window.clearTimeout(this.leaveTimer);
    this.leaveTimer = undefined;
    this.set({ gameLeaving: null });
  }

  private async refreshGame(): Promise<void> {
    // Pas de demande pour une fenêtre cachée, ni pendant un geste : sa réponse est plus récente que tout sondage.
    if (this.gameLoading || this.gamePending > 0 || document.hidden || this.state.route !== 'gameMode') {
      return;
    }
    this.gameLoading = true;
    const epoch = this.gameEpoch;
    try {
      const view = await this.backend.gameView();
      this.gameFailures = 0;
      if (epoch === this.gameEpoch && this.gamePending === 0 && this.state.route === 'gameMode') {
        this.set({ gameFailed: false });
        this.adoptGame(view);
      }
    } catch {
      // La page garde sa dernière vue ; la prochaine demande dira. Si la première ne vient jamais, elle le dit.
      this.gameFailures += 1;
      if (this.gameFailures >= VIEW_FAILURES && this.state.game === null) {
        this.set({ gameFailed: true });
      }
    } finally {
      this.gameLoading = false;
    }
  }

  /** Rend le focus au bouton qui porte cette clé (`data-fk`), une fois la page redessinée, si personne ne l'a pris. */
  private focusKey(key: string): void {
    queueMicrotask(() => {
      const active = document.activeElement;
      if (active !== null && active !== document.body) {
        return;
      }
      for (const element of this.root.querySelectorAll<HTMLElement>('[data-fk]')) {
        if (element.dataset['fk'] === key) {
          element.focus();
          return;
        }
      }
    });
  }

  /**
   * Envoie un geste du mode jeu. `key` : tant qu'il est en vol, le même geste
   * n'est pas renvoyé et son bouton est désactivé. `optimistic` change la vue
   * tout de suite ; `undo` la remet comme elle était si le cœur refuse, et la
   * page relit alors la vue du cœur.
   */
  private async gameCommand(
    request: () => Promise<GameView>,
    options: {
      key?: string;
      /** Appelé si le geste est abandonné faute de réponse. */
      abandoned?: () => void;
      optimistic?: (view: GameView) => GameView;
      undo?: (view: GameView) => GameView;
    } = {},
  ): Promise<boolean> {
    const { key, abandoned, optimistic, undo } = options;
    if (key !== undefined) {
      if (this.state.gameBusy.includes(key)) {
        return false;
      }
      this.set({ gameBusy: [...this.state.gameBusy, key] });
    }
    if (this.gamePending > 0) {
      this.gamesCrossed = true;
    }
    this.gamePending += 1;
    this.gameEpoch += 1;
    this.gameSeq += 1;
    const mine = this.gameSeq;
    const before = this.state.game;
    if (optimistic !== undefined && before !== null) {
      this.set({ game: optimistic(before) });
    }
    // La réponse arrive (ou le geste est abandonné) : un sondage parti avant serait plus vieux qu'elle.
    let over = false;
    const answered = (): boolean => {
      if (over) {
        return false;
      }
      over = true;
      window.clearTimeout(giveUp);
      this.gamePending -= 1;
      this.gameEpoch += 1;
      if (key !== undefined) {
        this.set({ gameBusy: this.state.gameBusy.filter((other) => other !== key) });
      }
      if (this.gamePending === 0 && this.gamesCrossed) {
        // Le cœur a pu appliquer les gestes dans l'autre ordre : on relit tout de suite, sans attendre le sondage.
        this.gamesCrossed = false;
        queueMicrotask(() => void this.refreshGame());
      }
      return true;
    };
    // Un geste sans réponse bloquerait le sondage et son bouton pour toujours : passé le délai, on l'abandonne.
    // Sa réponse, si elle arrive plus tard, est ignorée (`over`).
    const giveUp = window.setTimeout(() => {
      if (answered()) {
        abandoned?.();
        void this.refreshGame();
      }
    }, GESTURE_TIMEOUT);
    try {
      const view = await request();
      if (!answered()) {
        return false;
      }
      if (mine === this.gameSeq) {
        this.adoptGame(view);
      }
      return true;
    } catch (error) {
      if (!answered()) {
        return false;
      }
      const current = this.state.game;
      if (undo !== undefined && current !== null) {
        this.set({ game: undo(current) });
      }
      this.toast(errorMessage(error));
      void this.refreshGame();
      return false;
    }
  }

  private async setGameOption(option: GameOption, on: boolean): Promise<void> {
    const before = this.state.game;
    if (before === null || optionOf(before, option) === on) {
      return;
    }
    const previous = optionOf(before, option);
    await this.gameCommand(() => this.backend.setGameOption(option, on), {
      optimistic: (view) => withOption(view, option, on),
      undo: (view) => withOption(view, option, previous),
    });
  }

  private async setQuietApp(exe: string, on: boolean): Promise<void> {
    const view = this.state.game;
    const before = view?.apps.find((app) => app.exe === exe);
    if (view === null || before === undefined || before.on === on) {
      return;
    }
    if (!before.builtIn) {
      // Une app ajoutée par la personne disparaît de la liste : rien ne se montre avant la réponse du cœur.
      const keys = shownApps(view.apps).map((app) => focusKey.quiet(app.exe));
      const done = await this.gameCommand(() => this.backend.setQuietApp(exe, on), { key: busyKey.quiet(exe) });
      if (done) {
        this.focusKey(neighbour(keys, focusKey.quiet(exe)) ?? focusKey.addApp);
      }
      return;
    }
    await this.gameCommand(() => this.backend.setQuietApp(exe, on), {
      optimistic: (current) => withQuietApp(current, exe, on),
      undo: (current) => withQuietApp(current, exe, before.on),
    });
  }

  /** « Tout remettre maintenant » : la page montre tout de suite la partie remise en place ; l'agent le notera à son pas suivant. */
  private async releaseGame(): Promise<void> {
    const session = this.state.game?.session ?? null;
    if (session === null || session.released) {
      return;
    }
    this.releasing = { path: session.path, until: Date.now() + RELEASE_GRACE };
    const request = this.gameCommand(() => this.backend.releaseGame(), {
      key: busyKey.release,
      optimistic: (view) => withReleased(view, true),
      undo: (view) => {
        // Le cœur a refusé : la partie n'est pas remise en place, la page ne le prétend plus.
        this.releasing = null;
        return withReleased(view, false);
      },
    });
    // Le bouton disparaît : le focus va à celui qui reste.
    this.focusKey(focusKey.headerNotGame);
    await request;
  }

  /** « Ce n'est pas un jeu », « Retirer » et « Rétablir » : le focus va à la ligne voisine de celle qui disparaît. */
  private async setGameStatus(path: string, status: GameStatus): Promise<void> {
    const view = this.state.game;
    const keys =
      view === null
        ? []
        : status === 'forgotten' && view.notGames.some((entry) => entry.path === path)
          ? view.notGames.map((entry) => focusKey.never(entry.path))
          : view.games.map((game) => focusKey.game(game.path));
    const row = view?.notGames.some((entry) => entry.path === path) ? focusKey.never(path) : focusKey.game(path);
    // La partie en cours : l'en-tête attend que l'agent la termine (au plus LEAVE_GRACE), et le focus ira au titre quand elle aura disparu.
    const ownGame = status === 'notGame' && view?.session?.path === path;
    // Le cœur a refusé, ou le geste est abandonné : l'en-tête reprend son état, et son bouton, grisé, retrouve le focus.
    const giveBack = (): void => {
      if (ownGame && this.state.gameLeaving?.path === path) {
        this.endLeaving();
        this.focusKey(focusKey.headerNotGame);
      }
    };
    if (ownGame) {
      this.set({ gameLeaving: { path, until: Date.now() + LEAVE_GRACE } });
      window.clearTimeout(this.leaveTimer);
      this.leaveTimer = window.setTimeout(() => this.settleLeaving(), LEAVE_GRACE);
    }
    const done = await this.gameCommand(() => this.backend.setGameStatus(path, status), {
      key: busyKey.status(path),
      abandoned: giveBack,
    });
    if (!done) {
      giveBack();
    } else if (!ownGame) {
      this.focusKey(neighbour(keys, row) ?? focusKey.addGame);
    }
  }

  /** Ouvre le sélecteur ; la liste des apps ouvertes est demandée au cœur à ce moment-là. */
  private async openPicker(kind: 'game' | 'app'): Promise<void> {
    if (this.state.picker !== null) {
      return;
    }
    this.pickerEpoch += 1;
    const epoch = this.pickerEpoch;
    this.set({ picker: { kind, apps: null } });
    try {
      const apps = await this.backend.openApps();
      if (epoch === this.pickerEpoch) {
        this.set({ picker: { kind, apps } });
      }
    } catch (error) {
      if (epoch === this.pickerEpoch) {
        this.set({ picker: null });
        this.toast(errorMessage(error));
      }
    }
  }

  private closePicker(): void {
    this.pickerEpoch += 1;
    this.set({ picker: null });
  }

  /** Le choix part avec le numéro et le chemin que le cœur a donnés ; la boîte reste, ses choix grisés, jusqu'à la réponse. */
  private async pickOpenApp(id: number): Promise<void> {
    const picker = this.state.picker;
    const chosen = picker?.apps?.find((app) => app.id === id);
    if (picker === null || chosen === undefined || this.state.gameBusy.includes(busyKey.pick)) {
      return;
    }
    const epoch = this.pickerEpoch;
    await this.gameCommand(
      () => (picker.kind === 'game' ? this.backend.addGame(id, chosen.path) : this.backend.addQuietApp(id, chosen.path)),
      { key: busyKey.pick },
    );
    // Si la personne a fermé la boîte (ou en a ouvert une autre) entre-temps, il n'y a plus rien à refermer.
    if (epoch === this.pickerEpoch) {
      this.closePicker();
    }
  }

  // ── Démarrage ────────────────────────────────────────────────────────────

  /**
   * À l'arrivée sur la page Démarrage, la vue est demandée une fois ; elle l'est
   * de nouveau quand la fenêtre retrouve le focus (la personne a pu régler
   * quelque chose dans Windows). Aucun sondage. Hors de la page, rien n'est
   * demandé ni gardé : la vue d'une visite ne sert pas à la suivante.
   */
  private watchStartup(): void {
    if (this.state.route !== 'startup') {
      this.stopWatchingStartup();
      return;
    }
    if (this.startupWatching) {
      // Déjà sur la page : on relit seulement la vue.
      void this.refreshStartup(true);
      return;
    }
    this.startupWatching = true;
    this.startupVisit += 1;
    this.startupLoading = false;
    this.startupLoadedAt = 0;
    this.startupRetried = false;
    this.clearSay();
    this.set({ startup: null, startupFailed: false, startupBusy: null });
    void this.refreshStartup(true);
  }

  private stopWatchingStartup(): void {
    if (!this.startupWatching) {
      return;
    }
    this.startupWatching = false;
    this.startupVisit += 1;
    this.startupLoading = false;
    this.startupRetried = false;
    this.clearSay();
    this.set({ startup: null, startupFailed: false, startupBusy: null });
    this.syncStartupPoll();
  }

  /**
   * Arme la relecture toutes les deux secondes tant que la vue dit qu'un changement du PC attend (même d'une
   * page rechargée : c'est le cœur qui le sait), et l'arrête dans tout autre cas, dont celui où ce geste-ci
   * attend lui-même : sa réponse suffit. Jamais armée deux fois ; la suivante part deux secondes après la réponse.
   */
  private syncStartupPoll(): void {
    const wanted =
      this.startupWatching && this.state.route === 'startup' && shouldPoll(this.state.startup, this.startupPending > 0);
    if (!wanted) {
      window.clearTimeout(this.startupTimer);
      this.startupTimer = undefined;
      return;
    }
    if (this.startupTimer !== undefined) {
      return;
    }
    this.startupTimer = window.setTimeout(() => {
      this.startupTimer = undefined;
      void this.refreshStartup(true).finally(() => this.syncStartupPoll());
    }, STARTUP_POLL_DELAY);
  }

  /** « Réessayer » sous le message d'erreur de la première vue. */
  private retryStartup(): void {
    this.set({ startupFailed: false });
    // Le bouton disparaît le temps du chargement : le focus ira au titre, ou reviendra au bouton si la panne dure.
    this.startupRetried = true;
    void this.refreshStartup(true);
  }

  /**
   * Demande la vue. Au retour de la fenêtre (`force` faux), une demande déjà en
   * vol, ou finie depuis moins d'une demi-seconde, n'est pas relancée : le focus
   * et la visibilité arrivent ensemble.
   */
  private async refreshStartup(force = false): Promise<void> {
    // Pas de demande pour une fenêtre cachée, ni pendant un changement : sa réponse est plus récente que toute vue.
    // Sauf pour une visite qui n'a encore rien : le cœur répond tout de suite, avec `pending` si le changement attend.
    if ((this.startupPending > 0 && this.state.startup !== null) || document.hidden || this.state.route !== 'startup') {
      return;
    }
    if (!force && (this.startupLoading || Date.now() - this.startupLoadedAt < STARTUP_RELOAD_GAP)) {
      return;
    }
    this.startupAsk += 1;
    const ask = this.startupAsk;
    const action = this.startupAction;
    const visit = this.startupVisit;
    this.startupLoading = true;
    const current = (): boolean =>
      ask === this.startupAsk && action === this.startupAction && visit === this.startupVisit;
    const finish = (): void => {
      if (ask === this.startupAsk) {
        this.startupLoading = false;
        this.startupLoadedAt = Date.now();
      }
    };
    try {
      const view = await this.backend.startupView();
      finish();
      if (current()) {
        this.adoptStartup(view);
      }
    } catch {
      finish();
      // La page garde sa dernière vue ; sans vue, elle le dit et propose « Réessayer ».
      if (current() && this.state.startup === null) {
        this.set({ startupFailed: true });
        if (this.startupRetried) {
          this.focusKey(startupFocus.retry);
        }
      }
    }
  }

  /**
   * Adopte une vue. Ce qui avait le focus est suivi : si sa ligne a disparu, le
   * focus passe à la ligne voisine, sinon au titre. Une phrase du retour que la
   * vue démentirait est effacée.
   */
  private adoptStartup(view: StartupView): void {
    const before = this.state.startup;
    // La clé du contrôle de ligne qui a le focus (interrupteur ou bouton de fin de ligne).
    const key = (document.activeElement as { dataset?: Record<string, string | undefined> } | null)?.dataset?.['fk'];
    const say = this.state.startupSay;
    const retried = this.startupRetried && before === null;
    this.set({ startup: view, startupFailed: false, now: Date.now() });
    if (retried) {
      this.startupRetried = false;
      this.focusKey(TITLE_KEY);
    }
    if (say !== null && sayStale(say, view)) {
      // Si le focus était dans la barre, il va à la ligne de la phrase, ou à sa voisine.
      const wanted = say.undo === null ? null : focusTarget(before, view, startupFocus.entry(say.undo.id));
      this.clearSay(wanted ?? TITLE_KEY);
    }
    const target = key === undefined ? null : focusTarget(before, view, key);
    if (target !== null) {
      this.focusKey(target);
    }
    this.syncStartupPoll();
  }

  /** Écrit dans la zone de retour ; elle s'efface seule au bout de 12 secondes, sauf tant qu'on la lit. */
  private say(say: Say | null): void {
    if (say === null) {
      this.clearSay();
      return;
    }
    this.set({ startupSay: say });
    this.armSay();
  }

  private armSay(): void {
    window.clearTimeout(this.startupSayTimer);
    if (!this.sayFocus && !this.sayHover && this.state.startupSay !== null) {
      this.startupSayTimer = window.setTimeout(() => this.clearSay(), SAY_DELAY);
    }
  }

  /**
   * Efface la phrase. Si la zone avait le focus, il retourne à `refocus` : par défaut l'interrupteur
   * de la ligne de la phrase, sinon sa voisine ou le titre.
   */
  private clearSay(refocus?: string): void {
    window.clearTimeout(this.startupSayTimer);
    const say = this.state.startupSay;
    const hadFocus = this.sayFocus;
    this.sayFocus = false;
    this.sayHover = false;
    if (say === null) {
      return;
    }
    this.set({ startupSay: null });
    if (hadFocus && say.undo !== null) {
      const view = this.state.startup;
      this.focusKey(
        refocus ?? (view === null ? TITLE_KEY : (focusTarget(view, view, startupFocus.entry(say.undo.id)) ?? TITLE_KEY)),
      );
    }
  }

  private holdStartupSay(by: 'focus' | 'hover', on: boolean): void {
    if (by === 'focus') {
      // Une fenêtre qui perd le focus système fait partir le focus de la barre sans que la personne l'ait
      // quittée : le focus « logique » y reste, et la minuterie reste suspendue jusqu'à sa vraie sortie.
      if (!on && typeof document.hasFocus === 'function' && !document.hasFocus()) {
        return;
      }
      this.sayFocus = on;
    } else {
      this.sayHover = on;
    }
    this.armSay();
  }

  /** Montre la ligne du geste : la zone de retour flotte au bas de la page et ne doit pas la cacher. */
  private revealStartupRow(id: string): void {
    queueMicrotask(() => {
      for (const element of this.root.querySelectorAll<HTMLElement>('[data-fk]')) {
        if (element.dataset['fk'] === startupFocus.entry(id)) {
          const row = element.closest('li') ?? element;
          if (typeof row.scrollIntoView === 'function') {
            row.scrollIntoView({ block: 'nearest' });
          }
          return;
        }
      }
    });
  }

  /**
   * Coupe ou remet une entrée du compte. Un seul changement à la fois : le
   * cœur en refuse un second. L'interrupteur bouge tout de suite ; si le cœur
   * refuse, il revient et la zone de retour le dit.
   */
  private async setStartupEntry(id: string, enabled: boolean): Promise<void> {
    const view = this.state.startup;
    const before = view === null ? undefined : entryById(view, id);
    if (view === null || before === undefined || !before.adjustable || this.startupPending > 0 || isWaiting(view)) {
      return;
    }
    if ((before.state === 'enabled') === enabled) {
      return;
    }
    const visit = this.startupVisit;
    this.startupPending += 1;
    this.startupAction += 1;
    // Une vue en vol est périmée par ce geste.
    this.startupLoading = false;
    // Le geste suivant efface la phrase du précédent.
    this.say(null);
    // Une ligne du PC ne bouge pas avant la réponse : Windows peut refuser, ou la personne annuler. Elle montre l'attente.
    const machine = before.scope === 'machine';
    this.set({
      startup: machine ? { ...view, pending: { id, enabled } } : withEntryEnabled(view, id, enabled, Date.now()),
      startupBusy: id,
    });
    const answered = (): boolean => {
      this.startupPending -= 1;
      this.startupAction += 1;
      this.startupLoadedAt = 0;
      if (visit !== this.startupVisit || this.state.route !== 'startup') {
        // La personne a quitté la page entre-temps : la page suivante repart de zéro.
        void this.refreshStartup(true);
        return false;
      }
      return true;
    };
    try {
      const answer: StartupView = await this.backend.setStartupEntry(id, enabled);
      if (answered()) {
        this.set({ startupBusy: null });
        this.adoptStartup(answer);
        // Le cœur a accepté, mais la vue montre l'entrée dans l'autre état : on ne dit pas ce qui n'est pas.
        const said = sayDone(before, enabled);
        this.say(sayStale(said, answer) ? { text: fr.startup.say.failed, undo: null } : said);
        this.focusKey(startupFocus.entry(id));
        this.revealStartupRow(id);
      }
    } catch (error) {
      if (answered()) {
        const current = this.state.startup;
        this.set({
          startup:
            current === null
              ? null
              : machine
                ? { ...current, pending: null }
                : withEntry(current, before),
          startupBusy: null,
        });
        this.say(sayRefused(error));
        this.focusKey(startupFocus.entry(id));
        this.revealStartupRow(id);
        void this.refreshStartup(true);
      }
    }
  }

  private undoStartup(): void {
    const undo = this.state.startupSay?.undo;
    if (undo !== null && undo !== undefined) {
      void this.setStartupEntry(undo.id, undo.enabled);
    }
  }

  // ── Désinstallation ──────────────────────────────────────────────────────

  /**
   * À l'arrivée sur la page Désinstallation, la vue est demandée une fois ; elle l'est de nouveau quand la
   * fenêtre retrouve le focus. Tant qu'une désinstallation est en cours (`launching`, `waiting`, `settling`),
   * elle l'est toutes les deux secondes, même si la personne est ailleurs : voir `syncAppsPoll`. Hors de la
   * page, rien n'est gardé, sauf cette attente : la vue d'une visite ne sert pas à la suivante.
   */
  private watchApps(): void {
    if (this.state.route !== 'uninstall') {
      this.stopWatchingApps();
      return;
    }
    if (this.appsWatching) {
      // Déjà sur la page : on relit seulement la vue.
      void this.refreshApps('force');
      return;
    }
    this.appsWatching = true;
    this.appsVisit += 1;
    this.appsLoading = false;
    this.appsLoadedAt = 0;
    this.appsRetried = false;
    if (isPolling(this.state.apps?.follow)) {
      // Une désinstallation est en cours : la page retrouve sa scène, et la relit tout de suite.
      this.set({ appsFailed: false });
    } else {
      this.dropApps();
    }
    void this.refreshApps('force');
  }

  private stopWatchingApps(): void {
    if (!this.appsWatching) {
      return;
    }
    this.appsWatching = false;
    this.appsVisit += 1;
    this.appsRetried = false;
    if (!isPolling(this.state.apps?.follow)) {
      this.dropApps();
    }
  }

  /** Oublie la vue et tout ce que la page en avait tiré : texte cherché, tri, cases, phrases. */
  private dropApps(): void {
    window.clearTimeout(this.appsTimer);
    this.appsTimer = undefined;
    this.set({
      apps: null,
      appsFailed: false,
      appsQuery: '',
      appsSort: 'name',
      appsConfirm: null,
      appsChecked: [],
      appsFailures: {},
      appsDestroyed: [],
      appsEmptied: false,
      appsAnnounce: '',
    });
  }

  /**
   * Arme la minuterie d'interrogation tant que la phase est `launching`, `waiting` ou `settling`, et
   * l'arrête dans toute autre. Elle n'est jamais armée deux fois : une demande à la fois, la suivante
   * part deux secondes après la réponse.
   */
  private syncAppsPoll(): void {
    if (!isPolling(this.state.apps?.follow)) {
      window.clearTimeout(this.appsTimer);
      this.appsTimer = undefined;
      return;
    }
    if (this.appsTimer !== undefined) {
      return;
    }
    this.appsTimer = window.setTimeout(() => {
      this.appsTimer = undefined;
      void this.refreshApps('poll');
    }, APPS_POLL_DELAY);
  }

  /**
   * Demande la vue. `poll` : le sondage de l'attente, jamais empilé. `focus` : le retour de la fenêtre, sans
   * demande si une est en vol, finie depuis moins d'une demi-seconde, ou inutile (l'attente interroge déjà).
   * `force` : une visite, « Réessayer », un geste refusé.
   */
  private async refreshApps(mode: 'poll' | 'focus' | 'force'): Promise<void> {
    if (mode !== 'force' && this.appsLoading) {
      return;
    }
    if (mode === 'focus') {
      const state = this.state;
      if (
        !this.appsWatching ||
        state.route !== 'uninstall' ||
        document.hidden ||
        state.appsBusy !== null ||
        isPolling(state.apps?.follow) ||
        Date.now() - this.appsLoadedAt < APPS_RELOAD_GAP
      ) {
        return;
      }
    }
    if (mode === 'poll' && !isPolling(this.state.apps?.follow)) {
      return;
    }
    this.appsAsk += 1;
    const ask = this.appsAsk;
    const action = this.appsAction;
    this.appsLoading = true;
    const current = (): boolean => ask === this.appsAsk && action === this.appsAction;
    const finish = (): void => {
      if (ask === this.appsAsk) {
        this.appsLoading = false;
        this.appsLoadedAt = Date.now();
      }
    };
    try {
      const view = await this.backend.appsView();
      finish();
      // Pendant « Désinstaller », le cœur peut encore répondre « rien en cours » : la page attend déjà.
      if (current() && !(this.state.appsBusy === 'start' && view.follow.phase === 'idle')) {
        this.adoptApps(view);
      } else {
        // Jetée parce qu'une autre demande est partie : son « kept » ne se redira pas.
        this.sayKept(view);
      }
    } catch {
      finish();
      // La page garde sa dernière vue ; sans vue, elle le dit et propose « Réessayer ».
      if (current() && this.state.apps === null) {
        this.set({ appsFailed: true });
        if (this.appsRetried) {
          this.focusKey(uninstallFocus.retry);
        }
      }
    } finally {
      this.syncAppsPoll();
    }
  }

  /**
   * Adopte une vue. Un changement de phase est annoncé une fois ; un changement d'écran (la liste, l'attente,
   * les restes) donne le focus au titre ; sur la liste, ce qui avait le focus est suivi.
   */
  private adoptApps(view: AppsView): void {
    const before = this.state.apps;
    const name = followedName(view.follow);
    if (name !== null) {
      this.appsLastName = name;
    }
    this.sayKept(view);
    if (!this.appsWatching && !isPolling(view.follow)) {
      // Hors de la page, la vue ne sert qu'à suivre l'attente : le reste se relit à la prochaine visite.
      if (before !== null) {
        this.dropApps();
      }
      return;
    }
    const wasPhase = before?.follow.phase ?? 'idle';
    const wasScreen = screenOf(before?.follow);
    const screen = screenOf(view.follow);
    const patch: Partial<AppState> = { apps: view, appsFailed: false, now: Date.now() };
    if (view.follow.phase !== wasPhase) {
      patch.appsAnnounce = announcement(view.follow);
    }
    if (view.follow.phase === 'removed' && wasPhase !== 'removed') {
      // Des restes tout neufs : rien n'est coché, aucun échec à montrer.
      patch.appsChecked = [];
      patch.appsFailures = {};
      patch.appsDestroyed = [];
      patch.appsEmptied = false;
    }
    const key = this.activeKey();
    const state = this.state;
    const shownBefore = before === null ? [] : listedApps(before, state.appsQuery, state.appsSort);
    const retried = this.appsRetried;
    this.appsRetried = false;
    this.set(patch);
    this.syncAppsPoll();
    if (retried) {
      // « Réessayer » a disparu avec son message : le focus va au titre (s'il est encore au bouton, il n'y bouge pas).
      this.focusKey(uninstallFocus.title);
    }
    if (!this.appsWatching || state.route !== 'uninstall') {
      return;
    }
    if (before !== null && screen !== wasScreen) {
      this.focusTitle();
    } else if (screen === 'list' && key !== null && before !== null) {
      const target = appsFocusTarget(shownBefore, listedApps(view, state.appsQuery, state.appsSort), key);
      if (target !== null) {
        this.focusKey(target);
      }
    }
  }

  /** Le cœur ne dit `outcome: 'kept'` qu'une fois : la réponse qui le porte le dit, même si la page ne la garde pas. */
  private sayKept(view: AppsView): void {
    if (view.outcome === 'kept') {
      this.toast(this.appsLastName === null ? fr.uninstall.toast.keptUnknown : fr.uninstall.toast.kept(this.appsLastName));
    }
  }

  /** La clé de focus (`data-fk`) de ce qui a le focus, ou `null`. */
  private activeKey(): string | null {
    const active = document.activeElement as { dataset?: Record<string, string | undefined> } | null;
    return active?.dataset?.['fk'] ?? null;
  }

  /**
   * Le titre prend le focus quand l'écran change (la liste, l'attente, les restes) : ce qui l'avait a disparu,
   * ou le focus est ailleurs dans la page. Il ne quitte pas le menu ni une boîte ouverte.
   */
  private focusTitle(): void {
    queueMicrotask(() => {
      const active = document.activeElement;
      const main = this.root.querySelector('.main');
      const inPage = active !== null && active !== document.body && main?.contains(active) === true;
      if (active !== null && active !== document.body && !inPage) {
        return;
      }
      for (const element of this.root.querySelectorAll<HTMLElement>('[data-fk]')) {
        if (element.dataset['fk'] === uninstallFocus.title) {
          element.focus();
          return;
        }
      }
    });
  }

  /** Le focus va au premier de ces contrôles qui existe, si personne ne l'a pris. */
  private focusAny(keys: readonly string[]): void {
    queueMicrotask(() => {
      const active = document.activeElement;
      if (active !== null && active !== document.body) {
        return;
      }
      for (const key of keys) {
        for (const element of this.root.querySelectorAll<HTMLElement>('[data-fk]')) {
          if (element.dataset['fk'] === key) {
            element.focus();
            return;
          }
        }
      }
    });
  }

  /** « Réessayer » : la liste des apps n'est pas venue, ou pas en entier. */
  private retryApps(): void {
    this.set({ appsFailed: false });
    // Le bouton disparaît le temps du chargement : le focus ira au titre, ou reviendra au bouton si la panne dure.
    this.appsRetried = true;
    void this.refreshApps('force');
  }

  private askUninstall(id: string): void {
    const view = this.state.apps;
    const app = appById(view, id);
    if (app === undefined || app.removal !== 'offered' || this.state.appsBusy !== null || view?.follow.phase !== 'idle') {
      return;
    }
    this.set({ appsConfirm: askFor(app) });
  }

  private dismissUninstall(): void {
    // Une fois la demande partie, la confirmation est déjà fermée : cette fonction ne l'arrête pas.
    if (this.state.appsConfirm !== null) {
      this.set({ appsConfirm: null });
    }
  }

  /**
   * La personne a confirmé : la page montre tout de suite `launching` (le cœur n'y passe qu'avant de
   * lancer, et `start_uninstall` ne rend la main qu'après la réponse à la fenêtre de Windows). Un second
   * geste est refusé tant que la réponse n'est pas là.
   */
  private async confirmUninstall(): Promise<void> {
    const ask = this.state.appsConfirm;
    const before = this.state.apps;
    if (ask === null || this.state.appsBusy !== null) {
      return;
    }
    if (before === null || before.follow.phase !== 'idle') {
      this.set({ appsConfirm: null });
      return;
    }
    // Une vue en vol est périmée par ce geste.
    this.appsAction += 1;
    this.appsLoading = false;
    this.appsLastName = ask.name;
    const follow = { phase: 'launching', id: ask.id, name: ask.name } as const;
    this.set({
      appsConfirm: null,
      appsBusy: 'start',
      apps: { ...before, follow },
      appsAnnounce: announcement(follow),
    });
    this.syncAppsPoll();
    this.focusTitle();
    try {
      const answer = await this.backend.startUninstall(ask.id);
      this.appsAction += 1;
      this.set({ appsBusy: null });
      this.adoptApps(answer);
    } catch (error) {
      this.appsAction += 1;
      const refusal = refusalOf(error, ask.name);
      const current = this.state.apps;
      // La région vivante revient à la liste : une seconde tentative sur la même app s'annonce de nouveau.
      this.set({
        appsBusy: null,
        apps: current === null ? null : { ...current, follow: before.follow },
        appsAnnounce: announcement(before.follow),
      });
      this.syncAppsPoll();
      if (refusal.toast !== null) {
        this.toast(refusal.toast);
      }
      if (refusal.reload) {
        await this.refreshApps('force');
      }
      // La liste est revenue : le focus retrouve le bouton de l'app, ou le titre si elle n'y est plus.
      this.focusAny([uninstallFocus.row(ask.id), uninstallFocus.title]);
    }
  }

  /** « Revenir à la liste » et « Terminé » : rien n'est tué ni touché, le cœur oublie seulement la désinstallation. */
  private async stopUninstall(): Promise<void> {
    if (this.state.appsBusy !== null || screenOf(this.state.apps?.follow) === 'list') {
      return;
    }
    this.appsAction += 1;
    this.appsLoading = false;
    this.set({ appsBusy: 'stop' });
    try {
      const answer = await this.backend.stopUninstall();
      this.appsAction += 1;
      this.set({ appsBusy: null });
      this.adoptApps(answer);
    } catch (error) {
      this.appsAction += 1;
      this.set({ appsBusy: null });
      this.toast(errorMessage(error));
      await this.refreshApps('force');
    }
  }

  /** « Chercher à nouveau » : les restes sont cherchés de nouveau, les cases repartent vides. */
  private async rescanLeftovers(): Promise<void> {
    if (this.state.appsBusy !== null || this.state.apps?.follow.phase !== 'removed') {
      return;
    }
    this.appsAction += 1;
    this.appsLoading = false;
    this.set({ appsBusy: 'rescan' });
    try {
      const answer = await this.backend.rescanLeftovers();
      this.appsAction += 1;
      this.set({ appsBusy: null, appsChecked: [], appsFailures: {}, appsDestroyed: [], appsEmptied: false });
      this.adoptApps(answer);
    } catch (error) {
      this.appsAction += 1;
      this.set({ appsBusy: null });
      this.toast(errorMessage(error));
      await this.refreshApps('force');
    }
  }

  private toggleLeftover(path: string): void {
    const view = this.state.apps;
    if (this.state.appsBusy !== null || this.state.appsFailures[path] === 'destroyed' || view === null || !proposed(view.follow).some((row) => row.path === path)) {
      return;
    }
    const checked = this.state.appsChecked;
    this.set({ appsChecked: checked.includes(path) ? checked.filter((one) => one !== path) : [...checked, path] });
  }

  /** L'orbe : seules les clés des restes cochés partent, jamais un chemin. Ce qui ne part pas reste dans la liste. */
  private async removeLeftovers(): Promise<void> {
    const view = this.state.apps;
    if (this.state.appsBusy !== null || view === null || view.follow.phase !== 'removed') {
      return;
    }
    const rows = proposed(view.follow);
    const picked = checkedRows(checkable(rows, this.state.appsFailures), this.state.appsChecked);
    if (picked.length === 0) {
      return;
    }
    this.appsAction += 1;
    this.appsLoading = false;
    this.set({ appsBusy: 'remove' });
    try {
      const answer = await this.backend.removeLeftovers(picked.map((row) => row.key));
      this.appsAction += 1;
      const removal = removalOf(rows, answer);
      const after = proposed(answer.view.follow);
      // Ceux que Windows a supprimés pour de bon ne sont plus dans la vue : la page les garde, avec leur raison.
      const stillThere = (row: LeftoverRow): boolean => after.some((one) => one.path === row.path);
      const destroyed = [
        ...this.state.appsDestroyed.filter((row) => !removal.settled.includes(row.path) && !stillThere(row)),
        ...removal.destroyed.filter((row) => !stillThere(row)),
      ];
      const failures = mergedFailures(this.state.appsFailures, removal, [...after, ...destroyed]);
      this.set({
        appsBusy: null,
        appsChecked: [],
        appsDestroyed: destroyed,
        appsFailures: failures,
        appsEmptied: noneBecause(answer.view.follow) === 'none',
      });
      this.adoptApps(answer.view);
      const said = removalToast(removal);
      if (said !== null) {
        this.toast(said);
      }
      if (this.appsWatching && this.state.route === 'uninstall' && screenOf(answer.view.follow) === 'leftovers') {
        // L'orbe est inactif : le focus va au premier reste qui est encore là (avec sa raison), sinon à « Terminé ».
        const target = focusAfterRemoval(checkable(after, failures));
        this.focusAny([target]);
        this.forceFocus(target);
      }
    } catch (error) {
      this.appsAction += 1;
      this.set({ appsBusy: null });
      this.toast(errorMessage(error));
      await this.refreshApps('force');
    }
  }

  /** Donne le focus à ce contrôle même si l'orbe, devenu inactif, l'a encore. */
  private forceFocus(key: string): void {
    queueMicrotask(() => {
      const active = document.activeElement as { dataset?: Record<string, string | undefined> } | null;
      if (active?.dataset?.['fk'] !== uninstallFocus.orb) {
        return;
      }
      for (const element of this.root.querySelectorAll<HTMLElement>('[data-fk]')) {
        if (element.dataset['fk'] === key) {
          element.focus();
          return;
        }
      }
    });
  }

  private setAppsSort(sort: AppsSort): void {
    if (sort !== this.state.appsSort) {
      this.set({ appsSort: sort });
    }
  }

  private async openWindowsSettings(page: WindowsPage): Promise<void> {
    try {
      await this.backend.openWindowsSettings(page);
    } catch (error) {
      this.toast(errorMessage(error));
    }
  }

  // ── Gestes ───────────────────────────────────────────────────────────────

  private navigate(route: Route): void {
    this.set({ route });
    this.watchGame();
    this.watchStartup();
    this.watchApps();
    this.ensureData();
    if (route === 'bin') {
      void this.refreshOverview(2000);
    }
  }

  private readonly actions: Actions = {
    navigate: (route) => this.navigate(route),
    startScan: () => void this.startScan(),
    cancelScan: () => void this.cancelScan(),
    showScan: () => this.set({ route: 'home', pane: 'scan' }),
    showResults: () => {
      if (this.state.session.summary !== null) {
        this.set({ route: 'home', pane: 'results' });
        this.ensureData();
      }
    },
    showHome: () => this.set({ route: 'home', pane: 'home' }),
    toggle: (row) => this.set({ selection: toggled(this.state.session.summary, this.state.selection, row) }),
    askClean: () => void this.askClean(),
    confirmClean: () => void this.confirmClean(),
    dismissClean: () => this.dismissClean(),
    stopClean: () => void this.stopClean(),
    closeReport: () => void this.cancelScan(),
    loadMore: (list) => void this.loadMore(list),
    keepDuplicate: (choice) => void this.keepDuplicate(choice),
    setTheme: (theme) => void this.setTheme(theme),
    addFolder: () => void this.addFolder(),
    removeFolder: (path) => void this.changeFolders(() => this.backend.removeExcludedFolder(path)),
    acknowledgeIssue: () => void this.acknowledgeIssue(),
    setGameOption: (option, on) => void this.setGameOption(option, on),
    setQuietApp: (exe, on) => void this.setQuietApp(exe, on),
    releaseGame: () => void this.releaseGame(),
    setGameStatus: (path: string, status: GameStatus) => void this.setGameStatus(path, status),
    openPicker: (kind) => void this.openPicker(kind),
    closePicker: () => this.closePicker(),
    pickOpenApp: (id) => void this.pickOpenApp(id),
    setStartupEntry: (id, enabled) => void this.setStartupEntry(id, enabled),
    undoStartup: () => this.undoStartup(),
    holdStartupSay: (by, on) => this.holdStartupSay(by, on),
    retryStartup: () => this.retryStartup(),
    setAppsQuery: (query) => this.set({ appsQuery: query }),
    setAppsSort: (sort) => this.setAppsSort(sort),
    retryApps: () => this.retryApps(),
    askUninstall: (id) => this.askUninstall(id),
    confirmUninstall: () => void this.confirmUninstall(),
    dismissUninstall: () => this.dismissUninstall(),
    stopUninstall: () => void this.stopUninstall(),
    rescanLeftovers: () => void this.rescanLeftovers(),
    toggleLeftover: (path) => this.toggleLeftover(path),
    removeLeftovers: () => void this.removeLeftovers(),
    openWindowsSettings: (page) => void this.openWindowsSettings(page),
    minimize: () => void this.backend.minimizeWindow(),
    toggleMaximize: () => void this.backend.toggleMaximizeWindow(),
    close: () => void this.backend.closeWindow(),
  };
}
