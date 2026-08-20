/** Bookmarks and tags. */

import { invoke } from './ipc';
import type { Id, Timestamp } from '$lib/types/manuscript';

export type Bookmark = {
  id: Id;
  pageId: Id;
  label: string;
  pageTitle: string;
  chapterTitle: string;
  volumeId: Id;
  volumeTitle: string;
  preview: string;
  createdAt: Timestamp;
};

export type Tag = {
  id: Id;
  name: string;
  slug: string;
  /** How many things carry this tag. */
  uses: number;
};

export type TaggableKind = 'volume' | 'chapter' | 'page';

/** Marks a Page, or clears the mark. Resolves to whether it is now marked. */
export function toggleBookmark(pageId: string, label?: string): Promise<boolean> {
  return invoke('bookmark_toggle', { pageId, label: label ?? null });
}

export function listBookmarks(volumeId?: string | null): Promise<Bookmark[]> {
  return invoke('bookmarks_list', { volumeId: volumeId ?? null });
}

/** Bookmarked Page ids in a Volume, for marking them in the tree. */
export function bookmarkedPages(volumeId: string): Promise<string[]> {
  return invoke('bookmarked_pages', { volumeId });
}

export function tagsFor(entityId: string): Promise<Tag[]> {
  return invoke('tags_for', { entityId });
}

export function allTags(): Promise<Tag[]> {
  return invoke('tags_all');
}

export function attachTag(kind: TaggableKind, entityId: string, name: string): Promise<Tag> {
  return invoke('tag_attach', { kind, entityId, name });
}

export function detachTag(entityId: string, tagId: string): Promise<void> {
  return invoke('tag_detach', { entityId, tagId });
}
