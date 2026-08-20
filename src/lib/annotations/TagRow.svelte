<script lang="ts">
  import Icon from '$lib/components/Icon.svelte';
  import { attachTag, detachTag, tagsFor, type Tag } from '$lib/services/bookmarks';
  import { notices } from '$lib/stores/notices.svelte';

  /**
   * Tags for the open Page, in the Margin.
   *
   * Tags live here rather than in their own region because they are the same
   * kind of thing as a margin note: something the writer says *about* the page
   * rather than in it. Adding a region for two words of metadata would cost
   * more chrome than the feature is worth.
   */

  interface Props {
    pageId: string | null;
  }

  let { pageId }: Props = $props();

  let tags = $state<Tag[]>([]);
  let adding = $state(false);
  let draft = $state('');
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    const id = pageId;
    if (!id) {
      tags = [];
      return;
    }
    void tagsFor(id)
      .then((found) => {
        tags = found;
      })
      .catch(() => {
        tags = [];
      });
  });

  async function add() {
    const name = draft.trim();
    const id = pageId;
    draft = '';
    if (!name || !id) {
      adding = false;
      return;
    }
    try {
      await attachTag('page', id, name);
      tags = await tagsFor(id);
    } catch (error) {
      notices.failure(error);
    }
    // Stay open: tags are usually added two or three at a time.
    input?.focus();
  }

  async function remove(tag: Tag) {
    const id = pageId;
    if (!id) return;
    try {
      await detachTag(id, tag.id);
      tags = await tagsFor(id);
    } catch (error) {
      notices.failure(error);
    }
  }

  function onKeydown(event: KeyboardEvent) {
    if (event.key === 'Enter') {
      event.preventDefault();
      void add();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      draft = '';
      adding = false;
    }
  }
</script>

{#if pageId}
  <div class="tags">
    {#each tags as tag (tag.id)}
      <span class="tag">
        <span class="name">{tag.name}</span>
        <button type="button" class="remove" aria-label="Remove tag {tag.name}" onclick={() => remove(tag)}>
          <Icon name="close" size={12} />
        </button>
      </span>
    {/each}

    {#if adding}
      <!-- svelte-ignore a11y_autofocus -- opened by an explicit press on Add. -->
      <input
        bind:this={input}
        bind:value={draft}
        type="text"
        class="input selectable"
        placeholder="Tag…"
        autofocus
        maxlength="40"
        aria-label="New tag"
        onkeydown={onKeydown}
        onblur={() => {
          if (!draft.trim()) adding = false;
        }}
      />
    {:else}
      <button type="button" class="add" onclick={() => (adding = true)}>
        <Icon name="plus" size={13} />
        <span>Tag</span>
      </button>
    {/if}
  </div>
{/if}

<style>
  .tags {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-1);
    padding: var(--space-2) var(--space-3);
    border-top: var(--border-width) solid var(--border-subtle);
  }

  .tag {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 2px 2px var(--space-2);
    border-radius: var(--radius-pill);
    background: var(--surface-active);
    font-size: var(--text-xs);
    color: var(--text-secondary);
  }

  /* The remove control is always present rather than hover-revealed. A tablet
     has no hover, and a tag you cannot remove is a tag you cannot correct. */
  .remove {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: var(--radius-pill);
    color: var(--text-tertiary);
  }

  .remove:hover {
    background: var(--surface-hover);
    color: var(--state-danger);
  }

  .add {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    min-height: 26px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-pill);
    border: var(--border-width) dashed var(--border-default);
    font-size: var(--text-xs);
    color: var(--text-tertiary);
  }

  .add:hover {
    color: var(--text-primary);
    border-color: var(--border-strong);
  }

  .input {
    min-width: 6rem;
    max-width: 10rem;
    min-height: 26px;
    padding: 0 var(--space-2);
    border-radius: var(--radius-pill);
    border: var(--border-width) solid var(--accent);
    background: var(--surface-page);
    font-size: var(--text-xs);
  }

  .input:focus-visible {
    outline: none;
  }
</style>
