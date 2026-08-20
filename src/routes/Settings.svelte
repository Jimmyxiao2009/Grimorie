<script lang="ts">
  import { onMount } from 'svelte';

  import Button from '$lib/components/Button.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import AiSettings from '$lib/settings/AiSettings.svelte';
  import SettingRow from '$lib/settings/SettingRow.svelte';
  import Toggle from '$lib/settings/Toggle.svelte';

  import { THEME_CHOICES, type ThemeChoice } from '$lib/design/theme.svelte';
  import { viewport } from '$lib/design/viewport.svelte';
  import { rebuildSearchIndex } from '$lib/services/search';
  import { invoke, isDesktop } from '$lib/services/ipc';
  import { ai } from '$lib/stores/ai.svelte';
  import { notices } from '$lib/stores/notices.svelte';
  import { router } from '$lib/stores/router.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { count } from '$lib/utils/format';
  import type { IconName } from '$lib/design/icons';

  type SectionId = 'appearance' | 'editor' | 'ai' | 'data' | 'keyboard' | 'about';

  const sections: { id: SectionId; label: string; icon: IconName }[] = [
    { id: 'appearance', label: 'Appearance', icon: 'palette' },
    { id: 'editor', label: 'Editor', icon: 'page' },
    { id: 'ai', label: 'AI', icon: 'sparkle' },
    { id: 'data', label: 'Data', icon: 'database' },
    { id: 'keyboard', label: 'Keyboard', icon: 'keyboard' },
    { id: 'about', label: 'About', icon: 'info' }
  ];

  let section = $state<SectionId>('appearance');
  let info = $state<{ version: string; sqliteVersion: string; libraryPath: string } | null>(null);
  let rebuilding = $state(false);

  const themeLabels: Record<string, string> = {
    system: 'System',
    paper: 'Paper',
    light: 'Light',
    dark: 'Dark',
    night: 'Night'
  };

  const themeHints: Record<string, string> = {
    system: 'Paper by day, Night after dark.',
    paper: 'Warm and cream, for reading.',
    light: 'A plainer light theme.',
    dark: 'Warm and dark.',
    night: 'Softer still, for long sessions.'
  };

  const faces = [
    { id: 'serif', label: 'Serif' },
    { id: 'sans', label: 'Sans' },
    { id: 'mono', label: 'Mono' }
  ];

  const shortcuts = [
    { keys: 'Ctrl + N', action: 'New Page' },
    { keys: 'Ctrl + Shift + N', action: 'New Chapter' },
    { keys: 'Ctrl + S', action: 'Save now' },
    { keys: 'Ctrl + F', action: 'Search this Volume' },
    { keys: 'Ctrl + B', action: 'Bookmark this Page' },
    { keys: 'Ctrl + Shift + B', action: 'Show bookmarks' },
    { keys: 'Ctrl + ,', action: 'Settings' },
    { keys: 'Escape', action: 'Close an overlay' }
  ];

  onMount(() => {
    void ai.loadConfiguration();
    if (isDesktop) {
      void invoke<typeof info>('app_info')
        .then((result) => {
          info = result;
        })
        .catch(() => {});
    }
  });

  async function rebuild() {
    rebuilding = true;
    try {
      const indexed = await rebuildSearchIndex();
      notices.info(`Search index rebuilt — ${count(indexed)} entries.`);
    } catch (error) {
      notices.failure(error);
    } finally {
      rebuilding = false;
    }
  }
</script>

<div class="settings">
  <header class="head">
    <IconButton name="arrowLeft" label="Close Settings" onclick={() => router.closeSettings()} />
    <h1>Settings</h1>
  </header>

  <div class="body">
    <nav class="sections" aria-label="Settings sections">
      {#each sections as entry (entry.id)}
        <button
          type="button"
          class="section-link"
          class:selected={section === entry.id}
          aria-current={section === entry.id ? 'page' : undefined}
          onclick={() => (section = entry.id)}
        >
          <Icon name={entry.icon} size={17} />
          <span>{entry.label}</span>
        </button>
      {/each}
    </nav>

    <main class="panel" data-scroll>
      {#if section === 'appearance'}
        <section class="group">
          <h2>Appearance</h2>

          <SettingRow label="Theme" stacked>
            {#snippet control()}
              <div class="themes">
                {#each THEME_CHOICES as choice (choice)}
                  <button
                    type="button"
                    class="theme"
                    class:selected={settingsStore.settings.theme === choice}
                    aria-pressed={settingsStore.settings.theme === choice}
                    onclick={() => settingsStore.update({ theme: choice as ThemeChoice })}
                  >
                    <span class="theme-name">{themeLabels[choice]}</span>
                    <span class="theme-hint">{themeHints[choice]}</span>
                  </button>
                {/each}
              </div>
            {/snippet}
          </SettingRow>
        </section>

        <section class="group">
          <h3 class="eyebrow">Manuscript type</h3>
          <p class="sample" style:font-family="var(--font-manuscript)">
            The road had been salt once, or so the carters said, and the wind still carried the
            taste of it in dry months.
          </p>

          <SettingRow label="Typeface">
            {#snippet control()}
              <div class="chips">
                {#each faces as face (face.id)}
                  <button
                    type="button"
                    class="chip"
                    class:selected={settingsStore.settings.manuscriptFace === face.id}
                    aria-pressed={settingsStore.settings.manuscriptFace === face.id}
                    onclick={() => settingsStore.update({ manuscriptFace: face.id })}
                  >
                    {face.label}
                  </button>
                {/each}
              </div>
            {/snippet}
          </SettingRow>

          <SettingRow label="Size">
            {#snippet control()}
              <input
                type="range"
                min="0.875"
                max="1.75"
                step="0.0625"
                aria-label="Manuscript text size"
                value={settingsStore.settings.manuscriptSize}
                oninput={(event) =>
                  settingsStore.update({ manuscriptSize: Number(event.currentTarget.value) })}
              />
            {/snippet}
          </SettingRow>

          <SettingRow label="Line height">
            {#snippet control()}
              <input
                type="range"
                min="1.2"
                max="2.4"
                step="0.05"
                aria-label="Line height"
                value={settingsStore.settings.manuscriptLeading}
                oninput={(event) =>
                  settingsStore.update({ manuscriptLeading: Number(event.currentTarget.value) })}
              />
            {/snippet}
          </SettingRow>

          <SettingRow label="Paragraph spacing">
            {#snippet control()}
              <input
                type="range"
                min="0"
                max="2"
                step="0.05"
                aria-label="Paragraph spacing"
                value={settingsStore.settings.manuscriptParagraphGap}
                oninput={(event) =>
                  settingsStore.update({
                    manuscriptParagraphGap: Number(event.currentTarget.value)
                  })}
              />
            {/snippet}
          </SettingRow>

          <SettingRow
            label="Column width"
            hint="How wide a line of manuscript may run, whatever the size of the window."
          >
            {#snippet control()}
              <input
                type="range"
                min="24"
                max="60"
                step="1"
                aria-label="Manuscript column width"
                value={settingsStore.settings.manuscriptMeasure}
                oninput={(event) =>
                  settingsStore.update({ manuscriptMeasure: Number(event.currentTarget.value) })}
              />
            {/snippet}
          </SettingRow>
        </section>
      {:else if section === 'editor'}
        <section class="group">
          <h2>Editor</h2>

          <SettingRow
            label="Autosave delay"
            hint="How long typing pauses before Grimoire saves. Lower is more frequent."
          >
            {#snippet control()}
              <div class="stepper">
                <input
                  type="range"
                  min="200"
                  max="5000"
                  step="100"
                  aria-label="Autosave delay in milliseconds"
                  value={settingsStore.settings.autosaveDebounceMs}
                  oninput={(event) =>
                    settingsStore.update({
                      autosaveDebounceMs: Number(event.currentTarget.value)
                    })}
                />
                <span class="value tabular">{settingsStore.settings.autosaveDebounceMs} ms</span>
              </div>
            {/snippet}
          </SettingRow>

          <SettingRow
            label="Snapshot interval"
            hint="How often a version is kept while you work, so you can step back through a session."
          >
            {#snippet control()}
              <div class="stepper">
                <input
                  type="range"
                  min="60"
                  max="3600"
                  step="60"
                  aria-label="Snapshot interval in seconds"
                  value={settingsStore.settings.revisionIntervalSeconds}
                  oninput={(event) =>
                    settingsStore.update({
                      revisionIntervalSeconds: Number(event.currentTarget.value)
                    })}
                />
                <span class="value tabular">
                  {Math.round(settingsStore.settings.revisionIntervalSeconds / 60)} min
                </span>
              </div>
            {/snippet}
          </SettingRow>

          <SettingRow label="Check spelling" hint="Uses the system dictionary.">
            {#snippet control()}
              <Toggle
                checked={settingsStore.settings.spellcheck}
                label="Check spelling"
                onchange={(checked) => settingsStore.update({ spellcheck: checked })}
              />
            {/snippet}
          </SettingRow>
        </section>
      {:else if section === 'ai'}
        <AiSettings />
      {:else if section === 'data'}
        <section class="group">
          <h2>Data</h2>
          <p class="preamble">
            Everything Grimoire knows is in one file on this computer. Nothing is uploaded, and
            there is no account.
          </p>

          <SettingRow label="Library file" hint="Copy this file to back up everything." stacked>
            {#snippet control()}
              <code class="path selectable">{info?.libraryPath ?? 'Loading…'}</code>
            {/snippet}
          </SettingRow>

          <SettingRow
            label="Rebuild the search index"
            hint="Use this if search stops finding text you know is there."
          >
            {#snippet control()}
              <Button variant="secondary" size="sm" disabled={rebuilding} onclick={rebuild}>
                {rebuilding ? 'Rebuilding…' : 'Rebuild'}
              </Button>
            {/snippet}
          </SettingRow>
        </section>
      {:else if section === 'keyboard'}
        <section class="group">
          <h2>Keyboard</h2>
          <table class="shortcuts">
            <tbody>
              {#each shortcuts as shortcut (shortcut.keys)}
                <tr>
                  <td><kbd>{shortcut.keys}</kbd></td>
                  <td>{shortcut.action}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </section>
      {:else}
        <section class="group">
          <h2>About</h2>
          <p class="creed">
            The Library holds the work. The Volume gives it identity. The Chapter gives it
            structure. The Page is where writing happens. The Margin is where thinking happens.
            The manuscript belongs to you.
          </p>
          <dl class="facts">
            <dt>Version</dt>
            <dd class="tabular">{info?.version ?? '—'}</dd>
            <dt>SQLite</dt>
            <dd class="tabular">{info?.sqliteVersion ?? '—'}</dd>
            <dt>Layout</dt>
            <dd>{viewport.layout} · {viewport.width}×{viewport.height}</dd>
          </dl>
        </section>
      {/if}
    </main>
  </div>
</div>

<style>
  .settings {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--surface-app);
  }

  .head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    min-height: var(--rail-height);
    padding: 0 var(--space-2);
    border-bottom: var(--border-width) solid var(--border-subtle);
    background: var(--surface-pane);
  }

  h1 {
    font-size: var(--text-lg);
    font-weight: var(--weight-medium);
  }

  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .sections {
    flex: none;
    width: 13rem;
    padding: var(--space-2);
    border-right: var(--border-width) solid var(--border-subtle);
    background: var(--surface-pane);
    overflow-y: auto;
  }

  .section-link {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    min-height: var(--touch-min);
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    color: var(--text-secondary);
    font-size: var(--text-md);
  }

  .section-link:hover {
    background: var(--surface-hover);
    color: var(--text-primary);
  }

  .section-link.selected {
    background: var(--surface-selected);
    color: var(--accent);
    font-weight: var(--weight-medium);
  }

  .panel {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    padding: var(--space-6) var(--space-5) var(--space-8);
  }

  .group {
    max-width: 46rem;
    margin: 0 auto var(--space-7);
  }

  h2 {
    font-size: var(--text-xl);
    margin-bottom: var(--space-4);
  }

  h3 {
    margin-bottom: var(--space-3);
  }

  .preamble {
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-normal);
    margin-bottom: var(--space-4);
  }

  .sample {
    padding: var(--space-4);
    margin-bottom: var(--space-3);
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-subtle);
    border-radius: var(--radius-md);
    font-size: var(--manuscript-size);
    line-height: var(--manuscript-leading);
    max-width: var(--measure-editor);
  }

  .themes {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(9rem, 1fr));
    gap: var(--space-2);
  }

  .theme {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-height: var(--touch-comfortable);
    padding: var(--space-2) var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
    text-align: left;
  }

  .theme.selected {
    border-color: var(--accent);
    background: var(--accent-quiet);
  }

  .theme-name {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  .theme-hint {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }

  .chips {
    display: flex;
    gap: var(--space-1);
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

  .stepper {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }

  .value {
    font-size: var(--text-sm);
    color: var(--text-tertiary);
    min-width: 4rem;
    text-align: right;
  }

  input[type='range'] {
    width: 12rem;
    accent-color: var(--accent);
  }

  .path {
    display: block;
    padding: var(--space-2) var(--space-3);
    background: var(--surface-sunken);
    border-radius: var(--radius-sm);
    font-family: var(--font-mono);
    font-size: var(--text-xs);
    color: var(--text-secondary);
    overflow-wrap: anywhere;
  }

  .shortcuts {
    width: 100%;
    border-collapse: collapse;
  }

  .shortcuts td {
    padding: var(--space-2) 0;
    border-bottom: var(--border-width) solid var(--border-subtle);
    font-size: var(--text-md);
  }

  .shortcuts td:first-child {
    width: 12rem;
  }

  kbd {
    font-family: var(--font-ui);
    font-size: var(--text-sm);
    padding: 2px var(--space-2);
    border-radius: var(--radius-sm);
    background: var(--surface-sunken);
    border: var(--border-width) solid var(--border-subtle);
    color: var(--text-secondary);
  }

  .creed {
    font-family: var(--font-serif);
    font-size: var(--text-lg);
    line-height: var(--leading-normal);
    color: var(--text-secondary);
    max-width: 34rem;
    margin-bottom: var(--space-5);
  }

  .facts {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: var(--space-2) var(--space-5);
    font-size: var(--text-md);
  }

  .facts dt {
    color: var(--text-tertiary);
  }

  /* On a narrow screen the section list becomes a scrolling strip across the
     top, because a 13rem sidebar beside content leaves neither usable. */
  @media (max-width: 700px) {
    .body {
      flex-direction: column;
    }

    .sections {
      width: auto;
      display: flex;
      gap: var(--space-1);
      overflow-x: auto;
      border-right: none;
      border-bottom: var(--border-width) solid var(--border-subtle);
      scrollbar-width: none;
    }

    .section-link {
      width: auto;
      white-space: nowrap;
    }

    .panel {
      padding: var(--space-5) var(--space-4) var(--space-7);
    }
  }
</style>
