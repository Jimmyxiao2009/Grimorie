<script lang="ts">
  import AnnotationCard from './AnnotationCard.svelte';
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import TextField from '$lib/components/TextField.svelte';
  import TagRow from './TagRow.svelte';
  import AskGrimoire from './AskGrimoire.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import { margin } from '$lib/stores/margin.svelte';
  import {
    AUTHORABLE_KINDS,
    KIND_LABELS,
    type Annotation,
    type AnnotationKind
  } from '$lib/types/annotation';

  interface Props {
    pageId: string | null;
    /** The current selection in plain-text offsets, or null if nothing is selected. */
    selection: { from: number; to: number; text: string } | null;
    onreveal: (annotation: Annotation) => void;
    /** Shown on narrow layouts, where the Margin is an overlay. */
    onclose?: () => void;
    /** Called after an AI suggestion is applied, so the Page can reload. */
    onapplied?: () => void;
  }

  let { pageId, selection, onreveal, onclose, onapplied }: Props = $props();

  let askOpen = $state(false);

  let composing = $state(false);
  let composeKind = $state<AnnotationKind>('note');
  let composeBody = $state('');
  /** Captured when composing opens: the selection moves as soon as focus does. */
  let composeRange = $state<{ from: number; to: number; text: string } | null>(null);

  let editing = $state(false);
  let editTarget = $state<Annotation | null>(null);
  let editBody = $state('');

  let deleting = $state(false);
  let deleteTarget = $state<Annotation | null>(null);

  const hasSelection = $derived(selection !== null && selection.to > selection.from);

  function beginNote() {
    // The selection is read *now* and held, because opening a dialog moves
    // focus and the editor's selection collapses the moment it does.
    composeRange = hasSelection ? selection : null;
    composeKind = 'note';
    composeBody = '';
    composing = true;
  }

  async function submitNote() {
    const body = composeBody.trim();
    if (!pageId || !body) return;
    const range = composeRange;
    composing = false;

    if (range) await margin.createAnchored(pageId, composeKind, body, range.from, range.to);
    else await margin.createForPage(pageId, composeKind, body);
  }

  function beginEdit(annotation: Annotation) {
    editTarget = annotation;
    editBody = annotation.body;
    editing = true;
  }

  async function submitEdit() {
    const target = editTarget;
    const body = editBody.trim();
    if (!target || !body) return;
    editing = false;
    await margin.updateBody(target.id, body);
  }

  function beginDelete(annotation: Annotation) {
    deleteTarget = annotation;
    deleting = true;
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target) return;
    deleting = false;
    await margin.remove(target.id);
  }

  async function toggleResolved(annotation: Annotation) {
    if (annotation.status === 'resolved') await margin.reopen(annotation.id);
    else await margin.resolve(annotation.id);
  }
</script>

<section class="margin" aria-label="Margin">
  <header class="head">
    <h2 class="eyebrow">Margin</h2>
    {#if margin.openCount > 0}
      <span class="tally tabular">{margin.openCount}</span>
    {/if}
    <div class="head-actions">
      <IconButton
        name="eye"
        label={margin.showResolved ? 'Hide resolved notes' : 'Show resolved notes'}
        size="sm"
        pressed={margin.showResolved}
        onclick={() => (margin.showResolved = !margin.showResolved)}
      />
      {#if onclose}
        <IconButton name="close" label="Close the Margin" size="sm" onclick={onclose} />
      {/if}
    </div>
  </header>

  <div class="notes" data-scroll>
    {#if ai.isStreaming}
      <div class="streaming" aria-live="polite">
        <header class="streaming-head">
          <Icon name="sparkle" size={14} />
          <span class="streaming-title">
            {ai.running?.profileName ?? 'Reading'} · {ai.running?.actionLabel ?? ''}
          </span>
          <button type="button" class="stop" onclick={() => ai.cancel()}>Stop</button>
        </header>
        <p class="streaming-body selectable">
          {ai.streamed}{#if ai.streamed === ''}Reading the passage…{/if}
        </p>
      </div>
    {/if}

    {#if margin.loading}
      <p class="status">Reading the Margin…</p>
    {:else if margin.visible.length === 0}
      <EmptyState
        size="sm"
        title="No margin notes yet."
        hint={hasSelection
          ? 'Add a note about the text you have selected.'
          : 'Select some text in the manuscript, then leave a note about it.'}
      />
    {:else}
      {#each margin.visible as annotation (annotation.id)}
        <AnnotationCard
          {annotation}
          focused={margin.focusedId === annotation.id}
          onfocus={(id) => (margin.focusedId = margin.focusedId === id ? null : id)}
          onedit={beginEdit}
          onresolve={toggleResolved}
          ondelete={beginDelete}
          {onreveal}
          onapplied={() => onapplied?.()}
        />
      {/each}
    {/if}
  </div>

  <TagRow {pageId} />

  <footer class="foot">
    <Button variant="secondary" size="sm" icon="plus" block disabled={!pageId} onclick={beginNote}>
      {hasSelection ? 'Note on selection' : 'Note on this Page'}
    </Button>

    {#if ai.isConfigured}
      <Button
        variant="ghost"
        size="sm"
        icon="sparkle"
        block
        disabled={!pageId || !hasSelection || ai.isStreaming}
        onclick={() => (askOpen = true)}
      >
        {hasSelection ? 'Ask Grimoire' : 'Select text to ask'}
      </Button>
    {/if}
  </footer>
</section>

<AskGrimoire
  bind:open={askOpen}
  {pageId}
  {selection}
  oncomplete={() => {}}
/>

<Dialog
  bind:open={composing}
  title={composeRange ? 'Note on the selected text' : 'Note on this Page'}
>
  <div class="form">
    {#if composeRange}
      <blockquote class="quoted selectable">{composeRange.text}</blockquote>
    {/if}

    <fieldset class="kinds">
      <legend class="eyebrow">Kind</legend>
      <div class="chips">
        {#each AUTHORABLE_KINDS as kind (kind)}
          <button
            type="button"
            class="chip"
            class:selected={composeKind === kind}
            aria-pressed={composeKind === kind}
            onclick={() => (composeKind = kind)}
          >
            {KIND_LABELS[kind]}
          </button>
        {/each}
      </div>
    </fieldset>

    <TextField bind:value={composeBody} label="Note" multiline rows={4} autofocus />
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (composing = false)}>Cancel</Button>
    <Button variant="primary" disabled={!composeBody.trim()} onclick={submitNote}>Add note</Button>
  {/snippet}
</Dialog>

<Dialog bind:open={editing} title="Edit note">
  <TextField bind:value={editBody} label="Note" multiline rows={4} autofocus />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (editing = false)}>Cancel</Button>
    <Button variant="primary" disabled={!editBody.trim()} onclick={submitEdit}>Save</Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={deleting}
  title="Delete this note?"
  description="The manuscript itself is not affected."
>
  <p class="note-preview">{deleteTarget?.body ?? ''}</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleting = false)}>Cancel</Button>
    <Button variant="danger" onclick={confirmDelete}>Delete</Button>
  {/snippet}
</Dialog>

<style>
  .margin {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--surface-pane);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    min-height: var(--rail-height);
    padding: 0 var(--space-2) 0 var(--space-3);
    border-bottom: var(--border-width) solid var(--border-subtle);
  }

  .head h2 {
    flex: none;
  }

  .tally {
    padding: 1px var(--space-2);
    border-radius: var(--radius-pill);
    background: var(--surface-active);
    color: var(--text-secondary);
    font-size: var(--text-2xs);
  }

  .head-actions {
    display: flex;
    align-items: center;
    margin-left: auto;
  }

  .notes {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-3) var(--space-5);
  }

  .status {
    padding: var(--space-4);
    color: var(--text-tertiary);
    font-size: var(--text-sm);
  }

  .foot {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    flex: none;
    padding: var(--space-2);
    border-top: var(--border-width) solid var(--border-subtle);
  }

  /* The answer as it arrives, above the settled notes. It is not a card yet —
     nothing has been saved — so it is drawn as a dashed provisional block. */
  .streaming {
    padding: var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) dashed var(--state-ai);
    background: var(--surface-raised);
  }

  .streaming-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-bottom: var(--space-2);
    color: var(--state-ai);
  }

  .streaming-title {
    flex: 1;
    font-size: var(--text-xs);
    font-weight: var(--weight-medium);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stop {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
  }

  .stop:hover {
    background: var(--surface-hover);
    color: var(--state-danger);
  }

  .streaming-body {
    font-size: var(--text-md);
    line-height: var(--leading-snug);
    color: var(--text-primary);
    white-space: pre-wrap;
    overflow-wrap: break-word;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .quoted {
    padding: var(--space-2) var(--space-3);
    border-left: 2px solid var(--accent);
    background: var(--surface-page);
    border-radius: var(--radius-sm);
    font-family: var(--font-manuscript);
    font-size: var(--text-md);
    line-height: var(--leading-normal);
    color: var(--text-secondary);
    max-height: 8rem;
    overflow-y: auto;
  }

  fieldset {
    border: none;
    padding: 0;
    margin: 0;
  }

  legend {
    padding: 0;
    margin-bottom: var(--space-2);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .chip {
    min-height: var(--touch-min);
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .chip:hover {
    background: var(--surface-hover);
  }

  .chip.selected {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }

  .note-preview {
    padding: var(--space-3);
    background: var(--surface-page);
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
    color: var(--text-secondary);
    white-space: pre-wrap;
  }
</style>
