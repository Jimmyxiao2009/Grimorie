<script lang="ts">
  import { onMount } from 'svelte';
  import type { Editor } from '@tiptap/core';

  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import TextField from '$lib/components/TextField.svelte';

  import ManuscriptTree from '$lib/manuscript/ManuscriptTree.svelte';
  import Margin from '$lib/annotations/Margin.svelte';
  import RevisionHistory from '$lib/revisions/RevisionHistory.svelte';
  import SearchDialog from '$lib/search/SearchDialog.svelte';
  import BookmarksDialog from '$lib/bookmarks/BookmarksDialog.svelte';
  import { setHighlightRanges, type HighlightRange } from '$lib/editor/highlights';
  import { textRangeToSelection } from '$lib/editor/offsets';
  import FormatBar from '$lib/editor/FormatBar.svelte';
  import PageEditor, { type EditorState } from '$lib/editor/PageEditor.svelte';
  import SaveIndicator from '$lib/editor/SaveIndicator.svelte';
  import { autosave } from '$lib/editor/autosave.svelte';

  import { viewport } from '$lib/design/viewport.svelte';
  import { activePage } from '$lib/stores/page.svelte';
  import { router } from '$lib/stores/router.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import { bookmarks } from '$lib/stores/bookmarks.svelte';
  import { margin } from '$lib/stores/margin.svelte';
  import { workspace } from '$lib/stores/workspace.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { count } from '$lib/utils/format';
  import { anchorOf, type Annotation } from '$lib/types/annotation';

  interface Props {
    volumeId: string;
    pageId: string | null;
  }

  let { volumeId, pageId }: Props = $props();

  let editor = $state<Editor | null>(null);
  let editorState = $state<EditorState | null>(null);
  let navOpen = $state(false);

  type Target = { kind: 'chapter' | 'page'; id: string; title: string };
  let renameOpen = $state(false);
  let renameTarget = $state<Target | null>(null);
  let renameTitle = $state('');

  let deleteOpen = $state(false);
  let deleteTarget = $state<Target | null>(null);

  let linkOpen = $state(false);
  let linkUrl = $state('');

  let historyOpen = $state(false);
  let searchOpen = $state(false);
  let bookmarksOpen = $state(false);


  onMount(() => {
    void workspace.open(volumeId, pageId);
    void bookmarks.loadForVolume(volumeId);
    void ai.loadConfiguration();
    margin.start();

    // A finished AI run has written a note and possibly a suggestion; both are
    // re-read rather than assembled from the stream, so the Margin shows what
    // was actually stored.
    const stopAi = ai.start(() => {
      void margin.refresh();
      void ai.loadSuggestions(workspace.activePageId);
    });
    autosave.onsaved = (saved) => activePage.adoptSaved(saved);

    // A save must not be left waiting on a debounce timer when the writer
    // switches away from the window, closes the lid, or shuts down.
    const flush = () => void autosave.flush();
    window.addEventListener('blur', flush);
    document.addEventListener('visibilitychange', flush);
    window.addEventListener('beforeunload', flush);

    return () => {
      window.removeEventListener('blur', flush);
      document.removeEventListener('visibilitychange', flush);
      window.removeEventListener('beforeunload', flush);
      autosave.onsaved = undefined;
      workspace.close();
      activePage.clear();
      margin.clear();
      bookmarks.clear();
      stopAi();
      ai.clear();
    };
  });

  // Load whichever Page the tree has selected.
  $effect(() => {
    const id = workspace.activePageId;
    if (!id) {
      activePage.clear();
      return;
    }
    if (activePage.page?.id === id) return;
    void activePage.load(id);
    void margin.load(id);
    void ai.loadSuggestions(id);
  });

  // Saving re-anchors annotations in the same transaction, so the Margin is
  // re-read once the save lands rather than left showing pre-save offsets.
  $effect(() => {
    if (autosave.state === 'saved' && autosave.lastSavedAt !== null) void margin.refresh();
  });

  // Push annotated ranges into the editor whenever either side changes.
  $effect(() => {
    const view = editor?.view ?? null;
    const map = editorState?.offsets;
    if (!view || !map) return;

    const ranges: HighlightRange[] = [];
    for (const annotation of margin.visible) {
      const anchor = anchorOf(annotation);
      // Whole-Page notes have nothing to underline.
      if (!anchor) continue;
      const { from, to } = textRangeToSelection(map, anchor);
      ranges.push({
        id: annotation.id,
        from,
        to,
        kind: annotation.kind,
        stale: annotation.status === 'stale',
        focused: margin.focusedId === annotation.id
      });
    }

    setHighlightRanges(view, ranges);
  });

  /**
   * Reloads after a suggestion has been applied.
   *
   * The backend rewrote the document, so the editor is rebuilt from the stored
   * Page rather than patched — the editor is keyed by Page id and a genuinely
   * new record is what it needs.
   */
  async function reloadAfterApply() {
    const id = workspace.activePageId;
    if (!id) return;
    await activePage.load(id);
    await margin.refresh();
    await ai.loadSuggestions(id);
  }

  /** Scrolls the manuscript to an annotation's text and selects it. */
  function reveal(annotation: Annotation) {
    const anchor = anchorOf(annotation);
    const map = editorState?.offsets;
    if (!anchor || !map || !editor) return;
    const { from, to } = textRangeToSelection(map, anchor);
    editor.chain().focus().setTextSelection({ from, to }).scrollIntoView().run();
    margin.focusedId = annotation.id;
  }

  // Keep the editor's debounce in step with the setting.
  $effect(() => {
    autosave.setDebounce(settingsStore.settings.autosaveDebounceMs);
  });

  const chapterOf = $derived(
    workspace.chapters.find((chapter) =>
      chapter.pages.some((page) => page.id === workspace.activePageId)
    ) ?? null
  );

  const showNavPane = $derived(viewport.navIsPane);
  // The Margin is a real pane only when all three regions fit. Below that it
  // slides over, so the manuscript never gets squeezed to make room for notes.
  const marginIsPane = $derived(viewport.marginIsPane && margin.paneOpen);

  async function leave() {
    await autosave.flush();
    router.toLibrary();
  }

  function beginRename(target: Target) {
    renameTarget = target;
    renameTitle = target.title;
    renameOpen = true;
  }

  async function confirmRename() {
    const target = renameTarget;
    const title = renameTitle.trim();
    if (!target || !title) return;
    renameOpen = false;
    if (target.kind === 'chapter') await workspace.renameChapter(target.id, title);
    else await workspace.renamePage(target.id, title);
  }

  function beginDelete(target: Target) {
    deleteTarget = target;
    deleteOpen = true;
  }

  async function confirmDelete() {
    const target = deleteTarget;
    if (!target) return;
    deleteOpen = false;
    if (target.kind === 'chapter') await workspace.deleteChapter(target.id);
    else await workspace.deletePage(target.id);
  }

  async function openHistory() {
    // Flush first, so the version list includes what was just typed rather
    // than showing history that stops a paragraph short.
    await autosave.flush();
    historyOpen = true;
  }

  function openLinkDialog() {
    linkUrl = (editor?.getAttributes('link')['href'] as string | undefined) ?? '';
    linkOpen = true;
  }

  function applyLink() {
    const url = linkUrl.trim();
    linkOpen = false;
    if (!editor) return;
    if (!url) {
      editor.chain().focus().unsetLink().run();
      return;
    }
    // Bare domains are what people actually type; without a scheme the browser
    // would treat the href as a relative path.
    const href = /^[a-z][a-z0-9+.-]*:/i.test(url) ? url : `https://${url}`;
    editor.chain().focus().extendMarkRange('link').setLink({ href }).run();
  }

  async function onKeydown(event: KeyboardEvent) {
    const mod = event.ctrlKey || event.metaKey;
    if (!mod) {
      if (event.key === 'Escape' && navOpen) {
        event.preventDefault();
        navOpen = false;
      }
      return;
    }

    switch (event.key.toLowerCase()) {
      case 's':
        event.preventDefault();
        await autosave.flush();
        break;
      case 'n':
        event.preventDefault();
        if (event.shiftKey) await workspace.createChapter();
        else if (chapterOf) await workspace.createPage(chapterOf.id);
        else if (workspace.chapters[0]) await workspace.createPage(workspace.chapters[0].id);
        break;
      case 'f':
        // Ctrl+F searches this Volume, Ctrl+Shift+F the whole library. Both
        // open the same panel, which can switch scope without retyping.
        event.preventDefault();
        searchOpen = true;
        break;
      case ',':
        event.preventDefault();
        await autosave.flush();
        router.toSettings();
        break;
      case 'b':
        event.preventDefault();
        if (event.shiftKey) bookmarksOpen = true;
        else if (workspace.activePageId) await bookmarks.toggle(workspace.activePageId);
        break;
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="workspace" data-layout={viewport.layout}>
  <header class="rail">
    <div class="rail-left">
      {#if !showNavPane}
        <IconButton name="menu" label="Show manuscript" size="sm" onclick={() => (navOpen = true)} />
      {/if}
      <IconButton name="arrowLeft" label="Back to Library" size="sm" onclick={leave} />

      <nav class="breadcrumb" aria-label="Location">
        <button type="button" class="crumb volume truncate" onclick={leave}>
          {workspace.volume?.title ?? 'Loading…'}
        </button>
        {#if chapterOf}
          <Icon name="chevronRight" size={13} />
          <span class="crumb truncate">{chapterOf.title}</span>
        {/if}
      </nav>
    </div>

    {#if !viewport.isNarrow}
      <div class="rail-format">
        <FormatBar {editor} state={editorState} onlink={openLinkDialog} />
      </div>
    {/if}

    <div class="rail-right">
      <SaveIndicator />
      <IconButton name="search" label="Search" size="sm" onclick={() => (searchOpen = true)} />
      <IconButton
        name="settings"
        label="Settings"
        size="sm"
        onclick={async () => {
          await autosave.flush();
          router.toSettings();
        }}
      />
      <IconButton
        name="bookmark"
        label={bookmarks.isMarked(workspace.activePageId) ? 'Remove bookmark' : 'Bookmark this Page'}
        size="sm"
        tone={bookmarks.isMarked(workspace.activePageId) ? 'accent' : 'default'}
        pressed={bookmarks.isMarked(workspace.activePageId)}
        disabled={!workspace.activePageId}
        onclick={() => workspace.activePageId && bookmarks.toggle(workspace.activePageId)}
      />
      <IconButton
        name="restore"
        label="History"
        size="sm"
        disabled={!activePage.page}
        onclick={openHistory}
      />
      <span class="margin-toggle">
        <IconButton
          name="panelRight"
          label={marginIsPane
            ? margin.openCount > 0
              ? `Margin, ${margin.openCount} notes`
              : 'Margin'
            : 'Open the Margin'}
          size="sm"
          pressed={margin.paneOpen}
          disabled={!activePage.page}
          onclick={() => margin.setPaneOpen(!margin.paneOpen)}
        />
        {#if margin.openCount > 0}
          <span class="badge tabular" aria-hidden="true">{margin.openCount}</span>
        {/if}
      </span>
    </div>
  </header>

  <div class="body">
    {#if showNavPane}
      <aside class="pane nav">
        <ManuscriptTree onrename={beginRename} ondelete={beginDelete} />
      </aside>
    {/if}

    <main class="editor" data-scroll aria-label="Manuscript">
      {#if workspace.failure}
        <EmptyState title={workspace.failure} hint="Your manuscripts have not been changed.">
          {#snippet action()}
            <Button variant="secondary" onclick={() => router.toLibrary()}>Back to Library</Button>
          {/snippet}
        </EmptyState>
      {:else if workspace.loading}
        <p class="status">Opening…</p>
      {:else if workspace.isEmpty}
        <EmptyState
          title="This Volume has no Chapters."
          hint="A Chapter gives the manuscript its structure. Pages go inside it."
        >
          {#snippet action()}
            <Button variant="primary" icon="plus" onclick={() => void workspace.createChapter()}>
              New Chapter
            </Button>
          {/snippet}
        </EmptyState>
      {:else if activePage.failure}
        <EmptyState title={activePage.failure} />
      {:else if !workspace.activePageId}
        <EmptyState
          title="No Page is open."
          hint="Choose a Page from the manuscript, or add one to a Chapter."
        />
      {:else if activePage.page}
        <!-- Keyed on the Page id: opening a different Page builds a fresh
             editor, which is what keeps undo history from crossing Pages. -->
        {#key activePage.page.id}
          <PageEditor
            page={activePage.page}
            spellcheck={settingsStore.settings.spellcheck}
            onready={(instance) => (editor = instance)}
            onstate={(state) => (editorState = state)}
          />
        {/key}
      {:else}
        <p class="status">Opening Page…</p>
      {/if}
    </main>

    {#if marginIsPane}
      <aside class="pane margin-pane">
        <Margin
          pageId={activePage.page?.id ?? null}
          selection={editorState?.selection ?? null}
          onreveal={reveal}
          onapplied={reloadAfterApply}
        />
      </aside>
    {/if}
  </div>

  {#if viewport.isNarrow}
    <div class="bottom">
      <FormatBar {editor} state={editorState} onlink={openLinkDialog} />
      <span class="tally tabular">{count(activePage.words)} words</span>
    </div>
  {/if}
</div>

<!-- The manuscript as an overlay, for widths where it cannot be a pane. -->
{#if !showNavPane && navOpen}
  <div
    class="scrim"
    role="presentation"
    onpointerdown={() => (navOpen = false)}
  ></div>
  <aside class="pane overlay" aria-label="Manuscript">
    <header class="overlay-head">
      <span class="overlay-title truncate">{workspace.volume?.title ?? ''}</span>
      <IconButton name="close" label="Close manuscript" size="sm" onclick={() => (navOpen = false)} />
    </header>
    <ManuscriptTree
      onrename={beginRename}
      ondelete={beginDelete}
      onnavigate={() => (navOpen = false)}
    />
  </aside>
{/if}

<Dialog
  bind:open={renameOpen}
  title={renameTarget?.kind === 'chapter' ? 'Rename Chapter' : 'Rename Page'}
>
  <TextField bind:value={renameTitle} label="Title" autofocus onenter={confirmRename} />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (renameOpen = false)}>Cancel</Button>
    <Button variant="primary" disabled={!renameTitle.trim()} onclick={confirmRename}>Save</Button>
  {/snippet}
</Dialog>

<Dialog
  bind:open={deleteOpen}
  title={deleteTarget ? `Delete “${deleteTarget.title}”?` : 'Delete?'}
  description={deleteTarget?.kind === 'chapter'
    ? 'The Chapter and every Page inside it will be deleted. This cannot be undone.'
    : 'This Page will be deleted. This cannot be undone.'}
>
  <p class="warning">Nothing else in the Volume is affected.</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleteOpen = false)}>Cancel</Button>
    <Button variant="danger" onclick={confirmDelete}>Delete</Button>
  {/snippet}
</Dialog>

{#if !marginIsPane && margin.paneOpen}
  <div class="scrim" role="presentation" onpointerdown={() => margin.setPaneOpen(false)}></div>
  <aside class="pane margin-overlay">
    <Margin
      pageId={activePage.page?.id ?? null}
      selection={editorState?.selection ?? null}
      onreveal={reveal}
      onclose={() => margin.setPaneOpen(false)}
    />
  </aside>
{/if}

<BookmarksDialog
  bind:open={bookmarksOpen}
  volumeId={workspace.volume?.id ?? null}
  onselect={(bookmark) => {
    if (bookmark.volumeId !== workspace.volume?.id) {
      router.toWorkspace(bookmark.volumeId, bookmark.pageId);
      return;
    }
    workspace.selectPage(bookmark.pageId);
  }}
/>

<SearchDialog
  bind:open={searchOpen}
  volumeId={workspace.volume?.id ?? null}
  volumeTitle={workspace.volume?.title ?? null}
  onselect={(hit) => {
    if (hit.volumeId && hit.volumeId !== workspace.volume?.id) {
      router.toWorkspace(hit.volumeId, hit.pageId);
      return;
    }
    if (hit.pageId) workspace.selectPage(hit.pageId);
  }}
/>

<RevisionHistory
  bind:open={historyOpen}
  pageId={activePage.page?.id ?? null}
  pageTitle={activePage.page?.title ?? ''}
  currentText={activePage.page?.plainText ?? ''}
  currentWords={activePage.words}
  onrestored={(page) => {
    // Reload rather than patching in place: restoring replaces the document,
    // and the editor is keyed by Page id so it needs a genuinely new record.
    void activePage.load(page.id);
    void workspace.refresh();
  }}
/>

<Dialog bind:open={linkOpen} title="Link" description="Leave the field empty to remove the link.">
  <TextField
    bind:value={linkUrl}
    label="Address"
    placeholder="example.com"
    autofocus
    onenter={applyLink}
  />
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (linkOpen = false)}>Cancel</Button>
    <Button variant="primary" onclick={applyLink}>Apply</Button>
  {/snippet}
</Dialog>

<style>
  .workspace {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--surface-app);
  }

  .rail {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    min-height: var(--rail-height);
    padding: 0 var(--space-2);
    background: var(--surface-pane);
    border-bottom: var(--border-width) solid var(--border-subtle);
  }

  .rail-left {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
    flex: 1;
  }

  .rail-format {
    flex: none;
    min-width: 0;
  }

  .rail-right {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    flex: 1;
  }

  .breadcrumb {
    display: flex;
    align-items: center;
    gap: var(--space-1);
    min-width: 0;
    color: var(--text-tertiary);
    padding-left: var(--space-1);
  }

  .crumb {
    font-size: var(--text-sm);
    color: var(--text-secondary);
    min-width: 0;
    padding: var(--space-1) var(--space-1);
    border-radius: var(--radius-sm);
  }

  .crumb.volume {
    font-weight: var(--weight-medium);
    color: var(--text-primary);
  }

  .crumb.volume:hover {
    background: var(--surface-hover);
  }

  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .pane {
    flex: none;
    width: var(--pane-nav);
    min-width: 0;
    background: var(--surface-pane);
    border-right: var(--border-width) solid var(--border-subtle);
  }

  .editor {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    background: var(--surface-page);
  }

  .status {
    padding: var(--space-8) var(--space-4);
    text-align: center;
    color: var(--text-tertiary);
  }

  .warning {
    font-size: var(--text-md);
    color: var(--text-secondary);
  }

  /* On a narrow screen the formatting controls move to the bottom, where a
     thumb already is, rather than staying at the top out of reach. */
  .bottom {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    padding: var(--space-1) var(--space-2) max(var(--space-1), env(safe-area-inset-bottom));
    background: var(--surface-pane);
    border-top: var(--border-width) solid var(--border-subtle);
  }

  .tally {
    margin-left: auto;
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    white-space: nowrap;
    padding-right: var(--space-1);
  }

  .margin-pane {
    width: var(--pane-margin);
    border-right: none;
    border-left: var(--border-width) solid var(--border-subtle);
  }

  .margin-overlay {
    position: fixed;
    inset: 0 0 0 auto;
    z-index: var(--z-pane);
    width: min(22rem, 90vw);
    border-right: none;
    border-left: var(--border-width) solid var(--border-default);
    box-shadow: var(--shadow-overlay);
    animation: slide-in-right var(--motion-base) var(--ease-out);
  }

  .margin-toggle {
    position: relative;
    display: inline-flex;
  }

  /* A count on the toggle, so a collapsed Margin still says it holds notes. */
  .badge {
    position: absolute;
    top: 2px;
    right: 2px;
    min-width: 15px;
    padding: 0 3px;
    border-radius: var(--radius-pill);
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: 9px;
    line-height: 15px;
    text-align: center;
    pointer-events: none;
  }

  @keyframes slide-in-right {
    from {
      transform: translateX(100%);
    }
  }

  .scrim {
    position: fixed;
    inset: 0;
    z-index: var(--z-scrim);
    background: rgb(0 0 0 / 34%);
    animation: fade var(--motion-fast) var(--ease-out);
  }

  .overlay {
    position: fixed;
    inset: 0 auto 0 0;
    z-index: var(--z-pane);
    display: flex;
    flex-direction: column;
    width: min(20rem, 86vw);
    border-right: var(--border-width) solid var(--border-default);
    box-shadow: var(--shadow-overlay);
    animation: slide-in var(--motion-base) var(--ease-out);
  }

  .overlay-head {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    min-height: var(--rail-height);
    padding: 0 var(--space-2) 0 var(--space-3);
    border-bottom: var(--border-width) solid var(--border-subtle);
  }

  .overlay-title {
    flex: 1;
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
  }

  @keyframes slide-in {
    from {
      transform: translateX(-100%);
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
