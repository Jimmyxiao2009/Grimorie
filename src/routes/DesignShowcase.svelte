<script lang="ts">
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import Menu from '$lib/components/Menu.svelte';
  import TextField from '$lib/components/TextField.svelte';
  import { ICON_NAMES } from '$lib/design/icons';
  import { THEME_CHOICES, appearance, type ThemeChoice } from '$lib/design/theme.svelte';
  import { viewport } from '$lib/design/viewport.svelte';
  import type { MenuItem } from '$lib/types/ui';

  // A development-only surface for checking the design system in isolation.
  // It is rendered only under `import.meta.env.DEV` and never ships.

  let dialogOpen = $state(false);
  let fieldValue = $state('The Salt Road');
  let invalidValue = $state('');

  const menuItems: MenuItem[] = [
    { kind: 'heading', id: 'h', label: 'Page' },
    { id: 'rename', label: 'Rename', icon: 'pencil', hint: 'F2', select: () => {} },
    { id: 'duplicate', label: 'Duplicate', icon: 'duplicate', select: () => {} },
    { id: 'bookmark', label: 'Bookmark', icon: 'bookmark', hint: 'Ctrl+B', select: () => {} },
    { kind: 'separator', id: 's1' },
    { id: 'delete', label: 'Delete', icon: 'trash', danger: true, select: () => {} }
  ];

  const swatches = [
    'surface-app',
    'surface-page',
    'surface-pane',
    'surface-raised',
    'surface-sunken',
    'accent',
    'state-info',
    'state-success',
    'state-warning',
    'state-danger',
    'state-ai'
  ];
</script>

<div class="showcase" data-scroll>
  <header class="head">
    <div>
      <h1>Grimoire design system</h1>
      <p class="meta tabular">
        {viewport.width}×{viewport.height} · layout <strong>{viewport.layout}</strong> · theme
        <strong>{appearance.resolved}</strong>
        {viewport.coarsePointer ? '· coarse pointer' : ''}
      </p>
    </div>
    <div class="themes">
      {#each THEME_CHOICES as choice (choice)}
        <button
          type="button"
          class="theme-chip"
          class:selected={appearance.choice === choice}
          onclick={() => appearance.setChoice(choice as ThemeChoice)}
        >
          {choice}
        </button>
      {/each}
    </div>
  </header>

  <section>
    <h2>Manuscript type</h2>
    <div class="sheet">
      <h3 class="chapter-title">Chapter III — The Salt Road</h3>
      <p class="manuscript">
        The road had been salt once, or so the carters said, and the wind still carried the taste of
        it in dry months. Ilse walked it at dusk because dusk was when the crows left, and she had
        never been able to write with crows watching.
      </p>
      <p class="manuscript">
        手稿属于用户。The manuscript belongs to the user — a line she had copied from the flyleaf of a
        book whose title she no longer remembered, and which she had never once managed to believe.
      </p>
    </div>
  </section>

  <section>
    <h2>Buttons</h2>
    <div class="row">
      <Button variant="primary" icon="plus">New Volume</Button>
      <Button variant="secondary" icon="import">Import</Button>
      <Button variant="ghost" icon="settings">Settings</Button>
      <Button variant="danger" icon="trash">Delete</Button>
      <Button variant="secondary" disabled>Disabled</Button>
    </div>
    <div class="row">
      <Button size="sm" variant="secondary">Small</Button>
      <Button size="md" variant="secondary">Medium</Button>
      <Button size="lg" variant="secondary">Large</Button>
    </div>
  </section>

  <section>
    <h2>Icon buttons &amp; menus</h2>
    <div class="row">
      <IconButton name="panelLeft" label="Toggle navigation" />
      <IconButton name="panelRight" label="Toggle margin" pressed />
      <IconButton name="focusEnter" label="Focus mode" />
      <IconButton name="sparkle" label="Ask Grimoire" tone="accent" />
      <IconButton name="trash" label="Delete" tone="danger" />
      <IconButton name="search" label="Search" size="sm" />
      <IconButton name="search" label="Search" size="lg" />
      <Menu items={menuItems} label="Page actions" />
    </div>
  </section>

  <section>
    <h2>Fields</h2>
    <div class="grid-2">
      <TextField bind:value={fieldValue} label="Volume title" hint="Shown on the Library shelf." />
      <TextField
        bind:value={invalidValue}
        label="Chapter title"
        placeholder="Untitled Chapter"
        error="A Chapter needs a title."
      />
      <TextField bind:value={fieldValue} label="Description" multiline rows={3} />
    </div>
  </section>

  <section>
    <h2>Dialog &amp; empty states</h2>
    <div class="row">
      <Button variant="secondary" onclick={() => (dialogOpen = true)}>Open dialog</Button>
    </div>
    <div class="grid-2">
      <div class="panel-demo">
        <EmptyState title="Your library is empty." hint="Create a Volume to begin.">
          {#snippet action()}
            <Button variant="primary" icon="plus">New Volume</Button>
          {/snippet}
        </EmptyState>
      </div>
      <div class="panel-demo">
        <EmptyState size="sm" title="No margin notes yet." hint="Select text to leave one." />
      </div>
    </div>
  </section>

  <section>
    <h2>Colour</h2>
    <div class="swatches">
      {#each swatches as token (token)}
        <div class="swatch">
          <div class="chip" style:background="var(--{token})"></div>
          <code>{token}</code>
        </div>
      {/each}
    </div>
  </section>

  <section>
    <h2>Icons <span class="count tabular">({ICON_NAMES.length})</span></h2>
    <div class="icons">
      {#each ICON_NAMES as name (name)}
        <div class="icon-cell" title={name}>
          <Icon {name} size={22} />
          <span>{name}</span>
        </div>
      {/each}
    </div>
  </section>
</div>

<Dialog
  bind:open={dialogOpen}
  title="Delete “The Salt Road”?"
  description="The Volume and its 3 Chapters will be removed. This cannot be undone."
>
  <p class="dialog-body">
    A backup is written before deletion, so the Volume can still be recovered from the Data section
    of Settings.
  </p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (dialogOpen = false)}>Cancel</Button>
    <Button variant="danger" onclick={() => (dialogOpen = false)}>Delete Volume</Button>
  {/snippet}
</Dialog>

<style>
  .showcase {
    height: 100%;
    padding: var(--space-6) var(--space-5) var(--space-8);
    max-width: 66rem;
    margin: 0 auto;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: var(--space-6);
  }

  h1 {
    font-size: var(--text-2xl);
  }

  .meta {
    margin-top: var(--space-2);
    font-size: var(--text-sm);
    color: var(--text-tertiary);
  }

  .themes {
    display: flex;
    gap: var(--space-1);
    flex-wrap: wrap;
  }

  .theme-chip {
    min-height: var(--touch-min);
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
    font-size: var(--text-sm);
    color: var(--text-secondary);
    text-transform: capitalize;
  }

  .theme-chip.selected {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-contrast);
  }

  section {
    margin-bottom: var(--space-7);
  }

  h2 {
    font-size: var(--text-sm);
    font-weight: var(--weight-semibold);
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: var(--text-tertiary);
    padding-bottom: var(--space-2);
    border-bottom: var(--border-width) solid var(--border-subtle);
    margin-bottom: var(--space-4);
  }

  .count {
    font-weight: var(--weight-regular);
    text-transform: none;
    letter-spacing: 0;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-3);
  }

  .grid-2 {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
    gap: var(--space-4);
  }

  .sheet {
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-subtle);
    border-radius: var(--radius-md);
    box-shadow: var(--page-edge);
    padding: var(--space-6) var(--space-5);
  }

  .chapter-title {
    font-family: var(--font-serif);
    font-size: var(--text-xl);
    font-weight: var(--weight-regular);
    margin-bottom: var(--space-4);
  }

  .manuscript {
    font-family: var(--font-manuscript);
    font-size: var(--manuscript-size);
    line-height: var(--manuscript-leading);
    max-width: var(--measure-editor);
  }

  .manuscript + .manuscript {
    margin-top: var(--manuscript-paragraph-gap);
  }

  .panel-demo {
    background: var(--surface-pane);
    border: var(--border-width) solid var(--border-subtle);
    border-radius: var(--radius-md);
    min-height: 12rem;
    display: grid;
    place-items: center;
  }

  .swatches {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(9rem, 1fr));
    gap: var(--space-3);
  }

  .chip {
    height: 2.75rem;
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-default);
  }

  .swatch code {
    display: block;
    margin-top: var(--space-1);
    font-family: var(--font-mono);
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
  }

  .icons {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(7rem, 1fr));
    gap: var(--space-2);
  }

  .icon-cell {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-2);
    padding: var(--space-3) var(--space-1);
    border-radius: var(--radius-md);
    border: var(--border-width) solid var(--border-subtle);
    color: var(--text-secondary);
  }

  .icon-cell span {
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
    text-align: center;
    word-break: break-word;
  }

  .dialog-body {
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-normal);
  }
</style>
