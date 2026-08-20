/** Margin annotations. */

import { invoke } from './ipc';
import type { Annotation, AnnotationKind, AnnotationStatus } from '$lib/types/annotation';

export function listAnnotations(pageId: string): Promise<Annotation[]> {
  return invoke('annotations_list', { pageId });
}

/** Unresolved counts per Page, as `[pageId, count]` pairs. */
export function openAnnotationCounts(volumeId: string): Promise<[string, number][]> {
  return invoke('annotations_open_counts', { volumeId });
}

/**
 * Creates a note anchored to a range of the Page's plain text.
 *
 * Only offsets are sent. The anchored text and its surrounding context are read
 * from the stored copy by the backend, so an anchor cannot describe text the
 * Page does not contain.
 */
export function createAnnotation(
  pageId: string,
  kind: AnnotationKind,
  body: string,
  from: number,
  to: number
): Promise<Annotation> {
  return invoke('annotation_create', { pageId, kind, body, from, to });
}

export function createPageAnnotation(
  pageId: string,
  kind: AnnotationKind,
  body: string
): Promise<Annotation> {
  return invoke('annotation_create_for_page', { pageId, kind, body });
}

export function updateAnnotation(id: string, body: string): Promise<Annotation> {
  return invoke('annotation_update', { id, body });
}

export function setAnnotationStatus(id: string, status: AnnotationStatus): Promise<Annotation> {
  return invoke('annotation_set_status', { id, status });
}

export function deleteAnnotation(id: string): Promise<void> {
  return invoke('annotation_delete', { id });
}
