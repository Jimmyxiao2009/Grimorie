/**
 * Ink recognition store tests.
 *
 * The store is mocked at the service boundary so these run without a backend.
 * What matters: status state moves through the lifecycle, manual recognition
 * commits a transcript, a manual edit takes priority, and automatic
 * recognition is skipped when the setting is off.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';

const recognizeInk = vi.fn<(annotationId: string, snapshot: unknown) => Promise<string>>();
const recognizeInkManual = vi.fn<
  (annotationId: string, snapshot: unknown) => Promise<import('$lib/types/ink').InkRecognition>
>();
const recognitionForPage = vi.fn<(pageId: string) => Promise<import('$lib/types/ink').InkRecognition[]>>();
const editInkTranscript = vi.fn<
  (annotationId: string, text: string) => Promise<import('$lib/types/ink').InkRecognition>
>();
const convertInkToText = vi.fn<(annotationId: string) => Promise<string>>();
const cancelInkRecognition = vi.fn<(annotationId: string) => Promise<void>>();

vi.mock('$lib/ink/service', () => ({
  recognizeInk: (annotationId: string, snapshot: unknown) => recognizeInk(annotationId, snapshot),
  recognizeInkManual: (annotationId: string, snapshot: unknown) =>
    recognizeInkManual(annotationId, snapshot),
  recognitionForPage: (pageId: string) => recognitionForPage(pageId),
  editInkTranscript: (annotationId: string, text: string) => editInkTranscript(annotationId, text),
  convertInkToText: (annotationId: string) => convertInkToText(annotationId),
  cancelInkRecognition: (annotationId: string) => cancelInkRecognition(annotationId),
  // The ink-stroke service functions the store does not use, stubbed so the
  // mock module shape matches the real one for the ink store's own tests.
  createInkNote: vi.fn(),
  createAnchoredInkNote: vi.fn(),
  inkNotesForPage: vi.fn(),
  addInkStrokes: vi.fn(),
  deleteInkStroke: vi.fn(),
  deleteInkAnnotation: vi.fn(),
  recognitionStatus: vi.fn(),
  recognitionLanguage: vi.fn()
}));

// The rasterizer hits jsdom's canvas, which has no real PNG encoder. Stub it to
// return null so the store's "no raster, strokes only" path is exercised.
vi.mock('$lib/ink/rasterize', () => ({
  renderInkPngAsync: vi.fn().mockResolvedValue(null)
}));

vi.mock('$lib/stores/notices.svelte', () => ({
  notices: { failure: vi.fn(), info: vi.fn(), dismiss: vi.fn(), clear: vi.fn(), items: [] }
}));

// The event bridge, captured so a test can deliver a backend status update
// without a Tauri host. `isDesktop` is forced true so `start()` subscribes.
let statusHandler: ((event: { payload: unknown }) => void) | null = null;
const unlistenSpy = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, handler: (event: { payload: unknown }) => void) => {
    if (name === 'ink:recognition-status') statusHandler = handler;
    return Promise.resolve(unlistenSpy);
  }
}));
vi.mock('$lib/services/ipc', async (importOriginal) => ({
  ...(await importOriginal<typeof import('$lib/services/ipc')>()),
  isDesktop: true
}));

import { inkRecognition } from '$lib/stores/ink-recognition.svelte';
import { settingsStore } from '$lib/stores/settings.svelte';
import type { InkRecognition, InkStroke } from '$lib/types/ink';

function stroke(): InkStroke {
  return {
    id: 's1',
    tool: 'pen',
    color: 'ink-primary',
    width: 2,
    points: [
      { x: 0.1, y: 5 },
      { x: 0.2, y: 6 }
    ],
    createdAt: '2026-01-01T00:00:00Z'
  };
}

function recognized(text: string): InkRecognition {
  return {
    annotationId: 'note-1',
    status: 'recognized',
    recognizedText: text,
    confidence: null,
    provider: 'vision',
    model: 'gpt-4o',
    language: null,
    transcriptSource: 'recognized',
    contentHash: 'hash-1',
    error: null,
    recognizedAt: '2026-01-01T00:00:00Z',
    updatedAt: '2026-01-01T00:00:00Z'
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  inkRecognition.clear();
  // Automatic recognition is off by default; tests that need it flip it on.
  settingsStore.settings.inkAutoRecognition = false;
  settingsStore.settings.aiEnabled = false;
});

describe('ink recognition store', () => {
  it('loads recognition status for a page', async () => {
    recognitionForPage.mockResolvedValue([recognized('hello')]);
    await inkRecognition.load('page-1');
    expect(inkRecognition.forAnnotation('note-1')?.recognizedText).toBe('hello');
  });

  it('clears status when the page is null', async () => {
    recognitionForPage.mockResolvedValue([recognized('hello')]);
    await inkRecognition.load('page-1');
    await inkRecognition.load(null);
    expect(inkRecognition.forAnnotation('note-1')).toBeNull();
  });

  it('does not schedule automatic recognition when the setting is off', async () => {
    settingsStore.settings.inkAutoRecognition = false;
    await inkRecognition.scheduleAutomatic('note-1', [stroke()]);
    expect(recognizeInk).not.toHaveBeenCalled();
  });

  it('schedules automatic recognition when enabled, marking the note pending', async () => {
    settingsStore.settings.inkAutoRecognition = true;
    settingsStore.settings.aiEnabled = true;
    recognizeInk.mockResolvedValue('hash-1');

    await inkRecognition.scheduleAutomatic('note-1', [stroke()]);
    expect(recognizeInk).toHaveBeenCalledWith('note-1', expect.any(Object));
    expect(inkRecognition.forAnnotation('note-1')?.status).toBe('pending');
  });

  it('runs manual recognition and stores the committed view', async () => {
    recognizeInkManual.mockResolvedValue(recognized('machine transcript'));
    const view = await inkRecognition.recognizeNow('note-1', [stroke()]);
    expect(view.recognizedText).toBe('machine transcript');
    expect(inkRecognition.forAnnotation('note-1')?.recognizedText).toBe('machine transcript');
  });

  it('saves a manual transcript edit and updates the view', async () => {
    const edited: InkRecognition = {
      ...recognized('writer correction'),
      transcriptSource: 'user-edited'
    };
    editInkTranscript.mockResolvedValue(edited);
    const view = await inkRecognition.editTranscript('note-1', 'writer correction');
    expect(view.transcriptSource).toBe('user-edited');
    expect(inkRecognition.forAnnotation('note-1')?.recognizedText).toBe('writer correction');
    expect(editInkTranscript).toHaveBeenCalledWith('note-1', 'writer correction');
  });

  it('converts to a text note through the service', async () => {
    convertInkToText.mockResolvedValue('text-note-1');
    const id = await inkRecognition.convertToText('note-1');
    expect(id).toBe('text-note-1');
    expect(convertInkToText).toHaveBeenCalledWith('note-1');
  });

  it('cancels recognition through the service without throwing', async () => {
    cancelInkRecognition.mockRejectedValue(new Error('already gone'));
    // A failed cancel is best-effort and must not throw.
    await expect(inkRecognition.cancel('note-1')).resolves.toBeUndefined();
  });

  it('does not clobber a user-edited transcript with a pending mark', async () => {
    settingsStore.settings.inkAutoRecognition = true;
    settingsStore.settings.aiEnabled = true;
    // Seed a user-edited transcript.
    recognitionForPage.mockResolvedValue([
      { ...recognized('writer words'), transcriptSource: 'user-edited' }
    ]);
    await inkRecognition.load('page-1');

    recognizeInk.mockResolvedValue('hash-2');
    await inkRecognition.scheduleAutomatic('note-1', [stroke()]);

    // The status is not flipped to pending — the writer's edit wins.
    expect(inkRecognition.forAnnotation('note-1')?.transcriptSource).toBe('user-edited');
    expect(inkRecognition.forAnnotation('note-1')?.status).toBe('recognized');
  });
});

describe('ink recognition store — live status', () => {
  it('applies a backend status update without a refetch', async () => {
    recognitionForPage.mockResolvedValue([{ ...recognized('draft'), status: 'recognizing' }]);
    await inkRecognition.load('page-1');
    const stop = inkRecognition.start();

    expect(inkRecognition.forAnnotation('note-1')?.status).toBe('recognizing');

    // A background job finishes. Nothing is re-fetched.
    statusHandler?.({ payload: recognized('the finished transcript') });

    expect(inkRecognition.forAnnotation('note-1')?.status).toBe('recognized');
    expect(inkRecognition.forAnnotation('note-1')?.recognizedText).toBe(
      'the finished transcript'
    );
    expect(recognitionForPage).toHaveBeenCalledTimes(1);
    stop();
  });

  it('carries a failure through to the note', async () => {
    recognitionForPage.mockResolvedValue([{ ...recognized('draft'), status: 'recognizing' }]);
    await inkRecognition.load('page-1');
    const stop = inkRecognition.start();

    statusHandler?.({
      payload: { ...recognized('draft'), status: 'failed', error: 'no network' }
    });

    const row = inkRecognition.forAnnotation('note-1');
    expect(row?.status).toBe('failed');
    expect(row?.error).toBe('no network');
    stop();
  });

  it('ignores an update for a note that is not on the open page', async () => {
    recognitionForPage.mockResolvedValue([recognized('mine')]);
    await inkRecognition.load('page-1');
    const stop = inkRecognition.start();

    statusHandler?.({
      payload: { ...recognized('someone else'), annotationId: 'note-on-another-page' }
    });

    expect(inkRecognition.forAnnotation('note-on-another-page')).toBeNull();
    expect(inkRecognition.forAnnotation('note-1')?.recognizedText).toBe('mine');
    stop();
  });

  it('unsubscribes on teardown', async () => {
    const stop = inkRecognition.start();
    // The listen promise resolves on the microtask queue.
    await Promise.resolve();
    stop();
    expect(unlistenSpy).toHaveBeenCalled();
  });
});
