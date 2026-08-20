<script lang="ts">
  import type { Snippet } from 'svelte';

  interface Props {
    /** One plain sentence describing the state. No exclamation marks. */
    title: string;
    /** One sentence saying what to do next, if there is something to do. */
    hint?: string;
    action?: Snippet;
    size?: 'sm' | 'md';
  }

  let { title, hint, action, size = 'md' }: Props = $props();
</script>

<!-- Empty states are written, not decorated: no illustration, no mascot, no
     encouragement. A quiet sentence respects a writer more than a cartoon. -->
<div class="empty {size}">
  <p class="title">{title}</p>
  {#if hint}<p class="hint">{hint}</p>{/if}
  {#if action}<div class="action">{@render action()}</div>{/if}
</div>

<style>
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    text-align: center;
    padding: var(--space-6) var(--space-4);
    color: var(--text-secondary);
  }

  .sm {
    padding: var(--space-5) var(--space-3);
  }

  .title {
    font-family: var(--font-serif);
    font-size: var(--text-lg);
    color: var(--text-secondary);
  }

  .sm .title {
    font-size: var(--text-md);
  }

  .hint {
    font-size: var(--text-md);
    color: var(--text-tertiary);
    max-width: 26rem;
    line-height: var(--leading-snug);
  }

  .sm .hint {
    font-size: var(--text-sm);
  }

  .action {
    margin-top: var(--space-3);
  }
</style>
