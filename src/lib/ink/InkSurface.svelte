<script lang="ts">
  /**
   * The ink drawing surface: an SVG overlay that renders committed strokes and
   * captures pointer input to draw new ones.
   *
   * # Input discipline
   *
   * Pen, touch, and primary-button mouse all draw. Browser panning is disabled
   * while the surface is active; moving the paper is an explicit Hand-tool
   * action. One pointer owns each stroke, so a second pointer cannot hijack an
   * active line.
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
  import { inkRecognition } from '$lib/stores/ink-recognition.svelte';
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
      // Keep the recognition store's rasterizer in step with the live surface
      // width, so a snapshot's denormalised x coordinates match the strokes.
      inkRecognition.setSurfaceWidth(surfaceWidth);
    });
    surfaceWidth = svg.clientWidth;
    inkRecognition.setSurfaceWidth(surfaceWidth);
    observer.observe(svg);
    return () => observer.disconnect();
  });

  /** The pointer id that owns the current stroke, or null. */
  let drawingPointerId = $state<number | null>(null);

  function surfacePoint(event: PointerEvent): { x: number; y: number } {
    const rect = svg!.getBoundingClientRect();
    // y is stored relative to the surface top in content coordinates. The
    // surface scrolls with the manuscript inside the shared scroll container,
    // so the surface's own top is the origin; scrollOffset is not needed here
    // because the surface element moves with the content.
    return { x: event.clientX - rect.left, y: event.clientY - rect.top };
  }

  /**
   * A stored point from a pointer sample, carrying pressure when the device
   * reports it.
   *
   * A pen that does not support pressure, and a mouse with a button down, both
   * report exactly 0.5 — which `renderWidth` maps to the base width, so those
   * devices draw as though pressure were absent. A reading of 0 means "no
   * pressure information", not "pressed infinitely lightly", and is dropped
   * rather than stored as a hairline.
   */
  function pointFrom(event: PointerEvent): InkStroke['points'][number] {
    const point = normalizePoint(surfacePoint(event), surfaceWidth, 0);
    if (event.pointerType === 'pen' && event.pressure > 0) {
      point.pressure = event.pressure;
    }
    return point;
  }

  function onPointerDown(event: PointerEvent) {
    if (!active) return;
    // Pen, touch, and primary-button mouse all draw. Scrolling is an explicit
    // Hand-tool interaction on the manuscript rather than a browser gesture.
    if (event.pointerType !== 'pen' && event.pointerType !== 'mouse' && event.pointerType !== 'touch') return;
    // A mouse stroke only counts when the primary button is down.
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    // One stroke at a time: a second pointer that lands while one is active is
    // ignored. This is the application-level palm rejection — a resting palm
    // arrives as extra touch/pen pointers and must not start a second stroke.
    if (drawingPointerId !== null) return;

    const raw = surfacePoint(event);

    if (ink.tool === 'eraser') {
      // The eraser works on a down-tap: erase the nearest stroke and end. It
      // deliberately takes no pointer capture — there is no ongoing gesture to
      // follow, and capturing would leave the surface holding a pointer it has
      // no further use for. The raw surface-pixel point is passed because
      // hitTestStroke denormalises stored points to compare against it; a
      // normalised x here would be denormalised twice.
      event.preventDefault();
      void ink.eraseAt(raw, surfaceWidth);
      return;
    }

    drawingPointerId = event.pointerId;
    svg!.setPointerCapture(event.pointerId);
    event.preventDefault();

    activeStroke = {
      id: crypto.randomUUID(),
      tool: ink.tool === 'highlighter' ? 'highlighter' : 'pen',
      color: ink.activeColor,
      width: ink.activeWidth,
      points: [pointFrom(event)],
      createdAt: new Date().toISOString()
    };
  }

  function onPointerMove(event: PointerEvent) {
    if (!activeStroke) return;
    // Only the pointer that started the stroke may extend it.
    if (event.pointerId !== drawingPointerId) return;
    event.preventDefault();

    // Coalesced events carry the intermediate samples the OS bundled together,
    // so a fast stroke does not lose points between frames. Not every engine
    // implements it, and a missing method must not cost the stroke its point.
    const coalesced =
      typeof event.getCoalescedEvents === 'function' ? event.getCoalescedEvents() : [];
    if (coalesced.length > 0) {
      for (const sample of coalesced) {
        activeStroke.points.push(pointFrom(sample));
      }
    } else {
      activeStroke.points.push(pointFrom(event));
    }
  }

  /**
   * Ends the stroke in progress, if any.
   *
   * `keep` decides its fate: a finished stroke is committed, an interrupted one
   * is discarded. Either way the pointer bookkeeping is cleared, which is the
   * part that must never be skipped — `onPointerDown` refuses to start while
   * `drawingPointerId` is set, so a stroke that ends without clearing it wedges
   * the surface against every future stroke.
   */
  async function endStroke(keep: boolean): Promise<void> {
    const pointerId = drawingPointerId;
    drawingPointerId = null;

    if (pointerId !== null && svg?.hasPointerCapture(pointerId)) {
      svg.releasePointerCapture(pointerId);
    }

    const stroke = activeStroke;
    activeStroke = null;
    if (!keep || !stroke || stroke.points.length === 0) return;

    await ink.commitStroke(stroke);
    // Let the store settle so the committed stroke replaces the local one
    // without a flicker gap.
    await tick();
  }

  async function onPointerUp(event: PointerEvent) {
    if (event.pointerId !== drawingPointerId) return;
    await endStroke(true);
  }

  function onPointerCancel(event: PointerEvent) {
    if (event.pointerId !== drawingPointerId) return;
    // A cancelled stroke is discarded — it was interrupted, not finished, so
    // persisting a partial mark would be worse than dropping it.
    void endStroke(false);
  }

  /**
   * The safety net for capture lost without a pointerup or pointercancel.
   *
   * The browser drops capture on its own in cases the other two handlers never
   * see — the element being detached, a system gesture taking the pointer, the
   * window losing focus mid-stroke. Left unhandled, `drawingPointerId` stays
   * set and the surface silently refuses every subsequent stroke for the rest
   * of its life. That is the worst possible failure for a writing surface, so
   * it is caught explicitly.
   *
   * The stroke is *kept*: the pen really did draw it, and this codebase's rule
   * is that handwriting is never silently discarded. In the ordinary case this
   * fires just after `pointerup` has already ended the stroke, and finds
   * nothing to do.
   */
  function onLostPointerCapture(event: PointerEvent) {
    if (event.pointerId !== drawingPointerId) return;
    void endStroke(true);
  }

  // Leaving pen mode mid-stroke — closing the Margin overlay, switching tools,
  // a keyboard shortcut — must not strand the stroke. The surface stops taking
  // input the moment `active` goes false, so no pointerup would ever arrive.
  $effect(() => {
    if (!active && drawingPointerId !== null) {
      void endStroke(true);
    }
  });

  // Render the active stroke's path reactively. `activeStroke` is `$state`, so
  // mutating its points (and clearing it) re-runs this without waking the store.
  const activePath = $derived.by(() => {
    if (!activeStroke || activeStroke.points.length === 0) return '';
    return pathFromStroke(activeStroke, surfaceWidth);
  });

  // Active ink owns direct manipulation completely. `touch-action: none` keeps
  // Windows/WebView scrolling or pinch gestures from cancelling a stroke; the
  // dedicated Hand tool is the only path for moving the paper. When inactive,
  // the surface stops capturing input and normal UI behavior resumes.
  const touchAction = $derived(active ? 'none' : 'auto');
</script>

<svg
  bind:this={svg}
  class="ink-surface"
  class:active
  aria-hidden={active ? 'false' : 'true'}
  style:touch-action={touchAction}
  style:width="100%"
  style:height="100%"
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerCancel}
  onlostpointercapture={onLostPointerCapture}
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
