<script lang="ts">
  import { onMount } from 'svelte';

  import Library from './routes/Library.svelte';
  import Workspace from './routes/Workspace.svelte';
  import Notices from '$lib/components/Notices.svelte';

  import { appearance } from '$lib/design/theme.svelte';
  import { viewport } from '$lib/design/viewport.svelte';
  import { router } from '$lib/stores/router.svelte';
  import { settingsStore } from '$lib/stores/settings.svelte';
  import { invoke, isDesktop } from '$lib/services/ipc';

  let ready = $state(false);

  onMount(() => {
    const stopAppearance = appearance.start();
    const stopViewport = viewport.start();
    router.start();

    // Settings decide the theme and the manuscript's type, so the window is
    // revealed only after they have been applied. Showing it earlier would
    // paint the cached theme and then visibly jump.
    void settingsStore.load().finally(async () => {
      ready = true;
      if (isDesktop) await invoke('app_ready').catch(() => {});
    });

    return () => {
      stopAppearance();
      stopViewport();
    };
  });

  const route = $derived(router.current);

  /**
   * The design showcase, reachable at `#design` during development.
   *
   * It is lazily imported, and the condition is false in a production build, so
   * it is never fetched or evaluated there. The chunk is still *emitted* —
   * Rollup will not prove the branch dead through a reactive closure — which is
   * about 7 kB on disk that no user ever downloads. Not worth contorting the
   * app's structure to remove.
   */
  const showcase = $derived(
    import.meta.env.DEV && typeof location !== 'undefined' && location.hash === '#design'
  );
</script>

{#if showcase}
  {#await import('./routes/DesignShowcase.svelte') then module}
    <module.default />
  {/await}
{:else if ready}
  {#if route.name === 'workspace'}
    <!-- Keyed on the Volume so opening a different one rebuilds the workspace
         rather than trying to reconcile two manuscripts in place. -->
    {#key route.volumeId}
      <Workspace volumeId={route.volumeId} pageId={route.pageId} />
    {/key}
  {:else}
    <Library />
  {/if}
{/if}

<Notices />
