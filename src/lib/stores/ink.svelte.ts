/**
 * The ink store: ink notes for the open Page, the active pen tool, and an
 * undo/redo history kept separate from the editor's.
 *
 * # Where state lives
 *
 * The *active* stroke — the one currently under the pen — never enters this
 * store. It lives in the drawing component and is committed here only on
 * pointerup, so a hundred-point stroke costs one store update rather than a
 * hundred. This is the Svelte state discipline the feature depends on: global
 * state stays compact, and typing/scrolling performance never pays for ink.
 *
 * # Persistence
 *
 * Strokes are persisted on commit (pointerup), once. If a save fails the stroke
 * is *not* removed from view: it stays in local state with a subtle error flag,
 * because silently deleting handwriting a person just wrote is the one failure
 * mode this feature must not have. The stroke can be retried.
 *
 * # Undo
 *
 * Ink undo is its own stack, deliberately separate from Tiptap's text undo, so
 * Ctrl+Z in pen mode removes a stroke rather than deleting prose. The routing
 * of Ctrl+Z is decided in the workspace by whether pen mode is active.
 */

import * as service from '$lib/ink/service';
import type { Annotation } from '$lib/types/annotation';
import type { InkNote, InkStroke, InkTool } from '$lib/types/ink';
import { notices } from '$lib/stores/notices.svelte';

/** The colours offered, as semantic ids resolved per theme. */
export const INK_COLORS = ['ink-primary', 'ink-red', 'ink-blue'] as const;
export type InkColor = (typeof INK_COLORS)[number];

export const INK_COLOR_LABELS: Record<InkColor, string> = {
  'ink-primary': 'Graphite',
  'ink-red': 'Red',
  'ink-blue': 'Blue'
};

/** Default widths per tool, in surface pixels. */
export const DEFAULT_WIDTH: Record<InkTool, number> = {
  pen: 2,
  highlighter: 10
};

/** The eraser's hit tolerance in surface pixels. */
export const ERASE_TOLERANCE = 12;

export type InkToolState = InkTool | 'eraser';

type UndoEntry =
  | { kind: 'add'; annotationId: string; stroke: InkStroke }
  | { kind: 'erase'; annotationId: string; stroke: InkStroke };

class InkStore {
  /** Ink notes on the open Page, keyed by annotation id. */
  notes = $state<Map<string, InkNote>>(new Map());
  loading = $state(false);
  /** Set when a persistence operation failed; cleared on the next success. */
  saveError = $state(false);

  /** The tool selected in the toolbar. */
  tool = $state<InkToolState>('pen');
  /** Pen mode is the master switch: when off, the ink surface ignores input. */
  penMode = $state(false);
  color = $state<InkColor>('ink-primary');
  penWidth = $state(DEFAULT_WIDTH.pen);
  highlighterWidth = $state(DEFAULT_WIDTH.highlighter);

  /** Which ink note new strokes are written into, or null to create one. */
  activeAnnotationId = $state<string | null>(null);

  private pageId: string | null = null;
  private token = 0;
  private undoStack: UndoEntry[] = [];
  private redoStack: UndoEntry[] = [];

  readonly canUndo = $derived(this.undoStack.length > 0);
  readonly canRedo = $derived(this.redoStack.length > 0);

  /** The width the active tool draws with. */
  readonly activeWidth = $derived(
    this.tool === 'highlighter' ? this.highlighterWidth : this.penWidth
  );

  /** The colour the active tool draws with. The eraser has none. */
  readonly activeColor = $derived(
    this.tool === 'highlighter' ? 'ink-highlighter' : this.color
  );

  /** Every stroke across every note on the page, for hit-testing. */
  readonly allStrokes = $derived.by(() => {
    const out: Array<{ annotationId: string; stroke: InkStroke }> = [];
    for (const note of this.notes.values()) {
      for (const stroke of note.strokes) {
        out.push({ annotationId: note.annotation.id, stroke });
      }
    }
    return out;
  });

  /** Loads the ink notes for a Page, guarding against stale responses. */
  async load(pageId: string | null): Promise<void> {
    this.pageId = pageId;
    if (!pageId) {
      this.notes = new Map();
      this.activeAnnotationId = null;
      return;
    }

    const ticket = ++this.token;
    this.loading = true;
    try {
      const notes = await service.inkNotesForPage(pageId);
      if (ticket !== this.token) return;
      const map = new Map<string, InkNote>();
      for (const note of notes) map.set(note.annotation.id, note);
      this.notes = map;
      // Continue drawing into the most recent ink note, if there is one.
      this.activeAnnotationId = notes.length > 0 ? notes[0]!.annotation.id : null;
    } catch (error) {
      if (ticket !== this.token) return;
      this.notes = new Map();
      notices.failure(error);
    } finally {
      if (ticket === this.token) this.loading = false;
    }
  }

  /** Re-reads without the loading state, for use after a save. */
  async refresh(): Promise<void> {
    if (!this.pageId) return;
    const ticket = ++this.token;
    try {
      const notes = await service.inkNotesForPage(this.pageId);
      if (ticket !== this.token) return;
      const map = new Map<string, InkNote>();
      for (const note of notes) map.set(note.annotation.id, note);
      this.notes = map;
    } catch {
      // A failed refresh leaves the previous strokes on screen, which is better
      // than emptying the Margin because one request did not land.
    }
  }

  clear(): void {
    this.token++;
    this.notes = new Map();
    this.activeAnnotationId = null;
    this.pageId = null;
    this.undoStack = [];
    this.redoStack = [];
    this.saveError = false;
  }

  setTool(tool: InkToolState): void {
    this.tool = tool;
  }

  togglePenMode(): void {
    this.penMode = !this.penMode;
    // Leaving pen mode clears any selection-driven tool focus.
    if (!this.penMode) this.tool = 'pen';
  }

  setColor(color: InkColor): void {
    this.color = color;
  }

  setPenWidth(width: number): void {
    this.penWidth = width;
  }

  setHighlighterWidth(width: number): void {
    this.highlighterWidth = width;
  }

  /**
   * Commits a finished stroke: writes it to the active ink note, persists it,
   * and records an undo entry.
   *
   * The stroke is added to local state immediately (optimistic), so it is
   * visible before the round trip. On failure it is *not* rolled back — it
   * stays visible with a save-error flag, and the undo entry is still recorded
   * so the writer can remove it. Handwriting is never silently discarded.
   */
  async commitStroke(stroke: InkStroke): Promise<void> {
    const pageId = this.pageId;
    if (!pageId) return;

    // Ensure there is an ink note to draw into.
    let annotationId = this.activeAnnotationId;
    if (!annotationId) {
      try {
        const annotation = await service.createInkNote(pageId);
        annotationId = annotation.id;
        this.activeAnnotationId = annotationId;
        this.notes.set(annotationId, { annotation, strokes: [] });
      } catch (error) {
        notices.failure(error);
        return;
      }
    }

    // Optimistic: show it now.
    this.addStrokeLocal(annotationId, stroke);
    this.pushUndo({ kind: 'add', annotationId, stroke });
    this.redoStack = [];

    try {
      await service.addInkStrokes(annotationId, [stroke]);
      this.saveError = false;
    } catch (error) {
      // Keep the stroke visible; flag the error and offer a retry.
      this.saveError = true;
      notices.failure(error, () => void this.retryPending(annotationId!, stroke));
    }
  }

  /** Retries a stroke whose persistence failed. */
  private async retryPending(annotationId: string, stroke: InkStroke): Promise<void> {
    try {
      await service.addInkStrokes(annotationId, [stroke]);
      this.saveError = false;
    } catch (error) {
      notices.failure(error, () => void this.retryPending(annotationId, stroke));
    }
  }

  /**
   * Erases the stroke nearest a point, within the eraser tolerance. Removes it
   * optimistically and persists the deletion; on failure the stroke is restored.
   */
  async eraseAt(point: { x: number; y: number }, surfaceWidth: number): Promise<void> {
    const { hitTestStroke } = await import('$lib/ink/geometry');
    const candidates = this.allStrokes;
    if (candidates.length === 0) return;

    const hit = hitTestStroke(
      candidates.map((c) => c.stroke),
      point,
      surfaceWidth,
      ERASE_TOLERANCE
    );
    if (!hit) return;

    const owner = candidates.find((c) => c.stroke.id === hit);
    if (!owner) return;
    const { annotationId, stroke } = owner;

    // Optimistic removal.
    this.removeStrokeLocal(annotationId, stroke.id);
    this.pushUndo({ kind: 'erase', annotationId, stroke });
    this.redoStack = [];

    try {
      await service.deleteInkStroke(stroke.id);
      this.saveError = false;
    } catch (error) {
      // Restore the stroke so the writer does not see it vanish.
      this.addStrokeLocal(annotationId, stroke);
      this.saveError = true;
      notices.failure(error);
    }
  }

  /** Removes an entire ink note and its strokes. */
  async deleteNote(annotationId: string): Promise<void> {
    const note = this.notes.get(annotationId);
    if (!note) return;
    this.notes.delete(annotationId);
    if (this.activeAnnotationId === annotationId) this.activeAnnotationId = null;
    try {
      await service.deleteInkAnnotation(annotationId);
      this.saveError = false;
    } catch (error) {
      // Restore on failure.
      this.notes.set(annotationId, note);
      this.saveError = true;
      notices.failure(error);
    }
  }

  /** Undoes the last ink action (add or erase). */
  async undo(): Promise<void> {
    const entry = this.undoStack.pop();
    if (!entry) return;
    this.redoStack.push(entry);

    if (entry.kind === 'add') {
      // Undo an add by erasing the stroke.
      this.removeStrokeLocal(entry.annotationId, entry.stroke.id);
      try {
        await service.deleteInkStroke(entry.stroke.id);
      } catch (error) {
        this.addStrokeLocal(entry.annotationId, entry.stroke);
        notices.failure(error);
      }
    } else {
      // Undo an erase by re-adding the stroke.
      this.addStrokeLocal(entry.annotationId, entry.stroke);
      try {
        await service.addInkStrokes(entry.annotationId, [entry.stroke]);
      } catch (error) {
        this.removeStrokeLocal(entry.annotationId, entry.stroke.id);
        notices.failure(error);
      }
    }
  }

  /** Redoes the last undone ink action. */
  async redo(): Promise<void> {
    const entry = this.redoStack.pop();
    if (!entry) return;
    this.undoStack.push(entry);

    if (entry.kind === 'add') {
      this.addStrokeLocal(entry.annotationId, entry.stroke);
      try {
        await service.addInkStrokes(entry.annotationId, [entry.stroke]);
      } catch (error) {
        this.removeStrokeLocal(entry.annotationId, entry.stroke.id);
        notices.failure(error);
      }
    } else {
      this.removeStrokeLocal(entry.annotationId, entry.stroke.id);
      try {
        await service.deleteInkStroke(entry.stroke.id);
      } catch (error) {
        this.addStrokeLocal(entry.annotationId, entry.stroke);
        notices.failure(error);
      }
    }
  }

  /** The strokes for one ink note, in drawing order. */
  strokesFor(annotationId: string): InkStroke[] {
    return this.notes.get(annotationId)?.strokes ?? [];
  }

  /** The ink notes in drawing order, newest first. */
  readonly noteList = $derived([...this.notes.values()]);

  private addStrokeLocal(annotationId: string, stroke: InkStroke): void {
    const note = this.notes.get(annotationId);
    if (!note) return;
    note.strokes = [...note.strokes, stroke];
    this.notes.set(annotationId, note);
  }

  private removeStrokeLocal(annotationId: string, strokeId: string): void {
    const note = this.notes.get(annotationId);
    if (!note) return;
    note.strokes = note.strokes.filter((s) => s.id !== strokeId);
    this.notes.set(annotationId, note);
  }

  private pushUndo(entry: UndoEntry): void {
    this.undoStack.push(entry);
    // Bound the history so a long session does not grow it without limit.
    if (this.undoStack.length > 100) this.undoStack.shift();
  }
}

export const ink = new InkStore();
