<script lang="ts">
  import type { Editor } from '@tiptap/core';
  import IconButton from '$lib/components/IconButton.svelte';
  import Menu from '$lib/components/Menu.svelte';
  import type { MenuItem } from '$lib/types/ui';
  import type { EditorState } from './PageEditor.svelte';

  interface Props {
    editor: Editor | null;
    state: EditorState | null;
    /** Asks the workspace to open the link dialog. */
    onlink: () => void;
  }

  let { editor, state, onlink }: Props = $props();

  const marks = $derived(state?.marks ?? {});

  function chain() {
    return editor?.chain().focus();
  }

  const blockItems = $derived<MenuItem[]>([
    { kind: 'heading', id: 'h', label: 'Paragraph style' },
    { id: 'p', label: 'Body text', select: () => chain()?.setParagraph().run() },
    { id: 'h1', label: 'Heading 1', select: () => chain()?.toggleHeading({ level: 1 }).run() },
    { id: 'h2', label: 'Heading 2', select: () => chain()?.toggleHeading({ level: 2 }).run() },
    { id: 'h3', label: 'Heading 3', select: () => chain()?.toggleHeading({ level: 3 }).run() },
    { kind: 'separator', id: 's1' },
    {
      id: 'quote',
      label: 'Block quote',
      icon: 'quote',
      select: () => chain()?.toggleBlockquote().run()
    },
    {
      id: 'ul',
      label: 'Bulleted list',
      icon: 'listBullet',
      select: () => chain()?.toggleBulletList().run()
    },
    {
      id: 'ol',
      label: 'Numbered list',
      icon: 'listOrdered',
      select: () => chain()?.toggleOrderedList().run()
    },
    { kind: 'separator', id: 's2' },
    {
      id: 'rule',
      label: 'Horizontal rule',
      icon: 'rule',
      select: () => chain()?.setHorizontalRule().run()
    },
    {
      id: 'code',
      label: 'Inline code',
      icon: 'code',
      select: () => chain()?.toggleCode().run()
    }
  ]);

  const alignItems = $derived<MenuItem[]>([
    { kind: 'heading', id: 'h', label: 'Alignment' },
    { id: 'left', label: 'Left', icon: 'alignLeft', select: () => chain()?.setTextAlign('left').run() },
    {
      id: 'center',
      label: 'Centred',
      icon: 'alignCenter',
      select: () => chain()?.setTextAlign('center').run()
    },
    {
      id: 'right',
      label: 'Right',
      icon: 'alignRight',
      select: () => chain()?.setTextAlign('right').run()
    }
  ]);

  const blockLabel = $derived(
    marks['h1'] ? 'Heading 1' : marks['h2'] ? 'Heading 2' : marks['h3'] ? 'Heading 3' : 'Body'
  );
</script>

<div class="bar" role="toolbar" aria-label="Formatting">
  <IconButton
    name="bold"
    label="Bold"
    size="sm"
    pressed={marks['bold']}
    disabled={!editor}
    onclick={() => chain()?.toggleBold().run()}
  />
  <IconButton
    name="italic"
    label="Italic"
    size="sm"
    pressed={marks['italic']}
    disabled={!editor}
    onclick={() => chain()?.toggleItalic().run()}
  />
  <IconButton
    name="underline"
    label="Underline"
    size="sm"
    pressed={marks['underline']}
    disabled={!editor}
    onclick={() => chain()?.toggleUnderline().run()}
  />
  <IconButton
    name="strikethrough"
    label="Strike through"
    size="sm"
    pressed={marks['strike']}
    disabled={!editor}
    onclick={() => chain()?.toggleStrike().run()}
  />

  <span class="divider" aria-hidden="true"></span>

  <!-- Block style lives in a menu rather than as eight more icons. The
       manuscript is the point; the toolbar should not compete with it. -->
  <Menu items={blockItems} label="Paragraph style — currently {blockLabel}" icon="heading" size="sm" />
  <IconButton
    name="listBullet"
    label="Bulleted list"
    size="sm"
    pressed={marks['bulletList']}
    disabled={!editor}
    onclick={() => chain()?.toggleBulletList().run()}
  />
  <IconButton
    name="quote"
    label="Block quote"
    size="sm"
    pressed={marks['blockquote']}
    disabled={!editor}
    onclick={() => chain()?.toggleBlockquote().run()}
  />
  <IconButton
    name="link"
    label="Link"
    size="sm"
    pressed={marks['link']}
    disabled={!editor}
    onclick={onlink}
  />
  <Menu items={alignItems} label="Text alignment" icon="alignLeft" size="sm" />

  <span class="divider" aria-hidden="true"></span>

  <IconButton
    name="undo"
    label="Undo"
    size="sm"
    disabled={!state?.canUndo}
    onclick={() => chain()?.undo().run()}
  />
  <IconButton
    name="redo"
    label="Redo"
    size="sm"
    disabled={!state?.canRedo}
    onclick={() => chain()?.redo().run()}
  />
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 2px;
    /* Scrolls rather than wraps or shrinks: on a narrow tablet the controls
       stay full size and slide, instead of becoming targets too small to hit. */
    overflow-x: auto;
    scrollbar-width: none;
  }

  .bar::-webkit-scrollbar {
    display: none;
  }

  .divider {
    width: var(--border-width);
    height: 20px;
    background: var(--border-subtle);
    margin: 0 var(--space-2);
    flex: none;
  }
</style>
