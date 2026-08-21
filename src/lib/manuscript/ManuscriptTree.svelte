<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import Menu from '$lib/components/Menu.svelte';
  import { bookmarks } from '$lib/stores/bookmarks.svelte';
  import { workspace } from '$lib/stores/workspace.svelte';
  import { count } from '$lib/utils/format';
  import type { ChapterOutline, PageSummary } from '$lib/types/manuscript';
  import type { MenuItem } from '$lib/types/ui';

  /**
   * The contents of the manuscript.
   *
   * Written to read as the front matter of a book rather than as a file tree.
   * Chapters are section headings; Pages are entries beneath them. Word counts
   * and per-Chapter actions stay out of sight until a row is hovered or is the
   * one being written in — a column of numbers next to every line makes the
   * navigation compete with the manuscript for attention, and on a 10-inch
   * screen it is the manuscript that should win.
   */

  interface Props {
    onrename: (target: { kind: 'chapter' | 'page'; id: string; title: string }) => void;
    ondelete: (target: { kind: 'chapter' | 'page'; id: string; title: string }) => void;
    onnavigate?: () => void;
  }

  let { onrename, ondelete, onnavigate }: Props = $props();

  function chapterActions(chapter: ChapterOutline): MenuItem[] {
    return [
      {
        id: 'page',
        label: 'New Page',
        icon: 'plus',
        select: () => void workspace.createPage(chapter.id)
      },
      {
        id: 'rename',
        label: 'Rename…',
        icon: 'pencil',
        select: () => onrename({ kind: 'chapter', id: chapter.id, title: chapter.title })
      },
      {
        id: 'duplicate',
        label: 'Duplicate',
        icon: 'duplicate',
        select: () => void workspace.duplicateChapter(chapter.id)
      },
      { kind: 'separator', id: 's' },
      {
        id: 'delete',
        label: 'Delete…',
        icon: 'trash',
        danger: true,
        select: () => ondelete({ kind: 'chapter', id: chapter.id, title: chapter.title })
      }
    ];
  }

  function pageActions(page: PageSummary): MenuItem[] {
    return [
      {
        id: 'rename',
        label: 'Rename…',
        icon: 'pencil',
        select: () => onrename({ kind: 'page', id: page.id, title: page.title })
      },
      {
        id: 'bookmark',
        label: bookmarks.isMarked(page.id) ? 'Remove bookmark' : 'Bookmark',
        icon: 'bookmark',
        hint: 'Ctrl+B',
        select: () => void bookmarks.toggle(page.id)
      },
      {
        id: 'duplicate',
        label: 'Duplicate',
        icon: 'duplicate',
        select: () => void workspace.duplicatePage(page.id)
      },
      { kind: 'separator', id: 's' },
      {
        id: 'delete',
        label: 'Delete…',
        icon: 'trash',
        danger: true,
        select: () => ondelete({ kind: 'page', id: page.id, title: page.title })
      }
    ];
  }

  function selectPage(id: string) {
    workspace.selectPage(id);
    onnavigate?.();
  }

  /** The Chapter the open Page belongs to, which stays expanded and lit. */
  const currentChapterId = $derived(
    workspace.chapters.find((chapter) =>
      chapter.pages.some((page) => page.id === workspace.activePageId)
    )?.id ?? null
  );
</script>

<nav class="contents" aria-label="Manuscript" data-scroll>
  {#if workspace.isEmpty}
    <p class="blank">Nothing here yet.</p>
  {/if}

  <ol class="chapters">
    {#each workspace.chapters as chapter, index (chapter.id)}
      {@const collapsed = workspace.isCollapsed(chapter.id)}
      {@const current = currentChapterId === chapter.id}
      <li class="chapter" class:current>
        <div class="chapter-row">
          <button
            type="button"
            class="chapter-face"
            aria-expanded={!collapsed}
            onclick={() => workspace.toggleChapter(chapter.id)}
          >
            <span class="twist" aria-hidden="true">
              <Icon name={collapsed ? 'chevronRight' : 'chevronDown'} size={13} />
            </span>
            <span class="chapter-title">{chapter.title}</span>
          </button>

          <div class="chapter-menu">
            <Menu items={chapterActions(chapter)} label="{chapter.title} actions" size="sm" />
          </div>
        </div>

        {#if !collapsed}
          <ul class="pages">
            {#each chapter.pages as page (page.id)}
              {@const active = workspace.activePageId === page.id}
              <li class="page" class:active>
                <button type="button" class="page-face" onclick={() => selectPage(page.id)}>
                  <span class="page-title">{page.title}</span>
                  {#if bookmarks.isMarked(page.id)}
                    <span class="bookmarked" title="Bookmarked" aria-label="Bookmarked">
                      <Icon name="bookmark" size={11} />
                    </span>
                  {/if}
                  {#if page.wordCount > 0}
                    <span class="words tabular">{count(page.wordCount)}</span>
                  {/if}
                </button>

                <div class="page-menu">
                  <Menu items={pageActions(page)} label="{page.title} actions" size="sm" />
                </div>
              </li>
            {/each}

            {#if chapter.pages.length === 0 || current}
              <li>
                <button
                  type="button"
                  class="quiet-add"
                  onclick={() => void workspace.createPage(chapter.id)}
                >
                  New Page
                </button>
              </li>
            {/if}
          </ul>
        {/if}
      </li>
      {#if index < workspace.chapters.length - 1}
        <li class="gap" aria-hidden="true"></li>
      {/if}
    {/each}
  </ol>

  <!-- A line of text at the end of the contents, not a button bolted to the
       bottom of the pane. The old fixed bar read as a web-app call to action. -->
  <button type="button" class="quiet-add end" onclick={() => void workspace.createChapter()}>
    New Chapter
  </button>
</nav>

<style>
  .contents {
    height: 100%;
    min-height: 0;
    overflow-y: auto;
    padding: var(--space-4) var(--space-1) var(--space-7) var(--space-2);
  }

  .blank {
    padding: var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-tertiary);
  }

  .gap {
    height: var(--space-4);
  }

  /* --- Chapters ---------------------------------------------------------- */

  .chapter-row {
    position: relative;
    display: flex;
    align-items: center;
  }

  .chapter-face {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    flex: 1;
    min-width: 0;
    min-height: var(--touch-min);
    padding: 0 var(--space-2) 0 0;
    border-radius: var(--radius-sm);
    text-align: left;
  }

  .twist {
    display: grid;
    place-items: center;
    width: 18px;
    flex: none;
    color: var(--text-tertiary);
  }

  /* A section heading, in the manuscript's own face, rather than a tree node. */
  .chapter-title {
    font-family: var(--font-serif);
    font-size: var(--text-md);
    line-height: var(--leading-snug);
    color: var(--text-secondary);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .chapter.current .chapter-title {
    color: var(--text-primary);
  }

  .chapter-face:hover .chapter-title {
    color: var(--text-primary);
  }

  .chapter-menu,
  .page-menu {
    position: absolute;
    right: 0;
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .page-menu {
    top: 0;
  }

  /* Kept out of the way until wanted, but reachable: the row menu appears on
     hover, on keyboard focus, and on the active Page — which is how a tablet,
     with no hover at all, still gets to it. */
  .chapter-row:hover .chapter-menu,
  .chapter-menu:focus-within,
  .page:hover .page-menu,
  .page-menu:focus-within,
  .page.active .page-menu {
    opacity: 1;
  }

  /* --- Pages ------------------------------------------------------------- */

  .pages {
    margin-left: 18px;
    padding-left: var(--space-3);
    border-left: var(--border-width) solid var(--border-subtle);
  }

  .page {
    position: relative;
  }

  .page-face {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    width: 100%;
    min-height: 34px;
    padding: var(--space-1) var(--space-5) var(--space-1) var(--space-2);
    border-radius: var(--radius-sm);
    text-align: left;
    color: var(--text-tertiary);
  }

  .page-face:hover {
    background: var(--surface-hover);
    color: var(--text-secondary);
  }

  .page.active .page-face {
    background: var(--surface-selected);
    color: var(--text-primary);
  }

  .page-title {
    flex: 1;
    min-width: 0;
    font-size: var(--text-md);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bookmarked {
    flex: none;
    display: inline-flex;
    color: var(--accent);
  }

  /* Word counts are metadata, not navigation. They appear when a row is
     hovered or open, and are otherwise absent. */
  .words {
    flex: none;
    font-size: var(--text-2xs);
    color: var(--text-tertiary);
    opacity: 0;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .page-face:hover .words,
  .page.active .words {
    opacity: 1;
  }

  /* --- Adding ------------------------------------------------------------ */

  /* Italic, so it reads as an instruction rather than as one more entry in the
     list it sits at the end of. */
  .quiet-add {
    display: block;
    width: 100%;
    min-height: 32px;
    padding: 0 var(--space-2);
    text-align: left;
    font-size: var(--text-sm);
    font-style: italic;
    color: var(--text-tertiary);
    border-radius: var(--radius-sm);
  }

  .quiet-add:hover {
    color: var(--accent);
  }

  .end {
    margin-top: var(--space-5);
    padding-left: 18px;
  }
</style>
