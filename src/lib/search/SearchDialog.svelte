<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import IconButton from '$lib/components/IconButton.svelte';
  import { searchLibrary, type SearchHit } from '$lib/services/search';
  import { describeError } from '$lib/services/ipc';
  import type { IconName } from '$lib/design/icons';

  interface Props {
    open: boolean;
    /** Confines the search to one Volume; null searches the whole library. */
    volumeId?: string | null;
    volumeTitle?: string | null;
    onselect: (hit: SearchHit) => void;
  }

  let { open = $bindable(), volumeId = null, volumeTitle = null, onselect }: Props = $props();

  let query = $state('');
  let hits = $state<SearchHit[]>([]);
  let searching = $state(false);
  let failure = $state<string | null>(null);
  let active = $state(0);
  let scoped = $state(true);
  let input = $state<HTMLInputElement | null>(null);
  let listbox = $state<HTMLElement | null>(null);

  /** Supersedes in-flight queries, so a slow one cannot overwrite a newer one. */
  let token = 0;
  let timer: ReturnType<typeof setTimeout> | null = null;

  /**
   * Long enough that typing a word does not fire a query per keystroke, short
   * enough that results feel like they are keeping up.
   */
  const DEBOUNCE_MS = 140;

  const scope = $derived(scoped && volumeId ? volumeId : null);

  $effect(() => {
    if (!open) return;
    // Re-focus each time the dialog opens, and select what is there so a second
    // search replaces the first rather than appending to it.
    queueMicrotask(() => {
      input?.focus();
      input?.select();
    });
  });

  $effect(() => {
    const text = query;
    const within = scope;

    if (timer) clearTimeout(timer);
    if (!text.trim()) {
      hits = [];
      searching = false;
      failure = null;
      return;
    }

    searching = true;
    timer = setTimeout(() => {
      const ticket = ++token;
      searchLibrary(text, within)
        .then((found) => {
          if (ticket !== token) return;
          hits = found;
          active = 0;
          failure = null;
        })
        .catch((error) => {
          if (ticket !== token) return;
          hits = [];
          failure = describeError(error);
        })
        .finally(() => {
          if (ticket === token) searching = false;
        });
    }, DEBOUNCE_MS);
  });

  const icons: Record<string, IconName> = {
    volume: 'volume',
    chapter: 'chapter',
    page: 'page',
    annotation: 'margin',
    ink: 'pencil'
  };

  /** Splits a snippet into marked and unmarked runs, without building markup. */
  function segments(hit: SearchHit): { text: string; marked: boolean }[] {
    const chars = [...hit.snippet];
    const out: { text: string; marked: boolean }[] = [];
    let cursor = 0;

    for (const [from, to] of hit.highlights) {
      if (from > cursor) out.push({ text: chars.slice(cursor, from).join(''), marked: false });
      out.push({ text: chars.slice(from, to).join(''), marked: true });
      cursor = to;
    }
    if (cursor < chars.length) out.push({ text: chars.slice(cursor).join(''), marked: false });
    return out;
  }

  function choose(hit: SearchHit) {
    open = false;
    query = '';
    hits = [];
    onselect(hit);
  }

  function move(delta: number) {
    if (hits.length === 0) return;
    active = (active + delta + hits.length) % hits.length;
    listbox?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({
      block: 'nearest'
    });
  }

  function onKeydown(event: KeyboardEvent) {
    switch (event.key) {
      case 'Escape':
        event.preventDefault();
        open = false;
        break;
      case 'ArrowDown':
        event.preventDefault();
        move(1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        move(-1);
        break;
      case 'Enter': {
        const hit = hits[active];
        if (hit) {
          event.preventDefault();
          choose(hit);
        }
        break;
      }
    }
  }
</script>

{#if open}
  <div class="scrim" role="presentation" onpointerdown={() => (open = false)}></div>

  <!-- tabindex="-1" makes the panel focusable so the dialog role is valid;
       focus itself goes to the input, which is what the panel exists for. -->
  <div
    class="panel"
    role="dialog"
    aria-modal="true"
    aria-label="Search"
    tabindex="-1"
    onkeydown={onKeydown}
  >
    <div class="field">
      <Icon name="search" size={18} />
      <!-- svelte-ignore a11y_autofocus -- the dialog exists to receive typing;
           anything else would need a click before it was usable. -->
      <input
        bind:this={input}
        bind:value={query}
        type="text"
        class="input selectable"
        placeholder={scope ? `Search ${volumeTitle ?? 'this Volume'}…` : 'Search your library…'}
        autofocus
        role="combobox"
        aria-expanded={hits.length > 0}
        aria-controls="search-results"
        aria-autocomplete="list"
      />
      <IconButton name="close" label="Close search" size="sm" onclick={() => (open = false)} />
    </div>

    {#if volumeId}
      <div class="scopes" role="tablist" aria-label="Search scope">
        <button
          type="button"
          role="tab"
          class="scope"
          class:selected={scoped}
          aria-selected={scoped}
          onclick={() => (scoped = true)}>This Volume</button
        >
        <button
          type="button"
          role="tab"
          class="scope"
          class:selected={!scoped}
          aria-selected={!scoped}
          onclick={() => (scoped = false)}>Whole library</button
        >
      </div>
    {/if}

    <div class="results" id="search-results" role="listbox" bind:this={listbox} data-scroll>
      {#if failure}
        <p class="message">{failure}</p>
      {:else if !query.trim()}
        <p class="message">Search titles, manuscript text, and margin notes.</p>
      {:else if searching && hits.length === 0}
        <p class="message">Searching…</p>
      {:else if hits.length === 0}
        <p class="message">Nothing matched “{query.trim()}”.</p>
      {:else}
        {#each hits as hit, index (hit.entityId)}
          <button
            type="button"
            role="option"
            data-index={index}
            class="hit"
            class:active={index === active}
            aria-selected={index === active}
            onpointerenter={() => (active = index)}
            onclick={() => choose(hit)}
          >
            <span class="hit-icon"><Icon name={icons[hit.kind] ?? 'page'} size={16} /></span>

            <span class="hit-body">
              <span class="hit-title truncate">{hit.title}</span>
              {#if hit.path.length > 0}
                <span class="hit-path truncate">{hit.path.join(' › ')}</span>
              {/if}
              {#if hit.snippet}
                <span class="hit-snippet">
                  {#each segments(hit) as run, runIndex (runIndex)}
                    {#if run.marked}<mark>{run.text}</mark>{:else}{run.text}{/if}
                  {/each}
                </span>
              {/if}
            </span>
          </button>
        {/each}
      {/if}
    </div>
  </div>
{/if}

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: var(--z-scrim);
    background: rgb(0 0 0 / 34%);
    animation: fade var(--motion-fast) var(--ease-out);
  }

  /* Sits high rather than centred: the results grow downward, and a panel that
     re-centres as they arrive makes the list feel unstable. */
  .panel {
    position: fixed;
    top: max(8vh, var(--space-5));
    left: 50%;
    translate: -50% 0;
    z-index: var(--z-palette);
    display: flex;
    flex-direction: column;
    width: min(40rem, calc(100vw - 2rem));
    max-height: min(32rem, 78vh);
    background: var(--surface-raised);
    border: var(--border-width) solid var(--border-default);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-overlay);
    overflow: hidden;
    animation: rise var(--motion-fast) var(--ease-out);
  }

  .field {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    flex: none;
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-4);
    border-bottom: var(--border-width) solid var(--border-subtle);
    color: var(--text-tertiary);
  }

  .input {
    flex: 1;
    min-width: 0;
    min-height: var(--touch-comfortable);
    background: none;
    border: none;
    font-size: var(--text-lg);
    color: var(--text-primary);
  }

  .input:focus-visible {
    outline: none;
  }

  .input::placeholder {
    color: var(--text-tertiary);
  }

  .scopes {
    display: flex;
    gap: var(--space-1);
    flex: none;
    padding: var(--space-2) var(--space-3);
    border-bottom: var(--border-width) solid var(--border-subtle);
  }

  .scope {
    min-height: 34px;
    padding: 0 var(--space-3);
    border-radius: var(--radius-md);
    font-size: var(--text-sm);
    color: var(--text-tertiary);
  }

  .scope:hover {
    color: var(--text-primary);
  }

  .scope.selected {
    background: var(--surface-selected);
    color: var(--accent);
    font-weight: var(--weight-medium);
  }

  .results {
    flex: 1;
    min-height: 0;
    padding: var(--space-1);
  }

  .message {
    padding: var(--space-5) var(--space-4);
    text-align: center;
    color: var(--text-tertiary);
    font-size: var(--text-md);
  }

  .hit {
    display: flex;
    gap: var(--space-3);
    width: 100%;
    padding: var(--space-3);
    border-radius: var(--radius-md);
    text-align: left;
    min-height: var(--touch-comfortable);
  }

  .hit.active {
    background: var(--surface-hover);
  }

  .hit-icon {
    flex: none;
    padding-top: 2px;
    color: var(--text-tertiary);
  }

  .hit-body {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .hit-title {
    font-size: var(--text-md);
    font-weight: var(--weight-medium);
    color: var(--text-primary);
  }

  .hit-path {
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }

  .hit-snippet {
    margin-top: var(--space-1);
    font-family: var(--font-manuscript);
    font-size: var(--text-sm);
    line-height: var(--leading-snug);
    color: var(--text-secondary);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  mark {
    background: var(--accent-quiet);
    color: var(--text-primary);
    border-radius: 2px;
    padding: 0 1px;
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(-6px);
    }
  }

  @keyframes fade {
    from {
      opacity: 0;
    }
  }
</style>
