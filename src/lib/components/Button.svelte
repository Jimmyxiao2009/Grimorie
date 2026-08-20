<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from './Icon.svelte';
  import type { IconName } from '$lib/design/icons';

  type Variant = 'primary' | 'secondary' | 'ghost' | 'danger';
  type Size = 'sm' | 'md' | 'lg';

  interface Props {
    children: Snippet;
    variant?: Variant;
    size?: Size;
    icon?: IconName;
    trailingIcon?: IconName;
    type?: 'button' | 'submit';
    disabled?: boolean;
    /** Stretches to the container width — used in narrow overlays and dialogs. */
    block?: boolean;
    title?: string;
    onclick?: (event: MouseEvent) => void;
  }

  let {
    children,
    variant = 'secondary',
    size = 'md',
    icon,
    trailingIcon,
    type = 'button',
    disabled = false,
    block = false,
    title,
    onclick
  }: Props = $props();

  const iconSize = $derived(size === 'sm' ? 16 : 18);
</script>

<button
  {type}
  {disabled}
  {title}
  class="btn {variant} {size}"
  class:block
  {onclick}
>
  {#if icon}<Icon name={icon} size={iconSize} />{/if}
  <span class="label"><!--
 -->{@render children()}<!--
 --></span>
  {#if trailingIcon}<Icon name={trailingIcon} size={iconSize} />{/if}
</button>

<style>
  .btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    border-radius: var(--radius-md);
    font-weight: var(--weight-medium);
    border: var(--border-width) solid transparent;
    white-space: nowrap;
    transition:
      background-color var(--motion-instant) var(--ease-out),
      border-color var(--motion-instant) var(--ease-out),
      color var(--motion-instant) var(--ease-out);
  }

  /* Sizes. Every size clears --touch-min so a button is never a mouse-only
     target, including the small one. */
  .sm {
    min-height: var(--touch-min);
    padding: 0 var(--space-3);
    font-size: var(--text-sm);
  }

  .md {
    min-height: var(--touch-comfortable);
    padding: 0 var(--space-4);
    font-size: var(--text-md);
  }

  .lg {
    min-height: 48px;
    padding: 0 var(--space-5);
    font-size: var(--text-lg);
  }

  .block {
    width: 100%;
  }

  .primary {
    background: var(--accent);
    color: var(--accent-contrast);
  }

  .primary:hover:not(:disabled) {
    background: var(--accent-hover);
  }

  .secondary {
    background: var(--surface-raised);
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .secondary:hover:not(:disabled) {
    background: var(--surface-hover);
    border-color: var(--border-strong);
  }

  .ghost {
    color: var(--text-secondary);
  }

  .ghost:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .danger {
    background: transparent;
    color: var(--state-danger);
    border-color: var(--border-default);
  }

  .danger:hover:not(:disabled) {
    background: var(--state-danger);
    border-color: var(--state-danger);
    color: var(--accent-contrast);
  }

  /* Pressed feedback is a colour shift, not a transform. Scaling controls on a
     tablet reads as toy-like and costs a composite on weak hardware. */
  .btn:active:not(:disabled) {
    background: var(--surface-active);
  }

  .primary:active:not(:disabled) {
    background: var(--accent-hover);
    filter: brightness(0.94);
  }

  .danger:active:not(:disabled) {
    background: var(--state-danger);
    border-color: var(--state-danger);
    color: var(--accent-contrast);
    filter: brightness(0.94);
  }

  .btn:disabled {
    opacity: 0.45;
  }

  .label:empty {
    display: none;
  }
</style>
