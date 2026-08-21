<script lang="ts">
  /**
   * One handwritten note in the Margin.
   *
   * Not a card, like the text notes — handwriting is ink on the margin itself,
   * so the note is its drawing surface with a quiet rule on the left and
   * controls that appear on hover or focus. The surface is the note.
   */

  import InkSurface from '$lib/ink/InkSurface.svelte';
  import Menu from '$lib/components/Menu.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import { ink } from '$lib/stores/ink.svelte';
  import { relativeTime } from '$lib/utils/format';
  import { anchorOf, type Annotation } from '$lib/types/annotation';
  import type { MenuItem } from '$lib/types/ui';

  interface Props {
    annotation: Annotation;
    focused: boolean;
    onfocus: (id: string) => void;
    ondelete: (annotation: Annotation) => void;
    onreveal: (annotation: Annotation) => void;
  }

  let { annotation, focused, onfocus, ondelete, onreveal }: Props = $props();

  const anchor = $derived(anchorOf(annotation));
  const stale = $derived(annotation.status === 'stale');
  const strokeCount = $derived(ink.strokesFor(annotation.id).length);
  const active = $derived(ink.penMode);

  const actions = $derived<MenuItem[]>([
    { id: 'delete', label: 'Delete ink note', icon: 'trash', danger: true, select: () => ondelete(annotation) }
  ]);

  /** A min height so an empty ink note still offers room to write. */
  const MIN_HEIGHT = 120;
</script>

<article
  class="ink-note"
  class:focused
  class:stale
  class:active
  style:min-height="{MIN_HEIGHT}px"
  onfocusin={() => onfocus(annotation.id)}
>
  <span class="rule" aria-hidden="true"></span>

  <!-- The surface fills the note and is the writing area. -->
  <div class="surface">
    <InkSurface annotationId={annotation.id} {active} />
  </div>

  {#if strokeCount === 0 && !active}
    <p class="hint">Write here with the pen.</p>
  {/if}

  <div class="menu">
    <Menu items={actions} label="Ink note actions" size="sm" />
  </div>

  <button
    type="button"
    class="reveal"
    aria-label="Show this ink note in the manuscript"
    onclick={() => {
      if (anchor) onreveal(annotation);
      onfocus(annotation.id);
    }}
  >
    {#if stale}
      <span class="unstuck">Text changed</span>
    {/if}
    <span class="when">{strokeCount > 0 ? `${strokeCount} stroke${strokeCount === 1 ? '' : 's'}` : 'empty'} · {relativeTime(annotation.updatedAt)}</span>
  </button>
</article>

<style>
  .ink-note {
    position: relative;
    min-height: 120px;
    padding: var(--space-1) var(--space-2) var(--space-2) var(--space-3);
  }

  .rule {
    position: absolute;
    left: 0;
    top: 6px;
    bottom: var(--space-3);
    width: 1px;
    background: var(--border-default);
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .focused .rule {
    width: 2px;
    background: var(--accent);
  }

  .stale .rule {
    background: var(--state-warning);
    background-image: repeating-linear-gradient(
      to bottom,
      var(--state-warning) 0 3px,
      transparent 3px 6px
    );
  }

  .surface {
    position: relative;
    width: 100%;
    min-height: 96px;
  }

  .hint {
    position: absolute;
    top: var(--space-1);
    left: calc(var(--space-3) + var(--space-1));
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
    pointer-events: none;
  }

  .menu {
    position: absolute;
    top: 0;
    right: 0;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .ink-note:hover .menu,
  .menu:focus-within,
  .focused .menu {
    opacity: 1;
  }

  /* In pen mode the controls step aside so they never sit under the hand. */
  .active .menu {
    opacity: 0;
  }

  .reveal {
    display: block;
    width: 100%;
    padding: var(--space-1) 0 0 calc(var(--space-3) + var(--space-1));
    text-align: left;
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
  }

  .unstuck {
    color: var(--state-warning);
    margin-right: var(--space-2);
  }

  .when {
    /* tabular keeps the stroke count from shifting width as it changes. */
    font-variant-numeric: tabular-nums;
  }
</style>
