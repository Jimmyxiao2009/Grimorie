<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import type { AiSuggestion } from '$lib/services/ai';

  /**
   * The proposed edit attached to an AI note.
   *
   * Shows what would replace what, and offers the three answers the brief
   * requires: apply, dismiss, and re-evaluate. Applying is the only one that
   * touches the manuscript, and even then the backend re-checks the passage
   * first and refuses if it has changed.
   */

  interface Props {
    suggestion: AiSuggestion;
    onapplied: () => void;
  }

  let { suggestion, onapplied }: Props = $props();
  let busy = $state(false);

  const stale = $derived(suggestion.status === 'stale');
  const settled = $derived(
    suggestion.status === 'applied' || suggestion.status === 'dismissed'
  );

  async function apply() {
    busy = true;
    const applied = await ai.applySuggestion(suggestion.id);
    busy = false;
    if (applied) onapplied();
  }

  async function reevaluate() {
    busy = true;
    await ai.reevaluateSuggestion(suggestion.id);
    busy = false;
  }
</script>

<div class="suggestion" class:stale class:settled>
  {#if settled}
    <p class="settled-note">
      <Icon name={suggestion.status === 'applied' ? 'checkCircle' : 'closeCircle'} size={14} />
      <span>{suggestion.status === 'applied' ? 'Applied.' : 'Dismissed.'}</span>
    </p>
  {:else}
    <div class="diff">
      <p class="row removed">
        <span class="label">Now</span>
        <span class="text selectable">{suggestion.originalText}</span>
      </p>
      <p class="row added">
        <span class="label">Instead</span>
        <span class="text selectable">{suggestion.replacementText}</span>
      </p>
    </div>

    {#if stale}
      <p class="warning">
        <Icon name="warning" size={14} />
        <span>
          The text has changed since this was written. Applying it is blocked until it fits again.
        </span>
      </p>
    {/if}

    <div class="actions">
      <button type="button" class="action primary" disabled={busy || stale} onclick={apply}>
        <Icon name="check" size={14} />
        <span>Apply</span>
      </button>
      <button type="button" class="action" disabled={busy} onclick={reevaluate}>
        <Icon name="refresh" size={14} />
        <span>Re-evaluate</span>
      </button>
      <button
        type="button"
        class="action"
        disabled={busy}
        onclick={() => ai.dismissSuggestion(suggestion.id)}
      >
        <span>Dismiss</span>
      </button>
    </div>
  {/if}
</div>

<style>
  .suggestion {
    margin-top: var(--space-2);
    padding: var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-subtle);
  }

  .settled {
    opacity: 0.7;
  }

  .settled-note {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }

  .diff {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
  }

  .row {
    display: grid;
    grid-template-columns: 3.4rem 1fr;
    gap: var(--space-2);
    font-size: var(--text-sm);
    line-height: var(--leading-snug);
  }

  .label {
    font-size: var(--text-2xs);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-tertiary);
    padding-top: 2px;
  }

  .text {
    font-family: var(--font-manuscript);
    overflow-wrap: break-word;
  }

  /* The old text is struck through rather than tinted red: this is a proposal
     about prose, not a code review. */
  .removed .text {
    color: var(--text-tertiary);
    text-decoration: line-through;
    text-decoration-thickness: 1px;
  }

  .added .text {
    color: var(--text-primary);
  }

  .warning {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    line-height: var(--leading-snug);
    color: var(--state-warning);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1);
    margin-top: var(--space-2);
  }

  .action {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-height: 32px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    font-size: var(--text-xs);
    color: var(--text-secondary);
  }

  .action:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .action.primary {
    color: var(--accent);
    font-weight: var(--weight-medium);
  }

  .action:disabled {
    opacity: 0.4;
  }
</style>
