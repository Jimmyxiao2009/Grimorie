<script lang="ts">
  import type { Editor } from '@tiptap/core';
  import Icon from '$lib/components/Icon.svelte';
  import type { EditorState } from './PageEditor.svelte';
  import type { IconName } from '$lib/design/icons';

  /**
   * The formatting controls, shown only over a selection.
   *
   * They used to live in a permanent strip across the top of the window, which
   * made Grimoire look like a simplified word processor and spent the most
   * valuable row on screen saying "bold" to someone who was not using it. Here
   * they appear where the writer is already looking, over the words they have
   * just chosen, and vanish the moment the selection does.
   *
   * The two Grimoire-specific actions — leave a note, ask a reader — sit in the
   * same bar, because selecting a passage is exactly when a writer wants them.
   */

  interface Props {
    editor: Editor | null;
    /** Named `editorState`, not `state`: a local `state` shadows the rune. */
    editorState: EditorState | null;
    /** True when a provider is configured; hides Ask when it is not. */
    aiAvailable: boolean;
    onlink: () => void;
    onnote: () => void;
    onask: () => void;
  }

  let { editor, editorState, aiAvailable, onlink, onnote, onask }: Props = $props();

  let position = $state<{ left: number; top: number } | null>(null);
  let bar = $state<HTMLElement | null>(null);

  const marks = $derived(editorState?.marks ?? {});
  const active = $derived(editorState?.selection ?? null);

  function chain() {
    return editor?.chain().focus();
  }

  /**
   * Places the bar centred over the selection.
   *
   * Uses viewport coordinates and `position: fixed`, so it does not have to
   * track the editor's scrolling — the selection's own coordinates already move
   * with it.
   */
  function place() {
    if (!editor || !active) {
      position = null;
      return;
    }

    const { from, to } = editor.state.selection;
    const start = editor.view.coordsAtPos(Math.min(from, to));
    const end = editor.view.coordsAtPos(Math.max(from, to));

    const centre = (start.left + end.right) / 2;
    const width = bar?.offsetWidth ?? 300;
    const height = bar?.offsetHeight ?? 40;
    const margin = 8;

    let left = centre - width / 2;
    left = Math.min(Math.max(left, margin), window.innerWidth - width - margin);

    // Above the selection by preference; below it when there is no room, which
    // is the common case for a selection on the first line.
    let top = start.top - height - 10;
    if (top < margin) top = end.bottom + 10;

    position = { left, top };
  }

  // Re-place whenever the selection changes. Reading `active` is what
  // subscribes this effect to it.
  $effect(() => {
    void active;
    if (!active) {
      position = null;
      return;
    }
    // After the bar has been laid out, so its measured width centres correctly.
    const frame = requestAnimationFrame(place);
    return () => cancelAnimationFrame(frame);
  });

  $effect(() => {
    if (!position) return;
    const reposition = () => place();
    window.addEventListener('resize', reposition);
    window.addEventListener('scroll', reposition, true);
    return () => {
      window.removeEventListener('resize', reposition);
      window.removeEventListener('scroll', reposition, true);
    };
  });

  type Control = {
    id: string;
    icon: IconName;
    label: string;
    on?: boolean;
    run: () => void;
  };

  const controls = $derived<Control[]>([
    { id: 'bold', icon: 'bold', label: 'Bold', on: marks['bold'], run: () => chain()?.toggleBold().run() },
    {
      id: 'italic',
      icon: 'italic',
      label: 'Italic',
      on: marks['italic'],
      run: () => chain()?.toggleItalic().run()
    },
    {
      id: 'underline',
      icon: 'underline',
      label: 'Underline',
      on: marks['underline'],
      run: () => chain()?.toggleUnderline().run()
    },
    {
      id: 'quote',
      icon: 'quote',
      label: 'Block quote',
      on: marks['blockquote'],
      run: () => chain()?.toggleBlockquote().run()
    },
    {
      id: 'heading',
      icon: 'heading',
      label: 'Heading',
      on: marks['h2'],
      run: () => chain()?.toggleHeading({ level: 2 }).run()
    },
    { id: 'link', icon: 'link', label: 'Link', on: marks['link'], run: onlink }
  ]);
</script>

{#if position && active}
  <div
    bind:this={bar}
    class="bar"
    style:left="{position.left}px"
    style:top="{position.top}px"
    role="toolbar"
    aria-label="Formatting"
  >
    {#each controls as control (control.id)}
      <button
        type="button"
        class="control"
        class:on={control.on}
        aria-label={control.label}
        aria-pressed={control.on}
        title={control.label}
        onmousedown={(event) => event.preventDefault()}
        onclick={control.run}
      >
        <Icon name={control.icon} size={16} />
      </button>
    {/each}

    <span class="divider" aria-hidden="true"></span>

    <button
      type="button"
      class="control wide"
      onmousedown={(event) => event.preventDefault()}
      onclick={onnote}
    >
      <Icon name="margin" size={15} />
      <span>Note</span>
    </button>

    {#if aiAvailable}
      <button
        type="button"
        class="control wide"
        onmousedown={(event) => event.preventDefault()}
        onclick={onask}
      >
        <Icon name="sparkle" size={15} />
        <span>Ask</span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .bar {
    position: fixed;
    z-index: var(--z-menu);
    display: flex;
    align-items: center;
    gap: 1px;
    padding: 3px;
    background: var(--surface-raised);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-overlay);
    animation: appear var(--motion-fast) var(--ease-out);
  }

  .control {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-1);
    /* Smaller than the 40px floor used elsewhere: this bar hovers over the
       writer's own selection, and a full-size one would cover the words it is
       about. The targets are still 34px, which is comfortable for a finger on
       a control that appears exactly where the finger already is. */
    min-width: 34px;
    height: 34px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-sm);
    color: var(--text-secondary);
    font-size: var(--text-sm);
  }

  .control:hover {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .control.on {
    background: var(--surface-selected);
    color: var(--accent);
  }

  .wide {
    padding: 0 var(--space-2) 0 var(--space-2);
  }

  .divider {
    width: var(--border-width);
    height: 18px;
    margin: 0 var(--space-1);
    background: var(--border-subtle);
  }

  @keyframes appear {
    from {
      opacity: 0;
      transform: translateY(3px);
    }
  }
</style>
