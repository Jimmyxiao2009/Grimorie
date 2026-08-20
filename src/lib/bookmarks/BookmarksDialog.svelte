<script lang="ts">
  import Dialog from '$lib/components/Dialog.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { bookmarks } from '$lib/stores/bookmarks.svelte';
  import { relativeTime } from '$lib/utils/format';
  import type { Bookmark } from '$lib/services/bookmarks';

  interface Props {
    open: boolean;
    /** Confines the list to one Volume; null lists the whole library. */
    volumeId?: string | null;
    onselect: (bookmark: Bookmark) => void;
  }

  let { open = $bindable(), volumeId = null, onselect }: Props = $props();
  let scoped = $state(true);

  $effect(() => {
    if (!open) return;
    void bookmarks.loadAll(scoped ? volumeId : null);
  });

  function choose(bookmark: Bookmark) {
    open = false;
    onselect(bookmark);
  }
</script>

<Dialog bind:open title="Bookmarks" description="Pages you meant to come back to." width="md">
  {#if volumeId}
    <div class="scopes" role="tablist" aria-label="Bookmark scope">
      <button
        type="button"
        role="tab"
        class="scope"
        class:selected={scoped}
        aria-selected={scoped}
        onclick={() => (scoped = true)}>This Volume</button
      >
      <button
        type="button"
        role="tab"
        class="scope"
        class:selected={!scoped}
        aria-selected={!scoped}
        onclick={() => (scoped = false)}>Whole library</button
      >
    </div>
  {/if}

  {#if bookmarks.loading}
    <p class="status">Reading…</p>
  {:else if bookmarks.entries.length === 0}
    <EmptyState
      title="Nothing is bookmarked."
      hint="Bookmark a Page with Ctrl+B to find your way back to it."
    />
  {:else}
    <ul class="list">
      {#each bookmarks.entries as bookmark (bookmark.id)}
        <li>
          <button type="button" class="entry" onclick={() => choose(bookmark)}>
            <span class="mark"><Icon name="bookmark" size={16} /></span>
            <span class="body">
              <span class="title truncate">{bookmark.pageTitle}</span>
              <span class="path truncate">
                {bookmark.volumeTitle} › {bookmark.chapterTitle}
              </span>
              {#if bookmark.label}
                <span class="label">{bookmark.label}</span>
              {:else if bookmark.preview}
                <span class="preview truncate">{bookmark.preview}</span>
              {/if}
            </span>
            <span class="when">{relativeTime(bookmark.createdAt)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</Dialog>

<style>
  .scopes {
    display: flex;
    gap: var(--space-1);
    margin-bottom: var(--space-3);
  }

  .scope {
    min-height: 34px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    color: var(--text-tertiary);
  }

  .scope.selected {
    background: var(--surface-selected);
    color: var(--accent);
    font-weight: var(--weight-medium);
  }

  .status {
    padding: var(--space-5);
    text-align: center;
    color: var(--text-tertiary);
  }

  .entry {
    display: flex;
    gap: var(--space-3);
    width: 100%;
    min-height: var(--touch-comfortable);
    padding: var(--space-3);
    border-radius: var(--radius-md);
    text-align: left;
  }

  .entry:hover {
    background: var(--surface-hover);
  }

  .mark {
    flex: none;
    padding-top: 2px;
    color: var(--accent);
  }

  .body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .title {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  .path,
  .preview {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }

  .label {
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .when {
    flex: none;
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    white-space: nowrap;
  }
</style>
