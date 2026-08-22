/**
 * Ink surface pointer tests.
 *
 * The geometry is covered in `ink.test.ts` without a component; what needs the
 * real component is the *pointer state machine*, because its failure mode is
 * invisible in isolation: if a stroke ends without clearing the surface's
 * pointer bookkeeping, `onPointerDown` refuses to start every subsequent
 * stroke and the writing surface goes quietly dead.
 *
 * jsdom implements neither PointerEvent nor pointer capture, so both are stood
 * up here — narrowly, and only to the extent the component actually uses them.
 * These are environment gaps, not behaviour under test: the capture stub tracks
 * real state so a leaked capture would show up.
 */

import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/svelte';

const addInkStrokes = vi.fn<(annotationId: string, strokes: unknown[]) => Promise<void>>();
const deleteInkStroke = vi.fn<(id: string) => Promise<void>>();
const inkNotesForPage = vi.fn<(pageId: string) => Promise<import('$lib/types/ink').InkNote[]>>();

vi.mock('$lib/ink/service', () => ({
  createInkNote: vi.fn(),
  addInkStrokes: (annotationId: string, strokes: unknown[]) => addInkStrokes(annotationId, strokes),
  deleteInkStroke: (id: string) => deleteInkStroke(id),
  inkNotesForPage: (pageId: string) => inkNotesForPage(pageId),
  createAnchoredInkNote: vi.fn(),
  deleteInkAnnotation: vi.fn()
}));

vi.mock('$lib/stores/notices.svelte', () => ({
  notices: { failure: vi.fn(), info: vi.fn(), dismiss: vi.fn(), clear: vi.fn(), items: [] }
}));

vi.mock('$lib/stores/ink-recognition.svelte', () => ({
  inkRecognition: {
    scheduleAutomatic: vi.fn(),
    load: vi.fn(),
    clear: vi.fn(),
    setSurfaceWidth: vi.fn(),
    forAnnotation: vi.fn()
  }
}));

import InkSurface from '$lib/ink/InkSurface.svelte';
import { ink } from '$lib/stores/ink.svelte';

// --- environment gaps -------------------------------------------------------

// jsdom has no ResizeObserver; the surface observes itself to track its width.
class StubResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
}
vi.stubGlobal('ResizeObserver', StubResizeObserver);

/** The pointer ids each element currently holds capture for. */
const captured = new Map<Element, Set<number>>();

function installPointerCapture() {
  Element.prototype.setPointerCapture = function (id: number) {
    if (!captured.has(this)) captured.set(this, new Set());
    captured.get(this)!.add(id);
  };
  Element.prototype.releasePointerCapture = function (id: number) {
    captured.get(this)?.delete(id);
  };
  Element.prototype.hasPointerCapture = function (id: number) {
    return captured.get(this)?.has(id) ?? false;
  };
}
installPointerCapture();

type PointerInit = {
  pointerId?: number;
  pointerType?: string;
  pressure?: number;
  clientX?: number;
  clientY?: number;
  button?: number;
};

/**
 * A minimal PointerEvent. jsdom does not provide one, and the component reads
 * `pointerId`, `pointerType`, `pressure` and `getCoalescedEvents`.
 */
function pointerEvent(type: string, init: PointerInit = {}): PointerEvent {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: init.clientX ?? 0,
    clientY: init.clientY ?? 0,
    button: init.button ?? 0
  });
  Object.defineProperties(event, {
    pointerId: { value: init.pointerId ?? 1 },
    pointerType: { value: init.pointerType ?? 'pen' },
    pressure: { value: init.pressure ?? 0.5 },
    getCoalescedEvents: { value: () => [] }
  });
  return event as PointerEvent;
}

function surfaceOf(container: HTMLElement): SVGSVGElement {
  return container.querySelector('svg.ink-surface') as SVGSVGElement;
}

/** Draws a complete stroke and returns the surface it was drawn on. */
function draw(svg: SVGSVGElement, pointerId = 1) {
  svg.dispatchEvent(pointerEvent('pointerdown', { pointerId, clientX: 10, clientY: 10 }));
  svg.dispatchEvent(pointerEvent('pointermove', { pointerId, clientX: 40, clientY: 20 }));
  svg.dispatchEvent(pointerEvent('pointerup', { pointerId, clientX: 40, clientY: 20 }));
}

beforeEach(async () => {
  vi.clearAllMocks();
  captured.clear();
  ink.clear();
  while (!ink.penMode) ink.togglePenMode();
  ink.setTool('pen');

  addInkStrokes.mockResolvedValue(undefined);
  deleteInkStroke.mockResolvedValue(undefined);
  inkNotesForPage.mockResolvedValue([
    {
      annotation: {
        id: 'note-1',
        pageId: 'page-1',
        kind: 'ink',
        status: 'active',
        body: '',
        target: { kind: 'page' },
        authorProfile: null,
        createdAt: '2026-01-01T00:00:00Z',
        updatedAt: '2026-01-01T00:00:00Z'
      },
      strokes: []
    }
  ]);
  await ink.load('page-1');
});

describe('ink surface — input discipline', () => {
  it('draws with a pen', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    draw(surfaceOf(container));
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
  });

  it('draws with touch instead of handing the gesture to page scrolling', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    svg.dispatchEvent(
      pointerEvent('pointerdown', { pointerId: 9, pointerType: 'touch', clientX: 10, clientY: 10 })
    );
    svg.dispatchEvent(
      pointerEvent('pointermove', { pointerId: 9, pointerType: 'touch', clientX: 60, clientY: 60 })
    );
    svg.dispatchEvent(pointerEvent('pointerup', { pointerId: 9, pointerType: 'touch' }));

    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
  });

  it('ignores a second pointer landing mid-stroke', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    svg.dispatchEvent(pointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10 }));
    // A palm arriving as a second pen pointer must not start its own stroke.
    svg.dispatchEvent(pointerEvent('pointerdown', { pointerId: 2, clientX: 80, clientY: 80 }));
    svg.dispatchEvent(pointerEvent('pointermove', { pointerId: 2, clientX: 90, clientY: 90 }));
    svg.dispatchEvent(pointerEvent('pointerup', { pointerId: 1, clientX: 40, clientY: 20 }));

    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
  });

  it('does not draw when pen mode is off', () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: false });
    draw(surfaceOf(container));
    expect(ink.strokesFor('note-1').length).toBe(0);
  });

  it('records pen pressure but not the placeholder a mouse reports', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    svg.dispatchEvent(
      pointerEvent('pointerdown', { pointerId: 1, pressure: 0.8, clientX: 10, clientY: 10 })
    );
    svg.dispatchEvent(pointerEvent('pointerup', { pointerId: 1, clientX: 10, clientY: 10 }));
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
    expect(ink.strokesFor('note-1')[0]!.points[0]!.pressure).toBe(0.8);

    // A mouse reports a constant 0.5 that means nothing about how hard anyone
    // pressed, so it is not stored as though it did.
    svg.dispatchEvent(
      pointerEvent('pointerdown', {
        pointerId: 2,
        pointerType: 'mouse',
        clientX: 20,
        clientY: 20
      })
    );
    svg.dispatchEvent(pointerEvent('pointerup', { pointerId: 2, pointerType: 'mouse' }));
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(2));
    expect(ink.strokesFor('note-1')[1]!.points[0]!.pressure).toBeUndefined();
  });
});

describe('ink surface — a stroke never wedges the surface', () => {
  it('releases pointer capture when a stroke finishes', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);
    draw(svg);
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
    expect(svg.hasPointerCapture(1)).toBe(false);
  });

  it('keeps accepting strokes after a cancellation', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    svg.dispatchEvent(pointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10 }));
    svg.dispatchEvent(pointerEvent('pointermove', { pointerId: 1, clientX: 30, clientY: 15 }));
    // Interrupted: the partial mark is dropped rather than persisted.
    svg.dispatchEvent(pointerEvent('pointercancel', { pointerId: 1 }));
    expect(ink.strokesFor('note-1').length).toBe(0);
    expect(svg.hasPointerCapture(1)).toBe(false);

    // The surface is still alive.
    draw(svg, 2);
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
  });

  it('keeps the stroke and recovers when capture is lost without a pointerup', async () => {
    // The case that wedged the surface: the browser drops capture on its own —
    // a system gesture, focus loss, the element detaching — and no pointerup or
    // pointercancel ever arrives.
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    svg.dispatchEvent(pointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10 }));
    svg.dispatchEvent(pointerEvent('pointermove', { pointerId: 1, clientX: 30, clientY: 15 }));
    svg.dispatchEvent(pointerEvent('lostpointercapture', { pointerId: 1 }));

    // The pen really did draw it, so it is kept.
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));

    // And crucially the surface still accepts a new stroke.
    draw(svg, 2);
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(2));
  });

  it('is unbothered by lostpointercapture arriving after a normal pointerup', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    draw(svg);
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
    // The browser fires this on implicit release; it must not double-commit.
    svg.dispatchEvent(pointerEvent('lostpointercapture', { pointerId: 1 }));
    expect(ink.strokesFor('note-1').length).toBe(1);
  });

  it('takes no capture for the eraser', () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    ink.setTool('eraser');
    svg.dispatchEvent(pointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10 }));

    // No ongoing gesture to follow, so nothing is held — and the next stroke
    // is not blocked by bookkeeping the eraser left behind.
    expect(svg.hasPointerCapture(1)).toBe(false);
  });

  it('stays usable after erasing, so the eraser cannot dead-end the surface', async () => {
    const { container } = render(InkSurface, { annotationId: 'note-1', active: true });
    const svg = surfaceOf(container);

    ink.setTool('eraser');
    svg.dispatchEvent(pointerEvent('pointerdown', { pointerId: 1, clientX: 10, clientY: 10 }));
    svg.dispatchEvent(pointerEvent('pointerup', { pointerId: 1, clientX: 10, clientY: 10 }));

    ink.setTool('pen');
    draw(svg, 2);
    await vi.waitFor(() => expect(ink.strokesFor('note-1').length).toBe(1));
  });
});
