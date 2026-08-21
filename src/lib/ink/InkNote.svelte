<script lang="ts">
  /**
   * One handwritten note in the Margin.
   *
   * Not a card, like the text notes — handwriting is ink on the margin itself,
   * so the note is its drawing surface with a quiet rule on the left and
   * controls that appear on hover or focus. The surface is the note.
   */

  import InkSurface from '$lib/ink/InkSurface.svelte';
  import Menu from '$lib/components/Menu.svelte';
  import { ink } from '$lib/stores/ink.svelte';
  import { inkRecognition } from '$lib/stores/ink-recognition.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { notices } from '$lib/stores/notices.svelte';
  import { relativeTime } from '$lib/utils/format';
  import { anchorOf, type Annotation } from '$lib/types/annotation';
  import type { MenuItem } from '$lib/types/ui';
  import type { InkRecognition } from '$lib/types/ink';

  interface Props {
    annotation: Annotation;
    focused: boolean;
    onfocus: (id: string) => void;
    ondelete: (annotation: Annotation) => void;
    onreveal: (annotation: Annotation) => void;
    onconverted?: () => void;
    onask?: () => void;
  }

  let { annotation, focused, onfocus, ondelete, onreveal, onconverted, onask }: Props = $props();

  const anchor = $derived(anchorOf(annotation));
  const stale = $derived(annotation.status === 'stale');
  const strokes = $derived(ink.strokesFor(annotation.id));
  const strokeCount = $derived(strokes.length);
  const active = $derived(ink.penMode);

  const recognition = $derived<InkRecognition | null>(inkRecognition.forAnnotation(annotation.id));

  /** Whether the note has a transcript to inspect, search, or convert. */
  const hasTranscript = $derived(
    recognition !== null && !!recognition.recognizedText && recognition.recognizedText.length > 0
  );

  /** A short, unobtrusive status label for the metadata row. */
  const statusLabel = $derived.by(() => {
    if (!recognition) return '';
    switch (recognition.status) {
      case 'recognizing':
        return 'Recognizing…';
      case 'recognized':
        return recognition.transcriptSource === 'user-edited' ? 'Recognized · edited' : 'Recognized';
      case 'failed':
        return 'Recognition failed';
      case 'stale':
        return 'Re-recognizing…';
      case 'pending':
        return 'Recognizing…';
      case 'disabled':
        return '';
      default:
        return '';
    }
  });

  let showTranscript = $state(false);
  let editingTranscript = $state(false);
  let transcriptDraft = $state('');

  function toggleTranscript() {
    showTranscript = !showTranscript;
    if (!showTranscript) editingTranscript = false;
  }

  function beginEditTranscript() {
    transcriptDraft = recognition?.recognizedText ?? '';
    editingTranscript = true;
  }

  async function saveTranscript() {
    const text = transcriptDraft.trim();
    if (!text) {
      editingTranscript = false;
      return;
    }
    try {
      await inkRecognition.editTranscript(annotation.id, text);
      editingTranscript = false;
    } catch (error) {
      notices.failure(error);
    }
  }

  async function copyTranscript() {
    const text = recognition?.recognizedText ?? '';
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      notices.info('Transcript copied.');
    } catch {
      notices.failure(new Error('Grimoire could not copy the transcript.'));
    }
  }

  async function recognizeNow() {
    try {
      await inkRecognition.recognizeNow(annotation.id, strokes);
      showTranscript = true;
    } catch (error) {
      notices.failure(error);
    }
  }

  async function convertToText() {
    try {
      await inkRecognition.convertToText(annotation.id);
      notices.info('Converted to a text note. The handwriting is kept.');
      onconverted?.();
    } catch (error) {
      notices.failure(error);
    }
  }

  const canRecognize = $derived(
    settingsStore.settings.aiEnabled && strokeCount > 0
  );

  const actions = $derived<MenuItem[]>([
    {
      id: 'recognize',
      label: hasTranscript ? 'Recognize again' : 'Recognize handwriting',
      icon: 'sparkle',
      disabled: !canRecognize,
      select: recognizeNow
    },
    {
      id: 'show-transcript',
      label: showTranscript ? 'Hide recognized text' : 'Show recognized text',
      disabled: !hasTranscript,
      select: toggleTranscript
    },
    {
      id: 'ask-ai',
      label: 'Ask AI about this note',
      icon: 'sparkle',
      disabled: !canRecognize || !hasTranscript || !onask,
      select: () => onask?.()
    },
    { kind: 'separator', id: 'sep-1' },
    {
      id: 'edit-transcript',
      label: 'Edit transcript',
      disabled: !hasTranscript,
      select: beginEditTranscript
    },
    {
      id: 'copy-transcript',
      label: 'Copy transcript',
      disabled: !hasTranscript,
      select: copyTranscript
    },
    {
      id: 'convert',
      label: 'Convert to Text Note',
      disabled: !hasTranscript,
      select: convertToText
    },
    { kind: 'separator', id: 'sep-2' },
    { id: 'delete', label: 'Delete ink note', icon: 'trash', danger: true, select: () => ondelete(annotation) }
  ]);

  /** A min height so an empty ink note still offers room to write. */
  const MIN_HEIGHT = 120;

  /**
   * The surface's height grows to fit its strokes, so handwriting written low
   * in the margin is never clipped on reload. A little padding keeps the last
   * stroke's tail from sitting on the edge.
   */
  const surfaceHeight = $derived.by(() => {
    let max = 0;
    for (const stroke of strokes) {
      for (const point of stroke.points) {
        if (point.y > max) max = point.y;
      }
    }
    // 28px of slack below the lowest point, floored at the empty-note height.
    return Math.max(MIN_HEIGHT, max + 28);
  });
</script>

<article
  class="ink-note"
  class:focused
  class:stale
  class:active
  style:min-height="{MIN_HEIGHT}px"
  onfocusin={() => onfocus(annotation.id)}
>
  <span class="rule" aria-hidden="true"></span>

  <!-- The surface fills the note and is the writing area. -->
  <div class="surface" style:min-height="{surfaceHeight}px">
    <InkSurface annotationId={annotation.id} {active} />
  </div>

  {#if strokeCount === 0 && !active}
    <p class="hint">Write here with the pen.</p>
  {/if}

  <div class="menu">
    <Menu items={actions} label="Ink note actions" size="sm" />
  </div>

  <button
    type="button"
    class="reveal"
    aria-label="Show this ink note in the manuscript"
    onclick={() => {
      if (anchor) onreveal(annotation);
      onfocus(annotation.id);
    }}
  >
    {#if stale}
      <span class="unstuck">Text changed</span>
    {/if}
    <span class="when">
      {strokeCount > 0
        ? `${strokeCount} stroke${strokeCount === 1 ? '' : 's'}`
        : 'empty'} · {relativeTime(annotation.updatedAt)}
    </span>
    {#if statusLabel}
      <span class="recognition-status" class:failed={recognition?.status === 'failed'}>
        {statusLabel}
      </span>
    {/if}
  </button>

  {#if showTranscript && recognition?.recognizedText}
    <div class="transcript" class:hidden={!focused && !showTranscript}>
      {#if editingTranscript}
        <textarea
          class="transcript-edit"
          bind:value={transcriptDraft}
          rows="3"
          aria-label="Edit the recognized transcript"
        ></textarea>
        <div class="transcript-actions">
          <button type="button" class="ta" onclick={() => (editingTranscript = false)}>Cancel</button>
          <button type="button" class="ta primary" onclick={saveTranscript}>Save</button>
        </div>
      {:else}
        <p class="transcript-text selectable">{recognition.recognizedText}</p>
        {#if recognition.error}
          <p class="transcript-error">{recognition.error}</p>
        {/if}
        <div class="transcript-actions">
          <button type="button" class="ta" onclick={beginEditTranscript}>Edit</button>
          <button type="button" class="ta" onclick={copyTranscript}>Copy</button>
          <button type="button" class="ta" onclick={recognizeNow}>Recognize again</button>
          <button type="button" class="ta" onclick={convertToText}>Convert to Note</button>
        </div>
      {/if}
    </div>
  {/if}
</article>

<style>
  .ink-note {
    position: relative;
    min-height: 120px;
    padding: var(--space-1) var(--space-2) var(--space-2) var(--space-3);
  }

  .rule {
    position: absolute;
    left: 0;
    top: 6px;
    bottom: var(--space-3);
    width: 1px;
    background: var(--border-default);
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .focused .rule {
    width: 2px;
    background: var(--accent);
  }

  .stale .rule {
    background: var(--state-warning);
    background-image: repeating-linear-gradient(
      to bottom,
      var(--state-warning) 0 3px,
      transparent 3px 6px
    );
  }

  .surface {
    position: relative;
    width: 100%;
    min-height: 96px;
  }

  .hint {
    position: absolute;
    top: var(--space-1);
    left: calc(var(--space-3) + var(--space-1));
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
    pointer-events: none;
  }

  .menu {
    position: absolute;
    top: 0;
    right: 0;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .ink-note:hover .menu,
  .menu:focus-within,
  .focused .menu {
    opacity: 1;
  }

  /* In pen mode the controls step aside so they never sit under the hand. */
  .active .menu {
    opacity: 0;
  }

  /* The reveal link is metadata in pen mode, not a target. */
  .active .reveal {
    pointer-events: none;
  }

  .reveal {
    display: block;
    width: 100%;
    padding: var(--space-1) 0 0 calc(var(--space-3) + var(--space-1));
    text-align: left;
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
  }

  .unstuck {
    color: var(--state-warning);
    margin-right: var(--space-2);
  }

  .when {
    /* tabular keeps the stroke count from shifting width as it changes. */
    font-variant-numeric: tabular-nums;
  }

  /* The recognition status sits at the right of the metadata row, quiet unless
     it failed — a failure keeps a subtle retry affordance visible. */
  .recognition-status {
    margin-left: auto;
    color: var(--text-tertiary);
  }

  .recognition-status.failed {
    color: var(--state-warning);
  }

  /* The transcript is a lightweight expandable panel, not a duplicate typed
     paragraph under every note. It appears on demand and stays secondary to the
     handwriting, which is always what the note looks like. */
  .transcript {
    margin: var(--space-1) var(--space-1) 0 calc(var(--space-3) + var(--space-1));
    padding: var(--space-2) var(--space-2) var(--space-1);
    border-left: 2px solid var(--border-default);
    background: var(--surface-page);
    border-radius: var(--radius-sm);
  }

  .transcript-text {
    font-family: var(--font-manuscript);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
    color: var(--text-secondary);
    white-space: pre-wrap;
    margin: 0 0 var(--space-1);
  }

  .transcript-error {
    font-size: var(--text-2xs);
    color: var(--state-warning);
    margin: 0 0 var(--space-1);
  }

  .transcript-edit {
    width: 100%;
    font-family: var(--font-manuscript);
    font-size: var(--text-sm);
    line-height: var(--leading-normal);
    color: var(--text-secondary);
    background: var(--surface-base);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-sm);
    padding: var(--space-1) var(--space-2);
    resize: vertical;
  }

  .transcript-actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .ta {
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
  }

  .ta:hover {
    color: var(--text-secondary);
  }

  .ta.primary {
    color: var(--accent);
  }
</style>
