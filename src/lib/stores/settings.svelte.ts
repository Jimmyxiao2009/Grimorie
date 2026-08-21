/**
 * Application settings.
 *
 * SQLite is the source of truth. This store holds the loaded copy, pushes
 * appearance values into the design layer, and writes changes back — adopting
 * whatever the backend actually stored, since it clamps what it is given.
 */

import { loadSettings, saveSettings } from '$lib/services/settings';
import { appearance, type ManuscriptFace, type ThemeChoice } from '$lib/design/theme.svelte';
import { notices } from './notices.svelte';
import type { AppSettings } from '$lib/types/manuscript';

/**
 * Mirrors `AppSettings::default()` in Rust.
 *
 * Used only for the moment between launch and the first load completing, so
 * the UI has coherent values to render rather than nulls.
 */
export const DEFAULT_SETTINGS: AppSettings = {
  theme: 'system',
  manuscriptFace: 'serif',
  manuscriptSize: 1.125,
  manuscriptLeading: 1.7,
  manuscriptParagraphGap: 0.85,
  manuscriptMeasure: 36,
  autosaveDebounceMs: 900,
  revisionIntervalSeconds: 300,
  spellcheck: true,
  aiEnabled: false,
  aiContextBudgetChars: 8000,
  inkAutoRecognition: false,
  inkRecognitionModel: '',
  inkRecognitionLanguage: 'auto'
};

class SettingsStore {
  settings = $state<AppSettings>({ ...DEFAULT_SETTINGS });
  loaded = $state(false);

  async load(): Promise<void> {
    try {
      this.adopt(await loadSettings());
    } catch {
      // Settings failing to load must not stop someone reaching their
      // manuscripts. Defaults are always usable, and the appearance layer has
      // already painted from its own cache.
      this.adopt({ ...DEFAULT_SETTINGS });
    } finally {
      this.loaded = true;
    }
  }

  /** Applies a change immediately, then persists it. */
  async update(patch: Partial<AppSettings>): Promise<void> {
    const next = { ...this.settings, ...patch };
    // Applied first so the change is visible at once; the backend's clamped
    // answer replaces it a moment later.
    this.adopt(next);
    try {
      this.adopt(await saveSettings(next));
    } catch (error) {
      notices.failure(error);
    }
  }

  private adopt(settings: AppSettings): void {
    this.settings = settings;
    appearance.setChoice(settings.theme as ThemeChoice);
    appearance.setTypography({
      face: settings.manuscriptFace as ManuscriptFace,
      size: settings.manuscriptSize,
      leading: settings.manuscriptLeading,
      paragraphGap: settings.manuscriptParagraphGap,
      measure: settings.manuscriptMeasure
    });
  }
}

export const settingsStore = new SettingsStore();
