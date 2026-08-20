import { invoke } from './ipc';
import type { AppSettings } from '$lib/types/manuscript';

export function loadSettings(): Promise<AppSettings> {
  return invoke('settings_get');
}

/**
 * Saves settings and returns what was actually stored.
 *
 * Values are clamped by the backend, so the caller must adopt the returned
 * settings rather than assume the request was taken verbatim.
 */
export function saveSettings(settings: AppSettings): Promise<AppSettings> {
  return invoke('settings_save', { settings });
}
