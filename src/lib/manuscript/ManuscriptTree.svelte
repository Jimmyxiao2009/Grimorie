<script lang="ts">
  import Button from '$lib/components/Button.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import Menu from '$lib/components/Menu.svelte';
  import { workspace } from '$lib/stores/workspace.svelte';
  import { count } from '$lib/utils/format';
  import type { ChapterOutline, PageSummary } from '$lib/types/manuscript';
  import type { MenuItem } from '$lib/types/ui';

  interface Props {
    /** Asked to rename something; the workspace owns the dialog. */
    onrename: (target: { kind: 'chapter' | 'page'; id: string; title: string }) => void;
    ondelete: (target: { kind: 'chapter' | 'page'; id: string; title: string }) => void;
    /** Called after selecting a Page, so an overlay can close itself. */
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
</script>

<nav class="tree" aria-label="Manuscript">
  <div class="scroll" data-scroll>
    {#if workspace.isEmpty}
      <p class="blank">This Volume has no Chapters yet.</p>
    {/if}

    <ul class="chapters">
      {#each workspace.chapters as chapter (chapter.id)}
        {@const collapsed = workspace.isCollapsed(chapter.id)}
        <li class="chapter">
          <div class="row chapter-row">
            <!-- Collapsing is its own control rather than a click on the title:
                 on a tablet, one target that both selects and folds is a
                 target that does the wrong thing half the time. -->
            <button
              type="button"
              class="twist"
              aria-expanded={!collapsed}
              aria-label="{collapsed ? 'Expand' : 'Collapse'} {chapter.title}"
              onclick={() => workspace.toggleChapter(chapter.id)}
            >
              <Icon name={collapsed ? 'chevronRight' : 'chevronDown'} size={16} />
            </button>

            <span class="chapter-title truncate">{chapter.title}</span>

            <span class="tally tabular" aria-hidden="true">{chapter.pages.length}</span>

            <div class="row-menu">
              <Menu items={chapterActions(chapter)} label="{chapter.title} actions" size="sm" />
            </div>
          </div>

          {#if !collapsed}
            <ul class="pages">
              {#each chapter.pages as page (page.id)}
                <li>
                  <div class="row page-row" class:active={workspace.activePageId === page.id}>
                    <button type="button" class="page-open" onclick={() => selectPage(page.id)}>
                      <span class="page-title truncate">{page.title}</span>
                      {#if page.wordCount > 0}
                        <span class="tally tabular">{count(page.wordCount)}</span>
                      {/if}
                    </button>

                    <div class="row-menu">
                      <Menu items={pageActions(page)} label="{page.title} actions" size="sm" />
                    </div>
                  </div>
                </li>
              {/each}

              <li>
                <button
                  type="button"
                  class="row add"
                  onclick={() => void workspace.createPage(chapter.id)}
                >
                  <Icon name="plus" size={15} />
                  <span>New Page</span>
                </button>
              </li>
            </ul>
          {/if}
        </li>
      {/each}
    </ul>
  </div>

  <footer class="foot">
    <Button
      variant="secondary"
      size="sm"
      icon="plus"
      block
      onclick={() => void workspace.createChapter()}
    >
      New Chapter
    </Button>
  </footer>
</nav>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
  }

  .scroll {
    flex: 1;
    min-height: 0;
    padding: var(--space-2) var(--space-2) var(--space-4);
  }

  .blank {
    padding: var(--space-4) var(--space-3);
    font-size: var(--text-sm);
    color: var(--text-tertiary);
    line-height: var(--leading-snug);
  }

  .chapter + .chapter {
    margin-top: var(--space-1);
  }

  /* Every row clears the touch minimum. The tree is the control a tablet user
     touches most, and a dense desktop tree is unusable with a finger. */
  .row {
    display: flex;
    align-items: center;
    min-height: var(--touch-min);
    border-radius: var(--radius-md);
    padding-right: 2px;
  }

  .chapter-row {
    padding-left: 2px;
    color: var(--text-primary);
  }

  .chapter-row:hover,
  .page-row:hover {
    background: var(--surface-hover);
  }

  .twist {
    display: grid;
    place-items: center;
    width: 30px;
    height: var(--touch-min);
    flex: none;
    color: var(--text-tertiary);
    border-radius: var(--radius-sm);
  }

  .twist:hover {
    color: var(--text-primary);
  }

  .chapter-title {
    flex: 1;
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    padding: var(--space-1) var(--space-1);
  }

  .tally {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    padding: 0 var(--space-2);
    flex: none;
  }

  .pages {
    /* A hairline rule marking the chapter's extent, in place of indent guides
       that would be invisible at this contrast. */
    margin: var(--space-1) 0 var(--space-2) 15px;
    padding-left: var(--space-3);
    border-left: var(--border-width) solid var(--border-subtle);
  }

  .page-row {
    color: var(--text-secondary);
  }

  .page-row.active {
    background: var(--surface-selected);
    color: var(--text-primary);
  }

  .page-row.active:hover {
    background: var(--surface-selected);
  }

  .page-open {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: 1;
    min-width: 0;
    min-height: var(--touch-min);
    padding: 0 var(--space-2);
    border-radius: var(--radius-md);
  }

  .page-title {
    flex: 1;
    text-align: left;
    font-size: var(--text-md);
  }

  .active .page-title {
    font-weight: var(--weight-medium);
  }

  .add {
    width: 100%;
    gap: var(--space-2);
    padding-left: var(--space-2);
    color: var(--text-tertiary);
    font-size: var(--text-sm);
  }

  .add:hover {
    background: var(--surface-hover);
    color: var(--text-secondary);
  }

  /* The row menu is always present, never hover-revealed: a control that only
     exists under a mouse pointer does not exist on a tablet. It is simply
     quiet until it is wanted. */
  .row-menu {
    flex: none;
    opacity: 0.55;
    transition: opacity var(--motion-fast) var(--ease-out);
  }

  .row:hover .row-menu,
  .row-menu:focus-within {
    opacity: 1;
  }

  .foot {
    flex: none;
    padding: var(--space-2);
    border-top: var(--border-width) solid var(--border-subtle);
  }
</style>
