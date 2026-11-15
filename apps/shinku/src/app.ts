// L'application : elle tient l'état, parle au cœur et redessine les écrans.
//
// Un seul sens de circulation : un geste appelle une action, l'action
// interroge le cœur puis change l'état, et l'état redessine tout. Pendant une
// analyse ou un grand vide, l'application redemande une vue de la session
// quelques fois par seconde ; le cœur ne pousse jamais rien vers la fenêtre.

import { render } from 'lit-html';

import type { Backend, BackendError, SessionView, Settings, Theme } from './backend/types.ts';
import { fr } from './i18n/fr.ts';
import { cleanRequest, confirmedRequest, readiness, requestedSteps } from './model/clean.ts';
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
  setTheme: () => {},
  addFolder: () => {},
  removeFolder: () => {},
  acknowledgeIssue: () => {},
  minimize: () => {},
  toggleMaximize: () => {},
  close: () => {},
};

/** La page a été ouverte hors de Shinku : on l'explique, et rien d'autre. */
export function renderOutside(root: HTMLElement): void {
  render(appView({ ...initialState(Date.now()), boot: 'outside' }, NO_ACTIONS), root);
}

export class App {
  private state: AppState = initialState(Date.now());
  private readonly backend: Backend;
  private readonly root: HTMLElement;
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

  constructor(backend: Backend, root: HTMLElement) {
    this.backend = backend;
    this.root = root;
  }

  // ── État et dessin ───────────────────────────────────────────────────────

  private set(patch: Partial<AppState>): void {
    this.state = { ...this.state, ...patch };
    if (!this.renderQueued) {
      this.renderQueued = true;
      queueMicrotask(() => {
        this.renderQueued = false;
        render(appView(this.state, this.actions), this.root);
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
    render(appView(this.state, this.actions), this.root);
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
    window.addEventListener('focus', () => void this.refreshOverview(5000));
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
    if (this.state.loadingMore) {
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
    if (this.state.cleanBusy || this.state.confirm !== null) {
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

  // ── Gestes ───────────────────────────────────────────────────────────────

  private navigate(route: Route): void {
    this.set({ route });
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
    setTheme: (theme) => void this.setTheme(theme),
    addFolder: () => void this.addFolder(),
    removeFolder: (path) => void this.changeFolders(() => this.backend.removeExcludedFolder(path)),
    acknowledgeIssue: () => void this.acknowledgeIssue(),
    minimize: () => void this.backend.minimizeWindow(),
    toggleMaximize: () => void this.backend.toggleMaximizeWindow(),
    close: () => void this.backend.closeWindow(),
  };
}
