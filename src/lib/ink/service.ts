/**
 * Ink IPC service — thin wrappers over the Tauri commands in commands/ink.rs.
 *
 * The frontend talks to persistence only through here, mirroring the other
 * services. Strokes are sent on pointerup, never per point.
 */

import { invoke } from '$lib/services/ipc';
import type { Annotation } from '$lib/types/annotation';
import type { InkNote, InkPoint, InkRecognition, InkStroke, InkTool, LanguageHint } from '$lib/types/ink';

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

// ---------------------------------------------------------------------------
// Ink intelligence — handwriting recognition.
//
// These mirror commands/ink_recognition.rs. The snapshot the frontend sends
// carries both the strokes (the source of truth) and the raster the frontend
// rendered for a vision model (an input artifact, never persisted).
// ---------------------------------------------------------------------------

/** A stroke as it crosses IPC for a recognition snapshot, matching StrokeInput. */
export type SnapshotStroke = StrokeInput;

/** The ink snapshot sent to recognition: strokes plus an optional raster. */
export type InkSnapshotInput = {
  strokes: SnapshotStroke[];
  surfaceWidth: number;
  png?: Uint8Array;
  width?: number;
  height?: number;
};

/**
 * Schedules automatic recognition for a note after the backend's debounce.
 * Returns the content hash of the snapshot, so the frontend can mark the note
 * stale when its ink changes before recognition completes. Does nothing (and
 * returns the hash) when automatic recognition is off — no error is raised,
 * because the writer simply has not opted in.
 */
export function recognizeInk(annotationId: string, snapshot: InkSnapshotInput): Promise<string> {
  // Tauri serialises Uint8Array to bytes on the Rust side; the optional fields
  // are omitted when absent so the backend's `#[serde(default)]` applies.
  return invoke('ink_recognize', { annotationId, snapshot });
}

/**
 * Runs recognition immediately, ignoring the debounce and the automatic
 * settings. Used by the explicit "Recognize handwriting" action. Returns the
 * committed recognition view.
 */
export function recognizeInkManual(
  annotationId: string,
  snapshot: InkSnapshotInput
): Promise<InkRecognition> {
  return invoke('ink_recognize_manual', { annotationId, snapshot });
}

/** Every recognition row on a Page, for the Margin to paint on open. */
export function recognitionForPage(pageId: string): Promise<InkRecognition[]> {
  return invoke('ink_recognition_for_page', { pageId });
}

/** The recognition state for one note. */
export function recognitionStatus(annotationId: string): Promise<InkRecognition | null> {
  return invoke('ink_recognition_status', { annotationId });
}

/** Saves a writer's hand-correction of a transcript. */
export function editInkTranscript(annotationId: string, text: string): Promise<InkRecognition> {
  return invoke('ink_edit_transcript', { annotationId, text });
}

/**
 * Converts an ink note's transcript into a typed text note, keeping the ink.
 * Returns the new text annotation's id.
 */
export function convertInkToText(annotationId: string): Promise<string> {
  return invoke('ink_convert_to_text', { annotationId });
}

/** Cancels any pending automatic recognition for a note. */
export function cancelInkRecognition(annotationId: string): Promise<void> {
  return invoke('ink_recognize_cancel', { annotationId });
}

/** The configured recognition language, for the settings UI. */
export function recognitionLanguage(): Promise<LanguageHint> {
  return invoke('ink_recognition_language');
}
