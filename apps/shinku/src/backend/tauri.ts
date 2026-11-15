// Le guichet réel : les commandes de l'application Tauri (src-tauri/src/main.rs).

import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type {
  Backend,
  CleanPreview,
  DuplicatesPage,
  JunkReport,
  LargeFilesPage,
  Overview,
  SessionView,
  Settings,
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
    setTheme: (theme: Theme) => invoke<Settings>('set_theme', { theme }),
    addExcludedFolder: () => invoke<Settings | null>('add_excluded_folder'),
    removeExcludedFolder: (path) => invoke<Settings>('remove_excluded_folder', { path }),
    minimizeWindow: () => appWindow.minimize(),
    toggleMaximizeWindow: () => appWindow.toggleMaximize(),
    closeWindow: () => appWindow.close(),
  };
}
