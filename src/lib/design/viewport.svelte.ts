/**
 * Viewport state.
 *
 * Breakpoints change *what exists*, not just how wide it is, so the rest of the
 * app asks this store a semantic question ("is the Margin a pane or an
 * overlay?") rather than repeating pixel values in a dozen media queries.
 *
 * The three states, and why they sit where they do:
 *
 *   wide    ≥ 1180  navigation, editor, and Margin all fit as real panes
 *   medium   820    navigation stays; the Margin becomes a slide-over
 *   narrow  < 820   editor only; both side regions become overlays
 *
 * A Surface Go at 150% scaling reports roughly 1024 CSS px in landscape and
 * 768 in portrait, which lands squarely in medium and narrow respectively.
 * Neither is a degraded fallback — both are primary targets.
 */

export const BREAKPOINT_MEDIUM = 820;
export const BREAKPOINT_WIDE = 1180;

export type LayoutState = 'narrow' | 'medium' | 'wide';

function classify(width: number): LayoutState {
  if (width >= BREAKPOINT_WIDE) return 'wide';
  if (width >= BREAKPOINT_MEDIUM) return 'medium';
  return 'narrow';
}

class Viewport {
  width = $state(1280);
  height = $state(840);

  /** True when the primary input is touch — used to choose menu presentation. */
  coarsePointer = $state(false);

  readonly layout = $derived(classify(this.width));
  readonly isNarrow = $derived(this.layout === 'narrow');
  readonly isWide = $derived(this.layout === 'wide');

  /** The Margin is a persistent pane only when there is room for all three. */
  readonly marginIsPane = $derived(this.layout === 'wide');
  /** Navigation survives as a pane down to medium. */
  readonly navIsPane = $derived(this.layout !== 'narrow');

  /** Wired once at startup; returns a teardown. */
  start(): () => void {
    if (typeof window === 'undefined') return () => {};

    const measure = () => {
      this.width = window.innerWidth;
      this.height = window.innerHeight;
    };
    measure();
    window.addEventListener('resize', measure, { passive: true });

    const pointer = window.matchMedia('(pointer: coarse)');
    this.coarsePointer = pointer.matches;
    const onPointer = (event: MediaQueryListEvent) => {
      this.coarsePointer = event.matches;
    };
    pointer.addEventListener('change', onPointer);

    return () => {
      window.removeEventListener('resize', measure);
      pointer.removeEventListener('change', onPointer);
    };
  }
}

export const viewport = new Viewport();

/** Exported for tests. */
export const _internal = { classify };
