/**
 * Autosave.
 *
 * Rules, in priority order:
 *
 * 1. **Typing never waits.** `schedule` records the document and returns. No
 *    network, no database, no serialisation on the keystroke path.
 * 2. **A save is always addressed to the Page it came from.** Pending work
 *    carries its own page id, so a save that lands after the writer has moved
 *    on cannot write one Page's text into another. This is the failure that
 *    silently destroys a manuscript, and the reason the queue is keyed rather
 *    than a bare "current document".
 * 3. **One save in flight at a time.** Changes arriving mid-save are queued,
 *    not raced. SQLite would serialise them anyway; doing it here keeps the
 *    ordering explicit.
 * 4. **A failure is never silent, and never discards the text.** The document
 *    stays in the queue and stays in the editor.
 */

import { savePage } from '$lib/services/manuscript';
import { describeError } from '$lib/services/ipc';
import type { Page, ProseMirrorDocument } from '$lib/types/manuscript';

export type SaveState = 'saved' | 'unsaved' | 'saving' | 'failed';

type Pending = { pageId: string; document: ProseMirrorDocument };

export class Autosave {
  state = $state<SaveState>('saved');
  /** The message from the last failure, shown beside the indicator. */
  failure = $state<string | null>(null);
  lastSavedAt = $state<number | null>(null);

  /** Set when a save succeeds, so callers can adopt the backend's counts. */
  onsaved?: (page: Page) => void;

  private pending: Pending | null = null;
  private inFlight = false;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private debounceMs = 900;

  setDebounce(ms: number): void {
    this.debounceMs = Math.min(Math.max(ms, 200), 5_000);
  }

  readonly isDirty = $derived(this.state === 'unsaved' || this.state === 'failed');

  /** Records a change. Returns immediately; the write happens later. */
  schedule(pageId: string, document: ProseMirrorDocument): void {
    this.pending = { pageId, document };
    if (this.state !== 'saving') this.state = 'unsaved';

    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = null;
      void this.run();
    }, this.debounceMs);
  }

  /**
   * Writes any pending change now and waits for it.
   *
   * Called before leaving a Page, before closing a Volume, and when the window
   * loses focus — the moments where a debounce timer would otherwise be the
   * only thing standing between the writer and lost work.
   */
  async flush(): Promise<void> {
    if (this.timer) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    await this.run();
  }

  /** Drops queued work for a Page that no longer exists. */
  forget(pageId: string): void {
    if (this.pending?.pageId === pageId) {
      this.pending = null;
      if (!this.inFlight) this.state = 'saved';
    }
  }

  private async run(): Promise<void> {
    if (this.inFlight) {
      // A save is already running. Whatever is pending will be picked up by
      // the loop below when it finishes.
      return;
    }

    this.inFlight = true;
    try {
      while (this.pending) {
        const job = this.pending;
        this.pending = null;
        this.state = 'saving';

        try {
          const saved = await savePage(job.pageId, job.document);
          this.onsaved?.(saved);
          this.lastSavedAt = Date.now();
          this.failure = null;
        } catch (error) {
          // Put the work back so a retry — or the next keystroke — still has
          // it. Never overwrite newer pending work with this older document.
          if (!this.pending) this.pending = job;
          this.state = 'failed';
          this.failure = describeError(error);
          return;
        }
      }
      this.state = 'saved';
    } finally {
      this.inFlight = false;
    }
  }

  /** Tries a failed save again, at the user's request. */
  async retry(): Promise<void> {
    if (this.state !== 'failed') return;
    this.state = 'unsaved';
    await this.run();
  }
}

export const autosave = new Autosave();
