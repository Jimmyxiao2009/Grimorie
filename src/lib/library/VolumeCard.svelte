<script lang="ts">
  import Menu from '$lib/components/Menu.svelte';
  import { count, plural, relativeTime } from '$lib/utils/format';
  import type { MenuItem } from '$lib/types/ui';
  import type { VolumeSummary } from '$lib/types/manuscript';

  interface Props {
    volume: VolumeSummary;
    /** Compact form for the "Continue" strip. */
    compact?: boolean;
    onopen: (id: string) => void;
    onrename: (volume: VolumeSummary) => void;
    onarchive: (volume: VolumeSummary) => void;
    onduplicate: (volume: VolumeSummary) => void;
    ondelete: (volume: VolumeSummary) => void;
  }

  let { volume, compact = false, onopen, onrename, onarchive, onduplicate, ondelete }: Props =
    $props();

  const archived = $derived(volume.archivedAt !== null);

  const actions = $derived<MenuItem[]>([
    { id: 'open', label: 'Open', icon: 'volume', select: () => onopen(volume.id) },
    { id: 'rename', label: 'Rename…', icon: 'pencil', select: () => onrename(volume) },
    { id: 'duplicate', label: 'Duplicate', icon: 'duplicate', select: () => onduplicate(volume) },
    { kind: 'separator', id: 's1' },
    {
      id: 'archive',
      label: archived ? 'Restore to Library' : 'Archive',
      icon: archived ? 'restore' : 'archive',
      select: () => onarchive(volume)
    },
    { id: 'delete', label: 'Delete…', icon: 'trash', danger: true, select: () => ondelete(volume) }
  ]);
</script>

<article class="card" class:compact style:--spine="var(--tint-{volume.tint})">
  <!-- The whole card is the open target, with the menu layered above it. A
       writer reaching for a Volume on a tablet should not have to find a link. -->
  <button type="button" class="surface" onclick={() => onopen(volume.id)}>
    <span class="spine" aria-hidden="true"></span>

    <span class="text">
      <span class="title">{volume.title}</span>
      {#if volume.subtitle}
        <span class="subtitle truncate">{volume.subtitle}</span>
      {/if}

      <span class="meta tabular">
        {#if volume.pageCount === 0}
          Empty
        {:else}
          {plural(volume.chapterCount, 'chapter')} · {count(volume.wordCount)} words
        {/if}
      </span>
      <span class="meta">
        {#if archived}
          Archived {relativeTime(volume.archivedAt)}
        {:else if volume.lastOpenedAt}
          Opened {relativeTime(volume.lastOpenedAt)}
        {:else}
          Created {relativeTime(volume.createdAt)}
        {/if}
      </span>
    </span>
  </button>

  <div class="menu">
    <Menu items={actions} label="{volume.title} actions" size="sm" />
  </div>
</article>

<style>
  .card {
    position: relative;
    border-radius: var(--radius-md);
    background: var(--surface-page);
    border: var(--border-width) solid var(--border-subtle);
    box-shadow: var(--page-edge);
    transition:
      border-color var(--motion-fast) var(--ease-out),
      box-shadow var(--motion-fast) var(--ease-out);
  }

  .card:hover {
    border-color: var(--border-default);
  }

  .card:has(.surface:focus-visible) {
    outline: var(--border-width-strong) solid var(--accent);
    outline-offset: 2px;
  }

  .surface {
    display: flex;
    gap: var(--space-4);
    width: 100%;
    height: 100%;
    padding: var(--space-4);
    padding-right: var(--space-8);
    border-radius: inherit;
    text-align: left;
  }

  .surface:focus-visible {
    outline: none;
  }

  /* The binding. The only place a Volume's tint appears, so the shelf reads as
     books rather than as colour-coded cards. */
  .spine {
    flex: none;
    width: 5px;
    border-radius: var(--radius-pill);
    background: var(--spine);
    align-self: stretch;
  }

  .compact .spine {
    width: 4px;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: var(--space-1);
    min-width: 0;
    flex: 1;
  }

  .title {
    font-family: var(--font-serif);
    font-size: var(--text-lg);
    line-height: var(--leading-snug);
    color: var(--text-primary);
    /* Two lines, then ellipsis: long titles are real, and so is a tidy shelf. */
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .compact .title {
    font-size: var(--text-md);
    -webkit-line-clamp: 1;
    line-clamp: 1;
  }

  .subtitle {
    font-size: var(--text-sm);
    color: var(--text-secondary);
  }

  .meta {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }

  .meta:nth-of-type(1) {
    margin-top: var(--space-2);
  }

  .compact .meta:nth-of-type(2) {
    display: none;
  }

  .menu {
    position: absolute;
    top: var(--space-2);
    right: var(--space-2);
  }
</style>
