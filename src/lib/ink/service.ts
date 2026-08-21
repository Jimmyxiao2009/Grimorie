/**
 * Ink IPC service — thin wrappers over the Tauri commands in commands/ink.rs.
 *
 * The frontend talks to persistence only through here, mirroring the other
 * services. Strokes are sent on pointerup, never per point.
 */

import { invoke } from '$lib/services/ipc';
import type { Annotation } from '$lib/types/annotation';
import type { InkNote, InkPoint, InkStroke, InkTool } from '$lib/types/ink';

/** A stroke as it is sent across IPC, matching StrokeInput in Rust. */
export type StrokeInput = {
  id: string;
  tool: InkTool;
  color: string;
  width: number;
  points: InkPoint[];
  createdAt: string;
};

/** Creates a whole-Page ink note and returns it ready to draw into. */
export function createInkNote(pageId: string): Promise<Annotation> {
  return invoke('ink_create', { pageId });
}

/** Creates an ink note anchored to a range of the Page's plain text. */
export function createAnchoredInkNote(
  pageId: string,
  from: number,
  to: number
): Promise<Annotation> {
  return invoke('ink_create_anchored', { pageId, from, to });
}

/** Every ink note on a Page, with its strokes. */
export function inkNotesForPage(pageId: string): Promise<InkNote[]> {
  return invoke('ink_notes_for_page', { pageId });
}

/**
 * Persists strokes drawn locally. Called once per finished stroke (pointerup),
 * so the write rate is "once per pen lift" rather than once per point.
 */
export function addInkStrokes(annotationId: string, strokes: InkStroke[]): Promise<void> {
  const payload: StrokeInput[] = strokes.map((s) => ({
    id: s.id,
    tool: s.tool,
    color: s.color,
    width: s.width,
    points: s.points,
    createdAt: s.createdAt
  }));
  return invoke('ink_add_strokes', { annotationId, strokes: payload });
}

/** Removes a single stroke — the eraser's unit of work. */
export function deleteInkStroke(id: string): Promise<void> {
  return invoke('ink_delete_stroke', { id });
}

/** Deletes an ink note and every stroke under it. */
export function deleteInkAnnotation(id: string): Promise<void> {
  return invoke('ink_delete_annotation', { id });
}
