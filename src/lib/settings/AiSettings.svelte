<script lang="ts">
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import TextField from '$lib/components/TextField.svelte';
  import SettingRow from './SettingRow.svelte';
  import Toggle from './Toggle.svelte';

  import * as service from '$lib/services/ai';
  import { ai } from '$lib/stores/ai.svelte';
  import { notices } from '$lib/stores/notices.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import type { AiProfile, AiProvider, ContextPolicy } from '$lib/services/ai';

  let providerOpen = $state(false);
  let editing = $state<AiProvider | null>(null);
  let name = $state('');
  let baseUrl = $state('');
  let model = $state('');
  let apiKey = $state('');
  let temperature = $state(0.7);
  let contextLength = $state(8192);
  let saving = $state(false);

  let profileOpen = $state(false);
  let profileTarget = $state<AiProfile | null>(null);
  let profileName = $state('');
  let profileDescription = $state('');
  let profilePrompt = $state('');
  let profilePolicy = $state<ContextPolicy>('selection');

  let deleteOpen = $state(false);
  let deleteTarget = $state<AiProvider | null>(null);

  const policies: { id: ContextPolicy; label: string; hint: string }[] = [
    { id: 'selection', label: 'Selection', hint: 'The passage and a little either side.' },
    { id: 'page', label: 'Page', hint: 'The whole Page the passage is on.' },
    { id: 'chapter', label: 'Chapter', hint: 'The Page and its neighbours.' }
  ];

  function beginProvider(provider: AiProvider | null) {
    editing = provider;
    name = provider?.name ?? '';
    baseUrl = provider?.baseUrl ?? 'https://api.openai.com/v1';
    model = provider?.model ?? '';
    temperature = provider?.temperature ?? 0.7;
    contextLength = provider?.contextLength ?? 8192;
    // Never pre-filled: the backend cannot return a saved key, and showing a
    // placeholder that looks like one would be a lie about what is stored.
    apiKey = '';
    providerOpen = true;
  }

  async function saveProvider() {
    saving = true;
    try {
      await service.saveProvider({
        id: editing?.id ?? null,
        name: name.trim() || 'AI provider',
        baseUrl,
        model,
        temperature,
        contextLength,
        apiKey: apiKey.trim() || null
      });
      providerOpen = false;
      await ai.refreshConfiguration();
    } catch (error) {
      notices.failure(error);
    } finally {
      saving = false;
    }
  }

  async function confirmDeleteProvider() {
    const target = deleteTarget;
    deleteOpen = false;
    if (!target) return;
    try {
      await service.deleteProvider(target.id);
      await ai.refreshConfiguration();
    } catch (error) {
      notices.failure(error);
    }
  }

  function beginProfile(profile: AiProfile) {
    profileTarget = profile;
    profileName = profile.name;
    profileDescription = profile.description;
    profilePrompt = profile.systemPrompt;
    profilePolicy = profile.contextPolicy;
    profileOpen = true;
  }

  async function saveProfile() {
    const target = profileTarget;
    if (!target) return;
    profileOpen = false;
    try {
      await service.saveProfile({
        id: target.id,
        name: profileName,
        description: profileDescription,
        systemPrompt: profilePrompt,
        contextPolicy: profilePolicy
      });
      await ai.refreshConfiguration();
    } catch (error) {
      notices.failure(error);
    }
  }
</script>

<section class="section">
  <h2>AI</h2>
  <p class="preamble">
    Grimoire works fully without this. When it is on, manuscript text is sent only when you ask
    for a specific reading, and only the part that reading needs — you are shown exactly what will
    be sent before it is. Nothing is analysed in the background.
  </p>

  <SettingRow
    label="Use AI"
    hint="Off by default. Turning this off leaves everything else in Grimoire working."
  >
    {#snippet control()}
      <Toggle
        checked={settingsStore.settings.aiEnabled}
        label="Use AI"
        onchange={(checked) => settingsStore.update({ aiEnabled: checked })}
      />
    {/snippet}
  </SettingRow>

  <SettingRow
    label="Context budget"
    hint="The most manuscript text one request may carry, in characters."
  >
    {#snippet control()}
      <input
        type="number"
        class="number"
        min="500"
        max="200000"
        step="500"
        value={settingsStore.settings.aiContextBudgetChars}
        aria-label="Context budget in characters"
        onchange={(event) =>
          settingsStore.update({
            aiContextBudgetChars: Number(event.currentTarget.value)
          })}
      />
    {/snippet}
  </SettingRow>
</section>

<section class="section">
  <h3 class="eyebrow">Provider</h3>

  {#if ai.providers.length === 0}
    <p class="empty">
      No provider is configured. Grimoire speaks the OpenAI-compatible API, which most hosted and
      self-hosted servers also speak.
    </p>
    <Button variant="secondary" icon="plus" onclick={() => beginProvider(null)}>
      Add a provider
    </Button>
  {:else}
    <ul class="cards">
      {#each ai.providers as provider (provider.id)}
        <li class="card">
          <div class="card-body">
            <span class="card-title">{provider.name || 'AI provider'}</span>
            <span class="card-meta">{provider.model}</span>
            <span class="card-meta truncate">{provider.baseUrl}</span>
            <span class="key" class:present={provider.hasKey}>
              <Icon name={provider.hasKey ? 'checkCircle' : 'warning'} size={13} />
              <span>{provider.hasKey ? 'API key saved' : 'No API key saved'}</span>
            </span>
          </div>
          <div class="card-actions">
            <Button variant="ghost" size="sm" onclick={() => beginProvider(provider)}>Edit</Button>
            <Button
              variant="ghost"
              size="sm"
              onclick={() => {
                deleteTarget = provider;
                deleteOpen = true;
              }}>Remove</Button
            >
          </div>
        </li>
      {/each}
    </ul>
  {/if}

  <p class="privacy">
    <Icon name="shield" size={14} />
    <span>
      The API key is stored in this computer's credential manager, never in Grimoire's library
      file and never in a log. Grimoire cannot display it back to you — it can only replace it.
    </span>
  </p>
</section>

<section class="section">
  <h3 class="eyebrow">Readers</h3>
  <p class="preamble">
    Each reader is a role the model is asked to play, and how much of the manuscript it may see.
    You can rewrite any of them.
  </p>

  <ul class="cards">
    {#each ai.profiles as profile (profile.id)}
      <li class="card">
        <div class="card-body">
          <span class="card-title">{profile.name}</span>
          <span class="card-meta">{profile.description}</span>
          <span class="card-meta">
            Sends: {policies.find((p) => p.id === profile.contextPolicy)?.label ?? 'Selection'}
          </span>
        </div>
        <div class="card-actions">
          <Button variant="ghost" size="sm" onclick={() => beginProfile(profile)}>Edit</Button>
        </div>
      </li>
    {/each}
  </ul>
</section>

<Dialog
  bind:open={providerOpen}
  title={editing ? 'Edit provider' : 'Add a provider'}
  description="Any server that speaks the OpenAI chat-completions API."
  width="md"
>
  <div class="form">
    <TextField bind:value={name} label="Name" hint="What you call it." placeholder="OpenAI" />
    <TextField
      bind:value={baseUrl}
      label="Address"
      hint="The API base URL, ending in /v1."
      placeholder="https://api.openai.com/v1"
    />
    <TextField bind:value={model} label="Model" placeholder="gpt-4o-mini" />
    <TextField
      bind:value={apiKey}
      label="API key"
      hint={editing?.hasKey
        ? 'A key is already saved. Leave this blank to keep it.'
        : 'Stored in this computer’s credential manager.'}
      placeholder={editing?.hasKey ? '••••••••' : 'sk-…'}
    />

    <div class="pair">
      <label class="field">
        <span class="field-label">Temperature</span>
        <input type="number" class="number" min="0" max="2" step="0.1" bind:value={temperature} />
      </label>
      <label class="field">
        <span class="field-label">Context length</span>
        <input type="number" class="number" min="1000" step="1000" bind:value={contextLength} />
      </label>
    </div>
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (providerOpen = false)}>Cancel</Button>
    <Button
      variant="primary"
      disabled={saving || !baseUrl.trim() || !model.trim()}
      onclick={saveProvider}>Save</Button
    >
  {/snippet}
</Dialog>

<Dialog bind:open={profileOpen} title="Edit reader" width="md">
  <div class="form">
    <TextField bind:value={profileName} label="Name" />
    <TextField bind:value={profileDescription} label="Description" />
    <TextField
      bind:value={profilePrompt}
      label="Instructions"
      hint="What this reader is asked to attend to."
      multiline
      rows={7}
    />

    <fieldset>
      <legend class="field-label">How much it may see</legend>
      <div class="policies">
        {#each policies as policy (policy.id)}
          <button
            type="button"
            class="policy"
            class:selected={profilePolicy === policy.id}
            aria-pressed={profilePolicy === policy.id}
            onclick={() => (profilePolicy = policy.id)}
          >
            <span class="policy-name">{policy.label}</span>
            <span class="policy-hint">{policy.hint}</span>
          </button>
        {/each}
      </div>
    </fieldset>
  </div>

  {#snippet footer()}
    <Button variant="ghost" onclick={() => (profileOpen = false)}>Cancel</Button>
    <Button variant="primary" disabled={!profileName.trim()} onclick={saveProfile}>Save</Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={deleteOpen}
  title="Remove this provider?"
  description="The saved API key is removed from this computer's credential manager too."
>
  <p class="preamble">Your manuscripts are not affected.</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleteOpen = false)}>Cancel</Button>
    <Button variant="danger" onclick={confirmDeleteProvider}>Remove</Button>
  {/snippet}
</Dialog>

<style>
  .section {
    margin-bottom: var(--space-7);
  }

  h2 {
    font-size: var(--text-xl);
    margin-bottom: var(--space-2);
  }

  h3 {
    margin-bottom: var(--space-3);
  }

  .preamble,
  .empty {
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-normal);
    margin-bottom: var(--space-4);
    max-width: 44rem;
  }

  .cards {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    margin-bottom: var(--space-3);
  }

  .card {
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-subtle);
    background: var(--surface-raised);
  }

  .card-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .card-title {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  .card-meta {
    font-size: var(--text-sm);
    color: var(--text-tertiary);
  }

  .card-actions {
    display: flex;
    gap: var(--space-1);
    flex: none;
  }

  .key {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    margin-top: var(--space-2);
    font-size: var(--text-xs);
    color: var(--state-warning);
  }

  .key.present {
    color: var(--state-success);
  }

  .privacy {
    display: flex;
    align-items: flex-start;
    gap: var(--space-2);
    padding: var(--space-3);
    border-radius: var(--radius-md);
    background: var(--surface-pane);
    font-size: var(--text-sm);
    color: var(--text-secondary);
    line-height: var(--leading-snug);
    max-width: 44rem;
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  .pair {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--space-3);
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .field-label {
    font-size: var(--text-sm);
    font-weight: var(--weight-medium);
    color: var(--text-secondary);
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

  .number {
    min-height: var(--touch-comfortable);
    width: 100%;
    padding: 0 var(--space-3);
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-md);
    font-variant-numeric: tabular-nums;
  }

  .policies {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
    gap: var(--space-2);
  }

  .policy {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: var(--touch-comfortable);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
    text-align: left;
  }

  .policy.selected {
    border-color: var(--accent);
    background: var(--accent-quiet);
  }

  .policy-name {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  .policy-hint {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    line-height: var(--leading-snug);
  }
</style>
