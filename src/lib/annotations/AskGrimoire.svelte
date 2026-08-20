<script lang="ts">
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import { count } from '$lib/utils/format';
  import type { AiActionInfo, AiProfile, BuiltContext } from '$lib/services/ai';

  /**
   * Asking a model to read a passage.
   *
   * The writer picks a reader and a question, and is then shown exactly what
   * would be sent — how much text, from where, and whether the budget trimmed
   * anything — before anything leaves the machine. Nothing is sent by opening
   * this panel; it takes a second, deliberate press.
   */

  interface Props {
    open: boolean;
    pageId: string | null;
    selection: { from: number; to: number; text: string } | null;
    /** Called once a run finishes, so the Margin can reload. */
    oncomplete: () => void;
  }

  let { open = $bindable(), pageId, selection, oncomplete }: Props = $props();

  let profile = $state<AiProfile | null>(null);
  let action = $state<AiActionInfo | null>(null);
  let preview = $state<BuiltContext | null>(null);
  let previewing = $state(false);
  let previewFailure = $state<string | null>(null);

  const hasSelection = $derived(selection !== null && selection.to > selection.from);

  $effect(() => {
    if (!open) return;
    profile ??= ai.profiles[0] ?? null;
    action ??= ai.actions[0] ?? null;
  });

  // Whenever the choice changes, ask what it would send. This is a local
  // question — it reads the manuscript, it does not transmit it.
  $effect(() => {
    const chosenProfile = profile;
    const chosenAction = action;
    const page = pageId;
    const range = selection;

    if (!open || !chosenProfile || !chosenAction || !page || !range) {
      preview = null;
      return;
    }

    previewing = true;
    previewFailure = null;
    void ai
      .preview(page, chosenProfile.id, chosenAction.id, range.from, range.to)
      .then((built) => {
        preview = built;
      })
      .catch((error) => {
        preview = null;
        previewFailure = ai.describe(error);
      })
      .finally(() => {
        previewing = false;
      });
  });

  async function send() {
    const page = pageId;
    const range = selection;
    if (!profile || !action || !page || !range) return;
    open = false;
    await ai.run(page, profile, action, range.from, range.to);
    oncomplete();
  }
</script>

<Dialog bind:open title="Ask Grimoire" description="A reader for the passage you selected." width="md">
  {#if !hasSelection}
    <p class="guidance">
      Select a passage in the manuscript first. Grimoire asks about a specific piece of writing,
      not about the manuscript in general.
    </p>
  {:else if ai.profiles.length === 0}
    <p class="guidance">No AI profiles are available.</p>
  {:else}
    <div class="form">
      <fieldset>
        <legend class="eyebrow">Reader</legend>
        <div class="options">
          {#each ai.profiles as candidate (candidate.id)}
            <button
              type="button"
              class="option"
              class:selected={profile?.id === candidate.id}
              aria-pressed={profile?.id === candidate.id}
              onclick={() => (profile = candidate)}
            >
              <span class="option-name">{candidate.name}</span>
              <span class="option-note">{candidate.description}</span>
            </button>
          {/each}
        </div>
      </fieldset>

      <fieldset>
        <legend class="eyebrow">Question</legend>
        <div class="chips">
          {#each ai.actions as candidate (candidate.id)}
            <button
              type="button"
              class="chip"
              class:selected={action?.id === candidate.id}
              aria-pressed={action?.id === candidate.id}
              onclick={() => (action = candidate)}
            >
              {candidate.label}
              {#if candidate.proposesAnEdit}
                <span class="edits" title="Can propose replacement text">·</span>
              {/if}
            </button>
          {/each}
        </div>
        {#if action?.proposesAnEdit}
          <p class="hint">
            This can propose replacement text. Nothing is changed until you apply it, and Grimoire
            re-checks the passage first.
          </p>
        {/if}
      </fieldset>

      <!-- What leaves the machine, stated before it does. -->
      <section class="disclosure" aria-live="polite">
        <h3 class="eyebrow"><Icon name="shield" size={13} /> What will be sent</h3>
        {#if previewing}
          <p class="disclosure-body">Working it out…</p>
        {:else if previewFailure}
          <p class="disclosure-body failure">{previewFailure}</p>
        {:else if preview}
          <p class="disclosure-body">{preview.summary}</p>
          <p class="disclosure-meta tabular">
            {preview.whereFrom} · {count(preview.charsSent)} characters
          </p>
        {:else}
          <p class="disclosure-body">Nothing yet.</p>
        {/if}
      </section>
    </div>
  {/if}

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (open = false)}>Cancel</Button>
    <Button
      variant="primary"
      icon="sparkle"
      disabled={!hasSelection || !profile || !action || !preview || previewing}
      onclick={send}
    >
      Send
    </Button>
  {/snippet}
</Dialog>

<style>
  .guidance {
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-normal);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-5);
  }

  fieldset {
    border: none;
    padding: 0;
    margin: 0;
  }

  legend {
    padding: 0;
    margin-bottom: var(--space-2);
  }

  .options {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
    gap: var(--space-2);
  }

  .option {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: var(--touch-comfortable);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
    text-align: left;
  }

  .option:hover {
    background: var(--surface-hover);
  }

  .option.selected {
    border-color: var(--accent);
    background: var(--accent-quiet);
  }

  .option-name {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--text-primary);
  }

  .option-note {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    line-height: var(--leading-snug);
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }

  .chip {
    min-height: var(--touch-min);
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .chip.selected {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }

  .edits {
    opacity: 0.7;
  }

  .hint {
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    line-height: var(--leading-snug);
  }

  /* Deliberately plain and always present, rather than a warning that appears
     only sometimes. The writer should know what travels every single time. */
  .disclosure {
    padding: var(--space-3);
    border-radius: var(--radius-md);
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-subtle);
  }

  .disclosure h3 {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    margin-bottom: var(--space-2);
  }

  .disclosure-body {
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-snug);
  }

  .disclosure-body.failure {
    color: var(--state-danger);
  }

  .disclosure-meta {
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }
</style>
