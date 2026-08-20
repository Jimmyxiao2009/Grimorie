<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import Menu from '$lib/components/Menu.svelte';
  import SuggestionActions from './SuggestionActions.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import { relativeTime } from '$lib/utils/format';
  import { KIND_LABELS, KIND_TOKENS, anchorOf, type Annotation } from '$lib/types/annotation';
  import type { IconName } from '$lib/design/icons';
  import type { MenuItem } from '$lib/types/ui';

  interface Props {
    annotation: Annotation;
    focused: boolean;
    onfocus: (id: string) => void;
    onedit: (annotation: Annotation) => void;
    onresolve: (annotation: Annotation) => void;
    ondelete: (annotation: Annotation) => void;
    /** Scrolls the manuscript to the anchored text. */
    onreveal: (annotation: Annotation) => void;
    /** Called after a suggestion is applied, so the Page can be reloaded. */
    onapplied?: () => void;
  }

  let { annotation, focused, onfocus, onedit, onresolve, ondelete, onreveal, onapplied }: Props =
    $props();

  const suggestion = $derived(ai.suggestionFor(annotation.id));

  const anchor = $derived(anchorOf(annotation));
  const resolved = $derived(annotation.status === 'resolved');
  const stale = $derived(annotation.status === 'stale');

  const icons: Record<string, IconName> = {
    note: 'margin',
    question: 'question',
    suggestion: 'checkCircle',
    warning: 'warning',
    reference: 'external',
    'ai-review': 'sparkle',
    'ai-suggestion': 'sparkle'
  };

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

<article
  class="card"
  class:focused
  class:resolved
  class:stale
  style:--kind="var({KIND_TOKENS[annotation.kind]})"
>
  <header>
    <span class="kind">
      <Icon name={icons[annotation.kind] ?? 'margin'} size={15} />
      <span>{KIND_LABELS[annotation.kind]}</span>
    </span>
    <span class="when">{relativeTime(annotation.updatedAt)}</span>
    <Menu items={actions} label="Note actions" size="sm" />
  </header>

  {#if anchor}
    <!-- The anchored words, so a note in the Margin is readable without
         hunting for what it points at. -->
    <button type="button" class="quoted" onclick={() => onreveal(annotation)}>
      <span class="quoted-text">{anchor.selectedText}</span>
    </button>
  {/if}

  {#if stale}
    <p class="stale-note">
      <Icon name="warning" size={14} />
      <span>The text this pointed at has changed. The note has been kept.</span>
    </p>
  {/if}

  <button type="button" class="body selectable" onclick={() => onfocus(annotation.id)}>
    {annotation.body}
  </button>

  {#if suggestion}
    <SuggestionActions {suggestion} onapplied={() => onapplied?.()} />
  {/if}

  {#if annotation.authorProfile}
    <p class="author">{annotation.authorProfile}</p>
  {/if}
</article>

<style>
  .card {
    position: relative;
    padding: var(--space-3);
    border-radius: var(--radius-md);
    background: var(--surface-raised);
    border: var(--border-width) solid var(--border-subtle);
    /* The kind is carried by a spine rather than a filled background, so a
       Margin of six notes does not read as six coloured blocks. */
    border-left: 2px solid var(--kind);
    transition:
      border-color var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) var(--ease-out);
  }

  .card.focused {
    border-color: var(--accent);
    border-left-color: var(--kind);
    background: var(--surface-page);
  }

  .card.resolved {
    opacity: 0.6;
  }

  .card.stale {
    border-left-style: dashed;
  }

  header {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .kind {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    color: var(--kind);
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
  }

  .when {
    flex: 1;
    text-align: right;
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
    white-space: nowrap;
  }

  .quoted {
    display: block;
    width: 100%;
    text-align: left;
    margin-bottom: var(--space-2);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
  }

  .quoted:hover {
    background: var(--surface-hover);
  }

  .quoted-text {
    font-family: var(--font-manuscript);
    font-size: var(--text-sm);
    font-style: italic;
    color: var(--text-secondary);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .stale-note {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
    color: var(--state-warning);
    font-size: var(--text-xs);
    line-height: var(--leading-snug);
  }

  .body {
    display: block;
    width: 100%;
    text-align: left;
    font-size: var(--text-md);
    line-height: var(--leading-snug);
    color: var(--text-primary);
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .author {
    margin-top: var(--space-2);
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
  }
</style>
