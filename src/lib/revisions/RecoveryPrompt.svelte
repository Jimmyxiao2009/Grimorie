<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import {
    discardDraft,
    recoverDraft,
    recoverableDrafts
  } from '$lib/services/history';
  import { isDesktop } from '$lib/services/ipc';
  import { notices } from '$lib/stores/notices.svelte';
  import { count, relativeTime } from '$lib/utils/format';
  import type { RecoverableDraft } from '$lib/types/history';

  /**
   * Offers back text that was typed but never committed.
   *
   * Shown only when the backend reports something genuinely recoverable — it
   * discards stale and unchanged drafts rather than reporting them, so this
   * component never appears to say "nothing happened". Nothing is restored
   * automatically; the writer chooses, and the replaced text is snapshotted to
   * history either way.
   */

  interface Props {
    /** Called after a recovery so the open Page can be reloaded. */
    onrecovered?: (pageId: string) => void;
  }

  let { onrecovered }: Props = $props();

  let drafts = $state<RecoverableDraft[]>([]);
  let open = $state(false);
  let busy = $state(false);

  onMount(() => {
    if (!isDesktop) return;
    void recoverableDrafts()
      .then((found) => {
        if (found.length === 0) return;
        drafts = found;
        open = true;
      })
      .catch(() => {
        // A recovery check that itself fails must not block the app from
        // opening. The drafts stay on disk and are offered again next launch.
      });
  });

  const current = $derived(drafts[0] ?? null);

  async function next() {
    drafts = drafts.slice(1);
    if (drafts.length === 0) open = false;
  }

  async function keep() {
    const draft = current;
    if (!draft) return;
    busy = true;
    try {
      await recoverDraft(draft.pageId);
      onrecovered?.(draft.pageId);
      await next();
    } catch (error) {
      notices.failure(error);
      await next();
    } finally {
      busy = false;
    }
  }

  async function discard() {
    const draft = current;
    if (!draft) return;
    busy = true;
    try {
      await discardDraft(draft.pageId);
    } catch (error) {
      notices.failure(error);
    } finally {
      busy = false;
      await next();
    }
  }
</script>

{#if current}
  <Dialog
    bind:open
    title="Unsaved writing was recovered"
    description="Grimoire closed before this was saved."
    dismissible={false}
    width="md"
  >
    <div class="body">
      <p class="where">
        <strong>{current.pageTitle}</strong> in {current.volumeTitle} ·
        {relativeTime(current.capturedAt)}
      </p>

      <div class="compare">
        <section>
          <h3 class="eyebrow">Recovered <span class="tabular">{count(current.wordCount)} words</span></h3>
          <p class="excerpt selectable">{current.preview || 'Empty'}</p>
        </section>

        <section>
          <h3 class="eyebrow">
            Saved on the Page <span class="tabular">{count(current.savedWordCount)} words</span>
          </h3>
          <p class="excerpt muted selectable">{current.savedPreview || 'Empty'}</p>
        </section>
      </div>

      <p class="reassurance">
        Keeping the recovered text saves the version currently on the Page to its history first, so
        neither one is lost.
      </p>

      {#if drafts.length > 1}
        <p class="more tabular">{drafts.length - 1} more to review after this.</p>
      {/if}
    </div>

    {#snippet footer()}
      <Button variant="ghost" disabled={busy} onclick={discard}>Discard it</Button>
      <Button variant="primary" disabled={busy} onclick={keep}>Keep the recovered text</Button>
    {/snippet}
  </Dialog>
{/if}

<style>
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .where {
    font-size: var(--text-md);
    color: var(--text-secondary);
  }

  .compare {
    display: grid;
    gap: var(--space-4);
  }

  .compare h3 {
    display: flex;
    justify-content: space-between;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
  }

  .excerpt {
    padding: var(--space-3);
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-subtle);
    border-left: 2px solid var(--accent);
    border-radius: var(--radius-sm);
    font-family: var(--font-manuscript);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    color: var(--text-primary);
  }

  .excerpt.muted {
    border-left-color: var(--border-strong);
    color: var(--text-secondary);
  }

  .reassurance {
    font-size: var(--text-sm);
    color: var(--text-tertiary);
    line-height: var(--leading-snug);
  }

  .more {
    font-size: var(--text-sm);
    color: var(--text-tertiary);
  }
</style>
