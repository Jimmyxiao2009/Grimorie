<script lang="ts">
  import { onMount } from 'svelte';
  import DesignShowcase from './routes/DesignShowcase.svelte';
  import { appearance } from '$lib/design/theme.svelte';
  import { viewport } from '$lib/design/viewport.svelte';
  import { invoke, isDesktop } from '$lib/services/ipc';

  onMount(() => {
    const stopAppearance = appearance.start();
    const stopViewport = viewport.start();

    // Reveal the window only once the first frame has been painted.
    if (isDesktop) void invoke('app_ready').catch(() => {});

    return () => {
      stopAppearance();
      stopViewport();
    };
  });
</script>

<DesignShowcase />
