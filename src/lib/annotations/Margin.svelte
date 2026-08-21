<script lang="ts">
  import AnnotationCard from './AnnotationCard.svelte';
  import AskGrimoire from './AskGrimoire.svelte';
  import TagRow from './TagRow.svelte';
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import TextField from '$lib/components/TextField.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import { margin } from '$lib/stores/margin.svelte';
  import {
    AUTHORABLE_KINDS,
    KIND_GLYPHS,
    KIND_LABELS,
    anchorOf,
    type Annotation,
    type AnnotationKind
  } from '$lib/types/annotation';

  /**
   * The Margin: notes in the space beside the manuscript.
   *
   * Anchored notes sit level with the words they refer to, and scroll with
   * them, because that vertical relationship is what makes a marginal note a
   * marginal note rather than a comment in a sidebar. Notes that would overlap
   * are pushed down just far enough to clear each other, which is what a
   * person writing in a real margin does too.
   *
   * Whole-Page notes have nothing to sit level with, so they gather at the top
   * under a heading that says as much.
   */

  interface Props {
    pageId: string | null;
    selection: { from: number; to: number; text: string } | null;
    /** Vertical offset, in content pixels, of each anchored note's text. */
    offsets: Map<string, number>;
    /** Height of the manuscript content, so the rail can match it. */
    contentHeight: number;
    onreveal: (annotation: Annotation) => void;
    onapplied?: () => void;
  }

  let { pageId, selection, offsets, contentHeight, onreveal, onapplied }: Props = $props();

  let askOpen = $state(false);
  let composing = $state(false);
  let composeKind = $state<AnnotationKind>('note');
  let composeBody = $state('');
  let composeRange = $state<{ from: number; to: number; text: string } | null>(null);

  let editing = $state(false);
  let editTarget = $state<Annotation | null>(null);
  let editBody = $state('');

  let deleting = $state(false);
  let deleteTarget = $state<Annotation | null>(null);

  /** Measured heights, so notes can be stacked without overlapping. */
  let heights = $state<Map<string, number>>(new Map());

  const hasSelection = $derived(selection !== null && selection.to > selection.from);

  const loose = $derived(margin.visible.filter((note) => anchorOf(note) === null));
  const anchored = $derived(margin.visible.filter((note) => anchorOf(note) !== null));

  /**
   * Where the Margin is an overlay rather than a pane, it has no manuscript
   * beside it to line up with, so notes stack in reading order instead. The
   * alignment is a property of sitting next to the text, not of the note.
   */
  const stacked = $derived(contentHeight === 0);

  /** Gap kept between two notes that would otherwise collide. */
  const GAP = 12;
  /** Assumed height for a note that has not been measured yet. */
  const ASSUMED = 64;

  /**
   * Lays out everything in the Margin in one pass.
   *
   * Every entry asks for a height: a whole-Page note and the streaming answer
   * ask for the top, an anchored note asks for the line its words are on. They
   * are then placed in order, each pushed down only as far as it must go to
   * clear the one above — which is what a person writing in a real margin does
   * when two notes want the same inch of paper.
   *
   * Doing this in one pass, from a single origin, is what keeps a note level
   * with its text. Laying the page-level notes out in normal flow first, as an
   * earlier version did, silently pushed every anchored note down by their
   * height and broke the alignment the Margin exists for.
   */
  const layout = $derived.by(() => {
    const tops = new Map<string, number>();
    let floor = 0;

    const place = (id: string, at: number) => {
      const top = Math.max(at, floor);
      tops.set(id, top);
      floor = top + (heights.get(id) ?? ASSUMED) + GAP;
    };

    // Anchored notes are laid out first and get the positions they asked for.
    // Anything placed before them would push them off the lines they belong
    // to, and being level with its text is the whole of what makes a marginal
    // note one.
    // A note whose anchor could not be measured this frame still gets a
    // position, below the ones that could. Dropping it would make a note
    // disappear for a reason the writer cannot see — and in the overlay, where
    // there is nothing to measure against, it would hide all of them.
    const measured = anchored
      .map((note) => ({ id: note.id, at: offsets.get(note.id) }))
      .filter((entry): entry is { id: string; at: number } => entry.at !== undefined)
      .sort((a, b) => a.at - b.at);
    for (const entry of measured) place(entry.id, entry.at);

    for (const note of anchored) {
      if (!tops.has(note.id)) place(note.id, floor);
    }

    // Then the things with no line of their own, after the last of them —
    // where a note about the whole page belongs anyway.
    if (ai.isStreaming) place(STREAM_ID, floor);
    for (const note of loose) place(note.id, floor);

    return { tops, tail: floor };
  });

  const STREAM_ID = '__streaming';

  function measure(node: HTMLElement, id: string) {
    const observer = new ResizeObserver(() => {
      const next = new Map(heights);
      next.set(id, node.offsetHeight);
      heights = next;
    });
    observer.observe(node);
    return {
      destroy() {
        observer.disconnect();
      }
    };
  }

  export function beginNote() {
    // Read the selection now: opening a dialog moves focus and the editor's
    // selection collapses the instant it does.
    composeRange = hasSelection ? selection : null;
    composeKind = 'note';
    composeBody = '';
    composing = true;
  }

  export function beginAsk() {
    askOpen = true;
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

<div
  class="margin"
  class:stacked
  aria-label="Margin"
  style:min-height="{Math.max(contentHeight, layout.tail + 40)}px"
>
  {#if ai.isStreaming}
    <!-- The answer as it arrives. Provisional, so it is drawn as unfinished
         handwriting rather than as a saved note. -->
    <div
      class="placed"
      style:top={stacked ? undefined : `${layout.tops.get(STREAM_ID) ?? 0}px`}
      use:measure={STREAM_ID}
    >
      <div class="streaming" aria-live="polite">
        <p class="streaming-head">
          <span>{ai.running?.profileName ?? 'Reading'} · {ai.running?.actionLabel ?? ''}</span>
          <button type="button" class="stop" onclick={() => ai.cancel()}>Stop</button>
        </p>
        <p class="streaming-body selectable">
          {ai.streamed}{#if ai.streamed === ''}Reading…{/if}
        </p>
      </div>
    </div>
  {/if}

  {#each [...anchored, ...loose] as note (note.id)}
    {@const top = layout.tops.get(note.id)}
    {#if top !== undefined}
      <div class="placed" style:top={stacked ? undefined : `${top}px`} use:measure={note.id}>
        <AnnotationCard
          annotation={note}
          focused={margin.focusedId === note.id}
          onfocus={(id) => (margin.focusedId = margin.focusedId === id ? null : id)}
          onedit={beginEdit}
          onresolve={toggleResolved}
          ondelete={(target) => {
            deleteTarget = target;
            deleting = true;
          }}
          {onreveal}
          onapplied={() => onapplied?.()}
        />
      </div>
    {/if}
  {/each}

  {#if margin.visible.length === 0 && !ai.isStreaming}
    <p class="blank">
      {hasSelection
        ? 'Leave a note about the selected text.'
        : 'Select a passage to write beside it.'}
    </p>
  {/if}

  <div class="tags" style:top={stacked ? undefined : `${layout.tail + 8}px`}>
    <TagRow {pageId} />
  </div>
</div>

<AskGrimoire bind:open={askOpen} {pageId} {selection} oncomplete={() => {}} />

<Dialog
  bind:open={composing}
  title={composeRange ? 'Note on the selected text' : 'Note on this Page'}
>
  <div class="form">
    {#if composeRange}
      <blockquote class="quoted selectable">{composeRange.text}</blockquote>
    {/if}

    <fieldset>
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
            <span class="chip-mark">{KIND_GLYPHS[kind]}</span>
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
  <p class="quoted">{deleteTarget?.body ?? ''}</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleting = false)}>Cancel</Button>
    <Button variant="danger" onclick={confirmDelete}>Delete</Button>
  {/snippet}
</Dialog>

<style>
  .margin {
    position: relative;
    /* No padding: the top of this element is the top of the sheet beside it,
       and every note's offset is measured from there. Padding here would shift
       every note away from the line it belongs to. */
    padding-right: var(--space-3);
    padding-left: var(--space-2);
  }

  .placed {
    position: absolute;
    left: var(--space-2);
    right: var(--space-3);
    transition: top var(--motion-base) var(--ease-out);
  }

  .tags {
    position: absolute;
    left: var(--space-2);
    right: var(--space-3);
  }

  /* As an overlay there is no manuscript alongside to line up with, so notes
     fall back to reading order. The alignment belongs to sitting beside the
     text, not to the note. */
  .margin.stacked {
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    padding: var(--space-4) var(--space-3);
  }

  .margin.stacked .placed,
  .margin.stacked .tags {
    position: static;
    left: auto;
    right: auto;
  }

  .blank {
    padding: var(--space-5) var(--space-3);
    font-size: var(--text-sm);
    line-height: var(--leading-snug);
    color: var(--text-tertiary);
  }

  .streaming {
    padding: var(--space-2) var(--space-3);
    margin-bottom: var(--space-4);
    border-left: 2px dashed var(--state-ai);
  }

  .streaming-head {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    margin-bottom: var(--space-1);
    font-size: var(--text-2xs);
    color: var(--state-ai);
  }

  .stop {
    margin-left: auto;
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
  }

  .stop:hover {
    color: var(--state-danger);
  }

  .streaming-body {
    font-family: var(--font-manuscript);
    font-size: 0.9375rem;
    line-height: 1.45;
    color: var(--text-secondary);
    white-space: pre-wrap;
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
    white-space: pre-wrap;
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
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    min-height: var(--touch-min);
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .chip-mark {
    font-family: var(--font-serif);
    color: var(--text-tertiary);
  }

  .chip.selected {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }

  .chip.selected .chip-mark {
    color: var(--accent-contrast);
  }
</style>
