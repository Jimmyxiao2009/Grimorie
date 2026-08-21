<script lang="ts">
  import { onMount, tick } from 'svelte';
  import type { Editor } from '@tiptap/core';

  import Button from '$lib/components/Button.svelte';
  import Dialog from '$lib/components/Dialog.svelte';
  import EmptyState from '$lib/components/EmptyState.svelte';
  import Icon from '$lib/components/Icon.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import TextField from '$lib/components/TextField.svelte';

  import ManuscriptTree from '$lib/manuscript/ManuscriptTree.svelte';
  import Margin from '$lib/annotations/Margin.svelte';
  import InkToolbar from '$lib/ink/InkToolbar.svelte';
  import RevisionHistory from '$lib/revisions/RevisionHistory.svelte';
  import SearchDialog from '$lib/search/SearchDialog.svelte';
  import BookmarksDialog from '$lib/bookmarks/BookmarksDialog.svelte';
  import PageEditor, { type EditorState } from '$lib/editor/PageEditor.svelte';
  import SelectionToolbar from '$lib/editor/SelectionToolbar.svelte';
  import SaveIndicator from '$lib/editor/SaveIndicator.svelte';
  import { autosave } from '$lib/editor/autosave.svelte';
  import { setHighlightRanges, type HighlightRange } from '$lib/editor/highlights';
  import { textRangeToSelection } from '$lib/editor/offsets';

  import { viewport } from '$lib/design/viewport.svelte';
  import { activePage } from '$lib/stores/page.svelte';
  import { router } from '$lib/stores/router.svelte';
  import { ai } from '$lib/stores/ai.svelte';
  import { bookmarks } from '$lib/stores/bookmarks.svelte';
  import { margin } from '$lib/stores/margin.svelte';
  import { ink } from '$lib/stores/ink.svelte';
  import { inkRecognition } from '$lib/stores/ink-recognition.svelte';
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

  let sheet = $state<HTMLElement | null>(null);
  let marginRef = $state<ReturnType<typeof Margin> | null>(null);

  /** Vertical offset of each anchored note's text within the sheet. */
  let anchorOffsets = $state<Map<string, number>>(new Map());
  let contentHeight = $state(0);

  type Target = { kind: 'chapter' | 'page'; id: string; title: string };
  let renameOpen = $state(false);
  let renameTarget = $state<Target | null>(null);
  let renameTitle = $state('');

  let deleteOpen = $state(false);
  let deleteTarget = $state<Target | null>(null);

  let linkOpen = $state(false);
  let linkUrl = $state('');

  /** Transient, for widths where the Margin cannot be a pane. */
  let marginOverlayOpen = $state(false);
  let historyOpen = $state(false);
  let searchOpen = $state(false);
  let bookmarksOpen = $state(false);

  onMount(() => {
    void workspace.open(volumeId, pageId);
    void bookmarks.loadForVolume(volumeId);
    void ai.loadConfiguration();
    margin.start();
    autosave.onsaved = (saved) => activePage.adoptSaved(saved);

    const stopAi = ai.start(() => {
      void margin.refresh();
      void ai.loadSuggestions(workspace.activePageId);
    });

    // Background handwriting recognition reports through an event, so the
    // Margin follows a job to its end without polling or a Page reload.
    const stopInkRecognition = inkRecognition.start();

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
      ink.clear();
      bookmarks.clear();
      stopAi();
      stopInkRecognition();
      ai.clear();
    };
  });

  // Load whichever Page the tree has selected.
  $effect(() => {
    const id = workspace.activePageId;
    if (!id) {
      activePage.clear();
      ink.clear();
      return;
    }
    if (activePage.page?.id === id) return;
    void activePage.load(id);
    void margin.load(id);
    void ink.load(id);
    void ai.loadSuggestions(id);
  });

  // Saving re-anchors annotations in the same transaction, so the Margin is
  // re-read once the save lands rather than left showing pre-save offsets.
  $effect(() => {
    if (autosave.state === 'saved' && autosave.lastSavedAt !== null) {
      void margin.refresh();
      void ink.refresh();
    }
  });

  $effect(() => {
    autosave.setDebounce(settingsStore.settings.autosaveDebounceMs);
  });

  /**
   * Measures where each anchored note's text sits, so the Margin can put the
   * note level with it.
   *
   * Offsets are taken relative to the sheet, and the sheet and the Margin share
   * one scroll container — so these numbers are content coordinates and do not
   * have to be recomputed when the writer scrolls.
   */
  function measureAnchors() {
    const view = editor?.view;
    const map = editorState?.offsets;
    if (!view || !map || !sheet) return;

    const sheetTop = sheet.getBoundingClientRect().top;
    const size = view.state.doc.content.size;
    const measured = new Map<string, number>();

    for (const note of margin.visible) {
      const anchor = anchorOf(note);
      if (!anchor) continue;
      const { from } = textRangeToSelection(map, anchor);
      if (from < 0 || from > size) continue;
      try {
        const coords = view.coordsAtPos(from);
        measured.set(note.id, Math.max(0, Math.round(coords.top - sheetTop)));
      } catch {
        // A position the view cannot resolve yet. Leaving it out simply means
        // the note is not placed this frame.
      }
    }

    anchorOffsets = measured;
    contentHeight = sheet.scrollHeight;
  }

  // Re-measure when the document, the notes, or the window change. Reading
  // these values is what subscribes the effect to them.
  $effect(() => {
    void editorState;
    void margin.visible;
    void viewport.width;
    const frame = requestAnimationFrame(measureAnchors);
    return () => cancelAnimationFrame(frame);
  });

  // Push annotated ranges into the editor whenever either side changes.
  $effect(() => {
    const view = editor?.view ?? null;
    const map = editorState?.offsets;
    if (!view || !map) return;

    const ranges: HighlightRange[] = [];
    for (const annotation of margin.visible) {
      const anchor = anchorOf(annotation);
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

  const chapterOf = $derived(
    workspace.chapters.find((chapter) =>
      chapter.pages.some((page) => page.id === workspace.activePageId)
    ) ?? null
  );

  const showNavPane = $derived(viewport.navIsPane && navOpen);
  /** The Margin is beside the sheet only where all three regions fit. */
  const marginIsPane = $derived(viewport.marginIsPane && margin.paneOpen);
  const marginIsOverlay = $derived(!viewport.marginIsPane && marginOverlayOpen);
  const marginShown = $derived(marginIsPane || marginIsOverlay);

  function toggleMargin() {
    if (viewport.marginIsPane) margin.setPaneOpen(!margin.paneOpen);
    else marginOverlayOpen = !marginOverlayOpen;
  }

  async function reloadAfterApply() {
    const id = workspace.activePageId;
    if (!id) return;
    await activePage.load(id);
    await margin.refresh();
    await ink.refresh();
    await ai.loadSuggestions(id);
  }

  function reveal(annotation: Annotation) {
    const anchor = anchorOf(annotation);
    const map = editorState?.offsets;
    if (!anchor || !map || !editor) return;
    const { from, to } = textRangeToSelection(map, anchor);
    editor.chain().focus().setTextSelection({ from, to }).scrollIntoView().run();
    margin.focusedId = annotation.id;
  }

  /** Opens the Margin if it is closed, then runs an action inside it. */
  async function inMargin(action: 'note' | 'ask') {
    if (!marginShown) {
      if (viewport.marginIsPane) margin.setPaneOpen(true);
      else marginOverlayOpen = true;
      await tick();
    }
    if (action === 'note') marginRef?.beginNote();
    else marginRef?.beginAsk();
  }

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
    const href = /^[a-z][a-z0-9+.-]*:/i.test(url) ? url : `https://${url}`;
    editor.chain().focus().extendMarkRange('link').setLink({ href }).run();
  }

  async function onKeydown(event: KeyboardEvent) {
    const mod = event.ctrlKey || event.metaKey;
    if (!mod) {
      if (event.key === 'Escape') {
        if (ink.penMode) {
          // Escape leaves pen mode, the way it leaves other modal states.
          event.preventDefault();
          ink.togglePenMode();
          return;
        }
        if (marginOverlayOpen) {
          event.preventDefault();
          marginOverlayOpen = false;
        } else if (navOpen && !viewport.navIsPane) {
          event.preventDefault();
          navOpen = false;
        }
      }
      return;
    }

    // Ink undo/redo is a separate history from the editor's, so that Ctrl+Z in
    // pen mode removes a stroke rather than deleting prose. Only pen mode
    // claims the shortcut; otherwise the editor's text undo is untouched.
    if (ink.penMode && (event.key === 'z' || event.key === 'Z' || event.key === 'y')) {
      event.preventDefault();
      if (event.shiftKey || event.key === 'y') void ink.redo();
      else void ink.undo();
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
      case '\\':
        event.preventDefault();
        navOpen = !navOpen;
        break;
    }
  }

  // The navigation pane opens with the Volume on a screen wide enough for it.
  $effect(() => {
    navOpen = viewport.navIsPane;
  });
</script>

<svelte:window onkeydown={onKeydown} />

<div class="workspace" data-layout={viewport.layout}>
  <!-- One rail, and only for things that act on the whole Page or the whole
       Volume. Formatting lives over the selection, where it is being used. -->
  <header class="rail">
    <div class="rail-left">
      <IconButton
        name="panelLeft"
        label={navOpen ? 'Hide the manuscript' : 'Show the manuscript'}
        size="sm"
        pressed={navOpen}
        onclick={() => (navOpen = !navOpen)}
      />
      <IconButton name="arrowLeft" label="Back to Library" size="sm" onclick={leave} />

      <nav class="where" aria-label="Location">
        <button type="button" class="volume" onclick={leave}>
          {workspace.volume?.title ?? '…'}
        </button>
        {#if chapterOf}
          <span class="slash" aria-hidden="true">/</span>
          <span class="chapter">{chapterOf.title}</span>
        {/if}
      </nav>
    </div>

    <div class="rail-right">
      {#if activePage.page && !viewport.isNarrow}
        <span class="words tabular">{count(activePage.words)} words</span>
      {/if}
      <SaveIndicator />
      <IconButton name="search" label="Search" size="sm" onclick={() => (searchOpen = true)} />
      <IconButton
        name="bookmark"
        label={bookmarks.isMarked(workspace.activePageId) ? 'Remove bookmark' : 'Bookmark'}
        size="sm"
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
      {#if activePage.page && marginShown}
        <InkToolbar />
      {/if}
      <span class="margin-toggle">
        <IconButton
          name="panelRight"
          label={marginShown ? 'Hide the Margin' : 'Show the Margin'}
          size="sm"
          pressed={marginShown}
          onclick={toggleMargin}
        />
        {#if margin.openCount > 0 && !marginShown}
          <span class="tally tabular" aria-hidden="true">{margin.openCount}</span>
        {/if}
      </span>
      <IconButton
        name="settings"
        label="Settings"
        size="sm"
        onclick={async () => {
          await autosave.flush();
          router.toSettings();
        }}
      />
    </div>
  </header>

  <div class="body">
    {#if showNavPane}
      <aside class="pane nav"><ManuscriptTree onrename={beginRename} ondelete={beginDelete} /></aside>
    {/if}

    <!-- The sheet and its Margin share one scroll container, which is what lets
         a marginal note stay level with the words it belongs to. -->
    <main class="reading" data-scroll aria-label="Manuscript">
      {#if workspace.failure}
        <div class="centred">
          <EmptyState title={workspace.failure} hint="Your manuscripts have not been changed.">
            {#snippet action()}
              <Button variant="secondary" onclick={() => router.toLibrary()}>Back to Library</Button>
            {/snippet}
          </EmptyState>
        </div>
      {:else if workspace.loading}
        <p class="status">Opening…</p>
      {:else if workspace.isEmpty}
        <div class="centred">
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
        </div>
      {:else if activePage.failure}
        <div class="centred"><EmptyState title={activePage.failure} /></div>
      {:else if !workspace.activePageId}
        <div class="centred">
          <EmptyState title="No Page is open." hint="Choose one from the manuscript." />
        </div>
      {:else if activePage.page}
        <div class="spread" class:with-margin={marginIsPane}>
          <article class="sheet" bind:this={sheet}>
            {#key activePage.page.id}
              <PageEditor
                page={activePage.page}
                spellcheck={settingsStore.settings.spellcheck}
                onready={(instance) => (editor = instance)}
                onstate={(state) => (editorState = state)}
              />
            {/key}
          </article>

          {#if marginIsPane}
            <div class="beside">
              <Margin
                bind:this={marginRef}
                pageId={activePage.page.id}
                selection={editorState?.selection ?? null}
                offsets={anchorOffsets}
                {contentHeight}
                onreveal={reveal}
                onapplied={reloadAfterApply}
              />
            </div>
          {/if}
        </div>
      {:else}
        <p class="status">Opening Page…</p>
      {/if}
    </main>
  </div>
</div>

<SelectionToolbar
  {editor}
  {editorState}
  aiAvailable={ai.isConfigured && settingsStore.settings.aiEnabled}
  onlink={openLinkDialog}
  onnote={() => void inMargin('note')}
  onask={() => void inMargin('ask')}
/>

<!-- The manuscript as an overlay, for widths where it cannot be a pane. -->
{#if navOpen && !viewport.navIsPane}
  <div class="scrim" role="presentation" onpointerdown={() => (navOpen = false)}></div>
  <aside class="pane overlay left" aria-label="Manuscript">
    <ManuscriptTree
      onrename={beginRename}
      ondelete={beginDelete}
      onnavigate={() => (navOpen = false)}
    />
  </aside>
{/if}

{#if marginIsOverlay && activePage.page}
  <div class="scrim" role="presentation" onpointerdown={() => (marginOverlayOpen = false)}></div>
  <aside class="pane overlay right" aria-label="Margin" data-scroll>
    <Margin
      bind:this={marginRef}
      pageId={activePage.page.id}
      selection={editorState?.selection ?? null}
      offsets={new Map()}
      contentHeight={0}
      onreveal={reveal}
      onapplied={reloadAfterApply}
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
    void activePage.load(page.id);
    void workspace.refresh();
  }}
/>

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
  <p class="plain">Nothing else in the Volume is affected.</p>
  {#snippet footer()}
    <Button variant="ghost" onclick={() => (deleteOpen = false)}>Cancel</Button>
    <Button variant="danger" onclick={confirmDelete}>Delete</Button>
  {/snippet}
</Dialog>

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

  /* --- Rail -------------------------------------------------------------- */

  .rail {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    min-height: var(--rail-height);
    padding: 0 var(--space-1);
    background: var(--surface-app);
    /* No border. The sheet below provides the separation by being a different
       surface, and a rule here would only add a line to look at. */
  }

  .rail-left,
  .rail-right {
    display: flex;
    align-items: center;
    gap: 1px;
    min-width: 0;
  }

  .rail-left {
    flex: 1;
  }

  .rail-right {
    justify-content: flex-end;
  }

  .where {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
    min-width: 0;
    padding-left: var(--space-2);
  }

  .volume,
  .chapter {
    font-size: var(--text-sm);
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .volume {
    color: var(--text-secondary);
  }

  .volume:hover {
    color: var(--text-primary);
  }

  .slash {
    color: var(--text-tertiary);
    flex: none;
  }

  .chapter {
    font-family: var(--font-serif);
    color: var(--text-primary);
  }

  .words {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
    padding: 0 var(--space-2);
    white-space: nowrap;
  }

  .margin-toggle {
    position: relative;
    display: inline-flex;
  }

  .tally {
    position: absolute;
    top: 3px;
    right: 3px;
    min-width: 14px;
    padding: 0 3px;
    border-radius: var(--radius-pill);
    background: var(--accent);
    color: var(--accent-contrast);
    font-size: 9px;
    line-height: 14px;
    text-align: center;
    pointer-events: none;
  }

  /* --- Body -------------------------------------------------------------- */

  .body {
    display: flex;
    flex: 1;
    min-height: 0;
  }

  .pane {
    flex: none;
    width: var(--pane-nav);
    min-width: 0;
    background: var(--surface-app);
  }

  .reading {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    /* The desk. The sheet sits on it. */
    background: var(--surface-app);
  }

  /* The page and the space beside it, centred as one object — which is what a
     manuscript with margins actually looks like. */
  .spread {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    max-width: calc(var(--measure-editor) + var(--sheet-gutter) * 2);
    margin-inline: auto;
    min-height: 100%;
  }

  .spread.with-margin {
    grid-template-columns: minmax(0, 1fr) var(--pane-margin);
    max-width: calc(var(--measure-editor) + var(--sheet-gutter) * 2 + var(--pane-margin));
  }

  /* The paper. Distinct from the desk by surface, not by a drawn border, with
     the faintest edge so it reads as a physical sheet rather than a panel. */
  .sheet {
    background: var(--surface-page);
    box-shadow: var(--page-edge);
    min-height: 100%;
    min-width: 0;
  }

  .beside {
    min-width: 0;
  }

  .centred {
    display: grid;
    place-items: center;
    min-height: 60vh;
  }

  .status {
    padding: var(--space-8) var(--space-4);
    text-align: center;
    color: var(--text-tertiary);
  }

  .plain {
    font-size: var(--text-md);
    color: var(--text-secondary);
  }

  /* --- Overlays ---------------------------------------------------------- */

  .scrim {
    position: fixed;
    inset: 0;
    z-index: var(--z-scrim);
    background: rgb(0 0 0 / 34%);
    animation: fade var(--motion-fast) var(--ease-out);
  }

  .overlay {
    position: fixed;
    top: 0;
    bottom: 0;
    z-index: var(--z-pane);
    width: min(20rem, 88vw);
    background: var(--surface-pane);
    box-shadow: var(--shadow-overlay);
    overflow-y: auto;
  }

  .overlay.left {
    left: 0;
    animation: slide-in-left var(--motion-base) var(--ease-out);
  }

  .overlay.right {
    right: 0;
    animation: slide-in-right var(--motion-base) var(--ease-out);
  }

  @keyframes slide-in-left {
    from {
      transform: translateX(-100%);
    }
  }

  @keyframes slide-in-right {
    from {
      transform: translateX(100%);
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
