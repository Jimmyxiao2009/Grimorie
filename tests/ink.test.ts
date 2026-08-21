/**
 * Ink geometry tests. These are the resize-stability and hit-testing guarantees
 * the feature rests on, so they are exercised directly rather than through the
 * component.
 */

import { describe, expect, it } from 'vitest';

import {
  denormalizePoint,
  distanceToStroke,
  hitTestStroke,
  normalizePoint,
  pathFromStroke,
  renderWidth,
  resolveColor,
  type Theme
} from '$lib/ink/geometry';
import type { InkStroke } from '$lib/types/ink';

function stroke(
  points: Array<[number, number, number?]>,
  opts?: Partial<InkStroke>
): InkStroke {
  return {
    id: 's1',
    tool: 'pen',
    color: 'ink-primary',
    width: 2,
    points: points.map(([x, y, pressure]) =>
      pressure === undefined ? { x, y } : { x, y, pressure }
    ),
    createdAt: '2026-01-01T00:00:00Z',
    ...opts
  };
}

describe('normalizePoint', () => {
  it('normalises x against the surface width and offsets y by the surface top', () => {
    const p = normalizePoint({ x: 140, y: 250 }, 280, 200);
    expect(p.x).toBeCloseTo(0.5, 5);
    expect(p.y).toBe(50);
  });

  it('keeps the same fractional x after the surface widens', () => {
    // Written in a 280px surface at x=140 → 0.5. After resize to 360px, the
    // stored point is still 0.5; denormalising lands at 180, the middle of the
    // new width. That is what "no drift" means.
    const stored = normalizePoint({ x: 140, y: 50 }, 280, 0);
    expect(stored.x).toBeCloseTo(0.5, 5);
    expect(denormalizePoint(stored, 360).x).toBeCloseTo(180, 5);
  });

  it('defends against a zero-width surface', () => {
    const p = normalizePoint({ x: 140, y: 50 }, 0, 0);
    expect(p.x).toBe(0);
    expect(Number.isNaN(p.x)).toBe(false);
  });
});

describe('pathFromStroke', () => {
  it('returns an empty string for a stroke with no points', () => {
    expect(pathFromStroke(stroke([]), 280)).toBe('');
  });

  it('draws a single point as a dot path', () => {
    const d = pathFromStroke(stroke([[0.5, 10]]), 280);
    expect(d).toContain('M ');
    expect(d).toContain('l 0.01 0');
  });

  it('builds a continuous path with a leading move and a quadratic curve', () => {
    const d = pathFromStroke(stroke([[0, 0], [0.5, 10], [1, 0]]), 280);
    expect(d.startsWith('M ')).toBe(true);
    // A quadratic curve command should appear for the middle point.
    expect(d).toContain(' Q ');
  });

  it('denormalises x to the surface width on render', () => {
    const d = pathFromStroke(stroke([[0.5, 0]]), 280);
    // 0.5 * 280 = 140.
    expect(d).toContain('140');
  });
});

describe('renderWidth', () => {
  it('uses the base width when no pressure is present', () => {
    expect(renderWidth(stroke([[0, 0], [0.5, 5]]))).toBe(2);
  });

  it('widens the stroke for firmer pressure', () => {
    const s = stroke([[0, 0], [0.5, 5, 1.0]], { width: 2 });
    // factor = 0.5 + 1.0 = 1.5, clamped to max 1.5.
    expect(renderWidth(s)).toBeCloseTo(3, 5);
  });

  it('narrows the stroke for lighter pressure, but not below half', () => {
    const s = stroke([[0, 0], [0.5, 5, 0.0]], { width: 2 });
    // factor = 0.5 + 0.0 = 0.5, the floor.
    expect(renderWidth(s)).toBeCloseTo(1, 5);
  });
});

describe('resolveColor', () => {
  it('maps semantic ids to theme tokens', () => {
    expect(resolveColor('ink-primary', 'paper')).toBe('var(--text-primary)');
    expect(resolveColor('ink-primary', 'dark')).toBe('var(--text-secondary)');
    expect(resolveColor('ink-red', 'night')).toBe('var(--state-danger)');
  });

  it('passes raw css colours through unchanged', () => {
    expect(resolveColor('#3e3831', 'paper')).toBe('#3e3831');
    expect(resolveColor('rgb(0 0 0)', 'light')).toBe('rgb(0 0 0)');
  });

  it('gives the highlighter a translucent fill in every theme', () => {
    const light = resolveColor('ink-highlighter', 'paper' as Theme);
    const dark = resolveColor('ink-highlighter', 'dark' as Theme);
    expect(light).toContain('0.3');
    expect(dark).toContain('0.28');
  });
});

describe('hit testing', () => {
  const strokes: InkStroke[] = [
    stroke([[0, 0], [1, 0]], { id: 'top', width: 2 }),
    stroke([[0, 100], [1, 100]], { id: 'bottom', width: 2 })
  ];

  it('finds the nearest stroke within the tolerance', () => {
    const hit = hitTestStroke(strokes, { x: 140, y: 4 }, 280, 10);
    expect(hit).toBe('top');
  });

  it('returns null when nothing is within tolerance', () => {
    const hit = hitTestStroke(strokes, { x: 140, y: 50 }, 280, 10);
    expect(hit).toBeNull();
  });

  it('picks the closer of two strokes', () => {
    const hit = hitTestStroke(strokes, { x: 140, y: 96 }, 280, 10);
    expect(hit).toBe('bottom');
  });

  it('distance is measured to the segment, not the endpoints', () => {
    // A point directly above the middle of the bottom stroke.
    const dist = distanceToStroke(strokes[1]!, { x: 140, y: 95 }, 280);
    expect(dist).toBeCloseTo(5, 5);
  });
});
