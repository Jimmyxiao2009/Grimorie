/** Chapter and Page operations. */

import { invoke } from './ipc';
import type { Chapter, Page, PageSummary, ProseMirrorDocument } from '$lib/types/manuscript';

// --- Chapters ---------------------------------------------------------------

export function createChapter(volumeId: string, title: string): Promise<Chapter> {
  return invoke('chapter_create', { volumeId, title });
}

export function renameChapter(id: string, title: string): Promise<Chapter> {
  return invoke('chapter_rename', { id, title });
}

export function duplicateChapter(id: string): Promise<Chapter> {
  return invoke('chapter_duplicate', { id });
}

export function deleteChapter(id: string): Promise<void> {
  return invoke('chapter_delete', { id });
}

export function reorderChapters(volumeId: string, ordered: string[]): Promise<void> {
  return invoke('chapter_reorder', { volumeId, ordered });
}

// --- Pages ------------------------------------------------------------------

export function createPage(chapterId: string, title: string): Promise<Page> {
  return invoke('page_create', { chapterId, title });
}

export function getPage(id: string): Promise<Page> {
  return invoke('page_get', { id });
}

export function listPageSummaries(volumeId: string): Promise<PageSummary[]> {
  return invoke('page_summaries', { volumeId });
}

/**
 * Persists an editor document.
 *
 * Only the document is sent. Word counts, plain text, and the revision number
 * are recomputed by the backend from what it actually stores, so they cannot be
 * made to disagree with the text.
 */
export function savePage(id: string, document: ProseMirrorDocument): Promise<Page> {
  return invoke('page_save', { id, document });
}

export function renamePage(id: string, title: string): Promise<Page> {
  return invoke('page_rename', { id, title });
}

export function duplicatePage(id: string): Promise<Page> {
  return invoke('page_duplicate', { id });
}

export function deletePage(id: string): Promise<void> {
  return invoke('page_delete', { id });
}

export function reorderPages(chapterId: string, ordered: string[]): Promise<void> {
  return invoke('page_reorder', { chapterId, ordered });
}

/** Moves a Page to another Chapter, optionally at a specific index within it. */
export function movePage(id: string, chapterId: string, index?: number): Promise<Page> {
  return invoke('page_move', { id, chapterId, index: index ?? null });
}
