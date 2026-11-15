// L'application : elle tient l'état, parle au cœur et redessine les écrans.
//
// Un seul sens de circulation : un geste appelle une action, l'action
// interroge le cœur puis change l'état, et l'état redessine tout. Pendant une
// analyse ou un grand vide, l'application redemande une vue de la session
// quelques fois par seconde ; le cœur ne pousse jamais rien vers la fenêtre.

import { render } from 'lit-html';

import type {
  Backend,
  BackendError,
  DuplicateGroup,
  DuplicatesPage,
  GameOption,
  GameStatus,
  GameView,
  KeepChoice,
  SessionView,
  Settings,
  Theme,
  WindowsPage,
} from './backend/types.ts';
import { fr } from './i18n/fr.ts';
import { cleanRequest, confirmedRequest, readiness, requestedSteps } from './model/clean.ts';
import {
  busyKey,
  focusKey,
  GAME_POLL_DELAY,
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
  /**
   * Change au départ et à la réception de chaque geste du mode jeu : une vue
   * demandée avant l'un ou l'autre et arrivée après est ignorée.
   */
  private gameEpoch = 0;
  /** Le numéro du dernier geste parti : seule sa réponse change la vue. */
  private gameSeq = 0;
  /** Les gestes du mode jeu en vol : tant qu'il y en a, aucune vue demandée par sondage n'est adoptée. */
  private gamePending = 0;
  private gameLoading = false;
  /** Les demandes de la vue du mode jeu échouées de suite. */
  private gameFailures = 0;
  /** « Tout remettre maintenant » est parti : la page tient la partie pour remise en place le temps que l'agent le note. */
  private releasing: Releasing | null = null;
  /** Change à chaque ouverture ou fermeture du sélecteur : une liste arrivée trop tard est ignorée. */
  private pickerEpoch = 0;

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
    });
    // Une fenêtre réduite ou cachée ne demande rien ; elle relit la vue dès qu'on la regarde de nouveau.
    document.addEventListener('visibilitychange', () => void this.refreshGame());
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
      optimistic?: (view: GameView) => GameView;
      undo?: (view: GameView) => GameView;
    } = {},
  ): Promise<boolean> {
    const { key, optimistic, undo } = options;
    if (key !== undefined) {
      if (this.state.gameBusy.includes(key)) {
        return false;
      }
      this.set({ gameBusy: [...this.state.gameBusy, key] });
    }
    this.gamePending += 1;
    this.gameEpoch += 1;
    this.gameSeq += 1;
    const mine = this.gameSeq;
    const before = this.state.game;
    if (optimistic !== undefined && before !== null) {
      this.set({ game: optimistic(before) });
    }
    // La réponse arrive : un sondage parti avant elle serait plus vieux qu'elle.
    const answered = (): void => {
      this.gamePending -= 1;
      this.gameEpoch += 1;
      if (key !== undefined) {
        this.set({ gameBusy: this.state.gameBusy.filter((other) => other !== key) });
      }
    };
    try {
      const view = await request();
      answered();
      if (mine === this.gameSeq) {
        this.adoptGame(view);
      }
      return true;
    } catch (error) {
      answered();
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
    const done = await this.gameCommand(() => this.backend.setGameStatus(path, status), {
      key: busyKey.status(path),
    });
    if (done) {
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
    openWindowsSettings: (page) => void this.openWindowsSettings(page),
    minimize: () => void this.backend.minimizeWindow(),
    toggleMaximize: () => void this.backend.toggleMaximizeWindow(),
    close: () => void this.backend.closeWindow(),
  };
}
