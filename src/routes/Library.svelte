<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import TextField from '$lib/components/TextField.svelte';
  import VolumeCard from '$lib/library/VolumeCard.svelte';
  import { libraryStore } from '$lib/stores/library.svelte';
  import { router } from '$lib/stores/router.svelte';
  import type { Shelf, VolumeSummary } from '$lib/types/manuscript';

  let creating = $state(false);
  let newTitle = $state('');
  let newSubtitle = $state('');

  // Dialogs own an `open` flag separately from their subject: Dialog binds and
  // writes `open` itself, so deriving it from a nullable target would make the
  // component mutate a value it does not own.
  let renameOpen = $state(false);
  let renameTarget = $state<VolumeSummary | null>(null);
  let renameTitle = $state('');
  let renameSubtitle = $state('');

  let deleteOpen = $state(false);
  let deleteTarget = $state<VolumeSummary | null>(null);

  onMount(() => {
    void libraryStore.load();
  });

  const shelves: { id: Shelf; label: string }[] = [
    { id: 'active', label: 'Library' },
    { id: 'archived', label: 'Archived' }
  ];

  function open(id: string) {
    router.toWorkspace(id);
  }

  function beginCreate() {
    newTitle = '';
    newSubtitle = '';
    creating = true;
  }

  async function confirmCreate() {
    const title = newTitle.trim();
    if (!title) return;
    creating = false;
    const id = await libraryStore.create(title, newSubtitle);
    // Straight into the new Volume: creating one is always a prelude to
    // writing in it.
    if (id) open(id);
  }

  function beginRename(volume: VolumeSummary) {
    renameTarget = volume;
    renameTitle = volume.title;
    renameSubtitle = volume.subtitle ?? '';
    renameOpen = true;
  }

  function beginDelete(volume: VolumeSummary) {
    deleteTarget = volume;
    deleteOpen = true;
  }

  async function confirmRename() {
    const target = renameTarget;
    if (!target || !renameTitle.trim()) return;
    renameOpen = false;
    await libraryStore.rename(target.id, renameTitle.trim(), renameSubtitle);
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target) return;
    deleteOpen = false;
    await libraryStore.remove(target.id);
  }

  async function archiveInsteadOfDeleting() {
    const target = deleteTarget;
    if (!target) return;
    deleteOpen = false;
    await libraryStore.setArchived(target.id, true);
  }
</script>

<div class="library" data-scroll>
  <header class="head">
    <div class="identity">
      <h1>Grimoire</h1>
      <p class="tagline">The manuscript belongs to you.</p>
    </div>

    <div class="controls">
      <div class="shelves" role="tablist" aria-label="Library shelves">
        {#each shelves as shelf (shelf.id)}
          <button
            type="button"
            role="tab"
            class="shelf"
            class:selected={libraryStore.shelf === shelf.id}
            aria-selected={libraryStore.shelf === shelf.id}
            onclick={() => libraryStore.load(shelf.id)}
          >
            {shelf.label}
          </button>
        {/each}
      </div>

      <Button variant="primary" icon="plus" onclick={beginCreate}>New Volume</Button>
    </div>
  </header>

  {#if libraryStore.loading}
    <p class="status" aria-live="polite">Opening your library…</p>
  {:else if libraryStore.failure}
    <div class="failure">
      <EmptyState title={libraryStore.failure} hint="Your manuscripts have not been changed.">
        {#snippet action()}
          <Button variant="secondary" icon="refresh" onclick={() => libraryStore.load()}>
            Try again
          </Button>
        {/snippet}
      </EmptyState>
    </div>
  {:else if libraryStore.isEmpty}
    <div class="empty">
      {#if libraryStore.shelf === 'archived'}
        <EmptyState
          title="Nothing is archived."
          hint="Archiving a Volume takes it off the shelf without deleting a word of it."
        />
      {:else}
        <EmptyState title="Your library is empty." hint="Create a Volume to begin.">
          {#snippet action()}
            <Button variant="primary" icon="plus" onclick={beginCreate}>New Volume</Button>
          {/snippet}
        </EmptyState>
      {/if}
    </div>
  {:else}
    {#if libraryStore.recent.length > 0}
      <section class="section">
        <h2 class="eyebrow">Continue</h2>
        <div class="grid continue">
          {#each libraryStore.recent as volume (volume.id)}
            <VolumeCard
              {volume}
              compact
              onopen={open}
              onrename={beginRename}
              onduplicate={(v) => libraryStore.duplicate(v.id)}
              onarchive={(v) => libraryStore.setArchived(v.id, v.archivedAt === null)}
              ondelete={beginDelete}
            />
          {/each}
        </div>
      </section>
    {/if}

    <section class="section">
      <h2 class="eyebrow">
        {libraryStore.shelf === 'archived' ? 'Archived' : 'All Volumes'}
      </h2>
      <div class="grid">
        {#each libraryStore.volumes as volume (volume.id)}
          <VolumeCard
            {volume}
            onopen={open}
            onrename={beginRename}
            onduplicate={(v) => libraryStore.duplicate(v.id)}
            onarchive={(v) => libraryStore.setArchived(v.id, v.archivedAt === null)}
            ondelete={beginDelete}
          />
        {/each}
      </div>
    </section>
  {/if}
</div>

<Dialog bind:open={creating} title="New Volume" description="A Volume holds one work.">
  <div class="form">
    <TextField
      bind:value={newTitle}
      label="Title"
      placeholder="The Salt Road"
      autofocus
      onenter={confirmCreate}
    />
    <TextField
      bind:value={newSubtitle}
      label="Subtitle"
      hint="Optional."
      placeholder="A novel"
      onenter={confirmCreate}
    />
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (creating = false)}>Cancel</Button>
    <Button variant="primary" disabled={!newTitle.trim()} onclick={confirmCreate}>Create</Button>
  {/snippet}
</Dialog>

<Dialog bind:open={renameOpen} title="Rename Volume">
  <div class="form">
    <TextField bind:value={renameTitle} label="Title" autofocus onenter={confirmRename} />
    <TextField
      bind:value={renameSubtitle}
      label="Subtitle"
      hint="Optional."
      onenter={confirmRename}
    />
  </div>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (renameOpen = false)}>Cancel</Button>
    <Button variant="primary" disabled={!renameTitle.trim()} onclick={confirmRename}>Save</Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={deleteOpen}
  title={deleteTarget ? `Delete “${deleteTarget.title}”?` : 'Delete Volume?'}
  description="This removes the Volume and everything in it. It cannot be undone."
>
  <p class="warning">
    {#if deleteTarget && deleteTarget.pageCount > 0}
      {deleteTarget.chapterCount} chapters and {deleteTarget.pageCount} pages will be deleted.
    {:else}
      This Volume is empty.
    {/if}
    Archiving keeps a Volume without deleting a word of it.
  </p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleteOpen = false)}>Cancel</Button>
    <Button variant="secondary" icon="archive" onclick={archiveInsteadOfDeleting}>
      Archive instead
    </Button>
    <Button variant="danger" onclick={confirmDelete}>Delete</Button>
  {/snippet}
</Dialog>

<style>
  .library {
    height: 100%;
    padding: var(--space-6) var(--space-5) var(--space-8);
  }

  .head,
  .section,
  .empty,
  .failure,
  .status {
    max-width: 74rem;
    margin-inline: auto;
  }

  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    justify-content: space-between;
    gap: var(--space-4);
    margin-bottom: var(--space-7);
  }

  h1 {
    font-family: var(--font-serif);
    font-size: var(--text-3xl);
    font-weight: var(--weight-regular);
    letter-spacing: 0.01em;
  }

  .tagline {
    margin-top: var(--space-1);
    font-size: var(--text-sm);
    color: var(--text-tertiary);
    font-style: italic;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    flex-wrap: wrap;
  }

  .shelves {
    display: flex;
    gap: var(--space-1);
    padding: 3px;
    background: var(--surface-sunken);
    border-radius: var(--radius-md);
  }

  .shelf {
    min-height: 38px;
    padding: 0 var(--space-4);
    border-radius: var(--radius-sm);
    font-size: var(--text-md);
    color: var(--text-secondary);
    transition: background-color var(--motion-instant) var(--ease-out);
  }

  .shelf:hover {
    color: var(--text-primary);
  }

  .shelf.selected {
    background: var(--surface-raised);
    color: var(--text-primary);
    font-weight: var(--weight-medium);
    box-shadow: var(--shadow-raised);
  }

  .section + .section {
    margin-top: var(--space-7);
  }

  .section h2 {
    margin-bottom: var(--space-3);
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(17rem, 1fr));
    gap: var(--space-4);
  }

  .continue {
    grid-template-columns: repeat(auto-fill, minmax(15rem, 1fr));
  }

  .status {
    padding: var(--space-7) 0;
    text-align: center;
    color: var(--text-tertiary);
  }

  .empty,
  .failure {
    padding-top: var(--space-7);
  }

  .warning {
    font-size: var(--text-md);
    color: var(--text-secondary);
    line-height: var(--leading-normal);
  }

  .form {
    display: flex;
    flex-direction: column;
    gap: var(--space-4);
  }

  /* On a narrow screen the shelf becomes a single column and the header
     stacks, so nothing is squeezed into an unreadable two-column grid. */
  @media (max-width: 560px) {
    .library {
      padding: var(--space-5) var(--space-4) var(--space-7);
    }

    .head {
      align-items: stretch;
      flex-direction: column;
    }

    .controls {
      justify-content: space-between;
    }

    .grid,
    .continue {
      grid-template-columns: 1fr;
    }
  }
</style>
