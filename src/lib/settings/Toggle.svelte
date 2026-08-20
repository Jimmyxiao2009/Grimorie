<script lang="ts">
  interface Props {
    checked: boolean;
    label: string;
    disabled?: boolean;
    onchange: (checked: boolean) => void;
  }

  let { checked, label, disabled = false, onchange }: Props = $props();
</script>

<!-- A real checkbox underneath, so it is reachable by keyboard, announced
     correctly, and works in forced-colours mode without any extra code. -->
<label class="toggle" class:disabled>
  <input
    type="checkbox"
    {checked}
    {disabled}
    aria-label={label}
    onchange={(event) => onchange(event.currentTarget.checked)}
  />
  <span class="track" aria-hidden="true"><span class="thumb"></span></span>
</label>

<style>
  .toggle {
    display: inline-flex;
    align-items: center;
    /* The hit area is the touch minimum even though the switch is smaller. */
    min-width: var(--touch-comfortable);
    min-height: var(--touch-comfortable);
    justify-content: center;
    cursor: pointer;
  }

  .disabled {
    cursor: default;
    opacity: 0.5;
  }

  input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }

  .track {
    display: block;
    width: 40px;
    height: 22px;
    border-radius: var(--radius-pill);
    background: var(--surface-sunken);
    border: var(--border-width) solid var(--border-default);
    transition: background-color var(--motion-fast) var(--ease-out);
  }

  .thumb {
    display: block;
    width: 16px;
    height: 16px;
    margin: 2px;
    border-radius: var(--radius-pill);
    background: var(--text-tertiary);
    transition:
      transform var(--motion-fast) var(--ease-out),
      background-color var(--motion-fast) var(--ease-out);
  }

  input:checked + .track {
    background: var(--accent);
    border-color: var(--accent);
  }

  input:checked + .track .thumb {
    background: var(--accent-contrast);
    transform: translateX(18px);
  }

  input:focus-visible + .track {
    outline: var(--border-width-strong) solid var(--accent);
    outline-offset: 2px;
  }

  @media (forced-colors: active) {
    input:checked + .track {
      background: Highlight;
    }
  }
</style>
