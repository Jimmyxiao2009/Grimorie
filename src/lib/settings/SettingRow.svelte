<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    label: string;
    /** One sentence saying what the setting does, or why it exists. */
    hint?: string;
    control: Snippet;
    /** Puts the control on its own line, for anything wider than a toggle. */
    stacked?: boolean;
  }

  let { label, hint, control, stacked = false }: Props = $props();
</script>

<div class="row" class:stacked>
  <div class="text">
    <span class="label">{label}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
  </div>
  <div class="control">{@render control()}</div>
</div>

<style>
  .row {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    padding: var(--space-3) 0;
    border-bottom: var(--border-width) solid var(--border-subtle);
  }

  .row:last-child {
    border-bottom: none;
  }

  .stacked {
    flex-direction: column;
    align-items: stretch;
    gap: var(--space-2);
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .label {
    font-size: var(--text-md);
    color: var(--text-primary);
  }

  .hint {
    font-size: var(--text-sm);
    color: var(--text-tertiary);
    line-height: var(--leading-snug);
  }

  .control {
    flex: none;
  }

  .stacked .control {
    flex: 1;
  }

  /* On a narrow screen every row stacks, because a label and a control fighting
     for 320px leaves neither readable. */
  @media (max-width: 560px) {
    .row {
      flex-direction: column;
      align-items: stretch;
      gap: var(--space-2);
    }
  }
</style>
