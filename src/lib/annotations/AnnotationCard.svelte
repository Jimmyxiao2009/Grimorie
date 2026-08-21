<script lang="ts">
  import Menu from '$lib/components/Menu.svelte';
  import SuggestionActions from './SuggestionActions.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import { relativeTime } from '$lib/utils/format';
  import { KIND_GLYPHS, KIND_LABELS, anchorOf, type Annotation } from '$lib/types/annotation';
  import type { MenuItem } from '$lib/types/ui';

  /**
   * One note in the Margin.
   *
   * Not a card. There is no box, no fill, and no coloured spine — a marginal
   * note is text written beside text, and the only structure it needs is a
   * mark, a rule, and a smaller size than the manuscript it comments on. The
   * card treatment this replaced turned six notes into six tiles and made the
   * Margin compete with the writing it was supposed to serve.
   */

  interface Props {
    annotation: Annotation;
    focused: boolean;
    onfocus: (id: string) => void;
    onedit: (annotation: Annotation) => void;
    onresolve: (annotation: Annotation) => void;
    ondelete: (annotation: Annotation) => void;
    onreveal: (annotation: Annotation) => void;
    onapplied?: () => void;
  }

  let { annotation, focused, onfocus, onedit, onresolve, ondelete, onreveal, onapplied }: Props =
    $props();

  const suggestion = $derived(ai.suggestionFor(annotation.id));
  const anchor = $derived(anchorOf(annotation));
  const resolved = $derived(annotation.status === 'resolved');
  const stale = $derived(annotation.status === 'stale');

  const actions = $derived<MenuItem[]>([
    { id: 'edit', label: 'Edit', icon: 'pencil', select: () => onedit(annotation) },
    {
      id: 'resolve',
      label: resolved ? 'Reopen' : 'Resolve',
      icon: resolved ? 'restore' : 'check',
      select: () => onresolve(annotation)
    },
    { kind: 'separator', id: 's' },
    { id: 'delete', label: 'Delete', icon: 'trash', danger: true, select: () => ondelete(annotation) }
  ]);
</script>

<article class="note" class:focused class:resolved class:stale>
  <!-- The rule is the note's tie to the manuscript: it runs up the left edge,
       and on an anchored note it starts level with the words it refers to. -->
  <span class="rule" aria-hidden="true"></span>

  <button
    type="button"
    class="face"
    onclick={() => {
      onfocus(annotation.id);
      if (anchor) onreveal(annotation);
    }}
  >
    <span class="mark" aria-hidden="true">{KIND_GLYPHS[annotation.kind]}</span>
    <span class="text selectable">{annotation.body}</span>
  </button>

  <div class="menu">
    <Menu items={actions} label="{KIND_LABELS[annotation.kind]} actions" size="sm" />
  </div>

  {#if suggestion}
    <SuggestionActions {suggestion} onapplied={() => onapplied?.()} />
  {/if}

  <p class="meta">
    {#if stale}
      <span class="unstuck">Text changed</span>
    {/if}
    {#if annotation.authorProfile}
      <span class="author">{annotation.authorProfile}</span>
    {/if}
    <span class="when">{relativeTime(annotation.updatedAt)}</span>
  </p>
</article>

<style>
  .note {
    position: relative;
    padding: var(--space-1) var(--space-2) var(--space-3) var(--space-3);
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

  /* An unstuck anchor is the one thing colour is spent on, because it is the
     one thing the writer has to decide about. */
  .stale .rule {
    background: var(--state-warning);
    /* A broken rule, for an note whose text is gone. */
    background-image: repeating-linear-gradient(
      to bottom,
      var(--state-warning) 0 3px,
      transparent 3px 6px
    );
  }

  .resolved {
    opacity: 0.5;
  }

  .face {
    display: grid;
    grid-template-columns: 1.1rem 1fr;
    gap: var(--space-2);
    width: 100%;
    padding: var(--space-1) var(--space-5) var(--space-1) 0;
    text-align: left;
    border-radius: var(--radius-sm);
  }

  .mark {
    font-family: var(--font-serif);
    font-size: var(--text-md);
    line-height: 1.5;
    color: var(--text-tertiary);
    text-align: center;
  }

  .focused .mark {
    color: var(--accent);
  }

  /* Set in the manuscript face but noticeably smaller, so the Margin reads as
     an aside in the same hand rather than as interface text. */
  .text {
    font-family: var(--font-manuscript);
    font-size: 0.9375rem;
    line-height: 1.45;
    color: var(--text-secondary);
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .focused .text {
    color: var(--text-primary);
  }

  .menu {
    position: absolute;
    top: 0;
    right: 0;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  /* Revealed on hover, on focus, and whenever the note is the focused one —
     so a tablet, which has no hover, reaches it by tapping the note first. */
  .note:hover .menu,
  .menu:focus-within,
  .focused .menu {
    opacity: 1;
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-top: var(--space-1);
    padding-left: calc(1.1rem + var(--space-2));
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
  }

  .meta:empty {
    display: none;
  }

  .unstuck {
    color: var(--state-warning);
  }

  .author {
    font-style: italic;
  }

  .when {
    margin-left: auto;
  }
</style>
