/** Revision history and crash recovery. */

import { invoke } from './ipc';
import type { Page, ProseMirrorDocument } from '$lib/types/manuscript';
import type { RecoverableDraft, Revision, RevisionSummary } from '$lib/types/history';

export function listRevisions(pageId: string): Promise<RevisionSummary[]> {
  return invoke('revisions_list', { pageId });
}

export function getRevision(id: string): Promise<Revision> {
  return invoke('revision_get', { id });
}

/**
 * Puts a revision back as the Page's current content.
 *
 * The backend snapshots the text being replaced first, so this is itself
 * undoable.
 */
export function restoreRevision(id: string): Promise<Page> {
  return invoke('revision_restore', { id });
}

/** Journals an in-progress document against a possible crash. */
export function writeDraft(pageId: string, document: ProseMirrorDocument): Promise<void> {
  return invoke('draft_write', { pageId, document });
}

/**
 * Drafts worth offering back.
 *
 * Empty when there is nothing to recover — the backend discards stale and
 * unchanged drafts rather than reporting them — so the prompt is shown if and
 * only if this returns something.
 */
export function recoverableDrafts(): Promise<RecoverableDraft[]> {
  return invoke('drafts_recoverable');
}

export function recoverDraft(pageId: string): Promise<Page> {
  return invoke('draft_recover', { pageId });
}

export function discardDraft(pageId: string): Promise<void> {
  return invoke('draft_discard', { pageId });
}
