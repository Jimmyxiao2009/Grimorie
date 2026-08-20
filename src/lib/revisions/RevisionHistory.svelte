<script lang="ts">
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import { getRevision, listRevisions, restoreRevision } from '$lib/services/history';
  import { notices } from '$lib/stores/notices.svelte';
  import { count, relativeTime } from '$lib/utils/format';
  import { REVISION_REASON_LABELS, type Revision, type RevisionSummary } from '$lib/types/history';
  import type { Page } from '$lib/types/manuscript';

  interface Props {
    open: boolean;
    pageId: string | null;
    pageTitle: string;
    /** The Page's text right now, for the "Current" comparison row. */
    currentText: string;
    currentWords: number;
    onrestored: (page: Page) => void;
  }

  let {
    open = $bindable(),
    pageId,
    pageTitle,
    currentText,
    currentWords,
    onrestored
  }: Props = $props();

  let history = $state<RevisionSummary[]>([]);
  let loading = $state(false);
  let selectedId = $state<string | null>(null);
  let selected = $state<Revision | null>(null);
  let loadingPreview = $state(false);
  let confirming = $state(false);

  // Load whenever the dialog opens for a Page.
  $effect(() => {
    if (!open || !pageId) return;
    const id = pageId;
    loading = true;
    selectedId = null;
    selected = null;
    void listRevisions(id)
      .then((rows) => {
        history = rows;
      })
      .catch((error) => {
        history = [];
        notices.failure(error);
      })
      .finally(() => {
        loading = false;
      });
  });

  async function choose(id: string) {
    selectedId = id;
    loadingPreview = true;
    try {
      selected = await getRevision(id);
    } catch (error) {
      selected = null;
      notices.failure(error);
    } finally {
      loadingPreview = false;
    }
  }

  async function restore() {
    if (!selectedId) return;
    const id = selectedId;
    confirming = false;
    try {
      const page = await restoreRevision(id);
      open = false;
      onrestored(page);
    } catch (error) {
      notices.failure(error);
    }
  }

  function delta(value: number): string {
    if (value === 0) return 'no change';
    return value > 0 ? `+${count(value)}` : `−${count(Math.abs(value))}`;
  }
</script>

<Dialog bind:open title="History" description="Earlier versions of “{pageTitle}”." width="lg">
  {#if loading}
    <p class="status">Reading history…</p>
  {:else if history.length === 0}
    <EmptyState
      title="This Page has no earlier versions yet."
      hint="Grimoire snapshots a Page as you work, and always before anything replaces its text."
    />
  {:else}
    <div class="split">
      <ul class="list" data-scroll>
        <!-- The present, as a row, so "restore" is visibly a move between two
             versions rather than a jump into the past from nowhere. -->
        <li>
          <div class="entry current" aria-current="true">
            <span class="when">Current</span>
            <span class="detail tabular">{count(currentWords)} words</span>
            <span class="preview truncate">{currentText || 'Empty'}</span>
          </div>
        </li>

        {#each history as revision (revision.id)}
          <li>
            <button
              type="button"
              class="entry"
              class:selected={selectedId === revision.id}
              onclick={() => choose(revision.id)}
            >
              <span class="when">{relativeTime(revision.createdAt)}</span>
              <span class="detail tabular">
                {count(revision.wordCount)} words · {delta(revision.wordDelta)}
              </span>
              <span class="preview truncate">{revision.preview || 'Empty'}</span>
              {#if revision.reason !== 'checkpoint'}
                <span class="reason">{REVISION_REASON_LABELS[revision.reason]}</span>
              {/if}
            </button>
          </li>
        {/each}
      </ul>

      <div class="preview-pane" data-scroll>
        {#if loadingPreview}
          <p class="status">Reading…</p>
        {:else if selected}
          <p class="preview-text selectable">{selected.plainText || 'This version was empty.'}</p>
        {:else}
          <p class="status">Choose a version to read it.</p>
        {/if}
      </div>
    </div>
  {/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>Close</Button>
    <Button variant="primary" disabled={!selected} onclick={() => (confirming = true)}>
      Restore this version
    </Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={confirming}
  title="Restore this version?"
  description="The Page's current text will be replaced."
>
  <p class="note">
    The text being replaced is saved to history first, so this can be undone by restoring the
    version marked <em>Before restoring</em>.
  </p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (confirming = false)}>Cancel</Button>
    <Button variant="primary" onclick={restore}>Restore</Button>
  {/snippet}
</Dialog>

<style>
  .split {
    display: grid;
    grid-template-columns: minmax(0, 22rem) minmax(0, 1fr);
    gap: var(--space-4);
    height: 24rem;
  }

  .list {
    overflow-y: auto;
    border: var(--border-width) solid var(--border-subtle);
    border-radius: var(--radius-md);
  }

  .entry {
    display: grid;
    gap: 2px;
    width: 100%;
    padding: var(--space-3);
    text-align: left;
    border-bottom: var(--border-width) solid var(--border-subtle);
    min-height: var(--touch-comfortable);
  }

  .entry:hover:not(.current) {
    background: var(--surface-hover);
  }

  .entry.selected {
    background: var(--surface-selected);
  }

  .entry.current {
    background: var(--surface-pane);
    cursor: default;
  }

  .when {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--text-primary);
  }

  .detail {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }

  .preview {
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .reason {
    justify-self: start;
    margin-top: var(--space-1);
    padding: 1px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--accent-quiet);
    color: var(--accent);
    font-size: var(--text-2xs);
  }

  .preview-pane {
    overflow-y: auto;
    padding: var(--space-4);
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-subtle);
    border-radius: var(--radius-md);
  }

  .preview-text {
    font-family: var(--font-manuscript);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    white-space: pre-wrap;
    color: var(--text-primary);
  }

  .status {
    color: var(--text-tertiary);
    font-size: var(--text-md);
    padding: var(--space-4);
  }

  .note {
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-normal);
  }

  /* On a narrow screen the two panes stack: side-by-side at 320px would give
     neither enough room to read. */
  @media (max-width: 640px) {
    .split {
      grid-template-columns: 1fr;
      grid-template-rows: 14rem minmax(0, 1fr);
      height: 26rem;
    }
  }
</style>
