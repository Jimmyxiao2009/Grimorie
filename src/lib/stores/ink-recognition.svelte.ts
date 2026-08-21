/**
 * The ink recognition store: recognition status for the open Page's ink notes,
 * plus the orchestration that schedules automatic recognition after the writer
 * pauses and surfaces manual recognize / edit / convert actions.
 *
 * # Where the debounce lives
 *
 * The backend queue owns the real debounce (1.5s) and the concurrency limit;
 * this store does not reimplement either. Its job is the frontend half: after
 * the ink store commits a stroke, build a snapshot (strokes + rendered raster),
 * send it to the backend's `ink_recognize`, and keep the per-note status view
 * in step so the InkNote component can paint "Recognizing…" / "Recognized" /
 * "Recognition failed" without re-fetching.
 *
 * # Privacy
 *
 * Automatic recognition is gated by `inkAutoRecognition` and `aiEnabled` on the
 * backend; this store checks the local setting too, so it does not even render a
 * raster (which would do work) when automatic recognition is off. A manual
 * recognize always proceeds — the writer asked.
 */

import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import * as service from '$lib/ink/service';
import { renderInkPngAsync } from '$lib/ink/rasterize';
import { isDesktop } from '$lib/services/ipc';
import { settingsStore } from '$lib/stores/settings.svelte';
import type { InkRecognition, InkStroke } from '$lib/types/ink';

/** The backend event carrying one note's recognition row. */
const STATUS_EVENT = 'ink:recognition-status';

class InkRecognitionStore {
  /** Recognition state by annotation id, for the open Page. */
  status = $state<Map<string, InkRecognition>>(new Map());

  /** The surface width last seen, so a snapshot can be built without re-measuring. */
  private surfaceWidth = 0;

  private unlisten: UnlistenFn[] = [];

  /**
   * Subscribes to backend recognition status. Returns a teardown.
   *
   * Automatic recognition runs in a background task, so without this the Margin
   * would sit under an optimistic "Recognizing…" until the writer navigated
   * away and back. Each event carries the note's whole persisted row, so this
   * replaces the entry rather than patching it — two updates arriving close
   * together cannot interleave into a state that was never stored.
   */
  start(): () => void {
    if (!isDesktop) return () => {};

    void listen<InkRecognition>(STATUS_EVENT, ({ payload }) => {
      this.apply(payload);
    }).then((off) => this.unlisten.push(off));

    return () => {
      for (const off of this.unlisten) off();
      this.unlisten = [];
    };
  }

  /**
   * Applies one row from the backend.
   *
   * Notes for other Pages are ignored: the store only holds the open Page, and
   * a job that finishes for a note the writer has navigated away from has
   * nothing to update here — its result is already in the database and will be
   * read when that Page is next opened.
   */
  private apply(row: InkRecognition): void {
    if (!this.status.has(row.annotationId)) return;
    this.status.set(row.annotationId, row);
    this.status = new Map(this.status);
  }

  /** Loads recognition status for a Page. Called when the ink store loads. */
  async load(pageId: string | null): Promise<void> {
    if (!pageId) {
      this.status = new Map();
      return;
    }
    try {
      const rows = await service.recognitionForPage(pageId);
      const map = new Map<string, InkRecognition>();
      for (const row of rows) map.set(row.annotationId, row);
      this.status = map;
    } catch {
      // A failed load leaves the status empty; the Margin still shows ink.
    }
  }

  clear(): void {
    this.status = new Map();
  }

  /** Remembers the surface width, used when building a recognition snapshot. */
  setSurfaceWidth(width: number): void {
    this.surfaceWidth = width;
  }

  /** The recognition state for one note, or `null` when it has none. */
  forAnnotation(annotationId: string): InkRecognition | null {
    return this.status.get(annotationId) ?? null;
  }

  /**
   * Schedules automatic recognition for a note after a stroke was committed.
   *
   * Builds a snapshot from the note's live strokes and a rendered raster, then
   * hands it to the backend queue, which debounces and runs it. The note is
   * optimistically marked `pending` so the UI shows recognition is due; the
   * backend's eventual event (or a re-fetch) corrects it.
   *
   * Does nothing when automatic recognition is off — the raster is not even
   * rendered, since that is wasted work the writer did not ask for.
   */
  async scheduleAutomatic(annotationId: string, strokes: InkStroke[]): Promise<void> {
    if (!settingsStore.settings.inkAutoRecognition || !settingsStore.settings.aiEnabled) {
      return;
    }

    const snapshot = await this.buildSnapshot(strokes);
    if (!snapshot) return;

    try {
      await service.recognizeInk(annotationId, snapshot);
      this.markPending(annotationId);
    } catch {
      // A scheduling failure is not shown to the writer — the ink is safe, and
      // a transient network blip should not become a notice. The backend row
      // stays whatever it was.
    }
  }

  /**
   * Runs recognition immediately for a note, ignoring the automatic setting.
   * Used by the explicit "Recognize handwriting" action. Returns the committed
   * recognition view, or throws on failure so the caller can show a notice.
   */
  async recognizeNow(annotationId: string, strokes: InkStroke[]): Promise<InkRecognition> {
    const snapshot = await this.buildSnapshot(strokes);
    if (!snapshot) {
      throw new Error('There is not enough handwriting there to recognise yet.');
    }
    const view = await service.recognizeInkManual(annotationId, snapshot);
    this.status.set(annotationId, view);
    this.status = new Map(this.status);
    return view;
  }

  /** Saves a writer's hand-correction of a transcript. */
  async editTranscript(annotationId: string, text: string): Promise<InkRecognition> {
    const view = await service.editInkTranscript(annotationId, text);
    this.status.set(annotationId, view);
    this.status = new Map(this.status);
    return view;
  }

  /**
   * Converts an ink note's transcript into a typed text note, keeping the ink.
   * Returns the new text annotation's id.
   */
  async convertToText(annotationId: string): Promise<string> {
    return service.convertInkToText(annotationId);
  }

  /** Cancels any pending automatic recognition for a note. */
  async cancel(annotationId: string): Promise<void> {
    try {
      await service.cancelInkRecognition(annotationId);
    } catch {
      // Cancellation is best-effort; a failure here is harmless.
    }
  }

  /**
   * Builds a recognition snapshot from strokes: the strokes themselves (always,
   * as the source of truth and for the stale-rejection hash) and a rendered
   * raster (when one can be produced, for the vision recognizer). Returns `null`
   * when there is nothing meaningful to render.
   */
  private async buildSnapshot(
    strokes: InkStroke[]
  ): Promise<service.InkSnapshotInput | null> {
    if (strokes.length === 0) return null;

    const strokePayload: service.SnapshotStroke[] = strokes.map((s) => ({
      id: s.id,
      tool: s.tool,
      color: s.color,
      width: s.width,
      points: s.points,
      createdAt: s.createdAt
    }));

    // Render the raster off the main thread path. A failure to render is not
    // fatal: the strokes still travel, and a vector-capable recognizer could use
    // them. The vision recognizer will refuse if it needs an image and gets none.
    let png: Uint8Array | undefined;
    let width: number | undefined;
    let height: number | undefined;
    try {
      const raster = await renderInkPngAsync(strokes, this.surfaceWidth);
      if (raster) {
        png = raster.png;
        width = raster.width;
        height = raster.height;
      }
    } catch {
      // No raster; the snapshot carries strokes only.
    }

    return {
      strokes: strokePayload,
      surfaceWidth: this.surfaceWidth,
      png,
      width,
      height
    };
  }

  /** Marks a note pending optimistically, so the UI shows recognition is due. */
  private markPending(annotationId: string): void {
    const existing = this.status.get(annotationId);
    // Do not clobber a user-edited transcript's status with pending.
    if (existing && existing.transcriptSource === 'user-edited') return;
    const next: InkRecognition = existing
      ? { ...existing, status: 'pending', error: null }
      : {
          annotationId,
          status: 'pending',
          recognizedText: null,
          confidence: null,
          provider: null,
          model: null,
          language: null,
          transcriptSource: 'recognized',
          contentHash: null,
          error: null,
          recognizedAt: null,
          updatedAt: new Date().toISOString()
        };
    this.status.set(annotationId, next);
    this.status = new Map(this.status);
  }
}

export const inkRecognition = new InkRecognitionStore();
