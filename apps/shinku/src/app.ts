// L'application : elle tient l'état, parle au cœur et redessine les écrans.
//
// Un seul sens de circulation : un geste appelle une action, l'action
// interroge le cœur puis change l'état, et l'état redessine tout. Pendant une
// analyse, l'application redemande une vue de la session quelques fois par
// seconde ; le cœur ne pousse jamais rien vers la fenêtre.

import { render } from 'lit-html';

import type { Backend, BackendError, SessionView, Settings, Theme } from './backend/types.ts';
import { fr } from './i18n/fr.ts';
import { reportKey } from './model/scan.ts';
import { type Actions, type AppState, initialState, type Route } from './state.ts';
import { applyTheme } from './theme.ts';
import { appView } from './views/shell.ts';

/** Délai entre deux vues de session pendant une analyse, en millisecondes. */
const POLL_DELAY = 150;
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
   * Change à chaque geste qui lance ou arrête une analyse. Une vue de session
   * demandée avant ce geste et arrivée après lui est ignorée.
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
    // L'heure avance, et le disque change pendant que la fenêtre n'est pas regardée.
    window.setInterval(() => this.set({ now: Date.now() }), 30_000);
    window.addEventListener('focus', () => void this.refreshOverview(5000));
  }

  private async refreshOverview(minimumAge = 0): Promise<void> {
    if (Date.now() - this.lastOverviewRefresh < minimumAge) {
      return;
    }
    this.lastOverviewRefresh = Date.now();
    try {
      this.set({ overview: await this.backend.overview() });
    } catch {
      // La jauge garde ses derniers chiffres.
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

    if (session.state === 'idle' || session.state === 'failed') {
      // Ni analyse à suivre, ni résultats à montrer.
      patch.pane = 'home';
    } else if (this.state.pane === 'results' && session.summary === null) {
      patch.pane = 'scan';
    }
    if (session.state === 'failed' && previous.state !== 'failed') {
      this.toast(fr.toast.scanFailed);
    }
    this.set(patch);

    if (session.state === 'scanning') {
      void this.poll();
    } else if (previous.state === 'scanning') {
      // L'analyse vient de finir : la corbeille et le disque ont pu bouger.
      void this.refreshOverview();
    }
    this.ensureData();
  }

  private async poll(): Promise<void> {
    if (this.polling) {
      return;
    }
    this.polling = true;
    try {
      while (this.state.session.state === 'scanning') {
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
    if (session?.state === 'scanning') {
      // « Grand vide » montre désormais l'analyse, d'où qu'elle ait été lancée.
      this.set({ pane: 'scan' });
    }
  }

  private async cancelScan(): Promise<void> {
    await this.command(() => this.backend.cancelScan());
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
    toggle: (row) => this.set({ selection: { ...this.state.selection, [row]: !this.state.selection[row] } }),
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
