<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import { autosave } from './autosave.svelte';
</script>

<!-- Quiet by design. A successful save is reported by a word changing, never by
     a dialog or a toast — autosave that congratulates itself is noise. A
     failure is the one state that raises its voice, because it is the one state
     the writer has to act on. -->
{#if autosave.state === 'failed'}
  <button type="button" class="indicator failed" onclick={() => void autosave.retry()}>
    <Icon name="warning" size={15} />
    <span>Couldn’t save — retry</span>
  </button>
{:else}
  <p class="indicator" aria-live="polite">
    {#if autosave.state === 'saving'}
      <span class="dot saving" aria-hidden="true"></span>
      <span>Saving…</span>
    {:else if autosave.state === 'unsaved'}
      <span class="dot unsaved" aria-hidden="true"></span>
      <span>Unsaved</span>
    {:else}
      <span class="dot" aria-hidden="true"></span>
      <span>Saved</span>
    {/if}
  </p>
{/if}

<style>
  .indicator {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-2);
    min-height: 32px;
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    white-space: nowrap;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--border-strong);
    flex: none;
  }

  .dot.unsaved {
    background: var(--state-warning);
  }

  .dot.saving {
    background: var(--accent);
    animation: pulse 1.1s ease-in-out infinite;
  }

  .failed {
    color: var(--state-danger);
    border-radius: var(--radius-md);
    padding: 0 var(--space-2);
  }

  .failed:hover {
    background: var(--surface-hover);
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dot.saving {
      animation: none;
    }
  }
</style>
