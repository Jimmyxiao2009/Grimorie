<script lang="ts">
  import Icon from './Icon.svelte';
  import IconButton from './IconButton.svelte';
  import { notices } from '$lib/stores/notices.svelte';

  let expanded = $state<number | null>(null);
</script>

<!-- Failures stay until dismissed; a lost action must be seen, not glimpsed. -->
<div class="stack" role="region" aria-label="Notifications">
  {#each notices.items as notice (notice.id)}
    <div class="notice {notice.tone}" role={notice.tone === 'failure' ? 'alert' : 'status'}>
      <Icon name={notice.tone === 'failure' ? 'warning' : 'info'} size={18} />

      <div class="body">
        <p class="message selectable">{notice.message}</p>

        {#if notice.detail && expanded === notice.id}
          <pre class="detail selectable">{notice.detail}</pre>
        {/if}

        <div class="actions">
          {#if notice.retry}
            <button
              type="button"
              class="action"
              onclick={() => {
                notices.dismiss(notice.id);
                notice.retry?.();
              }}>Retry</button
            >
          {/if}
          {#if notice.detail}
            <button
              type="button"
              class="action quiet"
              aria-expanded={expanded === notice.id}
              onclick={() => (expanded = expanded === notice.id ? null : notice.id)}
            >
              {expanded === notice.id ? 'Hide details' : 'Details'}
            </button>
          {/if}
        </div>
      </div>

      <IconButton
        name="close"
        label="Dismiss"
        size="sm"
        onclick={() => notices.dismiss(notice.id)}
      />
    </div>
  {/each}
</div>

<style>
  .stack {
    position: fixed;
    right: var(--space-4);
    bottom: var(--space-4);
    z-index: var(--z-toast);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: min(26rem, calc(100vw - 2rem));
    pointer-events: none;
  }

  .notice {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    background: var(--surface-raised);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-overlay);
    pointer-events: auto;
    animation: rise var(--motion-base) var(--ease-out);
  }

  .failure {
    border-left: 3px solid var(--state-danger);
    color: var(--text-primary);
  }

  .failure :global(svg) {
    color: var(--state-danger);
  }

  .info :global(svg) {
    color: var(--text-tertiary);
  }

  .body {
    flex: 1;
    min-width: 0;
  }

  .message {
    font-size: var(--text-md);
    line-height: var(--leading-snug);
  }

  .detail {
    margin-top: var(--space-2);
    padding: var(--space-2);
    background: var(--surface-sunken);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-secondary);
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 9rem;
    overflow-y: auto;
  }

  .actions {
    display: flex;
    gap: var(--space-3);
  }

  .actions:empty {
    display: none;
  }

  .action {
    margin-top: var(--space-2);
    min-height: 32px;
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    color: var(--accent);
  }

  .action:hover {
    text-decoration: underline;
  }

  .action.quiet {
    color: var(--text-tertiary);
    font-weight: var(--weight-regular);
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
