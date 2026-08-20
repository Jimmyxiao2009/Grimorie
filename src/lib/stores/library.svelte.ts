/**
 * The Library shelf.
 *
 * Owns the list of Volumes and nothing else. Every mutation goes to the backend
 * first and adopts what comes back — there is no optimistic local edit, because
 * a shelf that shows a Volume the database rejected is a shelf that lies.
 */

import * as library from '$lib/services/library';
import { notices } from './notices.svelte';
import type { Shelf, VolumeSummary } from '$lib/types/manuscript';

/** Volumes shown in the "Continue" strip. */
export const CONTINUE_COUNT = 3;

class LibraryStore {
  shelf = $state<Shelf>('active');
  volumes = $state<VolumeSummary[]>([]);
  loading = $state(false);
  /** Set when the shelf itself could not be read, as opposed to one action. */
  failure = $state<string | null>(null);

  /**
   * Recently opened Volumes, for the strip above the shelf.
   *
   * Empty until there are enough Volumes for the distinction to mean anything —
   * a "Continue" row that repeats the only two Volumes below it is clutter.
   */
  readonly recent = $derived(
    this.shelf === 'active' && this.volumes.length > CONTINUE_COUNT
      ? this.volumes.filter((volume) => volume.lastOpenedAt !== null).slice(0, CONTINUE_COUNT)
      : []
  );

  readonly isEmpty = $derived(!this.loading && this.volumes.length === 0 && !this.failure);

  async load(shelf: Shelf = this.shelf): Promise<void> {
    this.shelf = shelf;
    this.loading = true;
    this.failure = null;
    try {
      this.volumes = await library.listVolumes(shelf);
    } catch (error) {
      this.volumes = [];
      this.failure =
        error instanceof Error
          ? error.message
          : 'Grimoire couldn’t read your library.';
    } finally {
      this.loading = false;
    }
  }

  async create(title: string, subtitle?: string): Promise<string | null> {
    try {
      const volume = await library.createVolume(title, subtitle);
      await this.load();
      return volume.id;
    } catch (error) {
      notices.failure(error, () => void this.create(title, subtitle));
      return null;
    }
  }

  async rename(id: string, title: string, subtitle?: string | null): Promise<boolean> {
    try {
      await library.updateVolume(id, title, subtitle);
      await this.load();
      return true;
    } catch (error) {
      notices.failure(error);
      return false;
    }
  }

  async setArchived(id: string, archived: boolean): Promise<void> {
    try {
      await library.setVolumeArchived(id, archived);
      await this.load();
    } catch (error) {
      notices.failure(error);
    }
  }

  async duplicate(id: string): Promise<void> {
    try {
      await library.duplicateVolume(id);
      await this.load();
    } catch (error) {
      notices.failure(error);
    }
  }

  async remove(id: string): Promise<void> {
    try {
      await library.deleteVolume(id);
      await this.load();
    } catch (error) {
      notices.failure(error);
    }
  }
}

export const libraryStore = new LibraryStore();
