/**
 * Renders an ink note's strokes to a theme-independent PNG for recognition.
 *
 * Recognition input is not presentation rendering: the goal is a clean,
 * high-contrast raster a vision model can read, regardless of whether the
 * writer's theme is paper, light, dark, or night. So the strokes are drawn on a
 * neutral light background with dark ink — never the live theme colours — and
 * cropped to their bounds with padding, so no edge stroke is clipped and no
 * unrelated UI is included.
 *
 * Pure with respect to the DOM beyond the `OffscreenCanvas`/`HTMLCanvasElement`
 * it draws into. It takes strokes and a surface width (the same logical width
 * the strokes were normalised against) and returns PNG bytes, or `null` when
 * there is nothing meaningful to render (an empty or dot-only note).
 */

import { denormalizePoint } from '$lib/ink/geometry';
import type { InkStroke } from '$lib/types/ink';

/** Padding around the stroke bounds, in pixels, so edge strokes are not clipped. */
const PADDING = 24;
/** The background the model reads against. Light and neutral. */
const BACKGROUND = '#ffffff';
/** The normalised ink colour. Dark on the light background, for any theme. */
const INK = '#111111';
/** Cap on the raster's longest side, so a long note does not produce a huge bitmap. */
const MAX_DIM = 2048;

/**
 * The bounds of a note's strokes in surface pixels, including stroke width.
 * Returns `null` when the note has no drawable points.
 */
export function strokeBounds(
  strokes: InkStroke[],
  surfaceWidth: number
): { minX: number; minY: number; maxX: number; maxY: number } | null {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  let found = false;

  for (const stroke of strokes) {
    if (stroke.points.length === 0) continue;
    const halfWidth = stroke.width / 2;
    for (const point of stroke.points) {
      const { x, y } = denormalizePoint(point, surfaceWidth);
      minX = Math.min(minX, x - halfWidth);
      minY = Math.min(minY, y - halfWidth);
      maxX = Math.max(maxX, x + halfWidth);
      maxY = Math.max(maxY, y + halfWidth);
      found = true;
    }
  }

  if (!found) return null;
  return { minX, minY, maxX, maxY };
}

/**
 * Renders strokes to PNG bytes asynchronously on a clean, theme-independent
 * background.
 *
 * Returns `null` when there is nothing meaningful to render — an empty note, or
 * one whose strokes are all single dots with no bounds — so the caller can skip
 * a model call rather than send a blank image.
 *
 * The pure bounds-and-draw logic is exercised through `strokeBounds` and the
 * injected `canvasFactory`; the only async step is the canvas's blob conversion,
 * which has no test doubles worth standing up.
 */
export async function renderInkPngAsync(
  strokes: InkStroke[],
  surfaceWidth: number,
  canvasFactory: CanvasFactory = defaultCanvasFactory
): Promise<{ png: Uint8Array; width: number; height: number } | null> {
  const bounds = strokeBounds(strokes, surfaceWidth);
  if (!bounds) return null;

  const { minX, minY, maxX, maxY } = bounds;
  const rawWidth = maxX - minX + PADDING * 2;
  const rawHeight = maxY - minY + PADDING * 2;
  if (rawWidth <= 0 || rawHeight <= 0) return null;

  const scale = Math.min(1, MAX_DIM / Math.max(rawWidth, rawHeight));
  const width = Math.max(1, Math.round(rawWidth * scale));
  const height = Math.max(1, Math.round(rawHeight * scale));

  const canvas = canvasFactory(width, height);
  if (!canvas) return null;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;

  ctx.fillStyle = BACKGROUND;
  ctx.fillRect(0, 0, width, height);
  ctx.translate(PADDING * scale, PADDING * scale);
  ctx.scale(scale, scale);
  ctx.translate(-minX, -minY);
  ctx.fillStyle = INK;
  ctx.strokeStyle = INK;
  ctx.lineCap = 'round';
  ctx.lineJoin = 'round';

  for (const stroke of strokes) {
    if (stroke.points.length === 0) continue;
    ctx.lineWidth = stroke.width;
    if (stroke.points.length === 1) {
      const p = denormalizePoint(stroke.points[0]!, surfaceWidth);
      ctx.beginPath();
      ctx.arc(p.x, p.y, stroke.width / 2, 0, Math.PI * 2);
      ctx.fill();
      continue;
    }
    ctx.beginPath();
    const first = denormalizePoint(stroke.points[0]!, surfaceWidth);
    ctx.moveTo(first.x, first.y);
    for (let i = 1; i < stroke.points.length; i++) {
      const p = denormalizePoint(stroke.points[i]!, surfaceWidth);
      ctx.lineTo(p.x, p.y);
    }
    ctx.stroke();
  }

  const blob = await canvasToPng(canvas);
  if (!blob) return null;
  const buffer = await blob.arrayBuffer();
  return { png: new Uint8Array(buffer), width, height };
}

/** A factory that produces a canvas of the given size. Injected for tests. */
export type CanvasFactory = (width: number, height: number) => CanvasLike | null;

/**
 * The subset of a canvas the rasterizer needs. Accepts both
 `OffscreenCanvasRenderingContext2D` and `CanvasRenderingContext2D` — the
 * drawing calls the rasterizer makes exist on both.
 */
export type CanvasLike = {
  width: number;
  height: number;
  getContext(contextId: '2d'): CanvasRenderingContext2D | OffscreenCanvasRenderingContext2D | null;
  convertToBlob?(options?: { type: string }): Promise<Blob | null>;
  toBlob?(callback: (blob: Blob | null) => void, type?: string, quality?: unknown): void;
};

/** Converts a canvas to a PNG blob, supporting both OffscreenCanvas and the
 *  HTMLCanvasElement `toBlob` callback shape. */
async function canvasToPng(canvas: CanvasLike): Promise<Blob | null> {
  if (canvas.convertToBlob) {
    try {
      return await canvas.convertToBlob({ type: 'image/png' });
    } catch {
      return null;
    }
  }
  if (canvas.toBlob) {
    return new Promise((resolve) => canvas.toBlob!(resolve, 'image/png'));
  }
  return null;
}

/** The default canvas factory: an `OffscreenCanvas` when available, else a
 * detached `<canvas>` element. Returns `null` in an environment with neither. */
export const defaultCanvasFactory: CanvasFactory = (width, height) => {
  if (typeof OffscreenCanvas !== 'undefined') {
    try {
      return new OffscreenCanvas(width, height);
    } catch {
      // Fall through to the element path.
    }
  }
  if (typeof document !== 'undefined') {
    const element = document.createElement('canvas');
    element.width = width;
    element.height = height;
    return element;
  }
  return null;
};
