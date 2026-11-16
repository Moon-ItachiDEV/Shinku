// Le guichet réel : les commandes de l'application Tauri (src-tauri/src/main.rs).

import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type {
  AppsView,
  Backend,
  CleanPreview,
  DuplicatesPage,
  ErasedTraces,
  GameView,
  HistoryView,
  JunkReport,
  LargeFilesPage,
  LicencesOpened,
  OpenApp,
  Overview,
  PrivacyView,
  RemoveLeftovers,
  SessionView,
  Settings,
  StartupView,
  Theme,
} from './types.ts';

/** Vrai si la page tourne dans la fenêtre de Shinku. */
export function insideTauri(): boolean {
  return '__TAURI_INTERNALS__' in window;
}

export function tauriBackend(): Backend {
  const appWindow = getCurrentWindow();
  return {
    overview: () => invoke<Overview>('overview'),
    acknowledgeSettingsIssue: () => invoke<Overview>('acknowledge_settings_issue'),
    startScan: () => invoke<SessionView>('start_scan'),
    cancelScan: () => invoke<SessionView>('cancel_scan'),
    sessionView: () => invoke<SessionView>('session_view'),
    cleanPreview: () => invoke<CleanPreview>('clean_preview'),
    startClean: (request) => invoke<SessionView>('start_clean', { request }),
    cancelClean: () => invoke<SessionView>('cancel_clean'),
    junk: () => invoke<JunkReport | null>('junk_details'),
    largeFiles: (offset, limit) => invoke<LargeFilesPage | null>('large_files_page', { offset, limit }),
    duplicates: (offset, limit) => invoke<DuplicatesPage | null>('duplicates_page', { offset, limit }),
    keepDuplicate: (choice) => invoke<boolean>('keep_duplicate', { choice }),
    setTheme: (theme: Theme) => invoke<Settings>('set_theme', { theme }),
    addExcludedFolder: () => invoke<Settings | null>('add_excluded_folder'),
    removeExcludedFolder: (path) => invoke<Settings>('remove_excluded_folder', { path }),
    gameView: () => invoke<GameView>('game_view'),
    setGameOption: (option, on) => invoke<GameView>('set_game_option', { option, on }),
    setQuietApp: (exe, on) => invoke<GameView>('set_quiet_app', { exe, on }),
    openApps: () => invoke<OpenApp[]>('open_apps'),
    addGame: (id, path) => invoke<GameView>('add_game', { id, path }),
    addQuietApp: (id, path) => invoke<GameView>('add_quiet_app', { id, path }),
    setGameStatus: (path, status) => invoke<GameView>('set_game_status', { path, status }),
    releaseGame: () => invoke<GameView>('release_game'),
    startupView: () => invoke<StartupView>('startup_view'),
    setStartupEntry: (id, enabled) => invoke<StartupView>('set_startup_entry', { id, enabled }),
    appsView: () => invoke<AppsView>('apps_view'),
    startUninstall: (id) => invoke<AppsView>('start_uninstall', { id }),
    stopUninstall: () => invoke<AppsView>('stop_uninstall'),
    rescanLeftovers: () => invoke<AppsView>('rescan_leftovers'),
    removeLeftovers: (keys) => invoke<RemoveLeftovers>('remove_leftovers', { keys }),
    privacyView: () => invoke<PrivacyView>('privacy_view'),
    eraseTraces: (ids) => invoke<ErasedTraces>('erase_traces', { ids }),
    historyView: () => invoke<HistoryView>('history_view'),
    forgetUninstalls: () => invoke<HistoryView>('forget_uninstalls'),
    openLicences: () => invoke<LicencesOpened>('open_licences'),
    openWindowsSettings: (page) => invoke<void>('open_windows_settings', { page }),
    minimizeWindow: () => appWindow.minimize(),
    toggleMaximizeWindow: () => appWindow.toggleMaximize(),
    closeWindow: () => appWindow.close(),
  };
}
