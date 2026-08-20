<script lang="ts">
  import type { Snippet } from 'svelte';
  import IconButton from './IconButton.svelte';

  interface Props {
    open: boolean;
    title: string;
    /** Optional supporting line, announced with the title. */
    description?: string;
    children: Snippet;
    footer?: Snippet;
    /** Suppresses Escape and the close button for a decision that must be made. */
    dismissible?: boolean;
    width?: 'sm' | 'md' | 'lg';
    onclose?: () => void;
  }

  let {
    open = $bindable(),
    title,
    description,
    children,
    footer,
    dismissible = true,
    width = 'sm',
    onclose
  }: Props = $props();

  let element = $state<HTMLDialogElement | null>(null);

  /**
   * Native <dialog> is used deliberately: the platform gives focus trapping,
   * focus restoration, inertness of the background, top-layer stacking and
   * Escape handling — all of which are easy to reimplement badly.
   */
  $effect(() => {
    const dialog = element;
    if (!dialog) return;
    if (open && !dialog.open) dialog.showModal();
    else if (!open && dialog.open) dialog.close();
  });

  function dismiss() {
    open = false;
    onclose?.();
  }

  function onCancel(event: Event) {
    // `cancel` fires on Escape and on the backdrop close gesture.
    if (!dismissible) {
      event.preventDefault();
      return;
    }
    event.preventDefault();
    dismiss();
  }

  function onBackdropPointerDown(event: MouseEvent) {
    if (!dismissible) return;
    // A press on the element itself is a press on the backdrop: the content
    // sits in a child, so anything hitting <dialog> directly missed the panel.
    if (event.target === element) dismiss();
  }
</script>

<dialog
  bind:this={element}
  class={width}
  aria-labelledby="dialog-title"
  aria-describedby={description ? 'dialog-description' : undefined}
  oncancel={onCancel}
  onpointerdown={onBackdropPointerDown}
>
  <div class="panel">
    <header>
      <div class="titles">
        <h2 id="dialog-title">{title}</h2>
        {#if description}<p id="dialog-description">{description}</p>{/if}
      </div>
      {#if dismissible}
        <IconButton name="close" label="Close" size="sm" onclick={dismiss} />
      {/if}
    </header>

    <div class="body" data-scroll>
      {@render children()}
    </div>

    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</dialog>

<style>
  dialog {
    padding: 0;
    border: none;
    background: none;
    max-width: none;
    max-height: none;
    color: var(--text-primary);
  }

  dialog::backdrop {
    background: rgb(0 0 0 / 38%);
    animation: fade var(--motion-fast) var(--ease-out);
  }

  .panel {
    display: flex;
    flex-direction: column;
    width: min(var(--dialog-width), calc(100vw - 2rem));
    max-height: min(40rem, calc(100vh - 3rem));
    background: var(--surface-raised);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-overlay);
    animation: rise var(--motion-base) var(--ease-out);
  }

  .sm {
    --dialog-width: 26rem;
  }

  .md {
    --dialog-width: 34rem;
  }

  .lg {
    --dialog-width: 46rem;
  }

  header {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-5) var(--space-5) var(--space-3);
  }

  .titles {
    flex: 1;
    min-width: 0;
  }

  h2 {
    font-size: var(--text-xl);
    font-weight: var(--weight-semibold);
  }

  header p {
    margin-top: var(--space-2);
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-snug);
  }

  .body {
    padding: 0 var(--space-5) var(--space-5);
    overflow-y: auto;
  }

  footer {
    display: flex;
    justify-content: flex-end;
    gap: var(--space-2);
    padding: var(--space-4) var(--space-5);
    border-top: var(--border-width) solid var(--border-subtle);
    background: var(--surface-pane);
    border-radius: 0 0 var(--radius-lg) var(--radius-lg);
  }

  /* On a narrow screen the dialog becomes a bottom sheet: full width, anchored
     to the bottom edge, within thumb reach. */
  @media (max-width: 519px) {
    dialog {
      align-self: flex-end;
      margin-bottom: 0;
    }

    .panel {
      width: 100vw;
      border-radius: var(--radius-lg) var(--radius-lg) 0 0;
      animation: slide-up var(--motion-base) var(--ease-out);
    }

    footer {
      border-radius: 0;
      padding-bottom: max(var(--space-4), env(safe-area-inset-bottom));
    }
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px) scale(0.995);
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
