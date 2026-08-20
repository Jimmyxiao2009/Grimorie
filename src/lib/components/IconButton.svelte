<script lang="ts">
  import Icon from './Icon.svelte';
  import type { IconName } from '$lib/design/icons';

  interface Props {
    name: IconName;
    /**
     * Required. An icon-only control has no visible text, so this is its only
     * accessible name — the type makes it impossible to ship one without.
     */
    label: string;
    size?: 'sm' | 'md' | 'lg';
    tone?: 'default' | 'accent' | 'danger';
    /** Renders the pressed state of a toggle and sets aria-pressed. */
    pressed?: boolean;
    disabled?: boolean;
    onclick?: (event: MouseEvent) => void;
  }

  let {
    name,
    label,
    size = 'md',
    tone = 'default',
    pressed,
    disabled = false,
    onclick
  }: Props = $props();

  const glyph = $derived(size === 'sm' ? 16 : size === 'lg' ? 22 : 19);
</script>

<button
  type="button"
  class="icon-button {size} {tone}"
  class:pressed
  aria-label={label}
  aria-pressed={pressed}
  title={label}
  {disabled}
  {onclick}
>
  <Icon {name} size={glyph} />
</button>

<style>
  .icon-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: var(--radius-md);
    color: var(--text-secondary);
    transition:
      background-color var(--motion-instant) var(--ease-out),
      color var(--motion-instant) var(--ease-out);
  }

  /* Even `sm` meets the touch minimum. The visual weight varies; the hit area
     does not. */
  .sm {
    width: var(--touch-min);
    height: var(--touch-min);
  }

  .md {
    width: var(--touch-comfortable);
    height: var(--touch-comfortable);
  }

  .lg {
    width: 48px;
    height: 48px;
  }

  .icon-button:hover:not(:disabled) {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .icon-button:active:not(:disabled) {
    background: var(--surface-active);
  }

  .pressed {
    background: var(--surface-selected);
    color: var(--accent);
  }

  .pressed:hover:not(:disabled) {
    background: var(--surface-selected);
    color: var(--accent);
  }

  .accent {
    color: var(--accent);
  }

  .danger:hover:not(:disabled) {
    color: var(--state-danger);
  }

  .icon-button:disabled {
    opacity: 0.4;
  }
</style>
