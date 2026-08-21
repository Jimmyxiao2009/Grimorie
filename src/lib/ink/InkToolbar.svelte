<script lang="ts">
  /**
   * The pen toolbar, kept deliberately small.
   *
   * Three tools — pen, highlighter, eraser — and a compact popover for colour
   * and width. The whole thing is one row of icon buttons so it never competes
   * with the manuscript it serves. Pen mode is the master switch; the tools are
   * inert until it is on.
   */

  import IconButton from '$lib/components/IconButton.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { ink, INK_COLORS, INK_COLOR_LABELS, type InkColor } from '$lib/stores/ink.svelte';

  let settingsOpen = $state(false);

  const penColor = $derived(ink.color);
  const isPen = $derived(ink.tool === 'pen');
  const isHighlighter = $derived(ink.tool === 'highlighter');
  const isEraser = $derived(ink.tool === 'eraser');

  function selectTool(tool: 'pen' | 'highlighter' | 'eraser') {
    ink.setTool(tool);
    if (ink.penMode) return;
    // Selecting a tool turns pen mode on — that is the gesture that says "I
    // want to write now". Toggling pen mode off is a separate, explicit action.
    ink.togglePenMode();
  }

  function selectColor(color: InkColor) {
    ink.setColor(color);
    if (ink.tool === 'eraser') ink.setTool('pen');
  }

  const widthOptions = [
    { value: 1.5, label: 'Fine' },
    { value: 2, label: 'Regular' },
    { value: 3, label: 'Bold' }
  ];
  const highlighterOptions = [
    { value: 8, label: 'Fine' },
    { value: 10, label: 'Regular' },
    { value: 14, label: 'Bold' }
  ];

  const currentWidthOptions = $derived(
    ink.tool === 'highlighter' ? highlighterOptions : widthOptions
  );
  const currentWidth = $derived(
    ink.tool === 'highlighter' ? ink.highlighterWidth : ink.penWidth
  );

  function setWidth(value: number) {
    if (ink.tool === 'highlighter') ink.setHighlighterWidth(value);
    else ink.setPenWidth(value);
  }

  /** Whether a width option is the one currently selected. Extracted because a
   *  bare `<` in a Svelte expression is read as a tag start. */
  function isCurrentWidth(value: number): boolean {
    return Math.abs(currentWidth - value) < 0.01;
  }

  /** Resolves a semantic colour id to the CSS variable it draws with. */
  function colorVar(color: InkColor): string {
    if (color === 'ink-primary') return 'var(--text-primary)';
    if (color === 'ink-red') return 'var(--state-danger)';
    return 'var(--accent)';
  }

  /** A width dot's inline size, as a single style string. */
  function dotStyle(value: number): string {
    const px = `${value * 2}px`;
    return `width:${px};height:${px}`;
  }

  /** Resolves the swatch colour shown on the settings trigger. */
  const swatchColor = $derived(
    isHighlighter
      ? 'var(--state-warning)'
      : penColor === 'ink-primary'
        ? 'var(--text-primary)'
        : penColor === 'ink-red'
          ? 'var(--state-danger)'
          : 'var(--accent)'
  );
</script>

<div class="ink-toolbar" class:active={ink.penMode}>
  <button
    type="button"
    class="tool pen-mode"
    class:pressed={ink.penMode}
    aria-pressed={ink.penMode}
    aria-label={ink.penMode ? 'Exit pen mode' : 'Enter pen mode'}
    title={ink.penMode ? 'Pen mode on' : 'Pen mode off'}
    onclick={() => ink.togglePenMode()}
  >
    <Icon name="pencil" size={16} />
  </button>

  <span class="divider" aria-hidden="true"></span>

  <button
    type="button"
    class="tool"
    class:pressed={isPen}
    aria-pressed={isPen}
    aria-label="Pen"
    title="Pen"
    disabled={!ink.penMode}
    onclick={() => ink.setTool('pen')}
  >
    <Icon name="pencil" size={16} />
  </button>

  <button
    type="button"
    class="tool"
    class:pressed={isHighlighter}
    aria-pressed={isHighlighter}
    aria-label="Highlighter"
    title="Highlighter"
    disabled={!ink.penMode}
    onclick={() => selectTool('highlighter')}
  >
    <Icon name="highlighter" size={16} />
  </button>

  <button
    type="button"
    class="tool"
    class:pressed={isEraser}
    aria-pressed={isEraser}
    aria-label="Eraser"
    title="Eraser"
    disabled={!ink.penMode}
    onclick={() => selectTool('eraser')}
  >
    <Icon name="eraser" size={16} />
  </button>

  <span class="divider" aria-hidden="true"></span>

  <button
    type="button"
    class="tool"
    aria-label="Undo ink stroke"
    title="Undo ink"
    disabled={!ink.canUndo}
    onclick={() => void ink.undo()}
  >
    <Icon name="undo" size={16} />
  </button>
  <button
    type="button"
    class="tool"
    aria-label="Redo ink stroke"
    title="Redo ink"
    disabled={!ink.canRedo}
    onclick={() => void ink.redo()}
  >
    <Icon name="redo" size={16} />
  </button>

  <button
    type="button"
    class="tool settings"
    class:pressed={settingsOpen}
    aria-pressed={settingsOpen}
    aria-label="Pen colour and width"
    title="Pen settings"
    disabled={!ink.penMode || isEraser}
    onclick={() => (settingsOpen = !settingsOpen)}
  >
    <span class="swatch" style={`background:${swatchColor}`}></span>
  </button>
</div>

{#if settingsOpen}
  <div class="scrim" role="presentation" onpointerdown={() => (settingsOpen = false)}></div>
  <div class="popover" role="dialog" aria-label="Pen colour and width">
    {#if !isHighlighter}
      <fieldset>
        <legend class="eyebrow">Colour</legend>
        <div class="swatches">
          {#each INK_COLORS as color (color)}
            <button
              type="button"
              class="swatch-btn"
              class:selected={ink.color === color}
              aria-label={INK_COLOR_LABELS[color]}
              aria-pressed={ink.color === color}
              style="background:{colorVar(color)}"
              onclick={() => selectColor(color)}
            ></button>
          {/each}
        </div>
      </fieldset>
    {/if}

    <fieldset>
      <legend class="eyebrow">Width</legend>
      <div class="widths">
        {#each currentWidthOptions as opt (opt.value)}
          <button
            type="button"
            class="width-btn"
            class:selected={isCurrentWidth(opt.value)}
            aria-pressed={isCurrentWidth(opt.value)}
            onclick={() => setWidth(opt.value)}
          >
            <span class="width-dot" style={dotStyle(opt.value)}></span>
            {opt.label}
          </button>
        {/each}
      </div>
    </fieldset>
  </div>
{/if}

<style>
  .ink-toolbar {
    display: inline-flex;
    align-items: center;
    gap: 1px;
    padding: 0 var(--space-1);
    border-radius: var(--radius-md);
    background: var(--surface-pane);
    opacity: 0.7;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .ink-toolbar.active {
    opacity: 1;
  }

  .tool {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--touch-min);
    height: var(--touch-min);
    border-radius: var(--radius-md);
    color: var(--text-tertiary);
    transition:
      background-color var(--motion-instant) var(--ease-out),
      color var(--motion-instant) var(--ease-out);
  }

  .tool:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .tool.pressed {
    background: var(--surface-selected);
    color: var(--accent);
  }

  .pen-mode.pressed {
    color: var(--state-ai);
    background: var(--surface-selected);
  }

  .tool:disabled {
    opacity: 0.35;
  }

  .divider {
    width: var(--border-width);
    height: 18px;
    background: var(--border-default);
    margin: 0 2px;
  }

  .swatch {
    width: 14px;
    height: 14px;
    border-radius: var(--radius-pill);
    border: var(--border-width) solid var(--border-default);
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: var(--z-scrim);
    background: transparent;
  }

  .popover {
    position: fixed;
    z-index: var(--z-menu);
    margin-top: var(--space-1);
    padding: var(--space-2);
    background: var(--surface-raised);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-overlay);
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    min-width: 11rem;
    animation: rise var(--motion-fast) var(--ease-out);
  }

  fieldset {
    border: none;
    padding: 0;
    margin: 0;
  }

  legend {
    padding: 0;
    margin-bottom: var(--space-1);
  }

  .swatches {
    display: flex;
    gap: var(--space-2);
  }

  .swatch-btn {
    width: 24px;
    height: 24px;
    border-radius: var(--radius-pill);
    border: 2px solid transparent;
    transition: border-color var(--motion-fast) var(--ease-out);
  }

  .swatch-btn.selected {
    border-color: var(--text-primary);
  }

  .widths {
    display: flex;
    gap: var(--space-1);
  }

  .width-btn {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-1) var(--space-2);
    border-radius: var(--radius-md);
    color: var(--text-secondary);
    font-size: var(--text-2xs);
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .width-btn:hover {
    background: var(--surface-hover);
  }

  .width-btn.selected {
    background: var(--surface-selected);
    color: var(--accent);
  }

  .width-dot {
    border-radius: var(--radius-pill);
    background: currentColor;
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
  }
</style>
