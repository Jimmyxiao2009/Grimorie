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
import { writeDraft } from '$lib/services/history';
import { describeError } from '$lib/services/ipc';
import type { Page, ProseMirrorDocument } from '$lib/types/manuscript';

export type SaveState = 'saved' | 'unsaved' | 'saving' | 'failed';

type Pending = { pageId: string; document: ProseMirrorDocument };

/**
 * How often the crash journal is written during unbroken typing.
 *
 * This is a *throttle*, not a debounce, and the distinction is the whole point:
 * a debounce never fires while someone is still typing, so the very session
 * most at risk — a long unbroken run of writing — would be the one with no
 * journal at all. A throttle bounds the loss to this interval no matter how
 * long the run.
 */
const JOURNAL_INTERVAL_MS = 1_200;

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

  private journalTimer: ReturnType<typeof setTimeout> | null = null;
  private lastJournalAt = 0;

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

    this.journal();
  }

  /**
   * Writes the crash journal, at most once per interval.
   *
   * Failures are swallowed on purpose. This is a safety net beneath autosave,
   * and a net that shouts when it cannot be woven would be worse than one that
   * quietly is not there — the real save is still coming, and it reports its
   * own failures.
   */
  private journal(): void {
    const elapsed = Date.now() - this.lastJournalAt;

    const write = () => {
      const job = this.pending;
      if (!job) return;
      this.lastJournalAt = Date.now();
      void writeDraft(job.pageId, job.document).catch(() => {});
    };

    if (elapsed >= JOURNAL_INTERVAL_MS) {
      write();
      return;
    }

    if (this.journalTimer) return;
    this.journalTimer = setTimeout(() => {
      this.journalTimer = null;
      write();
    }, JOURNAL_INTERVAL_MS - elapsed);
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
    // A pending journal write is pointless once the real save is happening —
    // the save clears the journal anyway.
    if (this.journalTimer) {
      clearTimeout(this.journalTimer);
      this.journalTimer = null;
    }
    await this.run();
  }

  /** Drops queued work for a Page that no longer exists. */
  forget(pageId: string): void {
    if (this.pending?.pageId === pageId) {
      this.pending = null;
      if (this.journalTimer) {
        clearTimeout(this.journalTimer);
        this.journalTimer = null;
      }
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
