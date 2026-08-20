<script lang="ts">
  import Icon from './Icon.svelte';
  import IconButton from './IconButton.svelte';
  import { viewport } from '$lib/design/viewport.svelte';
  import { isMenuAction, type MenuItem, type MenuPlacement } from '$lib/types/ui';
  import type { IconName } from '$lib/design/icons';

  interface Props {
    items: MenuItem[];
    /** Accessible name for the trigger, e.g. "Chapter III actions". */
    label: string;
    icon?: IconName;
    placement?: MenuPlacement;
    size?: 'sm' | 'md' | 'lg';
    disabled?: boolean;
  }

  let {
    items,
    label,
    icon = 'more',
    placement = 'bottom-end',
    size = 'md',
    disabled = false
  }: Props = $props();

  let open = $state(false);
  let trigger = $state<HTMLElement | null>(null);
  let panel = $state<HTMLElement | null>(null);
  let position = $state({ top: 0, left: 0 });
  let activeIndex = $state(-1);

  /**
   * Touch users get a bottom sheet rather than a dropdown. A dropdown anchored
   * to a small control puts its rows under the user's own hand and near the
   * screen edge; a sheet rises from the bottom, where a thumb already is.
   */
  const asSheet = $derived(viewport.isNarrow || viewport.coarsePointer);

  const actionIndexes = $derived(
    items.map((item, index) => (isMenuAction(item) && !item.disabled ? index : -1)).filter((i) => i >= 0)
  );

  function place() {
    if (!trigger || !panel) return;
    const anchor = trigger.getBoundingClientRect();
    const box = panel.getBoundingClientRect();
    const gap = 6;
    const margin = 8;

    let top = placement.startsWith('bottom') ? anchor.bottom + gap : anchor.top - box.height - gap;
    let left = placement.endsWith('end') ? anchor.right - box.width : anchor.left;

    // Flip rather than overflow, then clamp so a menu near a corner stays whole.
    if (top + box.height > window.innerHeight - margin) {
      const flipped = anchor.top - box.height - gap;
      top = flipped >= margin ? flipped : window.innerHeight - box.height - margin;
    }
    if (top < margin) top = margin;
    left = Math.min(Math.max(left, margin), window.innerWidth - box.width - margin);

    position = { top, left };
  }

  function show() {
    if (disabled) return;
    open = true;
    activeIndex = -1;
    // Position after the panel has been measured, not before it exists.
    queueMicrotask(() => {
      if (!asSheet) place();
      panel?.focus();
    });
  }

  function hide(returnFocus = true) {
    if (!open) return;
    open = false;
    activeIndex = -1;
    if (returnFocus) trigger?.querySelector('button')?.focus();
  }

  function choose(item: MenuItem) {
    if (!isMenuAction(item) || item.disabled) return;
    hide();
    item.select();
  }

  function moveActive(delta: number) {
    if (actionIndexes.length === 0) return;
    const current = actionIndexes.indexOf(activeIndex);
    const next = current === -1 ? (delta > 0 ? 0 : actionIndexes.length - 1) : current + delta;
    const wrapped = (next + actionIndexes.length) % actionIndexes.length;
    activeIndex = actionIndexes[wrapped] ?? -1;
  }

  function onKeydown(event: KeyboardEvent) {
    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        hide();
        break;
      case 'ArrowDown':
        event.preventDefault();
        moveActive(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        moveActive(-1);
        break;
      case 'Home':
        event.preventDefault();
        activeIndex = actionIndexes[0] ?? -1;
        break;
      case 'End':
        event.preventDefault();
        activeIndex = actionIndexes[actionIndexes.length - 1] ?? -1;
        break;
      case 'Enter':
      case ' ': {
        const item = items[activeIndex];
        if (item) {
          event.preventDefault();
          choose(item);
        }
        break;
      }
    }
  }

  $effect(() => {
    if (!open || asSheet) return;
    const reposition = () => place();
    window.addEventListener('resize', reposition);
    // Capture phase: a menu must follow or close when any ancestor scrolls.
    window.addEventListener('scroll', reposition, true);
    return () => {
      window.removeEventListener('resize', reposition);
      window.removeEventListener('scroll', reposition, true);
    };
  });
</script>

<span class="trigger" bind:this={trigger}>
  <IconButton name={icon} {label} {size} {disabled} pressed={open} onclick={() => (open ? hide() : show())} />
</span>

{#if open}
  <!-- Scrim: catches the outside press that dismisses the menu. It is visible
       only for the sheet, where it also dims the manuscript behind. -->
  <div
    class="scrim"
    class:visible={asSheet}
    role="presentation"
    onpointerdown={() => hide(false)}
  ></div>

  <div
    bind:this={panel}
    class="panel"
    class:sheet={asSheet}
    style:top={asSheet ? undefined : `${position.top}px`}
    style:left={asSheet ? undefined : `${position.left}px`}
    role="menu"
    aria-label={label}
    tabindex="-1"
    onkeydown={onKeydown}
  >
    {#if asSheet}
      <p class="sheet-title eyebrow">{label}</p>
    {/if}

    {#each items as item (item.id)}
      {#if item.kind === 'separator'}
        <hr class="rule" />
      {:else if item.kind === 'heading'}
        <p class="heading eyebrow">{item.label}</p>
      {:else}
        <button
          type="button"
          role="menuitem"
          class="item"
          class:danger={item.danger}
          class:active={activeIndex === items.indexOf(item)}
          disabled={item.disabled}
          onclick={() => choose(item)}
          onpointerenter={() => (activeIndex = items.indexOf(item))}
        >
          {#if item.icon}
            <Icon name={item.icon} size={17} />
          {:else}
            <span class="glyph-space"></span>
          {/if}
          <span class="item-label truncate">{item.label}</span>
          {#if item.hint}<kbd>{item.hint}</kbd>{/if}
        </button>
      {/if}
    {/each}

    {#if asSheet}
      <button type="button" class="cancel" onclick={() => hide()}>Cancel</button>
    {/if}
  </div>
{/if}

<style>
  .trigger {
    display: inline-flex;
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: var(--z-scrim);
    background: transparent;
  }

  .scrim.visible {
    background: rgb(0 0 0 / 34%);
    animation: fade var(--motion-fast) var(--ease-out);
  }

  .panel {
    position: fixed;
    z-index: var(--z-menu);
    min-width: 12rem;
    max-width: min(22rem, calc(100vw - 1rem));
    padding: var(--space-1);
    background: var(--surface-raised);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-overlay);
    animation: rise var(--motion-fast) var(--ease-out);
  }

  .panel.sheet {
    inset: auto 0 0 0;
    max-width: none;
    padding: var(--space-2) var(--space-2) max(var(--space-3), env(safe-area-inset-bottom));
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
    border-bottom: none;
    animation: slide-up var(--motion-base) var(--ease-out);
  }

  .sheet-title {
    padding: var(--space-2) var(--space-3) var(--space-3);
  }

  .heading {
    padding: var(--space-3) var(--space-3) var(--space-1);
  }

  .item {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-height: var(--touch-min);
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    color: var(--text-primary);
    font-size: var(--text-md);
  }

  .sheet .item {
    min-height: 48px;
    padding: 0 var(--space-4);
    font-size: var(--text-lg);
  }

  /* Hover and keyboard focus share one visual state, so moving between the two
     input styles never makes the menu look like it lost its place. */
  .item.active:not(:disabled) {
    background: var(--surface-hover);
  }

  .item:active:not(:disabled) {
    background: var(--surface-active);
  }

  .item.danger {
    color: var(--state-danger);
  }

  .item:disabled {
    opacity: 0.4;
  }

  .item-label {
    flex: 1;
    text-align: left;
  }

  .glyph-space {
    width: 17px;
    flex: none;
  }

  kbd {
    font-family: var(--font-ui);
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    white-space: nowrap;
  }

  .cancel {
    width: 100%;
    min-height: 48px;
    margin-top: var(--space-2);
    border-radius: var(--radius-md);
    background: var(--surface-hover);
    color: var(--text-secondary);
    font-size: var(--text-lg);
    font-weight: var(--weight-medium);
  }

  hr.rule {
    margin: var(--space-1) var(--space-2);
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(-2px);
    }
  }

  @keyframes slide-up {
    from {
      transform: translateY(100%);
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
