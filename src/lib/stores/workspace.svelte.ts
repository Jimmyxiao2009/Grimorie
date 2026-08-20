/**
 * The open Volume: its structure, what is selected, and what is collapsed.
 *
 * Structural changes go to the backend and then re-read the outline. Re-reading
 * costs two small queries and buys a guarantee: what the tree shows is what the
 * database holds, including the position repacking the backend performs. An
 * optimistic tree would have to reimplement that repacking to stay honest, and
 * would drift the moment the two disagreed.
 */

import * as manuscript from '$lib/services/manuscript';
import { openVolume, refreshOutline } from '$lib/services/library';
import { IpcError } from '$lib/services/ipc';
import { notices } from './notices.svelte';
import { router } from './router.svelte';
import type { ChapterOutline, Outline, PageSummary } from '$lib/types/manuscript';

const COLLAPSE_KEY = 'grimoire.collapsed';

function loadCollapsed(volumeId: string): Set<string> {
  try {
    const raw = localStorage.getItem(`${COLLAPSE_KEY}.${volumeId}`);
    return new Set<string>(raw ? (JSON.parse(raw) as string[]) : []);
  } catch {
    return new Set();
  }
}

class WorkspaceStore {
  outline = $state<Outline | null>(null);
  loading = $state(false);
  failure = $state<string | null>(null);
  activePageId = $state<string | null>(null);
  collapsed = $state<Set<string>>(new Set());

  readonly volume = $derived(this.outline?.volume ?? null);
  readonly chapters = $derived<ChapterOutline[]>(this.outline?.chapters ?? []);

  /** Every Page in manuscript order — what next/previous navigation walks. */
  readonly pages = $derived<PageSummary[]>(this.chapters.flatMap((chapter) => chapter.pages));

  readonly isEmpty = $derived(!this.loading && this.chapters.length === 0);

  readonly totalWords = $derived(
    this.pages.reduce((sum, page) => sum + page.wordCount, 0)
  );

  async open(volumeId: string, pageId: string | null = null): Promise<void> {
    this.loading = true;
    this.failure = null;
    this.collapsed = loadCollapsed(volumeId);
    try {
      this.outline = await openVolume(volumeId);
      // Honour a remembered Page only if it still exists.
      const remembered = pageId && this.pages.some((page) => page.id === pageId) ? pageId : null;
      this.selectPage(remembered ?? this.firstPageId());
    } catch (error) {
      this.outline = null;
      // A remembered Volume that no longer exists is not an error the writer
      // needs to read — it was deleted, or the library file changed. Go back to
      // the shelf rather than stranding them on a failure screen.
      if (error instanceof IpcError && error.isMissing) {
        router.toLibrary();
        return;
      }
      this.failure =
        error instanceof Error ? error.message : 'Grimoire couldn’t open that Volume.';
    } finally {
      this.loading = false;
    }
  }

  /** Re-reads the structure without recording another visit. */
  async refresh(): Promise<void> {
    const id = this.outline?.volume.id;
    if (!id) return;
    try {
      this.outline = await refreshOutline(id);
      // The active Page may have been deleted elsewhere in the tree.
      if (this.activePageId && !this.pages.some((page) => page.id === this.activePageId)) {
        this.selectPage(this.firstPageId());
      }
    } catch (error) {
      notices.failure(error);
    }
  }

  close(): void {
    this.outline = null;
    this.activePageId = null;
    this.failure = null;
  }

  firstPageId(): string | null {
    return this.pages[0]?.id ?? null;
  }

  selectPage(id: string | null): void {
    this.activePageId = id;
    // Recorded so relaunching returns the writer to the Page they left.
    router.setPage(id);
  }

  /** Moves to the next or previous Page in manuscript order. */
  step(delta: number): void {
    const all = this.pages;
    if (all.length === 0) return;
    const index = all.findIndex((page) => page.id === this.activePageId);
    const next = index === -1 ? 0 : Math.min(Math.max(index + delta, 0), all.length - 1);
    const target = all[next];
    if (target) this.selectPage(target.id);
  }

  isCollapsed(chapterId: string): boolean {
    return this.collapsed.has(chapterId);
  }

  toggleChapter(chapterId: string): void {
    const next = new Set(this.collapsed);
    if (!next.delete(chapterId)) next.add(chapterId);
    this.collapsed = next;
    this.persistCollapsed();
  }

  // --- Structure ------------------------------------------------------------

  async createChapter(title = 'Untitled Chapter'): Promise<string | null> {
    const volumeId = this.outline?.volume.id;
    if (!volumeId) return null;
    try {
      const chapter = await manuscript.createChapter(volumeId, title);
      await this.refresh();
      return chapter.id;
    } catch (error) {
      notices.failure(error);
      return null;
    }
  }

  async createPage(chapterId: string, title = 'Untitled Page'): Promise<string | null> {
    try {
      const page = await manuscript.createPage(chapterId, title);
      // A new Page is always created in order to write in it, so expand its
      // Chapter and put the cursor there.
      if (this.collapsed.has(chapterId)) this.toggleChapter(chapterId);
      await this.refresh();
      this.selectPage(page.id);
      return page.id;
    } catch (error) {
      notices.failure(error);
      return null;
    }
  }

  async renameChapter(id: string, title: string): Promise<void> {
    await this.mutate(() => manuscript.renameChapter(id, title));
  }

  async renamePage(id: string, title: string): Promise<void> {
    await this.mutate(() => manuscript.renamePage(id, title));
  }

  async duplicateChapter(id: string): Promise<void> {
    await this.mutate(() => manuscript.duplicateChapter(id));
  }

  async duplicatePage(id: string): Promise<void> {
    await this.mutate(() => manuscript.duplicatePage(id));
  }

  async deleteChapter(id: string): Promise<void> {
    await this.mutate(() => manuscript.deleteChapter(id));
  }

  async deletePage(id: string): Promise<void> {
    // Choose the neighbour to land on *before* the Page disappears, so deleting
    // does not dump the writer on an empty screen.
    const all = this.pages;
    const index = all.findIndex((page) => page.id === id);
    const neighbour = all[index + 1] ?? all[index - 1] ?? null;

    await this.mutate(() => manuscript.deletePage(id));

    if (this.activePageId === id) this.selectPage(neighbour?.id ?? this.firstPageId());
  }

  async reorderChapters(ordered: string[]): Promise<void> {
    const volumeId = this.outline?.volume.id;
    if (!volumeId) return;
    await this.mutate(() => manuscript.reorderChapters(volumeId, ordered));
  }

  async reorderPages(chapterId: string, ordered: string[]): Promise<void> {
    await this.mutate(() => manuscript.reorderPages(chapterId, ordered));
  }

  async movePage(id: string, chapterId: string, index?: number): Promise<void> {
    await this.mutate(() => manuscript.movePage(id, chapterId, index));
  }

  /** Applies a change, then re-reads. One place for the error handling. */
  private async mutate(work: () => Promise<unknown>): Promise<void> {
    try {
      await work();
      await this.refresh();
    } catch (error) {
      notices.failure(error);
    }
  }

  private persistCollapsed(): void {
    const id = this.outline?.volume.id;
    if (!id || typeof localStorage === 'undefined') return;
    try {
      localStorage.setItem(`${COLLAPSE_KEY}.${id}`, JSON.stringify([...this.collapsed]));
    } catch {
      // Collapse state is a convenience; losing it costs one click.
    }
  }
}

export const workspace = new WorkspaceStore();
