<script lang="ts" module>
  export type EditorState = {
    words: number;
    characters: number;
    marks: Record<string, boolean>;
    canUndo: boolean;
    canRedo: boolean;
  };
</script>

<script lang="ts">
  import { onMount } from 'svelte';
  import { Editor } from '@tiptap/core';
  import { buildExtensions } from './schema';
  import { autosave } from './autosave.svelte';
  import type { Page, ProseMirrorDocument } from '$lib/types/manuscript';

  interface Props {
    /**
     * The Page to edit.
     *
     * This component holds exactly one Page for its whole life. The parent
     * wraps it in `{#key page.id}` so opening a different Page builds a fresh
     * editor — which is the only reliable way to leave undo history behind.
     * Sharing one editor across Pages would let Ctrl+Z on one Page undo an
     * edit made in another.
     */
    page: Page;
    spellcheck?: boolean;
    autofocus?: boolean;
    onstate?: (state: EditorState) => void;
    onready?: (editor: Editor) => void;
  }

  let { page, spellcheck = true, autofocus = false, onstate, onready }: Props = $props();

  let host = $state<HTMLDivElement | null>(null);
  let editor: Editor | null = null;
  let frame = 0;

  // Captured once, on purpose. Later prop updates — a Page object replaced
  // after a save — must not be able to redirect this editor's writes to a
  // different row, and the parent rebuilds this component when the Page
  // genuinely changes.
  // svelte-ignore state_referenced_locally
  const pageId = page.id;

  function editorAttributes() {
    return {
      class: 'manuscript selectable',
      spellcheck: String(spellcheck),
      role: 'textbox',
      'aria-multiline': 'true',
      'aria-label': `${page.title}, manuscript`
    };
  }

  /**
   * Reports counts and formatting to the surrounding chrome, coalesced to one
   * report per animation frame.
   *
   * ProseMirror fires a transaction per keystroke. Waking Svelte on every one
   * would put reactive work directly in the typing path, which is the one place
   * this application cannot afford it.
   */
  function report() {
    if (frame || !onstate) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (!editor || !onstate) return;
      const counter = editor.storage['characterCount'] as
        | { words: () => number; characters: () => number }
        | undefined;

      onstate({
        words: counter?.words() ?? 0,
        characters: counter?.characters() ?? 0,
        marks: {
          bold: editor.isActive('bold'),
          italic: editor.isActive('italic'),
          underline: editor.isActive('underline'),
          strike: editor.isActive('strike'),
          code: editor.isActive('code'),
          link: editor.isActive('link'),
          blockquote: editor.isActive('blockquote'),
          bulletList: editor.isActive('bulletList'),
          orderedList: editor.isActive('orderedList'),
          h1: editor.isActive('heading', { level: 1 }),
          h2: editor.isActive('heading', { level: 2 }),
          h3: editor.isActive('heading', { level: 3 })
        },
        canUndo: editor.can().undo(),
        canRedo: editor.can().redo()
      });
    });
  }

  onMount(() => {
    if (!host) return;

    editor = new Editor({
      element: host,
      extensions: buildExtensions('Begin writing…'),
      content: page.document,
      autofocus: autofocus ? 'end' : false,
      editorProps: { attributes: editorAttributes() },
      onUpdate: ({ editor: instance }) => {
        // The entire keystroke path: hand the document to the save queue and
        // return. No serialisation to string, no IPC, no database.
        autosave.schedule(pageId, instance.getJSON() as ProseMirrorDocument);
        report();
      },
      onSelectionUpdate: report,
      onCreate: report
    });

    onready?.(editor);

    return () => {
      if (frame) cancelAnimationFrame(frame);
      editor?.destroy();
      editor = null;
      // Anything still queued stays queued: the save is addressed to this
      // Page's id and completes even though the editor is gone.
    };
  });

  // Spellcheck is a setting, so it can change while a Page is open.
  $effect(() => {
    const on = spellcheck;
    editor?.setOptions({
      editorProps: { attributes: { ...editorAttributes(), spellcheck: String(on) } }
    });
  });
</script>

<div class="host" bind:this={host}></div>

<style>
  .host {
    display: contents;
  }

  /* ProseMirror renders its own DOM, so the manuscript's typography lives in
     :global rules. This is the reading surface: everything here exists to make
     long text comfortable for hours. */
  :global(.manuscript) {
    max-width: var(--measure-editor);
    margin-inline: auto;
    /* Deep bottom padding so the line being written is never pinned to the
       bottom edge of the window. */
    padding: var(--space-7) var(--space-5) 40vh;
    min-height: 100%;
    font-family: var(--font-manuscript);
    font-size: var(--manuscript-size);
    line-height: var(--manuscript-leading);
    color: var(--text-primary);
    outline: none;
    overflow-wrap: break-word;
  }

  /* Reset every top-level block, then space them apart. Both selectors carry
     the same specificity so the later one wins — a more specific reset such as
     `.manuscript p { margin: 0 }` would silently beat the gap rule and collapse
     the paragraph spacing entirely. */
  :global(.manuscript > *) {
    margin: 0;
  }

  :global(.manuscript > * + *) {
    margin-top: var(--manuscript-paragraph-gap);
  }

  :global(.manuscript h1),
  :global(.manuscript h2),
  :global(.manuscript h3) {
    font-family: var(--font-manuscript);
    font-weight: var(--weight-semibold);
    line-height: var(--leading-snug);
    letter-spacing: -0.005em;
    margin-top: calc(var(--manuscript-paragraph-gap) * 2);
  }

  :global(.manuscript h1) {
    font-size: 1.6em;
  }

  :global(.manuscript h2) {
    font-size: 1.32em;
  }

  :global(.manuscript h3) {
    font-size: 1.12em;
  }

  :global(.manuscript blockquote) {
    padding-left: var(--space-4);
    border-left: 2px solid var(--border-default);
    color: var(--text-secondary);
    font-style: italic;
  }

  :global(.manuscript ul),
  :global(.manuscript ol) {
    padding-left: 1.4em;
  }

  :global(.manuscript ul) {
    list-style: disc;
  }

  :global(.manuscript ol) {
    list-style: decimal;
  }

  :global(.manuscript li) {
    margin-top: 0.25em;
  }

  :global(.manuscript li p) {
    margin: 0;
  }

  :global(.manuscript code) {
    font-family: var(--font-mono);
    font-size: 0.88em;
    padding: 0.12em 0.34em;
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
  }

  :global(.manuscript a) {
    color: var(--accent);
    text-decoration: underline;
    text-underline-offset: 0.18em;
    text-decoration-thickness: 1px;
  }

  :global(.manuscript hr) {
    border: none;
    height: var(--border-width);
    background: var(--border-default);
    margin-block: calc(var(--manuscript-paragraph-gap) * 1.5);
  }

  /* Shown only on a genuinely empty document — ghost text appearing between
     finished paragraphs would be a distraction. */
  :global(.manuscript p.is-editor-empty:first-child::before) {
    content: attr(data-placeholder);
    float: left;
    height: 0;
    pointer-events: none;
    color: var(--text-tertiary);
  }

  :global(.manuscript ::selection) {
    background: var(--accent-quiet);
  }
</style>
