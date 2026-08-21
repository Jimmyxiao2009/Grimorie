/**
 * Ink geometry: the pure functions that turn pointer coordinates into stored
 * strokes, stored strokes into SVG paths, and pointer taps into eraser hits.
 *
 * Nothing here touches the DOM, IPC, or global state. That is what makes the
 * resize-stability, smoothing, and hit-testing testable without a Surface Pen.
 */

import type { InkPoint, InkStroke } from '$lib/types/ink';

/**
 * Converts a raw pointer position into stored coordinates.
 *
 * `x` is normalised against the surface width so a note written in a 280px
 * Margin lands in the same fractional place after the Margin grows to 320px.
 * `y` is kept as an absolute pixel offset from the surface top, because the
 * surface has no fixed height to normalise against and vertical position is
 * what keeps ink level with the prose beside it.
 *
 * A zero-width surface is defended against: x collapses to 0 rather than NaN,
 * so an annotation that loads before its first layout does not corrupt its
 * first point.
 */
export function normalizePoint(
  raw: { x: number; y: number },
  surfaceWidth: number,
  surfaceTop: number
): InkPoint {
  const x = surfaceWidth > 0 ? raw.x / surfaceWidth : 0;
  const y = raw.y - surfaceTop;
  return { x, y };
}

/**
 * The inverse of {@link normalizePoint}, for rendering: turns a stored point
 * back into surface pixels so it can be drawn into an SVG sized to the live
 * surface. This is run every render, which is exactly where resize-stability
 * is decided — the same stored x produces a different pixel x as the surface
 * widens, and the handwriting follows the surface rather than drifting.
 */
export function denormalizePoint(
  point: InkPoint,
  surfaceWidth: number
): { x: number; y: number } {
  return { x: point.x * surfaceWidth, y: point.y };
}

/**
 * Smooths a stroke's points into a single SVG path string using quadratic
 * Bézier interpolation between midpoints.
 *
 * Each pair of adjacent points becomes a curve whose control point is the
 * point itself and whose endpoints are the midpoints to its neighbours. The
 * result is a continuous, naturally smoothed line drawn as one element —
 * never one path per raw point.
 *
 * Degenerate cases are handled explicitly so an empty or single-point stroke
 * produces a sensible path rather than an empty string the renderer would
 * silently drop.
 */
export function pathFromStroke(stroke: InkStroke, surfaceWidth: number): string {
  const pts = stroke.points;
  if (pts.length === 0) return '';

  // A single point is a dot: a zero-length line with a round cap renders a
  // filled circle of the stroke width, so a pen tap still leaves a mark.
  if (pts.length === 1) {
    const p = denormalizePoint(pts[0]!, surfaceWidth);
    return `M ${fmt(p.x)} ${fmt(p.y)} l 0.01 0`;
  }

  const first = denormalizePoint(pts[0]!, surfaceWidth);
  const second = denormalizePoint(pts[1]!, surfaceWidth);

  let d = `M ${fmt(first.x)} ${fmt(first.y)}`;
  // If the stroke begins with a straight segment, lead in with a line so the
  // first midpoint is reached before the curves begin.
  d += ` L ${fmt((first.x + second.x) / 2)} ${fmt((first.y + second.y) / 2)}`;

  for (let i = 1; i < pts.length - 1; i++) {
    const curr = denormalizePoint(pts[i]!, surfaceWidth);
    const next = denormalizePoint(pts[i + 1]!, surfaceWidth);
    const midX = (curr.x + next.x) / 2;
    const midY = (curr.y + next.y) / 2;
    d += ` Q ${fmt(curr.x)} ${fmt(curr.y)} ${fmt(midX)} ${fmt(midY)}`;
  }

  // Finish at the final point.
  const last = denormalizePoint(pts[pts.length - 1]!, surfaceWidth);
  d += ` L ${fmt(last.x)} ${fmt(last.y)}`;
  return d;
}

/**
 * The stroke width to render with, optionally widened by pressure.
 *
 * Pressure is treated as a multiplier around the base width, clamped to a
 * restrained range: calligraphy is not the goal, only a faint response to how
 * hard the pen pressed. When pressure is absent (a mouse, or a WebView that
 * does not expose it), the base width is used unchanged.
 */
export function renderWidth(stroke: InkStroke): number {
  if (stroke.points.length === 0) return stroke.width;
  // The last point's pressure governs the visible tail of the stroke, which is
  // where the eye lands.
  const pressure = stroke.points[stroke.points.length - 1]!.pressure;
  if (pressure === undefined) return stroke.width;
  // Map default pen pressure (0.5) to the base width, and allow ±50%.
  const factor = 0.5 + pressure;
  return stroke.width * Math.max(0.5, Math.min(factor, 1.5));
}

/**
 * Resolves a semantic colour id to a CSS colour for a given theme, falling
 * back to the raw value if it is already a colour or an unknown id.
 *
 * Ink is stored with semantic ids (`ink-primary`, `ink-highlighter`) so a
 * graphite stroke stays visible against both paper and night without the
 * writer choosing a colour per theme. A custom colour picked from the palette
 * is stored as its raw CSS value and used directly.
 */
export function resolveColor(id: string, theme: Theme): string {
  switch (id) {
    case 'ink-primary':
      // Graphite in light themes, a lighter ink in dark ones so the same stroke
      // reads on both.
      return theme === 'dark' || theme === 'night'
        ? 'var(--text-secondary)'
        : 'var(--text-primary)';
    case 'ink-highlighter':
      // A muted warm wash, low-opacity so it sits behind the pen.
      return theme === 'dark' || theme === 'night'
        ? 'rgb(205 162 92 / 0.28)'
        : 'rgb(168 131 47 / 0.30)';
    case 'ink-red':
      return 'var(--state-danger)';
    case 'ink-blue':
      return 'var(--accent)';
    default:
      // A raw CSS colour, used as-is.
      return id;
  }
}

/** The themes that change ink's resolved colour. */
export type Theme = 'paper' | 'light' | 'dark' | 'night';

/**
 * The distance from a point to a stroke, in surface pixels.
 *
 * Used by the eraser: a tap near a stroke removes the whole stroke. The
 * distance is the minimum over every segment of the stroke's polyline, with a
 * small tolerance so the writer does not have to hit the exact line. Points
 * are denormalised first so the test runs in the same pixel space as the
 * pointer.
 */
export function distanceToStroke(
  stroke: InkStroke,
  point: { x: number; y: number },
  surfaceWidth: number
): number {
  const pts = stroke.points;
  if (pts.length === 0) return Infinity;
  if (pts.length === 1) {
    const p = denormalizePoint(pts[0]!, surfaceWidth);
    return Math.hypot(p.x - point.x, p.y - point.y);
  }

  let min = Infinity;
  for (let i = 0; i < pts.length - 1; i++) {
    const a = denormalizePoint(pts[i]!, surfaceWidth);
    const b = denormalizePoint(pts[i + 1]!, surfaceWidth);
    min = Math.min(min, distanceToSegment(point, a, b));
  }
  return min;
}

/**
 * Whether a pointer position hits any stroke in a note, within the eraser's
 * tolerance. Returns the hit stroke's id, or null — so the eraser removes one
 * stroke per gesture rather than everything nearby.
 */
export function hitTestStroke(
  strokes: InkStroke[],
  point: { x: number; y: number },
  surfaceWidth: number,
  tolerance: number
): string | null {
  let best: string | null = null;
  let bestDist = Infinity;
  for (const stroke of strokes) {
    const dist = distanceToStroke(stroke, point, surfaceWidth);
    if (dist <= tolerance && dist < bestDist) {
      bestDist = dist;
      best = stroke.id;
    }
  }
  return best;
}

// The distance from point p to segment a–b.
function distanceToSegment(
  p: { x: number; y: number },
  a: { x: number; y: number },
  b: { x: number; y: number }
): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const lenSq = dx * dx + dy * dy;
  // A zero-length segment collapses to the distance to its single point.
  if (lenSq === 0) return Math.hypot(p.x - a.x, p.y - a.y);

  // Clamp the projection to the segment.
  let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / lenSq;
  t = Math.max(0, Math.min(1, t));
  const cx = a.x + t * dx;
  const cy = a.y + t * dy;
  return Math.hypot(p.x - cx, p.y - cy);
}

// Formats a number for an SVG path coordinate: enough precision to render
// smoothly, few enough decimals to keep the DOM string small.
function fmt(n: number): number {
  // Rounding to two decimals is sub-pixel and keeps path strings compact when
  // a long stroke holds hundreds of points.
  return Math.round(n * 100) / 100;
}
