/**
 * Bookmarks for the open Volume.
 *
 * The set of marked Page ids is kept separately from the full list, because the
 * tree needs to ask "is this one marked?" for every row it draws and should not
 * search a list of records to find out.
 */

import * as service from '$lib/services/bookmarks';
import { notices } from './notices.svelte';
import type { Bookmark } from '$lib/services/bookmarks';

class BookmarkStore {
  /** Page ids marked in the open Volume. */
  marked = $state<Set<string>>(new Set());
  /** The full list, loaded when the Bookmarks view is opened. */
  entries = $state<Bookmark[]>([]);
  loading = $state(false);

  private volumeId: string | null = null;

  isMarked(pageId: string | null): boolean {
    return pageId !== null && this.marked.has(pageId);
  }

  async loadForVolume(volumeId: string): Promise<void> {
    this.volumeId = volumeId;
    try {
      this.marked = new Set(await service.bookmarkedPages(volumeId));
    } catch {
      // A failed load leaves the tree unmarked, which is wrong but harmless;
      // interrupting the writer over a decoration would not be.
      this.marked = new Set();
    }
  }

  async loadAll(volumeId?: string | null): Promise<void> {
    this.loading = true;
    try {
      this.entries = await service.listBookmarks(volumeId ?? null);
    } catch (error) {
      this.entries = [];
      notices.failure(error);
    } finally {
      this.loading = false;
    }
  }

  /** Marks or unmarks a Page. Resolves to whether it is now marked. */
  async toggle(pageId: string): Promise<boolean> {
    try {
      const marked = await service.toggleBookmark(pageId);
      const next = new Set(this.marked);
      if (marked) next.add(pageId);
      else next.delete(pageId);
      this.marked = next;
      if (this.entries.length > 0) await this.loadAll(this.volumeId);
      return marked;
    } catch (error) {
      notices.failure(error);
      return this.marked.has(pageId);
    }
  }

  clear(): void {
    this.marked = new Set();
    this.entries = [];
    this.volumeId = null;
  }
}

export const bookmarks = new BookmarkStore();
