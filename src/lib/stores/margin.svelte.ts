/**
 * The Margin: annotations for the open Page.
 *
 * Reloads after every save, because saving re-anchors annotations in the same
 * transaction — offsets and staleness can change without the writer touching a
 * note. Keeping a local copy across a save would show the Margin pointing at a
 * version of the Page that no longer exists.
 */

import * as service from '$lib/services/annotations';
import { notices } from './notices.svelte';
import type { Annotation, AnnotationKind } from '$lib/types/annotation';

const PANE_KEY = 'grimoire.margin-open';

function restorePaneOpen(): boolean {
  try {
    const stored = localStorage.getItem(PANE_KEY);
    // Open by default: the Margin is where the thinking goes, and a writer who
    // does not want it can close it once and have that remembered.
    return stored === null ? true : stored === 'true';
  } catch {
    return true;
  }
}

class MarginStore {
  annotations = $state<Annotation[]>([]);
  /**
   * Whether the Margin sits beside the manuscript, remembered across sessions.
   *
   * This governs the *pane* only. Where the Margin can only be an overlay, it
   * is opened transiently and starts closed — restoring it there would cover
   * the writer's own text with notes the moment they opened the app, which is
   * not what "leave the Margin open" was meant to ask for.
   */
  paneOpen = $state(true);
  loading = $state(false);
  /** Which note is expanded for editing. */
  focusedId = $state<string | null>(null);
  /** Hides resolved notes, which is the default the Margin opens in. */
  showResolved = $state(false);

  private pageId: string | null = null;
  private token = 0;

  readonly visible = $derived(
    this.showResolved
      ? this.annotations
      : this.annotations.filter((a) => a.status !== 'resolved')
  );

  readonly openCount = $derived(
    this.annotations.filter((a) => a.status !== 'resolved').length
  );

  readonly staleCount = $derived(this.annotations.filter((a) => a.status === 'stale').length);

  start(): void {
    if (typeof localStorage === 'undefined') return;
    this.paneOpen = restorePaneOpen();
  }

  setPaneOpen(open: boolean): void {
    this.paneOpen = open;
    try {
      localStorage.setItem(PANE_KEY, String(open));
    } catch {
      // Losing the preference costs one click next launch.
    }
  }

  async load(pageId: string | null): Promise<void> {
    this.pageId = pageId;
    if (!pageId) {
      this.annotations = [];
      return;
    }

    // Guarded like the Page load: a slow response for a Page the writer has
    // navigated away from must not replace the Margin they are looking at.
    const ticket = ++this.token;
    this.loading = true;
    try {
      const rows = await service.listAnnotations(pageId);
      if (ticket !== this.token) return;
      this.annotations = rows;
    } catch (error) {
      if (ticket !== this.token) return;
      this.annotations = [];
      notices.failure(error);
    } finally {
      if (ticket === this.token) this.loading = false;
    }
  }

  /** Re-reads without the loading state, for use after a save. */
  async refresh(): Promise<void> {
    if (!this.pageId) return;
    const ticket = ++this.token;
    try {
      const rows = await service.listAnnotations(this.pageId);
      if (ticket === this.token) this.annotations = rows;
    } catch {
      // A failed refresh leaves the previous notes on screen, which is better
      // than emptying the Margin because one request did not land.
    }
  }

  clear(): void {
    this.token++;
    this.annotations = [];
    this.focusedId = null;
    this.pageId = null;
  }

  async createAnchored(
    pageId: string,
    kind: AnnotationKind,
    body: string,
    from: number,
    to: number
  ): Promise<Annotation | null> {
    try {
      const created = await service.createAnnotation(pageId, kind, body, from, to);
      await this.load(pageId);
      this.focusedId = created.id;
      return created;
    } catch (error) {
      notices.failure(error);
      return null;
    }
  }

  async createForPage(
    pageId: string,
    kind: AnnotationKind,
    body: string
  ): Promise<Annotation | null> {
    try {
      const created = await service.createPageAnnotation(pageId, kind, body);
      await this.load(pageId);
      this.focusedId = created.id;
      return created;
    } catch (error) {
      notices.failure(error);
      return null;
    }
  }

  async updateBody(id: string, body: string): Promise<void> {
    await this.mutate(() => service.updateAnnotation(id, body));
  }

  async resolve(id: string): Promise<void> {
    await this.mutate(() => service.setAnnotationStatus(id, 'resolved'));
  }

  async reopen(id: string): Promise<void> {
    await this.mutate(() => service.setAnnotationStatus(id, 'active'));
  }

  async remove(id: string): Promise<void> {
    if (this.focusedId === id) this.focusedId = null;
    await this.mutate(() => service.deleteAnnotation(id));
  }

  private async mutate(work: () => Promise<unknown>): Promise<void> {
    try {
      await work();
      await this.refresh();
    } catch (error) {
      notices.failure(error);
    }
  }
}

export const margin = new MarginStore();
