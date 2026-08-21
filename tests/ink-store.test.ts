/**
 * Ink store tests: the undo/redo history and the persistence-failure guarantee.
 *
 * The store is mocked at the service boundary so these run without a backend.
 * What matters here is the behaviour the feature is judged on: undo removes a
 * stroke, redo restores it, and a failed save never silently discards
 * handwriting.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

const createInkNote = vi.fn<(pageId: string) => Promise<import('$lib/types/annotation').Annotation>>();
const addInkStrokes = vi.fn<(annotationId: string, strokes: unknown[]) => Promise<void>>();
const deleteInkStroke = vi.fn<(id: string) => Promise<void>>();
const inkNotesForPage = vi.fn<(pageId: string) => Promise<import('$lib/types/ink').InkNote[]>>();

vi.mock('$lib/ink/service', () => ({
  createInkNote: (pageId: string) => createInkNote(pageId),
  addInkStrokes: (annotationId: string, strokes: unknown[]) => addInkStrokes(annotationId, strokes),
  deleteInkStroke: (id: string) => deleteInkStroke(id),
  inkNotesForPage: (pageId: string) => inkNotesForPage(pageId),
  createAnchoredInkNote: vi.fn(),
  deleteInkAnnotation: vi.fn()
}));

// Suppress the failure toasts the store raises on save errors.
vi.mock('$lib/stores/notices.svelte', () => ({
  notices: { failure: vi.fn(), info: vi.fn(), dismiss: vi.fn(), clear: vi.fn(), items: [] }
}));

// The recognition store is mocked so the ink store's scheduling calls can be
// inspected directly — what it hands recognition is a correctness question, not
// an implementation detail.
const scheduleAutomatic = vi.fn<(annotationId: string, strokes: InkStroke[]) => Promise<void>>();
vi.mock('$lib/stores/ink-recognition.svelte', () => ({
  inkRecognition: {
    scheduleAutomatic: (annotationId: string, strokes: InkStroke[]) =>
      scheduleAutomatic(annotationId, strokes),
    load: vi.fn(),
    clear: vi.fn(),
    setSurfaceWidth: vi.fn(),
    forAnnotation: vi.fn()
  }
}));

import { ink } from '$lib/stores/ink.svelte';
import type { InkStroke } from '$lib/types/ink';

function annotation(id: string): import('$lib/types/annotation').Annotation {
  return {
    id,
    pageId: 'page-1',
    kind: 'ink',
    status: 'active',
    body: '',
    target: { kind: 'page' },
    authorProfile: null,
    createdAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z'
  };
}

function stroke(id: string): InkStroke {
  return {
    id,
    tool: 'pen',
    color: 'ink-primary',
    width: 2,
    points: [{ x: 0.1, y: 5 }, { x: 0.5, y: 6 }],
    createdAt: '2026-01-01T00:00:00Z'
  };
}

beforeEach(async () => {
  vi.clearAllMocks();
  ink.clear();
  // Reset session tool state explicitly — clear() does not, because pen mode
  // survives a page switch in production. Tests need a known starting point.
  while (ink.penMode) ink.togglePenMode();
  ink.setTool('pen');
  ink.setColor('ink-primary');

  // Load seeds pageId and the active note through the real load path, so
  // commitStroke's page guard passes.
  inkNotesForPage.mockResolvedValue([
    { annotation: annotation('note-1'), strokes: [] }
  ]);
  await ink.load('page-1');
});

describe('ink store — undo and redo', () => {
  it('undo removes the last committed stroke and redo brings it back', async () => {
    addInkStrokes.mockResolvedValue(undefined);
    deleteInkStroke.mockResolvedValue(undefined);

    await ink.commitStroke(stroke('s1'));
    expect(ink.strokesFor('note-1').length).toBe(1);

    await ink.undo();
    expect(ink.strokesFor('note-1').length).toBe(0);
    expect(deleteInkStroke).toHaveBeenCalledWith('s1');

    await ink.redo();
    expect(ink.strokesFor('note-1').length).toBe(1);
    expect(addInkStrokes).toHaveBeenCalledTimes(2);
  });

  it('undo on an erase restores the stroke', async () => {
    addInkStrokes.mockResolvedValue(undefined);
    deleteInkStroke.mockResolvedValue(undefined);

    await ink.commitStroke(stroke('s1'));
    // Erase it via the store's erase path.
    deleteInkStroke.mockClear();
    await ink.eraseAt({ x: 1000, y: 6 }, 2000); // x=0.5*2000=1000, near the stroke
    expect(ink.strokesFor('note-1').length).toBe(0);

    await ink.undo();
    expect(ink.strokesFor('note-1').length).toBe(1);
  });

  it('canUndo and canRedo reflect the history', async () => {
    addInkStrokes.mockResolvedValue(undefined);
    deleteInkStroke.mockResolvedValue(undefined);

    expect(ink.canUndo).toBe(false);
    await ink.commitStroke(stroke('s1'));
    expect(ink.canUndo).toBe(true);
    expect(ink.canRedo).toBe(false);

    await ink.undo();
    expect(ink.canUndo).toBe(false);
    expect(ink.canRedo).toBe(true);

    await ink.redo();
    expect(ink.canUndo).toBe(true);
    expect(ink.canRedo).toBe(false);
  });
});

describe('ink store — failure handling', () => {
  it('keeps the stroke visible when persistence fails and flags the error', async () => {
    addInkStrokes.mockRejectedValueOnce(new Error('disk full'));

    await ink.commitStroke(stroke('s1'));

    // The stroke is still on screen — never silently discarded.
    expect(ink.strokesFor('note-1').length).toBe(1);
    expect(ink.saveError).toBe(true);
  });

  it('clears the error flag once a subsequent save succeeds', async () => {
    addInkStrokes.mockRejectedValueOnce(new Error('disk full'));
    await ink.commitStroke(stroke('s1'));
    expect(ink.saveError).toBe(true);

    addInkStrokes.mockResolvedValue(undefined);
    await ink.commitStroke(stroke('s2'));
    expect(ink.saveError).toBe(false);
    expect(ink.strokesFor('note-1').length).toBe(2);
  });
});

describe('ink store — what recognition is given', () => {
  it('sends the whole note for recognition, not just the stroke just drawn', async () => {
    // This is the defect that silently disabled automatic recognition: a
    // snapshot of one stroke hashes differently from the note the backend
    // re-reads, so every result on a multi-stroke note was rejected as stale.
    addInkStrokes.mockResolvedValue(undefined);

    await ink.commitStroke(stroke('s1'));
    await ink.commitStroke(stroke('s2'));
    await ink.commitStroke(stroke('s3'));

    const lastCall = scheduleAutomatic.mock.calls.at(-1);
    expect(lastCall?.[0]).toBe('note-1');
    expect(lastCall?.[1].map((s) => s.id)).toEqual(['s1', 's2', 's3']);
  });

  it('sends the remaining strokes after an erase', async () => {
    addInkStrokes.mockResolvedValue(undefined);
    deleteInkStroke.mockResolvedValue(undefined);

    await ink.commitStroke(stroke('s1'));
    await ink.commitStroke(stroke('s2'));
    scheduleAutomatic.mockClear();

    await ink.eraseAt({ x: 1000, y: 6 }, 2000);

    const lastCall = scheduleAutomatic.mock.calls.at(-1);
    expect(lastCall?.[1].length).toBe(1);
  });

  it('re-recognises after undo, so a transcript never outlives its ink', async () => {
    // A transcript that no longer matches the handwriting is worse than none:
    // it is confidently wrong, and it is what search and the AI context use.
    addInkStrokes.mockResolvedValue(undefined);
    deleteInkStroke.mockResolvedValue(undefined);

    await ink.commitStroke(stroke('s1'));
    await ink.commitStroke(stroke('s2'));
    scheduleAutomatic.mockClear();

    await ink.undo();

    expect(scheduleAutomatic).toHaveBeenCalledTimes(1);
    expect(scheduleAutomatic.mock.calls.at(-1)?.[1].map((s) => s.id)).toEqual(['s1']);
  });

  it('re-recognises after redo', async () => {
    addInkStrokes.mockResolvedValue(undefined);
    deleteInkStroke.mockResolvedValue(undefined);

    await ink.commitStroke(stroke('s1'));
    await ink.undo();
    scheduleAutomatic.mockClear();

    await ink.redo();

    expect(scheduleAutomatic.mock.calls.at(-1)?.[1].map((s) => s.id)).toEqual(['s1']);
  });

  it('does not re-recognise when an undo failed to persist', async () => {
    addInkStrokes.mockResolvedValue(undefined);
    await ink.commitStroke(stroke('s1'));
    scheduleAutomatic.mockClear();

    deleteInkStroke.mockRejectedValueOnce(new Error('disk full'));
    await ink.undo();

    // The stroke was restored, so the ink did not actually change.
    expect(ink.strokesFor('note-1').length).toBe(1);
    expect(scheduleAutomatic).not.toHaveBeenCalled();
  });

  it('does not schedule recognition when the stroke failed to persist', async () => {
    // Recognising ink the backend never stored would compare a snapshot hash
    // against strokes that are not there.
    addInkStrokes.mockRejectedValueOnce(new Error('disk full'));
    await ink.commitStroke(stroke('s1'));
    expect(scheduleAutomatic).not.toHaveBeenCalled();
  });
});

describe('ink store — tool state', () => {
  it('selecting a tool turns pen mode on', () => {
    expect(ink.penMode).toBe(false);
    ink.setTool('highlighter');
    ink.togglePenMode();
    expect(ink.penMode).toBe(true);
    expect(ink.tool).toBe('highlighter');
    expect(ink.activeColor).toBe('ink-highlighter');
  });

  it('leaving pen mode resets the tool to pen', () => {
    ink.togglePenMode();
    ink.setTool('eraser');
    ink.togglePenMode();
    expect(ink.penMode).toBe(false);
    expect(ink.tool).toBe('pen');
  });

  it('the highlighter draws wider than the pen', () => {
    ink.setTool('highlighter');
    expect(ink.activeWidth).toBeGreaterThan(2);
    ink.setTool('pen');
    expect(ink.activeWidth).toBe(2);
  });
});
