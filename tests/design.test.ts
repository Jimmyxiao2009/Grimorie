import { describe, expect, it } from 'vitest';
import {
  BREAKPOINT_MEDIUM,
  BREAKPOINT_WIDE,
  _internal as viewportInternal
} from '../src/lib/design/viewport.svelte';
import { THEME_CHOICES, _internal as themeInternal } from '../src/lib/design/theme.svelte';
import { ICONS, ICON_NAMES } from '../src/lib/design/icons';

const { classify } = viewportInternal;
const { resolve, isThemeChoice } = themeInternal;

describe('viewport breakpoints', () => {
  it('classifies the three layout states', () => {
    expect(classify(600)).toBe('narrow');
    expect(classify(1024)).toBe('medium');
    expect(classify(1440)).toBe('wide');
  });

  it('treats the breakpoints as inclusive lower bounds', () => {
    expect(classify(BREAKPOINT_MEDIUM - 1)).toBe('narrow');
    expect(classify(BREAKPOINT_MEDIUM)).toBe('medium');
    expect(classify(BREAKPOINT_WIDE - 1)).toBe('medium');
    expect(classify(BREAKPOINT_WIDE)).toBe('wide');
  });

  it('places both Surface Go orientations in a primary state', () => {
    // ~1024 CSS px landscape, ~768 portrait. Neither may collapse to a
    // fallback: both are reference orientations for the device.
    expect(classify(1024)).toBe('medium');
    expect(classify(768)).toBe('narrow');
  });
});

describe('theme resolution', () => {
  it('never leaves "system" unresolved', () => {
    for (const prefersDark of [true, false]) {
      const resolved = resolve('system', prefersDark);
      expect(resolved).not.toBe('system');
      expect(THEME_CHOICES).toContain(resolved);
    }
  });

  it('resolves system to Grimoire’s own reading themes', () => {
    expect(resolve('system', false)).toBe('paper');
    expect(resolve('system', true)).toBe('night');
  });

  it('leaves an explicit choice alone regardless of the OS setting', () => {
    expect(resolve('light', true)).toBe('light');
    expect(resolve('night', false)).toBe('night');
    expect(resolve('paper', true)).toBe('paper');
  });

  it('rejects unknown cached values', () => {
    expect(isThemeChoice('sepia')).toBe(false);
    expect(isThemeChoice(null)).toBe(false);
    expect(isThemeChoice('night')).toBe(true);
  });
});

describe('icons', () => {
  it('exposes every icon by name', () => {
    expect(ICON_NAMES.length).toBe(Object.keys(ICONS).length);
    expect(ICON_NAMES.length).toBeGreaterThan(20);
  });

  it('defines a real path for every icon', () => {
    for (const name of ICON_NAMES) {
      const path = ICONS[name];
      expect(path, name).toMatch(/^M/);
      expect(path.trim().length, name).toBeGreaterThanOrEqual(7);
    }
  });

  it('keeps every coordinate within reach of the 24px grid', () => {
    // Deliberately not a full path parser. Absolute coordinates and relative
    // deltas alike are bounded by the grid, so this catches the realistic
    // failure — a typo such as 120 for 12 — without pretending to compute true
    // curve extrema. Visual correctness is checked by rendering the whole set.
    for (const name of ICON_NAMES) {
      for (const raw of ICONS[name].match(/-?\d*\.?\d+/g) ?? []) {
        expect(Math.abs(Number(raw)), `${name} has coordinate ${raw}`).toBeLessThanOrEqual(24);
      }
    }
  });
});
