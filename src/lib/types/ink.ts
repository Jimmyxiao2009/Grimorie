/**
 * Ink types — the frontend mirror of the Rust `domain::ink` module.
 *
 * The same coordinate contract holds on both sides of the boundary, and it is
 * what makes handwriting survive resize:
 *
 * - `x` is normalised to `[0, 1]` against the ink surface's width.
 * - `y` is an absolute pixel offset from the top of the surface.
 *
 * See the Rust module docs for the full rationale.
 */

import type { Id, Timestamp } from '$lib/types/manuscript';
import type { Annotation } from '$lib/types/annotation';

export type InkTool = 'pen' | 'highlighter';

export type InkPoint = {
  /** Normalised horizontal position, `[0, 1]`, across the ink surface. */
  x: number;
  /** Absolute vertical position, in surface pixels, from the top. */
  y: number;
  pressure?: number;
  timestamp?: number;
};

export type InkStroke = {
  id: Id;
  tool: InkTool;
  /** A semantic colour id, or a raw CSS colour for custom picks. */
  color: string;
  /** Base stroke width in surface pixels. Pressure modulates around it. */
  width: number;
  points: InkPoint[];
  createdAt: Timestamp;
};

/** An ink annotation plus its strokes, as returned by `ink_notes_for_page`. */
export type InkNote = {
  annotation: Annotation;
  strokes: InkStroke[];
};
