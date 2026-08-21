/**
 * Rasterizer tests: the pure stroke-bounds calculation, which is what decides
 * whether a note is worth recognising and where its recognition image is cropped.
 *
 * The canvas drawing and PNG encoding are not tested directly — they have no
 * reliable test double in jsdom — but `strokeBounds` is the geometry that
 * matters: it must include stroke width, tolerate a single dot, and reject an
 * empty note.
 */

import { describe, expect, it } from 'vitest';
import { recognitionScale, renderInkPngAsync, strokeBounds } from '$lib/ink/rasterize';
import type { InkStroke, InkTool } from '$lib/types/ink';

function stroke(points: { x: number; y: number }[], width = 2): InkStroke {
  return {
    id: 's1',
    tool: 'pen',
    color: 'ink-primary',
    width,
    points,
    createdAt: '2026-01-01T00:00:00Z'
  };
}

function toolStroke(id: string, tool: InkTool, width: number): InkStroke {
  return {
    id,
    tool,
    color: tool === 'highlighter' ? 'ink-highlighter' : 'ink-primary',
    width,
    points: [
      { x: 0.1, y: 10 },
      { x: 0.6, y: 12 }
    ],
    createdAt: '2026-01-01T00:00:00Z'
  };
}

/**
 * A canvas that records what was drawn, so the recognition image's *content*
 * can be asserted without a real 2D backend. jsdom has no PNG encoder, which is
 * why the rasterizer takes an injectable factory in the first place.
 */
function recordingCanvas() {
  const painted: { color: string; lineWidth: number }[] = [];
  const ctx = {
    fillStyle: '',
    strokeStyle: '',
    lineWidth: 0,
    lineCap: '',
    lineJoin: '',
    fillRect: () => {},
    translate: () => {},
    scale: () => {},
    beginPath: () => {},
    moveTo: () => {},
    lineTo: () => {},
    arc: () => {},
    fill() {
      painted.push({ color: String(ctx.fillStyle), lineWidth: ctx.lineWidth });
    },
    stroke() {
      painted.push({ color: String(ctx.strokeStyle), lineWidth: ctx.lineWidth });
    }
  };
  const factory = (width: number, height: number) => ({
    width,
    height,
    getContext: () => ctx as unknown as CanvasRenderingContext2D,
    toBlob: (cb: (blob: Blob | null) => void) => cb(new Blob([new Uint8Array([1])]))
  });
  return { painted, factory };
}

describe('strokeBounds', () => {
  it('returns null for an empty note', () => {
    expect(strokeBounds([], 300)).toBeNull();
    expect(strokeBounds([stroke([], 2)], 300)).toBeNull();
  });

  it('bounds a single stroke, including its width', () => {
    const surfaceWidth = 300;
    const s = stroke(
      [
        { x: 0.1, y: 5 }, // 30px, 5px
        { x: 0.2, y: 25 } // 60px, 25px
      ],
      4
    );
    const bounds = strokeBounds([s], surfaceWidth)!;
    // Half-width is 2: the bounds extend 2px beyond the extreme points.
    expect(bounds.minX).toBe(30 - 2);
    expect(bounds.maxX).toBe(60 + 2);
    expect(bounds.minY).toBe(5 - 2);
    expect(bounds.maxY).toBe(25 + 2);
  });

  it('bounds multiple strokes together', () => {
    const surfaceWidth = 100;
    const a = stroke([{ x: 0.0, y: 0 }, { x: 0.5, y: 0 }], 2);
    const b = stroke([{ x: 0.5, y: 100 }, { x: 1.0, y: 100 }], 2);
    const bounds = strokeBounds([a, b], surfaceWidth)!;
    expect(bounds.minX).toBe(0 - 1);
    expect(bounds.maxX).toBe(100 + 1);
    expect(bounds.minY).toBe(0 - 1);
    expect(bounds.maxY).toBe(100 + 1);
  });

  it('bounds a single-dot stroke as a circle of the stroke width', () => {
    const s = stroke([{ x: 0.5, y: 50 }], 6);
    const bounds = strokeBounds([s], 200)!;
    // A dot at 100,50 with width 6 bounds to 97..103 in x and 47..53 in y.
    expect(bounds.minX).toBe(100 - 3);
    expect(bounds.maxX).toBe(100 + 3);
    expect(bounds.minY).toBe(50 - 3);
    expect(bounds.maxY).toBe(50 + 3);
  });
});

describe('recognitionScale', () => {
  it('magnifies a margin-sized note instead of sending a thumbnail', () => {
    // The case that matters: a three-line note in a 280px Margin. Sent at
    // native size a vision model gets ~240x90, which is below what handwriting
    // — and CJK especially — can be read at.
    const scale = recognitionScale(240, 90);
    expect(scale).toBeGreaterThan(3);
    expect(240 * scale).toBeCloseTo(1024, 0);
  });

  it('never exceeds the cap, even chasing the floor', () => {
    // A long, thin underline: the floor would happily magnify it forever.
    const scale = recognitionScale(4000, 8);
    expect(4000 * scale).toBeLessThanOrEqual(2048);
  });

  it('reduces a note that is larger than the cap', () => {
    const scale = recognitionScale(5000, 400);
    expect(scale).toBeLessThan(1);
    expect(5000 * scale).toBeCloseTo(2048, 0);
  });

  it('leaves a note between the floor and the cap at its natural size', () => {
    expect(recognitionScale(1500, 600)).toBe(1);
  });

  it('keys off the longest side, so a thin note is not blown up to the cap', () => {
    // Scaling by the shortest side would take this to the cap for no gain.
    const scale = recognitionScale(900, 10);
    expect(900 * scale).toBeCloseTo(1024, 0);
  });

  it('is defensive about a degenerate size', () => {
    expect(recognitionScale(0, 0)).toBe(1);
  });
});

describe('the recognition image', () => {
  it('draws a highlighter as a pale wash, not in the pen ink', async () => {
    // A highlighter is five times the pen's width. Drawn in the pen's ink it
    // is a solid bar through the handwriting it was meant to mark, and the
    // model reads a redaction.
    const { painted, factory } = recordingCanvas();
    await renderInkPngAsync(
      [toolStroke('pen-1', 'pen', 2), toolStroke('hl-1', 'highlighter', 10)],
      300,
      factory
    );

    const highlighter = painted.find((p) => p.lineWidth === 10)!;
    const pen = painted.find((p) => p.lineWidth === 2)!;
    expect(highlighter.color).not.toBe(pen.color);
    // The pen is the dark mark; the highlighter is lighter than it.
    expect(pen.color).toBe('#111111');
    expect(highlighter.color).not.toBe('#111111');
  });

  it('draws highlighters behind pen strokes, whatever order they were drawn in', async () => {
    // The SVG surface layers highlighters behind pens. If the raster used raw
    // array order, a highlight applied *after* a word would cover it here but
    // not on screen — the model would read something the writer never saw.
    const { painted, factory } = recordingCanvas();
    await renderInkPngAsync(
      [toolStroke('pen-1', 'pen', 2), toolStroke('hl-1', 'highlighter', 10)],
      300,
      factory
    );

    expect(painted.map((p) => p.lineWidth)).toEqual([10, 2]);
  });

  it('reports the dimensions it rendered at', async () => {
    const { factory } = recordingCanvas();
    const result = await renderInkPngAsync([toolStroke('pen-1', 'pen', 2)], 300, factory);
    expect(result).not.toBeNull();
    // A 150px-wide mark in a 300px surface is magnified toward the floor.
    expect(result!.width).toBeGreaterThan(1000);
    expect(result!.png.length).toBeGreaterThan(0);
  });

  it('returns null for a note with nothing in it', async () => {
    const { factory } = recordingCanvas();
    expect(await renderInkPngAsync([], 300, factory)).toBeNull();
  });
});
