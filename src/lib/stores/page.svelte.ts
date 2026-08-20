/**
 * The open Page.
 *
 * Holds the document currently in the editor. Loading is guarded by a request
 * token: a slow load for a Page the writer has already navigated away from must
 * never win and replace what is on screen.
 */

import { getPage } from '$lib/services/manuscript';
import { autosave } from '$lib/editor/autosave.svelte';
import type { Page } from '$lib/types/manuscript';

class ActivePage {
  page = $state<Page | null>(null);
  loading = $state(false);
  failure = $state<string | null>(null);

  /** Live counts from the editor, ahead of the next save. */
  words = $state(0);
  characters = $state(0);

  private token = 0;

  async load(id: string): Promise<void> {
    // Anything the writer typed before navigating must reach the database
    // before a different Page takes over the editor.
    await autosave.flush();

    const ticket = ++this.token;
    this.loading = true;
    this.failure = null;

    try {
      const page = await getPage(id);
      if (ticket !== this.token) return; // superseded
      this.page = page;
      this.words = page.wordCount;
      this.characters = page.characterCount;
    } catch (error) {
      if (ticket !== this.token) return;
      this.page = null;
      this.failure =
        error instanceof Error ? error.message : 'Grimoire couldn’t open that Page.';
    } finally {
      if (ticket === this.token) this.loading = false;
    }
  }

  clear(): void {
    this.token++;
    this.page = null;
    this.failure = null;
    this.words = 0;
    this.characters = 0;
  }

  /**
   * Adopts the record the backend returned after a save.
   *
   * Only the metadata is taken. The document is deliberately *not* pushed back
   * into the editor: the writer has almost certainly typed more since the save
   * began, and replacing their text with a slightly older version would be the
   * worst bug this application could have.
   */
  adoptSaved(saved: Page): void {
    if (!this.page || this.page.id !== saved.id) return;
    this.page = { ...this.page, ...saved, document: this.page.document };
    this.words = saved.wordCount;
    this.characters = saved.characterCount;
  }
}

export const activePage = new ActivePage();
