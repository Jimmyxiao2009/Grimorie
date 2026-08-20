/**
 * Volume operations.
 *
 * A typed wrapper per backend command, and nothing more — no caching, no
 * merging, no optimistic state. Stores own state; this module owns the
 * contract.
 */

import { invoke } from './ipc';
import type { Outline, Shelf, Volume, VolumeSummary } from '$lib/types/manuscript';

export function listVolumes(shelf: Shelf, limit?: number): Promise<VolumeSummary[]> {
  return invoke('volumes_list', { shelf, limit: limit ?? null });
}

export function createVolume(
  title: string,
  subtitle?: string,
  description?: string
): Promise<Volume> {
  return invoke('volume_create', {
    title,
    subtitle: subtitle?.trim() || null,
    description: description?.trim() || null
  });
}

export function getVolume(id: string): Promise<Volume> {
  return invoke('volume_get', { id });
}

/** Opens a Volume: returns its outline and records the visit, in one call. */
export function openVolume(id: string): Promise<Outline> {
  return invoke('volume_open', { id });
}

/** Re-reads a Volume's structure without recording another visit. */
export function refreshOutline(id: string): Promise<Outline> {
  return invoke('volume_outline', { id });
}

export function updateVolume(
  id: string,
  title: string,
  subtitle?: string | null,
  description?: string | null
): Promise<Volume> {
  return invoke('volume_update', {
    id,
    title,
    subtitle: subtitle?.trim() || null,
    description: description?.trim() || null
  });
}

export function setVolumeArchived(id: string, archived: boolean): Promise<Volume> {
  return invoke('volume_set_archived', { id, archived });
}

export function duplicateVolume(id: string): Promise<Volume> {
  return invoke('volume_duplicate', { id });
}

export function deleteVolume(id: string): Promise<void> {
  return invoke('volume_delete', { id });
}
