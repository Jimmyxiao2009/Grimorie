<script lang="ts">
  /**
   * The ink drawing surface: an SVG overlay that renders committed strokes and
   * captures pointer input to draw new ones.
   *
   * # Input discipline
   *
   * Pen and mouse draw; touch never draws. A finger is for scrolling and
   * tapping, and a palm resting on the screen while the pen writes must not
   * leave stray marks. That is enforced by checking `pointerType` on every
   * event and by ignoring every pointer that is not the one that started the
   * stroke — so a second finger or a resting palm cannot hijack an active
   * stroke.
   *
   * # Performance discipline
   *
   * The active stroke lives here, in local state, not in the global store. A
   * pointermove updates a local variable and re-renders one path; it never
   * touches the store, never rebuilds the annotation list, and never crosses
   * IPC. The store hears about the stroke only on pointerup.
   *
   * # Coordinate discipline
   *
   * Points are normalised against the surface width on capture and
   * denormalised on render, so handwriting stays put when the surface resizes.
   */

  import { tick } from 'svelte';
  import { ink } from '$lib/stores/ink.svelte';
  import { appearance } from '$lib/design/theme.svelte';
  import {
    denormalizePoint,
    normalizePoint,
    pathFromStroke,
    renderWidth,
    resolveColor,
    type Theme
  } from '$lib/ink/geometry';
  import type { InkStroke } from '$lib/types/ink';

  interface Props {
    /** The annotation id whose strokes this surface renders. */
    annotationId: string;
    /** Whether pen input is currently enabled. */
    active: boolean;
  }

  let { annotationId, active }: Props = $props();

  let svg = $state<SVGSVGElement | null>(null);
  /** Width of the surface in CSS pixels, measured live for normalisation. */
  let surfaceWidth = $state(0);

  // The active stroke, held locally so pointermove never wakes the store. It is
  // `$state` so mutating its points re-renders the live path without touching
  // the global store.
  let activeStroke = $state<InkStroke | null>(null);

  const theme = $derived(appearance.resolved as Theme);
  const strokes = $derived(ink.strokesFor(annotationId));

  // Highlighters render first (behind), pens after, so a highlight never
  // obscures handwriting drawn over it.
  const highlighters = $derived(strokes.filter((s) => s.tool === 'highlighter'));
  const pens = $derived(strokes.filter((s) => s.tool === 'pen'));

  $effect(() => {
    if (!svg) return;
    const observer = new ResizeObserver(() => {
      surfaceWidth = svg!.clientWidth;
    });
    surfaceWidth = svg.clientWidth;
    observer.observe(svg);
    return () => observer.disconnect();
  });

  /** The pointer id that owns the current stroke, or null. */
  let drawingPointerId: number | null = null;

  function surfacePoint(event: PointerEvent): { x: number; y: number } {
    const rect = svg!.getBoundingClientRect();
    // y is stored relative to the surface top in content coordinates. The
    // surface scrolls with the manuscript inside the shared scroll container,
    // so the surface's own top is the origin; scrollOffset is not needed here
    // because the surface element moves with the content.
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  function onPointerDown(event: PointerEvent) {
    if (!active) return;
    // Only pen and mouse draw. Touch is for scrolling and tapping.
    if (event.pointerType !== 'pen' && event.pointerType !== 'mouse') return;
    // A mouse stroke only counts when the primary button is down.
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    // One stroke at a time: a second pointer that lands while one is active is
    // ignored. This is the application-level palm rejection — a resting palm
    // arrives as extra touch/pen pointers and must not start a second stroke.
    if (drawingPointerId !== null) return;

    drawingPointerId = event.pointerId;
    svg!.setPointerCapture(event.pointerId);
    event.preventDefault();

    const point = normalizePoint(surfacePoint(event), surfaceWidth, 0);

    if (ink.tool === 'eraser') {
      // The eraser works on a down-tap: erase the nearest stroke and end.
      void ink.eraseAt(
        { x: point.x * surfaceWidth, y: point.y },
        surfaceWidth
      );
      drawingPointerId = null;
      return;
    }

    activeStroke = {
      id: crypto.randomUUID(),
      tool: ink.tool === 'highlighter' ? 'highlighter' : 'pen',
      color: ink.activeColor,
      width: ink.activeWidth,
      points: [point],
      createdAt: new Date().toISOString()
    };
  }

  function onPointerMove(event: PointerEvent) {
    if (!activeStroke) return;
    // Only the pointer that started the stroke may extend it.
    if (event.pointerId !== drawingPointerId) return;
    event.preventDefault();

    const raw = surfacePoint(event);
    const point = normalizePoint(raw, surfaceWidth, 0);
    // Coalesced events carry the intermediate samples the OS bundled together,
    // so a fast stroke does not lose points between frames.
    const coalesced = event.getCoalescedEvents();
    if (coalesced.length > 0) {
      for (const sample of coalesced) {
        const cp = surfacePoint(sample);
        activeStroke.points.push(normalizePoint(cp, surfaceWidth, 0));
      }
    } else {
      activeStroke.points.push(point);
    }
  }

  async function onPointerUp(event: PointerEvent) {
    if (event.pointerId !== drawingPointerId) return;
    drawingPointerId = null;
    if (svg!.hasPointerCapture(event.pointerId)) {
      svg!.releasePointerCapture(event.pointerId);
    }
    if (!activeStroke) return;

    const stroke = activeStroke;
    activeStroke = null;

    if (stroke.points.length === 0) return;
    await ink.commitStroke(stroke);
    // Let the store settle so the committed stroke replaces the local one
    // without a flicker gap.
    await tick();
  }

  function onPointerCancel(event: PointerEvent) {
    if (event.pointerId !== drawingPointerId) return;
    drawingPointerId = null;
    // A cancelled stroke is discarded — it was interrupted, not finished, so
    // persisting a partial mark would be worse than dropping it.
    activeStroke = null;
  }

  // Render the active stroke's path reactively. `activeStroke` is `$state`, so
  // mutating its points (and clearing it) re-runs this without waking the store.
  const activePath = $derived.by(() => {
    if (!activeStroke || activeStroke.points.length === 0) return '';
    return pathFromStroke(activeStroke, surfaceWidth);
  });

  // Touch drawing is blocked at the handler level, but we also tell the browser
  // not to synthesize touch gestures (scroll, double-tap zoom) from pen events
  // when active, so a pen stroke does not pan the page. When inactive, the
  // surface must not steal touch scrolling.
  const touchAction = $derived(active ? 'none' : 'auto');
</script>

<svg
  bind:this={svg}
  class="ink-surface"
  class:active
  aria-label="Handwriting surface"
  role="img"
  style:touch-action={touchAction}
  style:width="100%"
  style:height="100%"
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerCancel}
>
  <!-- Highlighters sit behind pens so they read as a wash under handwriting. -->
  {#each highlighters as stroke (stroke.id)}
    <path
      class="stroke highlighter"
      d={pathFromStroke(stroke, surfaceWidth)}
      stroke={resolveColor(stroke.color, theme)}
      stroke-width={renderWidth(stroke)}
    />
  {/each}

  {#each pens as stroke (stroke.id)}
    <path
      class="stroke pen"
      d={pathFromStroke(stroke, surfaceWidth)}
      stroke={resolveColor(stroke.color, theme)}
      stroke-width={renderWidth(stroke)}
    />
  {/each}

  {#if activePath}
    <path
      class="stroke live"
      d={activePath}
      stroke={activeStroke ? resolveColor(activeStroke.color, theme) : 'currentColor'}
      stroke-width={activeStroke ? renderWidth(activeStroke) : 2}
    />
  {/if}
</svg>

<style>
  .ink-surface {
    position: absolute;
    inset: 0;
    overflow: visible;
    pointer-events: none;
  }

  /* The surface only captures input when pen mode is on; otherwise it is a
     transparent render layer that does not block the text beneath it. */
  .ink-surface.active {
    pointer-events: auto;
    cursor: crosshair;
  }

  .ink-surface.active:has(.live) {
    cursor: none;
  }

  .stroke {
    fill: none;
    stroke-linecap: round;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }

  .highlighter {
    /* A wash behind the text and the pen, never over it. */
    mix-blend-mode: multiply;
  }

  .live {
    pointer-events: none;
  }
</style>
