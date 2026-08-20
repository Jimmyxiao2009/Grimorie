/**
 * Transient messages.
 *
 * Only for things that went wrong, or that the user asked for and cannot
 * otherwise see the result of. A successful save never appears here: autosave
 * that congratulates itself is noise, and the save indicator already says so
 * quietly.
 */

import { describeError } from '$lib/services/ipc';

export type NoticeTone = 'failure' | 'info';

export type Notice = {
  id: number;
  tone: NoticeTone;
  message: string;
  detail?: string;
  /** Offered when the failed operation can sensibly be tried again. */
  retry?: () => void;
};

/** Long enough to read a sentence; failures stay until dismissed. */
const INFO_DURATION_MS = 4_000;

let nextId = 1;

class Notices {
  items = $state<Notice[]>([]);

  /** Reports a failure. Stays until dismissed — a lost action must be seen. */
  failure(error: unknown, retry?: () => void): number {
    const message = describeError(error);
    const detail =
      error && typeof error === 'object' && 'detail' in error
        ? ((error as { detail?: string }).detail ?? undefined)
        : undefined;
    return this.push({ tone: 'failure', message, detail, retry });
  }

  info(message: string): number {
    const id = this.push({ tone: 'info', message });
    setTimeout(() => this.dismiss(id), INFO_DURATION_MS);
    return id;
  }

  dismiss(id: number): void {
    this.items = this.items.filter((notice) => notice.id !== id);
  }

  clear(): void {
    this.items = [];
  }

  private push(notice: Omit<Notice, 'id'>): number {
    const id = nextId++;
    // Newest first, and bounded: a failing loop must not build a wall of
    // identical messages over the manuscript.
    this.items = [{ ...notice, id }, ...this.items].slice(0, 4);
    return id;
  }
}

export const notices = new Notices();
