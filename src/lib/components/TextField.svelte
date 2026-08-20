<script lang="ts">
  interface Props {
    value: string;
    label: string;
    /** Hides the label visually but keeps it for assistive technology. */
    hideLabel?: boolean;
    placeholder?: string;
    hint?: string;
    error?: string | null;
    multiline?: boolean;
    rows?: number;
    autofocus?: boolean;
    disabled?: boolean;
    maxlength?: number;
    id?: string;
    oninput?: (value: string) => void;
    onenter?: () => void;
  }

  let {
    value = $bindable(),
    label,
    hideLabel = false,
    placeholder,
    hint,
    error = null,
    multiline = false,
    rows = 4,
    autofocus = false,
    disabled = false,
    maxlength,
    id = `field-${Math.random().toString(36).slice(2, 9)}`,
    oninput,
    onenter
  }: Props = $props();

  const describedBy = $derived(error ? `${id}-error` : hint ? `${id}-hint` : undefined);

  function handleInput(event: Event) {
    const target = event.currentTarget as HTMLInputElement | HTMLTextAreaElement;
    value = target.value;
    oninput?.(value);
  }

  function handleKeydown(event: KeyboardEvent) {
    // In a single-line field Enter submits. In a textarea it inserts a newline,
    // which is what a writer expects, so the shortcut is not stolen there.
    if (!multiline && event.key === 'Enter') {
      event.preventDefault();
      onenter?.();
    }
  }
</script>

<div class="field">
  <label for={id} class:sr-only={hideLabel}>{label}</label>

  {#if multiline}
    <textarea
      {id}
      {placeholder}
      {rows}
      {disabled}
      {maxlength}
      {value}
      class="control selectable"
      class:invalid={!!error}
      aria-describedby={describedBy}
      aria-invalid={error ? 'true' : undefined}
      oninput={handleInput}
    ></textarea>
  {:else}
    <!-- svelte-ignore a11y_autofocus -- dialogs open in response to an explicit
         user action and focusing the one field they contain is the expected
         behaviour, not a surprise focus steal. -->
    <input
      {id}
      type="text"
      {placeholder}
      {disabled}
      {maxlength}
      {value}
      {autofocus}
      class="control selectable"
      class:invalid={!!error}
      aria-describedby={describedBy}
      aria-invalid={error ? 'true' : undefined}
      oninput={handleInput}
      onkeydown={handleKeydown}
    />
  {/if}

  {#if error}
    <p class="message error" id="{id}-error" role="alert">{error}</p>
  {:else if hint}
    <p class="message hint" id="{id}-hint">{hint}</p>
  {/if}
</div>

<style>
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  label {
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    color: var(--text-secondary);
  }

  .control {
    width: 100%;
    min-height: var(--touch-comfortable);
    padding: var(--space-2) var(--space-3);
    background: var(--surface-page);
    color: var(--text-primary);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-md);
    font-size: var(--text-md);
    transition:
      border-color var(--motion-instant) var(--ease-out),
      background-color var(--motion-instant) var(--ease-out);
  }

  textarea.control {
    padding: var(--space-3);
    line-height: var(--leading-normal);
    resize: vertical;
  }

  .control::placeholder {
    color: var(--text-tertiary);
  }

  .control:hover:not(:disabled) {
    border-color: var(--border-strong);
  }

  .control:focus-visible {
    outline: none;
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
  }

  .control.invalid {
    border-color: var(--state-danger);
  }

  .control:disabled {
    opacity: 0.5;
  }

  .message {
    font-size: var(--text-sm);
    line-height: var(--leading-snug);
  }

  .hint {
    color: var(--text-tertiary);
  }

  .error {
    color: var(--state-danger);
  }
</style>
